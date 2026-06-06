//! Google Slides の pubembed から per-slide PDF を取得する (Phase D-3 戦略 A)。
//!
//! 旧 SVG 抽出経路 (slides.rs::extract_svgs + svg2pdf) は色付き背景 / 日本語フォント
//! のレンダリングが不安定だったため、戦略 A (per-slide navigate + Chrome 印刷 pdf)
//! に置き換える。
//!
//! 各 slide は `?slide=id.p<N>` クエリで個別表示できる (Phase 0 で発見)。1 枚ずつ
//! navigate して `set viewport 1280 720` + `pdf` で 16:9 1 page PDF を保存し、
//! caller (api::slides) で `lopdf` を使ってマージする。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::batch::{BatchBuilder, BatchResponse, LoadKind};
use crate::error::BrowserError;
use crate::process::AgentBrowser;

const SESSION_NAME: &str = "imoocs";

/// pubembed のページが描画完了 + Web フォントロード完了したかの判定。
/// Q13 の指針通り `document.fonts.ready` + `Array.from(document.images).every(complete)`
/// + svg page の DOM 存在で確認する。
const READY_JS: &str = r#"(function(){
if (typeof document === 'undefined' || !document.body) return false;
// punch viewer 自体が描画されているか
var svg = document.querySelector('svg.punch-viewer-svgpage, svg[viewBox]');
if (!svg) return false;
// フォントロードと画像ロード完了
try {
  var fontsReady = document.fonts && document.fonts.status === 'loaded';
  if (!fontsReady) return false;
} catch (e) { /* document.fonts 非対応ブラウザは無視 */ }
var imgs = Array.from(document.images || []);
if (imgs.length > 0 && !imgs.every(function(i){return i.complete;})) return false;
return true;
})()"#;

/// punch viewer の HTML 構造から slide 総枚数を推定する。pubembed が一覧で
/// 全 slide を render する仕様 (Phase 0 検証では 1 つしか出現しないケースもあるが
/// `viewerInfo` global を見れば確実) に基づいて多重 fallback を組む。
const COUNT_JS: &str = r#"(function(){
// 1. punch viewer の DOM 上の svg 数
var svgs = document.querySelectorAll('svg.punch-viewer-svgpage');
if (svgs.length > 0) return svgs.length;
// 2. viewerInfo global (Google Slides の punch viewer が公開している variable)
if (typeof window.viewerInfo === 'object' && Array.isArray(window.viewerInfo.slidesIds)) {
  return window.viewerInfo.slidesIds.length;
}
// 3. punch viewer の sidebar / counter UI から count を抜く
var counter = document.querySelector('.punch-viewer-svgpage-container, [aria-label*="スライド"]');
if (counter) {
  var m = (counter.getAttribute('aria-label') || '').match(/(\d+)\s*\/\s*(\d+)/);
  if (m) return parseInt(m[2], 10);
}
return 0;
})()"#;

/// pubembed の embed_url から slide 総枚数を取得する。
///
/// 0 が返った場合は枚数取得失敗 (DOM 構造が変わった可能性) として caller 側で
/// 単一 slide 扱いにフォールバックすることを想定。
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
/// `set viewport 1280 720` + `pdf <dest>` で 16:9 1page PDF を保存する。
///
/// embed_url に既存の `?` query が含まれていれば置換、無ければ末尾追加。
pub async fn fetch_slide_pdf(
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
        .pdf(dest)
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
            "fetch_slide_pdf({slide_index}): redirected to Google sign-in"
        )));
    }
    if let Some(failed) = outcomes.iter().find(|o| !o.success) {
        return Err(BrowserError::CommandFailed(format!(
            "fetch_slide_pdf({slide_index}): step {:?} failed: {}",
            failed.command,
            failed.error.as_deref().unwrap_or("unknown")
        )));
    }
    Ok(())
}

/// 高レベル: embed_url の全 slide を順次 PDF 化して `dest_dir/page_NNN.pdf` に保存する。
/// 戻り値は保存順 (= slide 順) の PathBuf 配列。
pub async fn fetch_slide_pdfs(
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
        let dest = dest_dir.join(format!("page_{i:03}.pdf"));
        fetch_slide_pdf(binary, embed_url, i, &dest).await?;
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
