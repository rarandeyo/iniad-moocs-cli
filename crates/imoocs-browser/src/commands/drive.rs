//! Google Drive folder listing / search / file fetch via agent-browser.
//!
//! 旧 `clients6.google.com/drive/v2beta/files` の reqwest 経路は SAPISIDHASH
//! 認証が 401 になりやすく、Drive Web UI 自体も batchexecute (GWT RPC) に
//! 移行している。代わりに以下の経路に置換した:
//!
//! - **list** (`drive.google.com/drive/folders/<id>`): grid view の
//!   `<tr role="row" data-id="...">` 行から folder/file 一覧を抽出。
//! - **search** (`drive.google.com/drive/search?q=<query>`): 同じ grid 構造を
//!   流用 (folder 限定のフィルタは caller 側で `kind == Folder` で絞る)。
//! - **fetch** (`drive.usercontent.google.com/download?id=<id>&export=download&confirm=t`):
//!   `open` で navigate → `wait --download <dest>` で完了を待ち、destination
//!   に保存する。

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::batch::{BatchBuilder, BatchResponse, LoadKind};
use crate::error::BrowserError;
use crate::process::AgentBrowser;

const SESSION_NAME: &str = "imoocs";

#[derive(Debug, Clone)]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    pub kind: DriveItemKind,
    /// `data-tooltip` の生値 ("Classroom フォルダ", "Google スライド" 等)。
    /// ロケール依存。MIME 推定の hint 用に core 側に流す。
    pub tooltip: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveItemKind {
    Folder,
    File,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    id: String,
    name: String,
    tooltip: String,
    #[serde(rename = "isFolder")]
    is_folder: bool,
}

/// grid 行 (`tr[role="row"][data-id]`) を全部走査して
/// `{id, name, tooltip, isFolder}` を JSON 文字列で返す。
const EXTRACT_JS: &str = r#"(function(){
var rows = document.querySelectorAll('tr[role="row"][data-id]');
return JSON.stringify(Array.from(rows).map(function(tr){
  var id = tr.getAttribute('data-id');
  var tt = tr.querySelector('[data-tooltip]');
  var tooltip = tt ? (tt.getAttribute('data-tooltip') || '') : '';
  var name = '';
  var s = tr.querySelector('strong.DNoYtb');
  if (s) { name = s.textContent; }
  else if (tt) {
    var s2 = tt.querySelector('strong');
    if (s2) { name = s2.textContent; }
  }
  // TODO(locale): 日本語固定。"フォルダ" を tooltip に含むものを folder 扱い。
  var isFolder = /フォルダ/.test(tooltip);
  return { id: id, name: name, tooltip: tooltip, isFolder: isFolder };
}));
})()"#;

/// grid view が描画完了したかの判定 (row 出現 or 空フォルダの empty-state)。
const READY_JS: &str = r#"(function(){
if (document.querySelectorAll('tr[role="row"][data-id]').length > 0) return true;
var main = document.querySelector('[role="main"]');
if (!main) return false;
var t = (main.innerText || '').slice(0, 500);
return /ファイルがありません|ここにファイル|まだ何もありません|アイテムがありません|This folder is empty/.test(t);
})()"#;

/// `https://drive.google.com/drive/folders/<id>` を navigate して中身を抽出。
/// `folder_id == "root"` の場合は My Drive を開く。
pub async fn list_drive_folder(binary: &Path, folder_id: &str) -> Result<Vec<DriveItem>, BrowserError> {
    let url = if folder_id == "root" {
        "https://drive.google.com/drive/my-drive".to_string()
    } else {
        format!("https://drive.google.com/drive/folders/{folder_id}")
    };
    navigate_and_extract(binary, &url, "list_drive_folder").await
}

/// `https://drive.google.com/drive/search?q=<query>` を navigate して結果を抽出。
/// folder のみのフィルタは caller 側で `kind == Folder` で絞る。
pub async fn search_drive(binary: &Path, query: &str) -> Result<Vec<DriveItem>, BrowserError> {
    let encoded = percent_encode(query);
    let url = format!("https://drive.google.com/drive/search?q={encoded}");
    navigate_and_extract(binary, &url, "search_drive").await
}

/// Drive ファイルを `drive.usercontent.google.com/download` 経由でダウンロードして
/// `dest_dir` に保存する。`confirm=t` を仕込んでいるので 25MB 超ファイルの virus-scan
/// interstitial も回避できる。
///
/// 実装の制約 (D-2.x 実機検証で確定):
/// - daemon Chrome は download をデフォルト directory (`~/Downloads`) に保存し、
///   `wait --download <path>` の path コピーは機能しない (0.21.2)
/// - download directory は `AGENT_BROWSER_DOWNLOAD_PATH` で **daemon 起動時にのみ** 指定可能
/// - daemon を再起動すると Google session は cookie restore では復活しない
///
/// したがって: daemon を close → env 付きで再起動 → session を自動回復
/// (`ensure_google_session`) → download。`dest_dir` に元の filename で落ちる。
///
/// 戻り値は実際に保存された path (Content-Disposition 由来の filename 付き)。
pub async fn fetch_drive_file(binary: &Path, file_id: &str, dest_dir: &Path) -> Result<PathBuf, BrowserError> {
    std::fs::create_dir_all(dest_dir).map_err(|e| BrowserError::Internal(format!("mkdir: {e}")))?;

    // 既存 daemon を落とす (download path は起動時にしか効かないため)。
    let plain = AgentBrowser::new(binary.to_path_buf(), SESSION_NAME);
    let _ = plain.run(&["close"]).await;

    // env 付き agent の最初のコマンドで daemon が AGENT_BROWSER_DOWNLOAD_PATH 込みで起動する。
    let agent = AgentBrowser::new(binary.to_path_buf(), SESSION_NAME)
        .with_env("AGENT_BROWSER_DOWNLOAD_PATH", dest_dir.to_string_lossy());

    // daemon 再起動で session 全損 → auth-vault profile + SAML chain で回復。
    // (この内部の spawn は env なしだが、daemon は既に env 付きで起動済みなので接続するだけ)
    let _ = agent.run(&["get", "url"]).await; // daemon を env 付きで先に起動させる
    crate::commands::auth_google::ensure_google_session(binary).await?;

    let url = format!("https://drive.usercontent.google.com/download?id={file_id}&export=download&confirm=t");
    let mut builder = BatchBuilder::new();
    // Chrome は navigation がダウンロードに変換されると `open` が
    // net::ERR_ABORTED を返す。実際にはダウンロードは開始されているので、
    // open の失敗は無視して wait --download の成否だけで判定する。
    builder.open(&url).push([
        "wait".to_string(),
        "--download".to_string(),
        "--timeout".to_string(),
        "120000".to_string(),
    ]);
    let json = builder.to_json().map_err(BrowserError::from)?;
    let before = list_files(dest_dir);
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;
    let outcomes: BatchResponse = serde_json::from_value(value)?;

    let wait_outcome = outcomes
        .get(1)
        .ok_or_else(|| BrowserError::Internal("fetch_drive_file: missing wait outcome".into()))?;
    if !wait_outcome.success {
        return Err(BrowserError::CommandFailed(format!(
            "fetch_drive_file: wait --download failed: {}",
            wait_outcome.error.as_deref().unwrap_or("unknown")
        )));
    }

    // dest_dir に新しく現れたファイルが download 結果。
    let after = list_files(dest_dir);
    let new_files: Vec<&PathBuf> = after.iter().filter(|p| !before.contains(p)).collect();
    match new_files.as_slice() {
        [single] => Ok((*single).clone()),
        [] => Err(BrowserError::CommandFailed(
            "fetch_drive_file: wait --download succeeded but no new file appeared in download dir".into(),
        )),
        many => {
            // 複数現れたら最新 mtime を採用 (.crdownload 等の中間ファイルは除外済みの想定)
            let newest = many
                .iter()
                .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
                .expect("non-empty");
            Ok((**newest).clone())
        }
    }
}

/// `.crdownload` (ダウンロード中の中間ファイル) を除いた dir 直下のファイル一覧。
fn list_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .filter(|p| p.extension().map(|e| e != "crdownload").unwrap_or(true))
                .collect()
        })
        .unwrap_or_default()
}

/// list/search 共通: navigate → wait_fn(READY_JS) → eval(EXTRACT_JS) → get_url
/// → extracted JSON を Vec<DriveItem> に変換。
///
/// sign-in リダイレクトを検出した場合は `ensure_google_session` で 1 回だけ
/// 自動回復してリトライする (daemon 再起動後の session 全損対策)。
async fn navigate_and_extract(binary: &Path, url: &str, what: &str) -> Result<Vec<DriveItem>, BrowserError> {
    match navigate_and_extract_once(binary, url, what).await {
        Err(BrowserError::CommandFailed(msg)) if msg.contains("sign-in") => {
            tracing::info!(target: "imoocs_browser::drive", %what, "session expired; attempting recovery + retry");
            crate::commands::auth_google::ensure_google_session(binary).await?;
            navigate_and_extract_once(binary, url, what).await
        }
        other => other,
    }
}

async fn navigate_and_extract_once(binary: &Path, url: &str, what: &str) -> Result<Vec<DriveItem>, BrowserError> {
    let agent = AgentBrowser::new(binary.to_path_buf(), SESSION_NAME);
    let mut builder = BatchBuilder::new();
    builder
        .open(url)
        .wait_load(LoadKind::DomContentLoaded)
        .wait_fn(READY_JS, 30_000)
        .eval(EXTRACT_JS)
        .get_url();
    let json = builder.to_json().map_err(BrowserError::from)?;
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;
    let outcomes: BatchResponse = serde_json::from_value(value)?;

    // sign-in リダイレクトは wait_fn timeout より先に判定したいので、
    // step 失敗チェックの前に final_url を見る。
    let final_url = outcomes
        .last()
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if final_url.contains("accounts.google.com") {
        return Err(BrowserError::CommandFailed(format!(
            "{what}: navigation redirected to Google sign-in; session expired"
        )));
    }

    if let Some(failed) = outcomes.iter().find(|o| !o.success) {
        return Err(BrowserError::CommandFailed(format!(
            "{what}: step {:?} failed: {}",
            failed.command,
            failed.error.as_deref().unwrap_or("unknown")
        )));
    }

    let extracted = outcomes
        .get(3)
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("result"))
        .and_then(Value::as_str)
        .ok_or_else(|| BrowserError::Internal(format!("{what}: missing eval result")))?;
    let raw: Vec<RawItem> = serde_json::from_str(extracted)
        .map_err(|e| BrowserError::Internal(format!("{what}: parse extracted JSON: {e}")))?;

    Ok(raw
        .into_iter()
        .map(|r| DriveItem {
            id: r.id,
            name: r.name,
            kind: if r.is_folder {
                DriveItemKind::Folder
            } else {
                DriveItemKind::File
            },
            tooltip: r.tooltip,
        })
        .collect())
}

/// URL の query value 用の percent encoding (RFC 3986 unreserved set 以外をエスケープ)。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_alphanum_passthrough() {
        assert_eq!(percent_encode("abc123"), "abc123");
        assert_eq!(percent_encode("hello-world_x.y~z"), "hello-world_x.y~z");
    }

    #[test]
    fn percent_encode_space_and_unicode() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(
            percent_encode("[受講生]講義資料"),
            "%5B%E5%8F%97%E8%AC%9B%E7%94%9F%5D%E8%AC%9B%E7%BE%A9%E8%B3%87%E6%96%99"
        );
    }
}
