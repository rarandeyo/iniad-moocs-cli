//! Google Slides の pubembed から per-slide screenshot を取得する (Phase D-3 戦略 A')。
//!
//! 当初は Chrome 印刷 (`pdf` コマンド) を予定していたが、実機検証で pubembed の
//! print CSS がビューア本体を隠して黒背景のみの PDF になることが判明した。
//! screenshot は viewport どおり完全に描画される (日本語フォント込み) ため、
//! **screenshot (PNG 1280x720) → caller 側で JPEG 変換 + lopdf 埋め込み** に変更した。
//!
//! 各 slide は `?slide=id.p<N>` クエリで個別表示できる (Phase 0 / D-3 実機検証済)。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::batch::{BatchBuilder, BatchResponse, LoadKind};
use crate::error::BrowserError;
use crate::process::AgentBrowser;

const SESSION_NAME: &str = "imoocs";

/// pubembed のページが描画完了 + Web フォントロード完了したかの判定。
/// 実機 DOM (D-3 検証): slide 本体の svg は `.punch-viewer-svgpage-svgcontainer` 配下
/// (svg 自体にクラスは無い)。
const READY_JS: &str = r#"(function(){
if (typeof document === 'undefined' || !document.body) return false;
var svg = document.querySelector('.punch-viewer-svgpage-svgcontainer svg');
if (!svg) return false;
try {
  if (document.fonts && document.fonts.status !== 'loaded') return false;
} catch (e) { /* document.fonts 非対応は無視 */ }
var imgs = Array.from(document.images || []);
if (imgs.length > 0 && !imgs.every(function(i){return i.complete;})) return false;
return true;
})()"#;

/// slide 総枚数を取得する。
/// 第一候補: `.punch-viewer-svgpage-a11yelement` の aria-label「スライド 1/3: 」
/// (実機検証済、locale 非依存の `数字/数字` パターンで抜く)。
const COUNT_JS: &str = r#"(function(){
var a11y = document.querySelector('.punch-viewer-svgpage-a11yelement');
if (a11y) {
  var m = (a11y.getAttribute('aria-label') || '').match(/(\d+)\s*\/\s*(\d+)/);
  if (m) return parseInt(m[2], 10);
}
// fallback 1: aria-label に カウンタを持つ任意の要素
var els = document.querySelectorAll('[aria-label]');
for (var i = 0; i < els.length; i++) {
  var m2 = (els[i].getAttribute('aria-label') || '').match(/^[^\d]*(\d+)\s*\/\s*(\d+)/);
  if (m2) return parseInt(m2[2], 10);
}
// fallback 2: viewerInfo global
if (typeof window.viewerInfo === 'object' && window.viewerInfo && Array.isArray(window.viewerInfo.slidesIds)) {
  return window.viewerInfo.slidesIds.length;
}
return 0;
})()"#;

/// screenshot 前にビューアのナビバー (ページャ + Google Slides ロゴ) を隠す。
/// スライド本体の描画には影響しない。
const HIDE_NAVBAR_JS: &str = r#"(function(){
document.querySelectorAll('.punch-viewer-navbar, .punch-viewer-branding').forEach(function(e){ e.style.display = 'none'; });
return true;
})()"#;

/// pubembed の embed_url から slide 総枚数を取得する。
///
/// 0 が返った場合は枚数取得失敗 (DOM 構造が変わった可能性) として caller 側で
/// エラーにすることを想定。
pub async fn count_slides(binary: &Path, embed_url: &str) -> Result<u32, BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), SESSION_NAME);
    let mut builder = BatchBuilder::new();
    builder
        .open(embed_url)
        .wait_load(LoadKind::DomContentLoaded)
        .wait_fn(READY_JS, 30_000)
        .eval(COUNT_JS)
        .get_url();
    let json = builder.to_json().map_err(BrowserError::from)?;
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;
    let outcomes: BatchResponse = serde_json::from_value(value)?;

    let final_url = outcomes
        .last()
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if final_url.contains("accounts.google.com") {
        return Err(BrowserError::CommandFailed(
            "Slides navigation redirected to Google sign-in; session expired".into(),
        ));
    }
    if let Some(failed) = outcomes.iter().take(3).find(|o| !o.success) {
        return Err(BrowserError::CommandFailed(format!(
            "count_slides: step {:?} failed: {}",
            failed.command,
            failed.error.as_deref().unwrap_or("unknown")
        )));
    }

    let count = outcomes
        .get(3)
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("result"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(count.min(u32::MAX as u64) as u32)
}

/// `embed_url` の `?slide=id.p<N>` クエリで指定 slide を navigate し、
/// `set viewport 1280 720` + `screenshot <dest>` で PNG (1280x720) を保存する。
pub async fn fetch_slide_screenshot(
    binary: &Path,
    embed_url: &str,
    slide_index: u32,
    dest: &Path,
) -> Result<(), BrowserError> {
    let target_url = build_slide_url(embed_url, slide_index);
    let agent = AgentBrowser::new(binary.to_path_buf(), SESSION_NAME);
    let mut builder = BatchBuilder::new();
    builder
        .set_viewport(1280, 720)
        .open(&target_url)
        .wait_load(LoadKind::DomContentLoaded)
        .wait_fn(READY_JS, 30_000)
        .eval(HIDE_NAVBAR_JS)
        .push(["screenshot".to_string(), dest.display().to_string()])
        .get_url();
    let json = builder.to_json().map_err(BrowserError::from)?;
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;
    let outcomes: BatchResponse = serde_json::from_value(value)?;

    let final_url = outcomes
        .last()
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if final_url.contains("accounts.google.com") {
        return Err(BrowserError::CommandFailed(format!(
            "fetch_slide_screenshot({slide_index}): redirected to Google sign-in"
        )));
    }
    if let Some(failed) = outcomes.iter().find(|o| !o.success) {
        return Err(BrowserError::CommandFailed(format!(
            "fetch_slide_screenshot({slide_index}): step {:?} failed: {}",
            failed.command,
            failed.error.as_deref().unwrap_or("unknown")
        )));
    }
    Ok(())
}

/// 高レベル: embed_url の全 slide を順次 screenshot して `dest_dir/page_NNN.png` に保存する。
/// 戻り値は保存順 (= slide 順) の PathBuf 配列。
pub async fn fetch_slide_screenshots(
    binary: &Path,
    embed_url: &str,
    dest_dir: &Path,
) -> Result<Vec<PathBuf>, BrowserError> {
    std::fs::create_dir_all(dest_dir).map_err(|e| BrowserError::Internal(format!("mkdir: {e}")))?;
    let total = count_slides(binary, embed_url).await?;
    if total == 0 {
        return Err(BrowserError::CommandFailed(
            "slides: could not detect slide count (DOM structure may have changed)".into(),
        ));
    }
    let mut paths = Vec::with_capacity(total as usize);
    for i in 1..=total {
        let dest = dest_dir.join(format!("page_{i:03}.png"));
        fetch_slide_screenshot(binary, embed_url, i, &dest).await?;
        paths.push(dest);
    }
    Ok(paths)
}

/// embed_url の query を `?slide=id.p<N>` に置換 (or 追加) する。
fn build_slide_url(embed_url: &str, slide_index: u32) -> String {
    let slide_param = format!("slide=id.p{slide_index}");
    if let Some(qpos) = embed_url.find('?') {
        let (base, query) = embed_url.split_at(qpos);
        let query = &query[1..]; // skip '?'
        let kept: Vec<&str> = query.split('&').filter(|p| !p.starts_with("slide=")).collect();
        let mut new_query = kept.join("&");
        if !new_query.is_empty() {
            new_query.push('&');
        }
        new_query.push_str(&slide_param);
        format!("{base}?{new_query}")
    } else {
        format!("{embed_url}?{slide_param}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_slide_url_appends_when_no_query() {
        let got = build_slide_url("https://docs.google.com/presentation/d/e/abc/pubembed", 3);
        assert_eq!(got, "https://docs.google.com/presentation/d/e/abc/pubembed?slide=id.p3");
    }

    #[test]
    fn build_slide_url_preserves_other_query_params() {
        let got = build_slide_url(
            "https://docs.google.com/presentation/d/e/abc/pubembed?start=false&loop=false",
            5,
        );
        assert!(got.contains("start=false"));
        assert!(got.contains("loop=false"));
        assert!(got.contains("slide=id.p5"));
    }

    #[test]
    fn build_slide_url_replaces_existing_slide_param() {
        let got = build_slide_url(
            "https://docs.google.com/presentation/d/e/abc/pubembed?slide=id.p1&start=false",
            7,
        );
        assert!(got.contains("slide=id.p7"));
        assert!(!got.contains("slide=id.p1"));
        assert!(got.contains("start=false"));
    }
}
