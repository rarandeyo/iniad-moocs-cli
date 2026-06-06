//! Drive folder/file アクセス (Phase D-2 移行版)。
//!
//! 3 つの entry point:
//!
//! - [`list_drive_folder`] — `drive.google.com/drive/folders/<id>` を navigate して
//!   grid view (`tr[role="row"][data-id]`) から folder/file 一覧を抽出する。
//! - [`search_drive_folders`] — `drive.google.com/drive/search?q=<name>` を navigate して
//!   同じ grid 構造から folder のみフィルタする。
//! - [`fetch_drive_file`] — `drive.usercontent.google.com/download?id=<id>&export=download&confirm=t`
//!   を navigate して `wait --download <path>` で完了待ち、結果を 24h TTL でキャッシュする。
//!
//! 旧 reqwest 経路 (clients6.google.com/drive/v2beta/files + SAPISIDHASH 認証) は
//! Drive Web UI 自体が batchexecute (GWT RPC) に移行したことに加え、SAPISIDHASH の
//! 401 が頻発するため Phase D-2 で全削除した。

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use tracing::debug;

use crate::error::{ImoocsError, Result};
use crate::paths::Paths;
use crate::schemas::{DriveFileFetchResult, DriveFolderListing, DriveItem, DriveKind, DriveSearchResult};
use crate::session::Session;

const DRIVE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

// ─── public API ──────────────────────────────────────────────────────────────

/// `drive.google.com/drive/folders/<id>` を navigate して grid view から一覧抽出。
pub async fn list_drive_folder(_session: &Session, folder_id: &str) -> Result<DriveFolderListing> {
    validate_drive_id_or_root(folder_id)?;
    let binary = super::agent_binary()?;
    let raw_items = imoocs_browser::commands::drive::list_drive_folder(&binary, folder_id)
        .await
        .map_err(super::map_browser_err)?;
    let items: Vec<DriveItem> = raw_items.into_iter().map(convert_browser_item).collect();
    Ok(DriveFolderListing {
        folder_id: folder_id.to_string(),
        items,
        // 旧 envelope 互換。DOM scrape では grid view が全件を render するため常に false。
        truncated: false,
        fetched_at: now_rfc3339(),
    })
}

/// `drive.google.com/drive/search?q=<name>` を navigate して、結果から folder のみ
/// + 名前一致 (exact / partial) で client-side フィルタする。
pub async fn search_drive_folders(_session: &Session, name: &str, exact: bool) -> Result<DriveSearchResult> {
    let query_name = name.trim();
    if query_name.is_empty() {
        return Err(ImoocsError::Validation("Drive search query must not be empty".into()));
    }
    let binary = super::agent_binary()?;
    let raw_items = imoocs_browser::commands::drive::search_drive(&binary, query_name)
        .await
        .map_err(super::map_browser_err)?;
    let query_lower = query_name.to_lowercase();
    let items: Vec<DriveItem> = raw_items
        .into_iter()
        .map(convert_browser_item)
        .filter(|i| i.kind == DriveKind::Folder)
        .filter(|i| {
            if exact {
                i.name == query_name
            } else {
                i.name.to_lowercase().contains(&query_lower)
            }
        })
        .collect();
    Ok(DriveSearchResult {
        query: query_name.to_string(),
        exact,
        items,
        fetched_at: now_rfc3339(),
    })
}

/// Phase D-2 暫定: agent-browser の `wait --download` は Chrome のデフォルト download
/// directory (典型的に `~/Downloads`) に保存し、指定 path には書かない挙動だった
/// (実機 spike で確定)。daemon 起動時に `AGENT_BROWSER_DOWNLOAD_PATH` を渡せば任意の
/// directory に向けられるが、daemon の再起動が必要で挙動が安定しない。
///
/// 当面はユーザに `imoocs open https://drive.google.com/file/d/<id>/view` で
/// ブラウザを開いてもらう運用に倒し、fetch は明確にエラーで案内する。
/// 完全な解決は Phase D-2.x で daemon spawn を制御する別経路として扱う。
pub async fn fetch_drive_file(
    _session: &Session,
    paths: &Paths,
    file_id: &str,
    no_cache: bool,
) -> Result<DriveFileFetchResult> {
    validate_drive_id(file_id)?;
    if !no_cache {
        if let Some(hit) = try_cache(paths, file_id)? {
            debug!(%file_id, "drive cache hit");
            return Ok(hit);
        }
    }
    Err(ImoocsError::Validation(format!(
        "drive fetch は Phase D-2 移行中で一時無効化されています。\n\
         代替: ブラウザで開く → `imoocs open https://drive.google.com/file/d/{file_id}/view`"
    )))
}

// ─── private helpers ─────────────────────────────────────────────────────────

/// cache directory を脱出させるような id、あるいは URL / shell のメタ文字を
/// 含む id を拒否する。Drive ID は base64url 類似 (alphanumeric + `_-`)。
fn validate_drive_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(ImoocsError::Validation(format!(
            "invalid Drive ID {id:?}: must be 1..=128 chars of [A-Za-z0-9_-]"
        )));
    }
    Ok(())
}

fn validate_drive_id_or_root(id: &str) -> Result<()> {
    if id == "root" || id == "my-drive" {
        return Ok(());
    }
    validate_drive_id(id)
}

fn convert_browser_item(i: imoocs_browser::commands::drive::DriveItem) -> DriveItem {
    use imoocs_browser::commands::drive::DriveItemKind;
    let kind = match i.kind {
        DriveItemKind::Folder => DriveKind::Folder,
        DriveItemKind::File => DriveKind::File,
    };
    DriveItem {
        id: i.id,
        name: i.name,
        mime: infer_mime_from_tooltip(&i.tooltip, kind),
        kind,
        // grid view の別セルから取れるが、現状は省略。Phase D-2.x で必要なら抽出。
        modified_at: None,
    }
}

/// tooltip suffix から MIME を推定 (日本語ロケール依存)。
/// 一致しないものは `application/octet-stream` に倒す。
fn infer_mime_from_tooltip(tooltip: &str, kind: DriveKind) -> String {
    if kind == DriveKind::Folder {
        return "application/vnd.google-apps.folder".into();
    }
    if tooltip.contains("Google スライド") {
        return "application/vnd.google-apps.presentation".into();
    }
    if tooltip.contains("Google ドキュメント") {
        return "application/vnd.google-apps.document".into();
    }
    if tooltip.contains("Google スプレッドシート") {
        return "application/vnd.google-apps.spreadsheet".into();
    }
    if tooltip.contains("Google フォーム") {
        return "application/vnd.google-apps.form".into();
    }
    if tooltip.contains("PDF") {
        return "application/pdf".into();
    }
    "application/octet-stream".into()
}

fn try_cache(paths: &Paths, file_id: &str) -> Result<Option<DriveFileFetchResult>> {
    let meta_path = drive_meta_path(paths, file_id);
    if !meta_path.exists() {
        return Ok(None);
    }
    let age = fs::metadata(&meta_path)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or(Duration::ZERO))
        .unwrap_or(DRIVE_CACHE_TTL + Duration::from_secs(1));
    if age > DRIVE_CACHE_TTL {
        return Ok(None);
    }
    let meta: DriveCacheMeta = match fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(m) => m,
        None => return Ok(None),
    };
    let binary_path = paths.drive_dir().join(&meta.filename);
    let binary_meta = match fs::metadata(&binary_path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    // 整合性チェック: cache 済み binary の size が meta の記録と一致すること。
    // meta 読込中に並行 fetch が binary を truncate したケースを検出する
    if binary_meta.len() != meta.size_bytes {
        return Ok(None);
    }
    Ok(Some(DriveFileFetchResult {
        file_id: file_id.to_string(),
        filename: meta.filename,
        mime: meta.mime,
        local_path: binary_path,
        size_bytes: meta.size_bytes,
        fetched_at: meta.fetched_at,
        from_cache: true,
    }))
}

fn drive_meta_path(paths: &Paths, file_id: &str) -> PathBuf {
    paths.drive_dir().join(format!("{file_id}.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DriveCacheMeta {
    filename: String,
    #[serde(default)]
    mime: Option<String>,
    size_bytes: u64,
    fetched_at: String,
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "drive_tests.rs"]
mod tests;
