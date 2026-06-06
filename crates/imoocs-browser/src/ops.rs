use std::sync::Mutex;

use async_trait::async_trait;

use crate::batch::BatchResponse;
use crate::error::BrowserError;
use crate::snapshot::Snapshot;

/// テスト容易性のための抽象。production は `BrowserSession`、test は `FakeBrowserSession`。
///
/// Phase A1 では最小 API のみ。Phase B/C/D で必要に応じて拡張。
#[async_trait]
pub trait BrowserOps: Send + Sync + std::fmt::Debug {
    /// `agent-browser open <url>` 相当。返り値の `url` は最終リダイレクト先。
    async fn navigate(&self, url: &str) -> Result<String, BrowserError>;
    /// 現在の URL を取得。
    async fn current_url(&self) -> Result<String, BrowserError>;
    /// snapshot を取得。
    async fn snapshot_interactive(&self, scope: Option<&str>) -> Result<Snapshot, BrowserError>;
    /// バッチ JSON を実行。
    async fn run_batch(&self, batch_json: &str) -> Result<BatchResponse, BrowserError>;
}

/// テスト用 fake。`navigate` → `current_url` で URL を記憶するだけ。
#[derive(Debug, Default)]
pub struct FakeBrowserSession {
    state: Mutex<FakeState>,
}

#[derive(Debug, Default)]
struct FakeState {
    current_url: String,
    snapshot_response: Option<Snapshot>,
    batch_response: Option<BatchResponse>,
}

impl FakeBrowserSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_initial_url(url: impl Into<String>) -> Self {
        Self {
            state: Mutex::new(FakeState {
                current_url: url.into(),
                ..Default::default()
            }),
        }
    }

    /// テスト fixture として snapshot を仕込む。次の `snapshot_interactive` でこれが返る。
    pub fn set_snapshot(&self, snapshot: Snapshot) {
        let mut s = self.state.lock().unwrap();
        s.snapshot_response = Some(snapshot);
    }

    /// テスト fixture として batch response を仕込む。
    pub fn set_batch_response(&self, response: BatchResponse) {
        let mut s = self.state.lock().unwrap();
        s.batch_response = Some(response);
    }
}

#[async_trait]
impl BrowserOps for FakeBrowserSession {
    async fn navigate(&self, url: &str) -> Result<String, BrowserError> {
        let mut s = self.state.lock().unwrap();
        s.current_url = url.to_string();
        Ok(url.to_string())
    }

    async fn current_url(&self) -> Result<String, BrowserError> {
        Ok(self.state.lock().unwrap().current_url.clone())
    }

    async fn snapshot_interactive(&self, _scope: Option<&str>) -> Result<Snapshot, BrowserError> {
        let s = self.state.lock().unwrap();
        s.snapshot_response.clone().ok_or_else(|| {
            BrowserError::Internal("FakeBrowserSession: no snapshot fixture set".into())
        })
    }

    async fn run_batch(&self, _batch_json: &str) -> Result<BatchResponse, BrowserError> {
        let s = self.state.lock().unwrap();
        s.batch_response
            .clone()
            .ok_or_else(|| BrowserError::Internal("FakeBrowserSession: no batch fixture set".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_session_navigate_and_current_url() {
        let fake = FakeBrowserSession::new();
        fake.navigate("https://example.com").await.unwrap();
        assert_eq!(fake.current_url().await.unwrap(), "https://example.com");
    }

    #[tokio::test]
    async fn fake_session_returns_fixture_snapshot() {
        let raw = r#"{
            "snapshot": "",
            "refs": {"e1": {"role": "button", "name": "OK"}}
        }"#;
        let snap: Snapshot = serde_json::from_str(raw).unwrap();
        let fake = FakeBrowserSession::new();
        fake.set_snapshot(snap);
        let got = fake.snapshot_interactive(None).await.unwrap();
        assert_eq!(got.refs.len(), 1);
        assert_eq!(got.refs["e1"].name, "OK");
    }

    #[tokio::test]
    async fn fake_session_errors_when_no_snapshot_set() {
        let fake = FakeBrowserSession::new();
        let result = fake.snapshot_interactive(None).await;
        assert!(matches!(result, Err(BrowserError::Internal(_))));
    }
}
