use std::io;

use thiserror::Error;

/// `imoocs-browser` 内で発生するエラー。
///
/// `imoocs-core` への変換は呼び出し側 (imoocs-core::auth など) が
/// `From<BrowserError> for ImoocsError` で行う想定 (Phase A2)。
#[derive(Debug, Error)]
pub enum BrowserError {
    /// agent-browser バイナリが PATH 上に無い (`imoocs setup` 案内)
    #[error("agent-browser binary not found. Run `imoocs setup` to install it.")]
    BinaryMissing,

    /// `agent-browser` の子プロセス起動に失敗
    #[error("failed to spawn agent-browser: {0}")]
    Spawn(#[from] io::Error),

    /// 子プロセスが exit code 非 0 で終了した (envelope パース不能 or stderr で報告)
    #[error("agent-browser exited with code {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },

    /// JSON envelope パース失敗
    #[error("failed to parse agent-browser JSON output: {0}")]
    Json(#[from] serde_json::Error),

    /// envelope の `success: false` が返った (operation 自体は実行されたが業務的に失敗)
    #[error("agent-browser command failed: {0}")]
    CommandFailed(String),

    /// Google SSO challenge (reCAPTCHA / 2FA / speedbump) が headless で通過できない
    /// → `imoocs-core` 側で headed フォールバックする
    #[error("auth challenge required (current url: {current_url})")]
    ChallengeRequired { current_url: String },

    /// agent-browser auth profile が見つからない (`imoocs auth login` 案内)
    #[error("auth profile `{name}` not found in auth-vault. Run `imoocs auth login`.")]
    AuthProfileMissing { name: String },

    /// その他の内部エラー (anyhow 包装)
    #[error("internal error: {0}")]
    Internal(String),
}

impl BrowserError {
    /// stderr 文字列を内部に持たせる短縮 ctor。
    pub fn non_zero_exit(code: Option<i32>, stderr: impl Into<String>) -> Self {
        Self::NonZeroExit {
            code,
            stderr: stderr.into(),
        }
    }
}
