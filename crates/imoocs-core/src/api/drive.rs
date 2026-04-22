//! Google Drive file / folder access via the SAML-authenticated session.
//!
//! Two entry points:
//!
//! - [`list_drive_folder`] — GETs `drive.google.com/drive/folders/<id>`, pulls the
//!   `window['_DRIVE_ivd']` payload out of the returned HTML, and parses it into
//!   a [`DriveFolderListing`]. No Drive API / OAuth — the session's SAML cookie is
//!   enough for folders the user's INIAD Google account has access to.
//! - [`fetch_drive_file`] — GETs
//!   `drive.usercontent.google.com/download?id=<id>&export=download&confirm=t` and
//!   caches the response under `$XDG_CACHE_HOME/imoocs/drive/<fileId>.<ext>` with
//!   a 24h TTL. The pre-supplied `confirm=t` token bypasses the virus-scan
//!   interstitial that Drive shows for files >25MB.
//!
//! Endpoint / format references (checked 2026-04-22):
//! - New DL host documented by Drive community + tanaikech 2024-01:
//!   <https://gist.github.com/tanaikech/f0f2d122e05bf5f971611258c22c110f>
//! - `confirm` token is not validated server-side; any value works.
//! - Google native types (`application/vnd.google-apps.document|spreadsheet|presentation`)
//!   return empty from `drive.usercontent.google.com`; they must use
//!   `docs.google.com/<kind>/d/<id>/export?exportFormat=...` (v2).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::StatusCode;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::auth::is_logged_in_google;
use crate::error::{ImoocsError, Result};
use crate::paths::Paths;
use crate::schemas::{DriveFileFetchResult, DriveFolderListing, DriveItem, DriveKind};
use crate::session::Session;

const DRIVE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const FOLDER_PAGE_HINT_SIZE: usize = 50;
const NATIVE_MIME_PREFIX: &str = "application/vnd.google-apps.";
const DRIVE_FILE_FETCH_URL: &str = "https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t";

/// Matches `window['_DRIVE_ivd'] = '<payload>';` (single-quoted, `\x??` hex-escaped).
static IVD_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"window\[['"]_DRIVE_ivd['"]\]\s*=\s*'((?:[^'\\]|\\.)*)'"#).unwrap());

/// Matches `\xHH` escape sequences used in the IVD payload.
static HEX_ESCAPE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\x([0-9a-fA-F]{2})").unwrap());

/// Matches `filename="..."` in a `Content-Disposition` header.
static CONTENT_DISPOSITION_FILENAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"filename\s*=\s*"([^"]+)""#).unwrap());

/// Matches the virus-scan confirm interstitial (fallback; with `confirm=t` this
/// should basically never fire — kept as a safety net).
static CONFIRM_TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"name="confirm"\s+value="([^"]+)""#).unwrap());

// ---------- Shared helpers ----------

/// Reject IDs that could escape the cache directory or contain URL/shell
/// metacharacters. Drive IDs are base64url-like (alphanumeric + `_-`).
fn validate_drive_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(ImoocsError::Validation(format!(
            "invalid Drive ID {id:?}: must be 1..=128 chars of [A-Za-z0-9_-]"
        )));
    }
    Ok(())
}

/// Map HTTP status to a typed error. Used by both the initial fetch and the
/// virus-scan confirm retry so they share the exit-code policy (DESIGN §4.7).
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

/// Write `bytes` to `target` atomically by writing to a per-process tempfile
/// and renaming on success. Prevents partial-write races when two processes
/// fetch the same fileId concurrently.
fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".tmp".to_string());
    let tmp_path = target.with_file_name(format!("{name}.tmp.{}", std::process::id()));
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target).map_err(|e| {
        // Best-effort cleanup; ignore secondary error.
        let _ = fs::remove_file(&tmp_path);
        ImoocsError::Io(e)
    })?;
    Ok(())
}

// ---------- Folder listing ----------

/// List items in a Drive folder by scraping the rendered-HTML `_DRIVE_ivd` blob.
pub async fn list_drive_folder(session: &Session, folder_id: &str) -> Result<DriveFolderListing> {
    validate_drive_id(folder_id)?;
    if !is_logged_in_google(session).await? {
        return Err(ImoocsError::Auth {
            reason: "Google session required for Drive folder listing".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }

    let url = format!("https://drive.google.com/drive/folders/{folder_id}");
    info!(%url, "fetching Drive folder HTML");
    let resp = session.client.get(&url).send().await?;
    let final_url = resp.url().clone();
    if final_url.host_str() == Some("accounts.google.com") {
        return Err(ImoocsError::Auth {
            reason: "redirected to Google sign-in; SAML session expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    classify_drive_status(resp.status(), &format!("drive folder {folder_id}"))?;

    let body = resp.text().await?;
    // Defensive: if we got 200 but the body smells like a Google sign-in page,
    // treat it as an auth failure instead of a parse error.
    if looks_like_google_login(&body) {
        return Err(ImoocsError::Auth {
            reason: "Drive folder returned Google sign-in HTML; SAML session may have expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    let items = parse_ivd(&body)?;
    let truncated = items.len() >= FOLDER_PAGE_HINT_SIZE;
    Ok(DriveFolderListing {
        folder_id: folder_id.to_string(),
        items,
        truncated,
        fetched_at: now_rfc3339(),
    })
}

fn looks_like_google_login(body: &str) -> bool {
    let head = &body[..body.len().min(4096)].to_ascii_lowercase();
    head.contains("accounts.google.com/servicelogin") || head.contains("accounts.google.com/v3/signin")
}

/// Extract items from the `window['_DRIVE_ivd']` payload embedded in a Drive
/// folder HTML page.
///
/// Pure function (no I/O) so it is fixture-testable. See the positional shape
/// documented in the plan §(b): `[0]=id, [1]=[parentId], [2]=name, [3]=mime, [9]=modifiedMs`.
pub fn parse_ivd(html: &str) -> Result<Vec<DriveItem>> {
    let caps = IVD_RE
        .captures(html)
        .ok_or_else(|| ImoocsError::Parse("no window['_DRIVE_ivd'] payload in folder HTML".into()))?;
    let escaped = &caps[1];
    let decoded = unescape_ivd(escaped);
    let value: Value =
        serde_json::from_str(&decoded).map_err(|e| ImoocsError::Parse(format!("_DRIVE_ivd JSON parse failed: {e}")))?;

    let outer = value
        .as_array()
        .ok_or_else(|| ImoocsError::Parse("_DRIVE_ivd: expected top-level array".into()))?;
    let items_arr = outer
        .first()
        .and_then(Value::as_array)
        .ok_or_else(|| ImoocsError::Parse("_DRIVE_ivd: expected items at [0]".into()))?;

    let mut items = Vec::with_capacity(items_arr.len());
    for (idx, raw) in items_arr.iter().enumerate() {
        match parse_item(raw) {
            Some(it) => items.push(it),
            None => warn!(idx, "skipping _DRIVE_ivd item with unexpected positional shape"),
        }
    }
    // If the payload had items but none parsed, the positional shape has
    // probably changed server-side. Surfacing a Parse error is better than
    // silently returning an empty listing that an agent would read as
    // "folder is empty".
    if !items_arr.is_empty() && items.is_empty() {
        return Err(ImoocsError::Parse(
            "_DRIVE_ivd items all failed to parse; Drive payload shape may have changed".into(),
        ));
    }
    Ok(items)
}

fn parse_item(raw: &Value) -> Option<DriveItem> {
    let arr = raw.as_array()?;
    let id = arr.first()?.as_str()?.to_string();
    // arr[1] = [parentFolderId], unused for now.
    let name = arr.get(2)?.as_str()?.to_string();
    let mime = arr.get(3)?.as_str()?.to_string();
    let modified_at = arr.get(9).and_then(Value::as_i64).map(ms_to_rfc3339);
    let kind = if mime == "application/vnd.google-apps.folder" {
        DriveKind::Folder
    } else {
        DriveKind::File
    };
    Some(DriveItem {
        id,
        name,
        mime,
        kind,
        modified_at,
    })
}

fn unescape_ivd(payload: &str) -> String {
    let hex_resolved = HEX_ESCAPE_RE.replace_all(payload, |caps: &regex::Captures<'_>| {
        let h = &caps[1];
        u32::from_str_radix(h, 16)
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default()
    });
    hex_resolved.replace("\\/", "/")
}

fn ms_to_rfc3339(ms: i64) -> String {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as i128;
    let total_ns = (secs as i128) * 1_000_000_000 + nanos;
    match time::OffsetDateTime::from_unix_timestamp_nanos(total_ns) {
        Ok(dt) => dt
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

// ---------- Single file download ----------

/// Download a single Drive file into the local cache.
pub async fn fetch_drive_file(
    session: &Session,
    paths: &Paths,
    file_id: &str,
    no_cache: bool,
) -> Result<DriveFileFetchResult> {
    validate_drive_id(file_id)?;

    // Cache hit path: we need filename from a side-by-side meta JSON.
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

    // Detect non-binary responses: empty body, login HTML, virus-scan
    // interstitial, or any other HTML we should refuse to cache as a file.
    if let Some(retry) = detect_html_response(session, paths, file_id, content_type.as_deref(), &bytes).await? {
        return Ok(retry);
    }

    let filename = content_disposition
        .as_deref()
        .and_then(extract_filename)
        .unwrap_or_else(|| format!("{file_id}.bin"));

    save_drive_file(paths, file_id, &filename, content_type.as_deref(), &bytes)
}

/// Inspect an initial Drive response body. Returns `Ok(Some(result))` only if a
/// virus-scan retry succeeded with the server-provided `confirm` token.
/// Returns `Ok(None)` when the response is a real binary we can cache.
/// Errors on every other HTML shape (empty body, login redirect, unrecognised HTML).
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
    // `drive.usercontent.google.com` returns an empty body for Google native
    // types (Docs/Sheets/Slides). Treat anything HTML-ish smaller than 1 KB as
    // a native export failure so agents don't see a 0-byte success.
    if bytes.len() < 1024 {
        return Err(ImoocsError::Api(
            "Drive download returned empty/tiny HTML (<1KB). This is typically a Google native \
             type (Docs/Sheets/Slides) returned empty from drive.usercontent.google.com. \
             Native export is scheduled for v2; for pubembed Slides use `imoocs slide fetch`."
                .into(),
        ));
    }
    // Larger HTML — look for a Google login page (rare because we check
    // final_url earlier, but kept as a defence in depth).
    let head_lower = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    if head_lower.contains("accounts.google.com/servicelogin") || head_lower.contains("accounts.google.com/v3/signin") {
        return Err(ImoocsError::Auth {
            reason: "Drive download returned Google sign-in HTML; SAML session may have expired".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }
    // Virus-scan interstitial fallback. With `confirm=t` prebaked into the URL
    // this should basically never fire, but Google may tighten token validation
    // in the future.
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
    // Still HTML? give up to avoid loop / bad cache.
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
    // Refuse to write anything identified as a Google-native type — protects
    // against servers that succeed with 200 but empty body.
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
    // Write binary first (atomic tempfile + rename), then meta. If the process
    // dies between the two, try_cache will find no meta and re-download.
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
    // Integrity check: cached binary must match the size recorded in meta.
    // Catches the case where a concurrent fetch truncated the binary while we
    // were reading meta.
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

// Keep Path import used even under cfg(test) gating if tests are removed.
#[allow(dead_code)]
fn _ensure_path_type(_p: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/drive_folder_sample.html");

    #[test]
    fn parse_ivd_extracts_fixture_items() {
        let items = parse_ivd(FIXTURE).expect("parse_ivd");
        assert_eq!(items.len(), 4, "synthetic fixture should have 4 items");
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"sample-a.html"));
        assert!(names.contains(&"sample-b.pdf"));
        assert!(names.contains(&"sample-c.pdf"));
        assert!(names.contains(&"sample-d.zip"));

        let zip = items
            .iter()
            .find(|i| i.name == "sample-d.zip")
            .expect("find sample-d.zip");
        assert_eq!(zip.id, "FIXTURE_FILE_04_ZIP_________________");
        assert_eq!(zip.mime, "application/x-zip-compressed");
        assert_eq!(zip.kind, DriveKind::File);
        assert!(zip.modified_at.is_some());
    }

    #[test]
    fn parse_ivd_errors_when_payload_absent() {
        let html = "<html><body>no ivd here</body></html>";
        assert!(parse_ivd(html).is_err());
    }

    #[test]
    fn folder_mime_maps_to_folder_kind() {
        let raw = serde_json::json!([
            "FOLDER_ID",
            ["PARENT"],
            "subdir",
            "application/vnd.google-apps.folder",
            0,
            null,
            0,
            0,
            0,
            1_700_000_000_000_i64
        ]);
        let item = parse_item(&raw).expect("parse_item");
        assert_eq!(item.kind, DriveKind::Folder);
        assert_eq!(item.name, "subdir");
        assert_eq!(item.id, "FOLDER_ID");
    }

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
    fn unescape_ivd_decodes_hex_and_slash() {
        let input = r"\x5b\x221ABC\x22,\x22\/path\x22\x5d";
        let out = unescape_ivd(input);
        assert_eq!(out, r#"["1ABC","/path"]"#);
    }

    #[test]
    fn validate_drive_id_accepts_typical_ids() {
        validate_drive_id("FAKE_DRIVE_FILE_ID_HIST_REDACT001").unwrap();
        validate_drive_id("FAKE_DRIVE_FOLDER_ID_HIST_REDACT1").unwrap();
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

    #[test]
    fn parse_ivd_shape_change_surfaces_parse_error() {
        // items array is non-empty but each item is a string (not the expected
        // positional array). parse_item returns None for all items.
        let synthetic_payload = r"\x5b\x5b\x22not-an-item\x22\x5d\x5d";
        let html = format!("<html><body><script>window['_DRIVE_ivd'] = '{synthetic_payload}';</script></body></html>");
        let err = parse_ivd(&html).unwrap_err();
        match err {
            ImoocsError::Parse(msg) => {
                assert!(msg.contains("all failed to parse"), "msg was {msg:?}");
            }
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn parse_ivd_empty_items_is_still_ok() {
        let empty_payload = r"\x5b\x5b\x5d\x5d";
        let html = format!("<html><body><script>window['_DRIVE_ivd'] = '{empty_payload}';</script></body></html>");
        let items = parse_ivd(&html).expect("empty folder should parse");
        assert_eq!(items.len(), 0);
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
        // No leftover tempfile.
        let remaining: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(remaining.len(), 1, "expected only target file, got {remaining:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
