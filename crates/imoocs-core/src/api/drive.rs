//! SAML 認証済み session を使った Google Drive のファイル / フォルダアクセス。
//!
//! 3 つの entry point:
//!
//! - [`list_drive_folder`] — folder id 配下の children を XHR で列挙し、
//!   [`DriveFolderListing`] に parse する。
//! - [`search_drive_folders`] — folder 名を XHR query で検索し、
//!   [`DriveSearchResult`] に parse する。
//! - [`fetch_drive_file`] —
//!   `drive.usercontent.google.com/download?id=<id>&export=download&confirm=t` を GET し、
//!   結果を `$XDG_CACHE_HOME/imoocs/drive/<fileId>.<ext>` に 24h TTL でキャッシュする。
//!   URL に事前に `confirm=t` を仕込んでおくことで、25MB 超のファイルで Drive が
//!   表示する virus-scan の interstitial を回避する。
//!
//! endpoint / 仕様の参照先 (2026-04-22 時点で確認済み):
//! - 新しい DL ホストについては Drive コミュニティ + tanaikech 2024-01 の gist:
//!   <https://gist.github.com/tanaikech/f0f2d122e05bf5f971611258c22c110f>
//! - `confirm` token は server 側で検証されない (値は何でもよい)。
//! - Google native 型 (`application/vnd.google-apps.document|spreadsheet|presentation`)
//!   は `drive.usercontent.google.com` から空レスポンスになるため、
//!   `docs.google.com/<kind>/d/<id>/export?exportFormat=...` を使う必要がある (v2 で対応予定)。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::StatusCode;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use tracing::{debug, info, warn};

use crate::auth::is_logged_in_google;
use crate::error::{ImoocsError, Result};
use crate::paths::Paths;
use crate::schemas::{DriveFileFetchResult, DriveFolderListing, DriveItem, DriveKind, DriveSearchResult};
use crate::session::Session;

const DRIVE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const NATIVE_MIME_PREFIX: &str = "application/vnd.google-apps.";
const DRIVE_FILE_FETCH_URL: &str = "https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t";

/// Drive Web UI が叩く非公式 XHR endpoint。`drive.google.com/drive/v2beta/files`
/// は 404、`content.googleapis.com` は OAuth 必須 (spike 済)。clients6 だけが通る。
const DRIVE_XHR_ENDPOINT: &str = "https://clients6.google.com/drive/v2beta/files";
const DRIVE_XHR_ORIGIN: &str = "https://drive.google.com";
/// Drive Web UI HTML に埋め込まれた公開 API key (secret ではなく app 識別子)。
/// rotate されたら 403 "unregistered callers" で検知できる。
const DRIVE_XHR_API_KEY: &str = "AIzaSyD_InbmSFufIEps5UAt2NmB_3LvBH3Sz_8";
const DRIVE_XHR_PAGE_SIZE: usize = 1000;
const DRIVE_XHR_MAX_PAGES: usize = 100;

static CONTENT_DISPOSITION_FILENAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"filename\s*=\s*"([^"]+)""#).unwrap());

/// virus-scan confirm interstitial にマッチする (fallback。`confirm=t` を
/// URL に仕込んでいる限り発火しないが、安全網として残す)。
static CONFIRM_TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"name="confirm"\s+value="([^"]+)""#).unwrap());

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

/// HTTP status を型付きエラーに変換する。初回 fetch と virus-scan confirm
/// retry の双方で使うので、同一 exit-code ポリシーになる。
fn classify_drive_status(status: StatusCode, what: &str) -> Result<()> {
    match status.as_u16() {
        200 => Ok(()),
        404 => Err(ImoocsError::NotFound { what: what.to_string() }),
        403 => Err(ImoocsError::Auth {
            reason: format!("access denied to {what}"),
            hint: Some("make sure the INIAD account you logged in with has access to this resource".into()),
        }),
        s if (500..600).contains(&s) => Err(ImoocsError::Api(format!("{what} returned {s}"))),
        other => Err(ImoocsError::Api(format!("unexpected status {other} from {what}"))),
    }
}

/// `bytes` を `target` に atomic に書く: プロセスごとの tempfile に書いて、
/// 成功時に rename する。同一 fileId を 2 プロセスが並行取得した場合の
/// 部分書き込みレースを防ぐ。
fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".tmp".to_string());
    let tmp_path = target.with_file_name(format!("{name}.tmp.{}", std::process::id()));
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target).map_err(|e| {
        // best-effort cleanup。2 次エラーは無視する
        let _ = fs::remove_file(&tmp_path);
        ImoocsError::Io(e)
    })?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveXhrPage {
    #[serde(default)]
    next_page_token: Option<String>,
    #[serde(default)]
    items: Vec<DriveXhrItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveXhrItem {
    id: String,
    title: String,
    mime_type: String,
    #[serde(default)]
    modified_date: Option<String>,
}

impl From<DriveXhrItem> for DriveItem {
    fn from(raw: DriveXhrItem) -> Self {
        let kind = if raw.mime_type == "application/vnd.google-apps.folder" {
            DriveKind::Folder
        } else {
            DriveKind::File
        };
        DriveItem {
            id: raw.id,
            name: raw.title,
            mime: raw.mime_type,
            kind,
            modified_at: raw.modified_date,
        }
    }
}

/// `{ts}_{hex_sha1("{ts} {sapisid} {origin}")}`. Google Web UI 共通の非公式 scheme。
fn sapisid_hash(ts: u64, sapisid: &str, origin: &str) -> String {
    let mut h = Sha1::new();
    h.update(format!("{ts} {sapisid} {origin}"));
    format!("{ts}_{}", hex::encode(h.finalize()))
}

fn build_drive_authorization(session: &Session, request_url: &reqwest::Url) -> Result<String> {
    let sapisid = session
        .cookie_value_for(request_url, "SAPISID")
        .ok_or_else(|| ImoocsError::Auth {
            reason: "SAPISID cookie absent for Drive XHR URL; Google SAML session incomplete or expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        })?;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(format!("SAPISIDHASH {}", sapisid_hash(ts, &sapisid, DRIVE_XHR_ORIGIN)))
}

/// XHR 固有の status → エラー分類。403 は body の Google error message を見て、
/// "unregistered callers" / "API key" 系なら API key rotation / endpoint drift として
/// `ImoocsError::Api` に昇格させる。それ以外の 403 は従来通り Auth。
fn classify_xhr_error(status: StatusCode, body: &str, what: &str) -> ImoocsError {
    let err_msg = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    match status.as_u16() {
        400 => ImoocsError::Api(format!(
            "Drive XHR rejected our query at {what}: {err_msg}. Query semantics may have changed upstream."
        )),
        403 if err_msg.contains("unregistered callers")
            || err_msg.contains("API key not valid")
            || err_msg.contains("API consumer identity") =>
        {
            ImoocsError::Api(format!(
                "Drive XHR rejected our API key at {what}: {err_msg}. \
                 Endpoint/key may have rotated upstream."
            ))
        }
        401 | 403 => ImoocsError::Auth {
            reason: format!("access denied to {what}: {err_msg}"),
            hint: Some("make sure the INIAD account you logged in with has access to this resource".into()),
        },
        404 => ImoocsError::NotFound { what: what.to_string() },
        s if (500..600).contains(&s) => ImoocsError::Api(format!("{what} returned {s}: {err_msg}")),
        other => ImoocsError::Api(format!("unexpected status {other} from {what}: {err_msg}")),
    }
}

fn parse_xhr_page(body: &str) -> Result<(Vec<DriveItem>, Option<String>)> {
    let page: DriveXhrPage = serde_json::from_str(body).map_err(|e| {
        ImoocsError::Parse(format!(
            "drive v2beta: JSON shape changed upstream ({e}). Drive XHR endpoint may have changed."
        ))
    })?;
    let items = page.items.into_iter().map(DriveItem::from).collect();
    Ok((items, page.next_page_token))
}

fn drive_query_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn build_folder_name_query(name: &str, exact: bool) -> String {
    let escaped = drive_query_literal(name);
    let comparator = if exact { "=" } else { "contains" };
    format!("title {comparator} '{escaped}' and mimeType = 'application/vnd.google-apps.folder'")
}

/// Phase D-2: `drive.google.com/drive/folders/<id>` を navigate して
/// grid view の `tr[role="row"][data-id]` から folder/file 一覧を抽出する。
/// 旧 reqwest 経路 (clients6.google.com/drive/v2beta/files) は削除。
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
        // DOM の別セルに表示されているが、現状は省略。Phase D-2.x で必要なら抽出。
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

pub async fn search_drive_folders(session: &Session, name: &str, exact: bool) -> Result<DriveSearchResult> {
    search_drive_folders_at(session, name, exact, DRIVE_XHR_ENDPOINT).await
}

async fn search_drive_folders_at(
    session: &Session,
    name: &str,
    exact: bool,
    endpoint: &str,
) -> Result<DriveSearchResult> {
    let query_name = name.trim();
    if query_name.is_empty() {
        return Err(ImoocsError::Validation("Drive search query must not be empty".into()));
    }
    if !is_logged_in_google(session).await? {
        return Err(ImoocsError::Auth {
            reason: "Google session required for Drive folder search".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    let endpoint_url: reqwest::Url = endpoint
        .parse()
        .map_err(|e| ImoocsError::Validation(format!("invalid Drive XHR endpoint URL {endpoint:?}: {e}")))?;
    let auth = build_drive_authorization(session, &endpoint_url)?;
    let search_query = build_folder_name_query(query_name, exact);
    let what = format!("drive folder search for {:?}", query_name);
    let mut items = fetch_drive_query_pages(&session.client, endpoint, &auth, &search_query, &what).await?;
    items.retain(|item| item.kind == DriveKind::Folder);
    Ok(DriveSearchResult {
        query: query_name.to_string(),
        exact,
        items,
        fetched_at: now_rfc3339(),
    })
}

async fn fetch_drive_query_pages(
    client: &reqwest::Client,
    endpoint: &str,
    auth: &str,
    query: &str,
    what: &str,
) -> Result<Vec<DriveItem>> {
    let mut all = Vec::<DriveItem>::new();
    let mut page_token: Option<String> = None;
    for iter in 0..DRIVE_XHR_MAX_PAGES {
        let mut query: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("pageSize", DRIVE_XHR_PAGE_SIZE.to_string()),
            (
                "fields",
                "nextPageToken,items(id,title,mimeType,modifiedDate)".to_string(),
            ),
            ("supportsAllDrives", "true".to_string()),
            ("includeItemsFromAllDrives", "true".to_string()),
            ("key", DRIVE_XHR_API_KEY.to_string()),
        ];
        if let Some(t) = &page_token {
            query.push(("pageToken", t.clone()));
        }
        info!(%endpoint, %what, iter, "fetching Drive XHR page");
        let resp = client
            .get(endpoint)
            .query(&query)
            .header(reqwest::header::AUTHORIZATION, auth)
            .header("X-Origin", DRIVE_XHR_ORIGIN)
            .header(reqwest::header::REFERER, "https://drive.google.com/")
            .send()
            .await?;
        let final_url = resp.url().clone();
        if final_url.host_str() == Some("accounts.google.com") {
            return Err(ImoocsError::Auth {
                reason: "Drive XHR redirected to Google sign-in; SAML session expired".into(),
                hint: Some("run `imoocs auth login-google`".into()),
            });
        }
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(classify_xhr_error(status, &body, &format!("{what} (page {iter})")));
        }
        let (page_items, next) = parse_xhr_page(&body)?;
        debug!(iter, count = page_items.len(), "drive xhr page received");
        all.extend(page_items);
        match next {
            Some(t) => {
                page_token = Some(t);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            None => return Ok(all),
        }
    }
    Err(ImoocsError::Api(format!(
        "{what} hit safety cap ({} pages) without terminating; Drive XHR endpoint may have changed",
        DRIVE_XHR_MAX_PAGES,
    )))
}

pub async fn fetch_drive_file(
    session: &Session,
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

    if !is_logged_in_google(session).await? {
        return Err(ImoocsError::Auth {
            reason: "Google session required for Drive download".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }

    let url = DRIVE_FILE_FETCH_URL.replace("{id}", file_id);
    info!(%url, "fetching Drive file");
    let resp = session.client.get(&url).send().await?;
    let final_url = resp.url().clone();
    if final_url.host_str() == Some("accounts.google.com") {
        return Err(ImoocsError::Auth {
            reason: "redirected to Google sign-in; SAML session expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    classify_drive_status(resp.status(), &format!("drive file {file_id}"))?;

    let content_disposition = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    let bytes = resp.bytes().await?;

    // バイナリでないレスポンスの検出: 空 body / ログイン HTML /
    // virus-scan interstitial などはファイルとしてキャッシュしない
    if let Some(retry) = detect_html_response(session, paths, file_id, content_type.as_deref(), &bytes).await? {
        return Ok(retry);
    }

    let filename = content_disposition
        .as_deref()
        .and_then(extract_filename)
        .unwrap_or_else(|| format!("{file_id}.bin"));

    save_drive_file(paths, file_id, &filename, content_type.as_deref(), &bytes)
}

/// 初回 Drive レスポンス body を解析する。server 提供の `confirm` token で
/// virus-scan retry が成功した場合のみ `Ok(Some(result))` を返す。
/// cache 可能な実バイナリなら `Ok(None)`。
/// それ以外の HTML (空 body / login リダイレクト / 不明な HTML) はすべてエラーにする。
async fn detect_html_response(
    session: &Session,
    paths: &Paths,
    file_id: &str,
    content_type: Option<&str>,
    bytes: &[u8],
) -> Result<Option<DriveFileFetchResult>> {
    let is_html = matches!(content_type, Some("text/html"));
    if !is_html {
        return Ok(None);
    }
    // `drive.usercontent.google.com` は Google native 型 (Docs/Sheets/Slides)
    // に対して空 body を返すため、1KB 未満の HTML ライクレスポンスは native
    // export 失敗扱いにして 0byte 成功を agent に見せない
    if bytes.len() < 1024 {
        return Err(ImoocsError::Api(
            "Drive download returned empty/tiny HTML (<1KB). This is typically a Google native \
             type (Docs/Sheets/Slides) returned empty from drive.usercontent.google.com. \
             Native export is scheduled for v2; for pubembed Slides use `imoocs slide fetch`."
                .into(),
        ));
    }
    // 1KB 以上の HTML: Google ログインページを検出する (final_url で事前
    // チェック済みなので稀だが defence in depth として残す)
    let head_lower = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    if head_lower.contains("accounts.google.com/servicelogin") || head_lower.contains("accounts.google.com/v3/signin") {
        return Err(ImoocsError::Auth {
            reason: "Drive download returned Google sign-in HTML; SAML session may have expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    // virus-scan interstitial fallback。URL に `confirm=t` を埋めているので
    // 通常は発火しないが、将来 Google が token 検証を厳格化した場合に備える
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    if let Some(confirm_token) = CONFIRM_TOKEN_RE
        .captures(&head)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
    {
        warn!(
            token = %confirm_token,
            "hit virus-scan interstitial; retrying with server-provided confirm token"
        );
        return fetch_with_confirm(session, paths, file_id, &confirm_token)
            .await
            .map(Some);
    }
    Err(ImoocsError::Api(
        "Drive download returned HTML of unexpected shape; refusing to cache as binary. \
         This may be a Google-native type (Docs/Sheets/Slides) — v2 will add native export."
            .into(),
    ))
}

async fn fetch_with_confirm(
    session: &Session,
    paths: &Paths,
    file_id: &str,
    token: &str,
) -> Result<DriveFileFetchResult> {
    let url = format!("https://drive.usercontent.google.com/download?id={file_id}&export=download&confirm={token}");
    let resp = session.client.get(&url).send().await?;
    let final_url = resp.url().clone();
    if final_url.host_str() == Some("accounts.google.com") {
        return Err(ImoocsError::Auth {
            reason: "Drive confirm-retry redirected to sign-in; SAML session expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    classify_drive_status(resp.status(), &format!("drive file {file_id} (confirm retry)"))?;

    let content_disposition = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
    let bytes = resp.bytes().await?;
    // retry してもまだ HTML ならループ / 不正キャッシュを避けるため諦める
    if matches!(content_type.as_deref(), Some("text/html")) {
        return Err(ImoocsError::Api(
            "Drive confirm-retry still returned HTML; refusing to cache".into(),
        ));
    }
    let filename = content_disposition
        .as_deref()
        .and_then(extract_filename)
        .unwrap_or_else(|| format!("{file_id}.bin"));
    save_drive_file(paths, file_id, &filename, content_type.as_deref(), &bytes)
}

fn save_drive_file(
    paths: &Paths,
    file_id: &str,
    filename: &str,
    mime: Option<&str>,
    bytes: &[u8],
) -> Result<DriveFileFetchResult> {
    // Google native 型と判定されたものは書き込みを拒否する。
    // 200 + 空 body を返すサーバから守るため
    if let Some(m) = mime {
        if m.starts_with(NATIVE_MIME_PREFIX) && m != "application/vnd.google-apps.folder" {
            return Err(ImoocsError::Api(format!(
                "Google native type {m} not supported; v2 will add `docs.google.com/.../export`"
            )));
        }
    }

    fs::create_dir_all(paths.drive_dir())?;
    let ext = extension_from(filename, mime);
    let binary_path = paths.drive_dir().join(match ext {
        Some(e) => format!("{file_id}.{e}"),
        None => format!("{file_id}.bin"),
    });
    // binary を先に書く (atomic tempfile + rename)、次に meta。途中で
    // プロセスが死んでも try_cache 側は meta を見つけられず再取得する
    atomic_write(&binary_path, bytes)?;

    let meta = DriveCacheMeta {
        filename: filename.to_string(),
        mime: mime.map(str::to_string),
        size_bytes: bytes.len() as u64,
        fetched_at: now_rfc3339(),
    };
    let meta_path = drive_meta_path(paths, file_id);
    let meta_json = serde_json::to_string_pretty(&meta)?;
    atomic_write(&meta_path, meta_json.as_bytes())?;

    Ok(DriveFileFetchResult {
        file_id: file_id.to_string(),
        filename: meta.filename,
        mime: meta.mime,
        local_path: binary_path,
        size_bytes: meta.size_bytes,
        fetched_at: meta.fetched_at,
        from_cache: false,
    })
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
    let ext = extension_from(&meta.filename, meta.mime.as_deref());
    let binary_path = paths.drive_dir().join(match ext {
        Some(e) => format!("{file_id}.{e}"),
        None => format!("{file_id}.bin"),
    });
    let binary_meta = match fs::metadata(&binary_path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    // 整合性チェック: cache 済み binary の size が meta の記録と一致すること。
    // meta 読込中に並行 fetch が binary を truncate したケースを検出する
    if binary_meta.len() != meta.size_bytes {
        warn!(
            path = %binary_path.display(),
            expected = meta.size_bytes,
            actual = binary_meta.len(),
            "drive cache binary size mismatch — ignoring cache"
        );
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

fn extract_filename(header: &str) -> Option<String> {
    CONTENT_DISPOSITION_FILENAME_RE
        .captures(header)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn extension_from(filename: &str, mime: Option<&str>) -> Option<String> {
    if let Some(dot) = filename.rfind('.') {
        let ext = &filename[dot + 1..];
        if !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(ext.to_ascii_lowercase());
        }
    }
    if let Some(m) = mime {
        if let Some(exts) = mime_guess::get_mime_extensions_str(m) {
            return exts.first().map(|s| s.to_string());
        }
    }
    None
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "drive_tests.rs"]
mod tests;
