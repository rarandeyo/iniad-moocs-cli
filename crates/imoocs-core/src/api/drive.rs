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

fn build_folder_children_query(folder_id: &str) -> String {
    format!("'{folder_id}' in parents")
}

fn build_folder_name_query(name: &str, exact: bool) -> String {
    let escaped = drive_query_literal(name);
    let comparator = if exact { "=" } else { "contains" };
    format!("title {comparator} '{escaped}' and mimeType = 'application/vnd.google-apps.folder'")
}

pub async fn list_drive_folder(session: &Session, folder_id: &str) -> Result<DriveFolderListing> {
    list_drive_folder_at(session, folder_id, DRIVE_XHR_ENDPOINT).await
}

async fn list_drive_folder_at(session: &Session, folder_id: &str, endpoint: &str) -> Result<DriveFolderListing> {
    validate_drive_id(folder_id)?;
    if !is_logged_in_google(session).await? {
        return Err(ImoocsError::Auth {
            reason: "Google session required for Drive folder listing".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    let endpoint_url: reqwest::Url = endpoint
        .parse()
        .map_err(|e| ImoocsError::Validation(format!("invalid Drive XHR endpoint URL {endpoint:?}: {e}")))?;
    let auth = build_drive_authorization(session, &endpoint_url)?;
    let items = fetch_all_pages(&session.client, endpoint, &auth, folder_id).await?;
    Ok(DriveFolderListing {
        folder_id: folder_id.to_string(),
        items,
        // envelope 後方互換で残している。XHR pagination で常に全件取得するため実質 dead。
        truncated: false,
        fetched_at: now_rfc3339(),
    })
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

async fn fetch_all_pages(
    client: &reqwest::Client,
    endpoint: &str,
    auth: &str,
    folder_id: &str,
) -> Result<Vec<DriveItem>> {
    let query = build_folder_children_query(folder_id);
    let what = format!("drive folder {folder_id}");
    fetch_drive_query_pages(client, endpoint, auth, &query, &what).await
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
mod tests {
    use super::*;

    #[test]
    fn extract_filename_basic() {
        assert_eq!(
            extract_filename(r#"attachment; filename="ai-01.zip""#).as_deref(),
            Some("ai-01.zip")
        );
        assert_eq!(extract_filename("attachment").as_deref(), None);
    }

    #[test]
    fn extension_from_prefers_filename_suffix() {
        assert_eq!(extension_from("ai-01.zip", None).as_deref(), Some("zip"));
        assert_eq!(extension_from("weird", Some("application/pdf")).as_deref(), Some("pdf"));
        assert_eq!(extension_from("noext", None), None);
    }

    #[test]
    fn validate_drive_id_accepts_typical_ids() {
        validate_drive_id("FAKE_DRIVE_FILE_ID_FOR_TESTS_0001").unwrap();
        validate_drive_id("FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001").unwrap();
        validate_drive_id("abc").unwrap();
    }

    #[test]
    fn validate_drive_id_rejects_path_traversal() {
        for bad in &[
            "",
            "../../etc/passwd",
            "..",
            "a/b",
            "foo.bar",
            "has space",
            "with#hash",
            "with?q",
            &"x".repeat(129),
        ] {
            assert!(
                validate_drive_id(bad).is_err(),
                "validate_drive_id should reject {bad:?}"
            );
        }
    }

    const XHR_PAGE1_FIXTURE: &str = include_str!("../../tests/fixtures/drive_xhr_page1.json");
    const XHR_PAGE2_FIXTURE: &str = include_str!("../../tests/fixtures/drive_xhr_page2_last.json");

    #[test]
    fn sapisid_hash_known_answer() {
        let got = sapisid_hash(1_000_000_000, "SAMPLE_SAPISID", "https://drive.google.com");
        assert_eq!(got, "1000000000_f8e785b009b005421a7e7e2a5a40c6db42a37ac9");
    }

    #[test]
    fn build_folder_name_query_exact_escapes_literal() {
        let got = build_folder_name_query("Bob's \\ folder", true);
        assert_eq!(
            got,
            "title = 'Bob\\'s \\\\ folder' and mimeType = 'application/vnd.google-apps.folder'"
        );
    }

    #[test]
    fn build_folder_name_query_partial_uses_contains() {
        let got = build_folder_name_query("[受講生]講義資料", false);
        assert_eq!(
            got,
            "title contains '[受講生]講義資料' and mimeType = 'application/vnd.google-apps.folder'"
        );
    }

    #[test]
    fn parse_xhr_page1_returns_items_and_next_token() {
        let (items, next) = parse_xhr_page(XHR_PAGE1_FIXTURE).expect("page1 parse");
        assert_eq!(items.len(), 3);
        assert_eq!(next.as_deref(), Some("FIXTURE_TOKEN_PAGE_2"));
        assert_eq!(items[0].name, "AI-01");
        assert_eq!(items[0].kind, DriveKind::Folder);
        assert_eq!(items[0].mime, "application/vnd.google-apps.folder");
        assert_eq!(items[1].name, "handout.pdf");
        assert_eq!(items[1].kind, DriveKind::File);
        assert_eq!(items[1].mime, "application/pdf");
        assert_eq!(items[2].modified_at.as_deref(), Some("2026-04-03T12:00:00.000Z"));
    }

    #[test]
    fn parse_xhr_page2_terminates_without_next_token() {
        let (items, next) = parse_xhr_page(XHR_PAGE2_FIXTURE).expect("page2 parse");
        assert_eq!(items.len(), 2);
        assert!(next.is_none(), "last page should have no nextPageToken");
        assert_eq!(items[0].name, "notes.txt");
        assert!(items[0].modified_at.is_none());
        assert_eq!(items[1].name, "sub-folder");
        assert_eq!(items[1].kind, DriveKind::Folder);
    }

    #[test]
    fn parse_xhr_page_error_on_shape_change() {
        let bad = r#"{"items": ["not an object"], "nextPageToken": null}"#;
        let err = parse_xhr_page(bad).unwrap_err();
        match err {
            ImoocsError::Parse(m) => assert!(m.contains("Drive XHR endpoint may have changed"), "got {m:?}"),
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn classify_xhr_error_unregistered_caller_maps_to_api() {
        let body = r#"{"error":{"code":403,"message":"Method doesn't allow unregistered callers (callers without established identity). Please use API Key or other form of API consumer identity to call this API."}}"#;
        let err = classify_xhr_error(StatusCode::FORBIDDEN, body, "test folder");
        match err {
            ImoocsError::Api(m) => {
                assert!(m.contains("rejected our API key"), "got {m:?}");
                assert!(m.contains("rotated upstream"), "should hint at regression, got {m:?}");
            }
            other => panic!("expected Api error (API-key regression), got {other:?}"),
        }
    }

    #[test]
    fn classify_xhr_error_permission_denied_maps_to_auth() {
        let body = r#"{"error":{"code":403,"message":"The caller does not have permission"}}"#;
        let err = classify_xhr_error(StatusCode::FORBIDDEN, body, "test folder");
        assert!(
            matches!(err, ImoocsError::Auth { .. }),
            "expected Auth error, got {err:?}"
        );
    }

    #[test]
    fn classify_xhr_error_invalid_query_maps_to_api() {
        let body = r#"{"error":{"code":400,"message":"Invalid Value"}}"#;
        let err = classify_xhr_error(StatusCode::BAD_REQUEST, body, "test search");
        match err {
            ImoocsError::Api(m) => assert!(m.contains("Query semantics may have changed"), "got {m:?}"),
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[test]
    fn classify_xhr_error_404_maps_to_not_found() {
        let body = r#"{"error":{"code":404,"message":"File not found"}}"#;
        let err = classify_xhr_error(StatusCode::NOT_FOUND, body, "test folder");
        assert!(matches!(err, ImoocsError::NotFound { .. }), "got {err:?}");
    }

    /// page2 の mock は `pageToken=FIXTURE_TOKEN_PAGE_2` 限定で matching するので、
    /// loop が token を次 URL に連結できていないと 5 件揃わず test が落ちる。
    #[tokio::test]
    async fn fetch_all_pages_chains_page_tokens() {
        let mut server = mockito::Server::new_async().await;
        let page1_mock = server
            .mock("GET", "/drive/v2beta/files")
            .match_query(mockito::Matcher::UrlEncoded(
                "q".into(),
                "'FIXTURE_FOLDER' in parents".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json; charset=UTF-8")
            .with_body(XHR_PAGE1_FIXTURE)
            .expect(1)
            .create_async()
            .await;
        let page2_mock = server
            .mock("GET", "/drive/v2beta/files")
            .match_query(mockito::Matcher::UrlEncoded(
                "pageToken".into(),
                "FIXTURE_TOKEN_PAGE_2".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json; charset=UTF-8")
            .with_body(XHR_PAGE2_FIXTURE)
            .expect(1)
            .create_async()
            .await;

        let endpoint = format!("{}/drive/v2beta/files", server.url());
        let client = reqwest::Client::new();
        let items = fetch_all_pages(&client, &endpoint, "SAPISIDHASH fake", "FIXTURE_FOLDER")
            .await
            .expect("fetch_all_pages should succeed");

        assert_eq!(items.len(), 5);
        assert_eq!(items[0].name, "AI-01");
        assert_eq!(items[3].name, "notes.txt");
        assert_eq!(items[4].name, "sub-folder");

        page1_mock.assert_async().await;
        page2_mock.assert_async().await;
    }

    /// HTTP 403 + Google "unregistered callers" body が Api error として surfacing される
    /// (auth 切れと混同しない)。
    #[tokio::test]
    async fn fetch_all_pages_surfaces_api_key_regression() {
        let mut server = mockito::Server::new_async().await;
        let body = r#"{"error":{"code":403,"message":"Method doesn't allow unregistered callers (callers without established identity). Please use API Key or other form of API consumer identity to call this API."}}"#;
        let m = server
            .mock("GET", "/drive/v2beta/files")
            .match_query(mockito::Matcher::UrlEncoded(
                "q".into(),
                "'FIXTURE_FOLDER' in parents".into(),
            ))
            .with_status(403)
            .with_header("content-type", "application/json; charset=UTF-8")
            .with_body(body)
            .expect(1)
            .create_async()
            .await;

        let endpoint = format!("{}/drive/v2beta/files", server.url());
        let client = reqwest::Client::new();
        let err = fetch_all_pages(&client, &endpoint, "SAPISIDHASH fake", "FIXTURE_FOLDER")
            .await
            .unwrap_err();
        match err {
            ImoocsError::Api(s) => assert!(s.contains("rejected our API key"), "got {s:?}"),
            other => panic!("expected Api error, got {other:?}"),
        }
        m.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_drive_query_pages_uses_arbitrary_query() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/drive/v2beta/files")
            .match_query(mockito::Matcher::UrlEncoded(
                "q".into(),
                "title = '[受講生]講義資料' and mimeType = 'application/vnd.google-apps.folder'".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json; charset=UTF-8")
            .with_body(
                r#"{"items":[
                    {"id":"FOLDER_A","title":"[受講生]講義資料","mimeType":"application/vnd.google-apps.folder"},
                    {"id":"FILE_B","title":"ignore.pdf","mimeType":"application/pdf"}
                ]}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let endpoint = format!("{}/drive/v2beta/files", server.url());
        let client = reqwest::Client::new();
        let items = fetch_drive_query_pages(
            &client,
            &endpoint,
            "SAPISIDHASH fake",
            "title = '[受講生]講義資料' and mimeType = 'application/vnd.google-apps.folder'",
            "drive folder search",
        )
        .await
        .expect("fetch_drive_query_pages should succeed");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, DriveKind::Folder);
        assert_eq!(items[1].kind, DriveKind::File);

        m.assert_async().await;
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!("imoocs-atomic-write-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("file.bin");
        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second").unwrap();
        let got = fs::read(&target).unwrap();
        assert_eq!(got, b"second");
        let remaining: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(remaining.len(), 1, "expected only target file, got {remaining:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
