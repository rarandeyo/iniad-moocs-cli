//! Google Drive folder listing via agent-browser DOM scrape (Phase D-2).
//!
//! 旧 `clients6.google.com/drive/v2beta/files` の reqwest 経路は SAPISIDHASH
//! 認証が 401 になりやすく、Drive Web UI 自体も batchexecute (GWT RPC) に
//! 移行している。代わりに `drive.google.com/drive/folders/<id>` を navigate
//! して grid view の `<tr role="row" data-id="...">` 行から folder/file 一覧を抽出する。

use std::path::Path;

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

    let agent = AgentBrowser::new(binary.to_path_buf(), SESSION_NAME);
    let mut builder = BatchBuilder::new();
    builder
        .open(&url)
        .wait_load(LoadKind::DomContentLoaded)
        .wait_fn(READY_JS, 30_000)
        .eval(EXTRACT_JS)
        .get_url();
    let json = builder.to_json().map_err(BrowserError::from)?;
    let value: Value = agent.run_raw(&["batch"], Some(json.as_bytes())).await?;
    let outcomes: BatchResponse = serde_json::from_value(value)?;

    if let Some(failed) = outcomes.iter().find(|o| !o.success) {
        return Err(BrowserError::CommandFailed(format!(
            "list_drive_folder: step {:?} failed: {}",
            failed.command,
            failed.error.as_deref().unwrap_or("unknown")
        )));
    }

    // get_url (最後) で sign-in リダイレクト検知
    let final_url = outcomes
        .last()
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if final_url.contains("accounts.google.com") {
        return Err(BrowserError::CommandFailed(
            "Drive folder navigation redirected to Google sign-in; session expired".into(),
        ));
    }

    // eval (index=3) の result.result が `JSON.stringify` した文字列
    let extracted = outcomes
        .get(3)
        .and_then(|o| o.result.as_ref())
        .and_then(|v| v.get("result"))
        .and_then(Value::as_str)
        .ok_or_else(|| BrowserError::Internal("list_drive_folder: missing eval result".into()))?;
    let raw: Vec<RawItem> = serde_json::from_str(extracted)
        .map_err(|e| BrowserError::Internal(format!("list_drive_folder: parse extracted JSON: {e}")))?;

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
