use std::path::PathBuf;
use std::process::Stdio;

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::BrowserError;

/// agent-browser バイナリを spawn する薄いラッパ。
///
/// 仕様:
/// - 任意のサブコマンド (`open`, `snapshot`, `auth save`, `batch`, ...) を受け付ける
/// - `--session-name <session>` と `--json` を**常に**自動付与する
/// - `stdin` への secret 注入 (auth save の password 等) を提供する
/// - 子プロセスの stderr は debug log として trace、stdout は JSON envelope としてパース
#[derive(Debug, Clone)]
pub struct AgentBrowser {
    binary: PathBuf,
    session_name: String,
    /// spawn 時に子プロセスへ渡す追加環境変数。daemon 起動時にだけ効くものは
    /// 事前に `close` してから使うこと (例: `AGENT_BROWSER_DOWNLOAD_PATH`)。
    envs: Vec<(String, String)>,
}

impl AgentBrowser {
    /// バイナリ path と session 名で構築。session 名 = `imoocs` 推奨。
    pub fn new(binary: PathBuf, session_name: impl Into<String>) -> Self {
        Self {
            binary,
            session_name: session_name.into(),
            envs: Vec::new(),
        }
    }

    /// spawn 時に渡す環境変数を追加する (builder スタイル)。
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }

    /// 内部 build: `--session-name <name> --json` を頭に置いた `Command` を返す。
    fn base_command(&self) -> Command {
        let mut cmd = Command::new(&self.binary);
        cmd.arg("--session-name")
            .arg(&self.session_name)
            .arg("--json");
        for (k, v) in &self.envs {
            cmd.env(k, v);
        }
        // stdout/stderr は piped (envelope を JSON 取り回しするため)
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd
    }

    /// サブコマンドと追加引数を渡して実行 (stdin なし)。
    /// 返値は agent-browser の標準 envelope を `serde_json::Value` でパースしたもの。
    pub async fn run(&self, args: &[&str]) -> Result<Value, BrowserError> {
        self.run_with_stdin(args, None).await
    }

    /// 戻り値を任意の `T` にデシリアライズする shortcut。
    pub async fn run_json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T, BrowserError> {
        let value = self.run(args).await?;
        Ok(serde_json::from_value::<T>(value)?)
    }

    /// envelope dispatch を **行わない** raw 版。`batch` のように戻り値が
    /// `[]` の配列を直接出すコマンドで使う (envelope `{success, data, error}` を期待しないため)。
    pub async fn run_raw(&self, args: &[&str], stdin_bytes: Option<&[u8]>) -> Result<Value, BrowserError> {
        self.run_inner(args, stdin_bytes, false).await
    }

    /// stdin から bytes を流し込む形 (`auth save --password-stdin` など)。
    ///
    /// 仕様 (Phase A1 で確定した安全要件):
    /// - 書き込み後に `stdin` を明示的に `drop` してパイプを閉じる
    /// - 成功・エラー両経路で `stdin` 自動 drop が走るので、`scopeguard` は不要
    pub async fn run_with_stdin(
        &self,
        args: &[&str],
        stdin_bytes: Option<&[u8]>,
    ) -> Result<Value, BrowserError> {
        self.run_inner(args, stdin_bytes, true).await
    }

    async fn run_inner(
        &self,
        args: &[&str],
        stdin_bytes: Option<&[u8]>,
        dispatch_envelope: bool,
    ) -> Result<Value, BrowserError> {
        let mut cmd = self.base_command();
        cmd.args(args);
        if stdin_bytes.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        tracing::debug!(target: "imoocs_browser::process", session = %self.session_name, ?args, "spawning agent-browser");

        let mut child = cmd.spawn().map_err(BrowserError::from)?;

        // stdin を書き込む (passwords など)。
        if let Some(bytes) = stdin_bytes {
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(bytes)
                    .await
                    .map_err(BrowserError::from)?;
                // 明示 drop で pipe を閉じる (Drop 時に flush も走るが念のため)
                drop(stdin);
            }
        }

        let output = child.wait_with_output().await.map_err(BrowserError::from)?;
        let stdout = output.stdout;
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            tracing::warn!(target: "imoocs_browser::process", code = ?output.status.code(), stderr = %stderr, "agent-browser exited non-zero");
            if let Ok(v) = serde_json::from_slice::<Value>(&stdout) {
                if dispatch_envelope {
                    return Self::dispatch_envelope(v);
                }
                return Ok(v);
            }
            return Err(BrowserError::non_zero_exit(output.status.code(), stderr));
        }

        let value: Value = serde_json::from_slice(&stdout)?;
        if dispatch_envelope {
            Self::dispatch_envelope(value)
        } else {
            Ok(value)
        }
    }

    /// `{success, data, error}` envelope を解釈して `data` フィールドを返す。
    fn dispatch_envelope(value: Value) -> Result<Value, BrowserError> {
        if value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Ok(value.get("data").cloned().unwrap_or(Value::Null));
        }
        let err_msg = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown agent-browser error")
            .to_string();
        Err(BrowserError::CommandFailed(err_msg))
    }
}
