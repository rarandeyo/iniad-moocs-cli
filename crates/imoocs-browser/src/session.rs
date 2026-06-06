use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::batch::BatchResponse;
use crate::error::BrowserError;
use crate::ops::BrowserOps;
use crate::process::AgentBrowser;
use crate::snapshot::Snapshot;

/// `--session-name imoocs` で固定された agent-browser セッション。
///
/// Phase A1 では最小 API のみ。Phase A2 で auth / Phase B で navigate などを追加する。
#[derive(Debug, Clone)]
pub struct BrowserSession {
    agent: AgentBrowser,
}

impl BrowserSession {
    /// `imoocs setup` で確保された agent-browser バイナリを使ってセッション構築。
    /// session 名は固定 `imoocs`。
    pub fn new(binary: PathBuf) -> Self {
        Self::new_with_session(binary, "imoocs")
    }

    /// テスト・特殊用途用に session 名を指定したい場合 (例: headed フォールバックの short-lived session)。
    pub fn new_with_session(binary: PathBuf, session_name: &str) -> Self {
        Self {
            agent: AgentBrowser::new(binary, session_name),
        }
    }

    /// 内部の `AgentBrowser` 参照を返す (低レベル API を直接叩きたい場合)。
    pub fn agent(&self) -> &AgentBrowser {
        &self.agent
    }
}

#[async_trait]
impl BrowserOps for BrowserSession {
    async fn navigate(&self, url: &str) -> Result<String, BrowserError> {
        let value: Value = self.agent.run(&["open", url]).await?;
        Ok(value
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or(url)
            .to_string())
    }

    async fn current_url(&self) -> Result<String, BrowserError> {
        let value: Value = self.agent.run(&["get", "url"]).await?;
        Ok(value
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn snapshot_interactive(&self, scope: Option<&str>) -> Result<Snapshot, BrowserError> {
        let mut args: Vec<&str> = vec!["snapshot", "-i"];
        if let Some(css) = scope {
            args.push("-s");
            args.push(css);
        }
        self.agent.run_json::<Snapshot>(&args).await
    }

    async fn run_batch(&self, batch_json: &str) -> Result<BatchResponse, BrowserError> {
        // `batch` は stdin に JSON 配列を渡す。stdout は per-command 結果の配列または envelope。
        let value: Value = self
            .agent
            .run_with_stdin(&["batch"], Some(batch_json.as_bytes()))
            .await?;
        // agent-browser 0.21.2 では `batch --json` は per-command result の配列を `data` に置く
        // ことがあるので、両方サポート。
        if let Some(array) = value.as_array() {
            Ok(serde_json::from_value(Value::Array(array.clone()))?)
        } else {
            Err(BrowserError::Internal(format!(
                "unexpected batch response shape: {value}"
            )))
        }
    }
}
