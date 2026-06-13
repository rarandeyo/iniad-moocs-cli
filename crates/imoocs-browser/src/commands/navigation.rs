//! ページ navigate + HTML 取得の高レベルヘルパ。
//!
//! 旧 `reqwest::get(url).text()` 相当を `agent-browser` 経由で実現する。
//! `document.documentElement.outerHTML` を `eval` で取れば完全な HTML (`<html>` 〜
//! `</html>`) が返ってくるので、既存の `scraper::Html::parse_document` がそのまま動く。

use std::path::Path;

use serde_json::Value;

use crate::batch::{BatchBuilder, BatchResponse};
use crate::error::BrowserError;
use crate::process::AgentBrowser;

/// fetch_page の結果: 完全 HTML + リダイレクト解決後の最終 URL。
#[derive(Debug, Clone)]
pub struct PageFetch {
    pub html: String,
    pub final_url: String,
}

/// URL に navigate して `(html, final_url)` を batch で 1 spawn で取得する。
///
/// `wait --load domcontentloaded` は MOOCs で 25 秒以上かかる (Web フォントや
/// 内部 API 待ちを全部しゃぶり尽くす) ため使わない。代わりに `wait --fn` で
/// `document.body` の存在だけ確認すれば、`outerHTML` は静的 HTML の組み立てが
/// 完了した時点で取れる (= 後続の AJAX 注入要素は別 batch で待つ)。
///
/// 内部動作 (1 batch = 1 spawn):
/// 1. `open <url>` (リダイレクト追従)
/// 2. `wait --fn document.body !== null`
/// 3. `eval document.documentElement.outerHTML`
/// 4. `get url`
pub async fn fetch_page(binary: &Path, url: &str) -> Result<PageFetch, BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), "imoocs");
    let mut builder = BatchBuilder::new();
    builder
        .open(url)
        .wait_fn("document.readyState === 'complete'", 30_000)
        .eval("document.documentElement.outerHTML")
        .get_url();
    let json = builder.to_json().map_err(BrowserError::from)?;
    // `batch` は envelope 無しで配列を直接出すので `run_raw` を使う
    // (`run_with_stdin` は envelope dispatch を行うため使えない)。
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;

    // batch の結果は配列。`commands::auth_moocs` 経由のテストで shape 確認済。
    let outcomes: BatchResponse = serde_json::from_value(value.clone()).or_else(|_| serde_json::from_value(value))?;
    if let Some(first_err) = outcomes.iter().find(|o| !o.success) {
        return Err(BrowserError::CommandFailed(format!(
            "fetch_page: command {:?} failed: {}",
            first_err.command,
            first_err.error.as_deref().unwrap_or("unknown")
        )));
    }
    // eval (index=2) と get url (index=3) から取り出す
    let html = outcomes
        .get(2)
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("result"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| BrowserError::Internal("fetch_page: missing eval result".into()))?
        .to_string();
    let final_url = outcomes
        .get(3)
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("url"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| BrowserError::Internal("fetch_page: missing url result".into()))?
        .to_string();
    Ok(PageFetch { html, final_url })
}

/// URL に navigate して完全 HTML だけ取る簡易版。final_url が不要なときに。
pub async fn fetch_html(binary: &Path, url: &str) -> Result<String, BrowserError> {
    fetch_page(binary, url).await.map(|p| p.html)
}
