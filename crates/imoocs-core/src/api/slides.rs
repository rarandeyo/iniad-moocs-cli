//! Google Slides の pubembed を取得し、埋め込み SVG を抜き出して 1 つの PDF に
//! マージする。結果は `$XDG_CACHE_HOME/imoocs/slides/<sha1(embedUrl)>.pdf` にキャッシュ。
//!
//! SVG の抽出方法は moocs-collect `src/repository/slide.rs:56-113` を参考にしている。

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use base64::Engine;
use futures::stream::{self, StreamExt};
use once_cell::sync::Lazy;
use regex::Regex;
use sha1::{Digest, Sha1};
use tracing::{debug, info, warn};

use crate::auth::is_logged_in_google;
use crate::error::{ImoocsError, Result};
use crate::paths::Paths;
use crate::schemas::FetchStatus;
use crate::session::Session;

const SLIDES_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// moocs-collect 由来の regex: Google Slides が JS init payload 内に
/// エスケープ付きで埋め込む `<svg>...</svg>` シーケンスにマッチする。
static SVG_ESCAPED_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\x3csvg[\s\S]*?\\x3c\\/svg\\x3e").unwrap());

static XLINK_HTTPS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"xlink:href="(https://[^"]+)""#).unwrap());

#[derive(Debug)]
pub struct SlideFetchResult {
    pub local_pdf_path: PathBuf,
    pub size_bytes: u64,
    pub page_count: u32,
    pub fetched_at: String,
    pub from_cache: bool,
}

/// `fetch_best_effort` の結果。失敗しても exit を落とさない `lesson show` /
/// `open` のデフォルトフローで使う。`status` が `FetchStatus::Ok` 以外のときは
/// path / サイズなどのメタデータはすべて `None`。
#[derive(Debug)]
pub struct FetchOutcome {
    pub status: FetchStatus,
    pub local_pdf_path: Option<PathBuf>,
    pub size_bytes: Option<u64>,
    pub page_count: Option<u32>,
    pub fetched_at: Option<String>,
    pub from_cache: bool,
}

pub async fn fetch_slide_pdf(
    session: &Session,
    paths: &Paths,
    embed_url: &str,
    no_cache: bool,
) -> Result<SlideFetchResult> {
    fetch_slide_pdf_with_dump(session, paths, embed_url, no_cache, None).await
}

/// `fetch_slide_pdf` の薄い best-effort ラッパ。Google SSO が未ログインなら
/// `Skipped`、ネットワーク等の実エラーなら `Failed` に倒し、いずれも stderr に
/// `tracing::warn!` を 1 行出す (`--quiet` 時は env filter で抑制される)。
/// 呼び出し側は `FetchOutcome` を見て埋め込みメタデータを埋めるだけでよく、
/// `Result` を畳む必要がない。
pub async fn fetch_best_effort(session: &Session, paths: &Paths, embed_url: &str, no_cache: bool) -> FetchOutcome {
    let res = fetch_slide_pdf(session, paths, embed_url, no_cache).await;
    match &res {
        Ok(_) => {}
        Err(ImoocsError::Auth { .. }) => {
            warn!(
                %embed_url,
                "slide fetch skipped: Google session unavailable (run `imoocs auth login-google` to enable)"
            );
        }
        Err(e) => {
            warn!(%embed_url, error = %e, "slide fetch failed; continuing without PDF");
        }
    }
    classify_fetch_outcome(res)
}

fn classify_fetch_outcome(res: Result<SlideFetchResult>) -> FetchOutcome {
    match res {
        Ok(r) => FetchOutcome {
            status: FetchStatus::Ok,
            local_pdf_path: Some(r.local_pdf_path),
            size_bytes: Some(r.size_bytes),
            page_count: if r.page_count > 0 { Some(r.page_count) } else { None },
            fetched_at: Some(r.fetched_at),
            from_cache: r.from_cache,
        },
        Err(ImoocsError::Auth { .. }) => FetchOutcome {
            status: FetchStatus::Skipped,
            local_pdf_path: None,
            size_bytes: None,
            page_count: None,
            fetched_at: None,
            from_cache: false,
        },
        Err(_) => FetchOutcome {
            status: FetchStatus::Failed,
            local_pdf_path: None,
            size_bytes: None,
            page_count: None,
            fetched_at: None,
            from_cache: false,
        },
    }
}

/// 挙動は `fetch_slide_pdf` と同じだが、`dump_dir` が渡された場合は中間成果物
/// (pubembed 生 HTML と抽出 SVG 群) をそのディレクトリにも書く。
/// auth flow を再実装せずに「真っ白な PDF」問題を debug するのに便利。
pub async fn fetch_slide_pdf_with_dump(
    session: &Session,
    paths: &Paths,
    embed_url: &str,
    no_cache: bool,
    dump_dir: Option<&std::path::Path>,
) -> Result<SlideFetchResult> {
    let cache_path = cache_file(paths, embed_url);
    if !no_cache && dump_dir.is_none() {
        if let Some(res) = reuse_cache_if_fresh(&cache_path)? {
            debug!(path = %cache_path.display(), "slide cache hit");
            return Ok(res);
        }
    }

    if !is_logged_in_google(session).await? {
        return Err(ImoocsError::Auth {
            reason: "Google session required for slide download".into(),
            hint: Some("run `imoocs auth login-google`".into()),
        });
    }

    info!(%embed_url, "fetching slide pubembed for PDF synthesis");
    let body = session
        .client
        .get(embed_url)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| ImoocsError::Api(format!("pubembed request failed: {e}")))?
        .text()
        .await?;

    if let Some(dir) = dump_dir {
        fs::create_dir_all(dir)?;
        fs::write(dir.join("pubembed.html"), &body)?;
    }

    let svgs = extract_svgs(&body);
    if svgs.is_empty() {
        return Err(ImoocsError::Parse(
            "no SVG content found in pubembed; the slide may be non-public or \
             the page format may have changed"
                .into(),
        ));
    }

    if let Some(dir) = dump_dir {
        for (i, svg) in svgs.iter().enumerate() {
            fs::write(dir.join(format!("slide_{i:03}.svg")), svg)?;
        }
    }

    // https: の image 参照を base64 data URI に inline する。svg2pdf が
    // 外部画像を取得しない仕様なので、これをやらないと image 多めの
    // slide で出力 PDF が真っ白になる
    let svgs = inline_image_refs(session, &svgs).await?;

    if let Some(dir) = dump_dir {
        for (i, svg) in svgs.iter().enumerate() {
            fs::write(dir.join(format!("slide_inlined_{i:03}.svg")), svg)?;
        }
    }

    let pdf_bytes = svgs_to_pdf(&svgs)?;
    fs::create_dir_all(paths.slides_dir())?;
    fs::write(&cache_path, &pdf_bytes)?;

    Ok(SlideFetchResult {
        local_pdf_path: cache_path.clone(),
        size_bytes: pdf_bytes.len() as u64,
        page_count: svgs.len() as u32,
        fetched_at: now_rfc3339(),
        from_cache: false,
    })
}

pub fn cache_file(paths: &Paths, embed_url: &str) -> PathBuf {
    let mut hasher = Sha1::new();
    hasher.update(embed_url.as_bytes());
    let digest = hex::encode(hasher.finalize());
    paths.slides_dir().join(format!("{digest}.pdf"))
}

fn reuse_cache_if_fresh(path: &PathBuf) -> Result<Option<SlideFetchResult>> {
    if !path.exists() {
        return Ok(None);
    }
    let meta = fs::metadata(path)?;
    let modified = meta.modified()?;
    let age = SystemTime::now().duration_since(modified).unwrap_or(Duration::ZERO);
    if age > SLIDES_CACHE_TTL {
        return Ok(None);
    }
    Ok(Some(SlideFetchResult {
        local_pdf_path: path.clone(),
        size_bytes: meta.len(),
        // cache 再利用時は page count を数え直さない (速度優先)。
        // agent は PDF を直接読む想定なので 0 のままでよい
        page_count: 0,
        fetched_at: time::OffsetDateTime::from(modified)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        from_cache: true,
    }))
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn extract_svgs(body: &str) -> Vec<String> {
    SVG_ESCAPED_RE
        .find_iter(body)
        .map(|m| m.as_str().to_string())
        .map(|s| s.replace(r"\/", "/"))
        .filter_map(|s| unicode_escape::decode(&s).ok())
        .collect()
}

/// SVG 群の `xlink:href="https://..."` をすべて base64 data URI に置換する
/// (resource は実際に取得する)。同一 URL を参照する SVG 間でダウンロードを共有。
/// 取得失敗時は warn ログを出し、元の URL をそのまま残す。
async fn inline_image_refs(session: &Session, svgs: &[String]) -> Result<Vec<String>> {
    let mut urls: Vec<String> = Vec::new();
    for svg in svgs {
        for cap in XLINK_HTTPS_RE.captures_iter(svg) {
            let u = cap[1].to_string();
            if !urls.contains(&u) {
                urls.push(u);
            }
        }
    }
    if urls.is_empty() {
        return Ok(svgs.to_vec());
    }
    info!(count = urls.len(), "pre-fetching inlined slide images");

    type FetchOutcome = std::result::Result<(Vec<u8>, Option<String>), String>;
    let results: Vec<(String, FetchOutcome)> = stream::iter(urls.into_iter())
        .map(|url| async move {
            let out = async {
                let resp = session.client.get(&url).send().await?.error_for_status()?;
                let content_type = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
                let bytes = resp.bytes().await?.to_vec();
                Ok::<_, reqwest::Error>((bytes, content_type))
            }
            .await
            .map_err(|e| format!("{e}"));
            (url, out)
        })
        .buffer_unordered(6)
        .collect()
        .await;

    let mut cache: HashMap<String, String> = HashMap::new();
    for (url, res) in results {
        match res {
            Ok((bytes, ct)) => {
                let mime = detect_mime(ct.as_deref(), &bytes);
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                cache.insert(url, format!("data:{mime};base64,{b64}"));
            }
            Err(e) => {
                warn!(%url, error = %e, "failed to fetch inline slide image");
            }
        }
    }

    let out: Vec<String> = svgs
        .iter()
        .map(|svg| {
            XLINK_HTTPS_RE
                .replace_all(svg, |caps: &regex::Captures<'_>| {
                    let url = &caps[1];
                    match cache.get(url) {
                        Some(data_uri) => format!(r#"xlink:href="{data_uri}""#),
                        None => caps[0].to_string(),
                    }
                })
                .into_owned()
        })
        .collect();
    Ok(out)
}

fn detect_mime(header_ct: Option<&str>, bytes: &[u8]) -> &'static str {
    if let Some(ct) = header_ct {
        return match ct {
            "image/jpeg" | "image/jpg" => "image/jpeg",
            "image/png" => "image/png",
            "image/gif" => "image/gif",
            "image/webp" => "image/webp",
            "image/svg+xml" => "image/svg+xml",
            _ => sniff_mime(bytes),
        };
    }
    sniff_mime(bytes)
}

fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// 複数の SVG ページから 1 つのマルチページ PDF を合成する。
///
/// `svg2pdf::to_pdf` で各 SVG を単ページ PDF bytes にし、
/// `lopdf` の定番手法 (id を renumber して pages を harvest) でマージする。
fn svgs_to_pdf(svgs: &[String]) -> Result<Vec<u8>> {
    use svg2pdf::{usvg, ConversionOptions, PageOptions};

    let mut opts = usvg::Options::default();
    opts.fontdb_mut().load_system_fonts();

    let mut per_slide_pdfs: Vec<Vec<u8>> = Vec::with_capacity(svgs.len());
    for (i, svg) in svgs.iter().enumerate() {
        let tree = usvg::Tree::from_str(svg, &opts)
            .map_err(|e| ImoocsError::Parse(format!("usvg parse failed on slide {i}: {e}")))?;
        let bytes = svg2pdf::to_pdf(&tree, ConversionOptions::default(), PageOptions::default())
            .map_err(|e| ImoocsError::Internal(format!("svg2pdf conversion failed on slide {i}: {e}")))?;
        per_slide_pdfs.push(bytes);
    }

    if per_slide_pdfs.len() == 1 {
        return Ok(per_slide_pdfs.into_iter().next().unwrap());
    }

    merge_pdfs_lopdf(&per_slide_pdfs)
}

/// 複数の PDF bytes を lopdf で 1 つにマージする。上流の正規例
/// (lopdf/examples/merge.rs) を踏襲している。
fn merge_pdfs_lopdf(inputs: &[Vec<u8>]) -> Result<Vec<u8>> {
    use std::collections::BTreeMap;

    use lopdf::{dictionary, Document, Object, ObjectId};

    // 各 doc を load し object id を renumber して衝突を避ける
    let mut max_id: u32 = 1;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();

    for (i, bytes) in inputs.iter().enumerate() {
        let mut doc =
            Document::load_mem(bytes).map_err(|e| ImoocsError::Internal(format!("lopdf load slide {i}: {e}")))?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        documents_pages.extend(doc.get_pages().into_values().map(|object_id| {
            let value = doc.get_object(object_id).cloned().unwrap_or(Object::Null);
            (object_id, value)
        }));
        documents_objects.extend(doc.objects);
    }

    let mut document = Document::with_version("1.5");

    // 入力群から Catalog/Pages オブジェクトを検出する。
    // 中央の Pages entry は後で再構築するので集めるだけ
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;
    for (object_id, object) in documents_objects.iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => catalog_object = Some((catalog_object.map_or(*object_id, |(id, _)| id), object.clone())),
            b"Pages" => {
                if let Ok(dict) = object.as_dict() {
                    let mut dict = dict.clone();
                    dict.set("Parent", pages_object.as_ref().map_or(*object_id, |(id, _)| *id));
                    if let Some((_, Object::Dictionary(prev))) = pages_object.clone() {
                        dict.extend(&prev);
                    }
                    pages_object = Some((pages_object.map_or(*object_id, |(id, _)| id), Object::Dictionary(dict)));
                }
            }
            _ => {}
        }
    }

    let (pages_object_id, pages_object_value) = match pages_object {
        Some(p) => p,
        None => return Err(ImoocsError::Internal("no Pages object across merged PDFs".into())),
    };

    for (object_id, object) in documents_pages.iter() {
        if let Ok(dict) = object.as_dict() {
            let mut dict = dict.clone();
            dict.set("Parent", pages_object_id);
            documents_objects.insert(*object_id, Object::Dictionary(dict));
        }
    }

    let page_ids: Vec<Object> = documents_pages.keys().map(|id| Object::Reference(*id)).collect();
    let page_count = page_ids.len() as i64;

    let mut pages_dict = match pages_object_value {
        Object::Dictionary(d) => d,
        _ => dictionary!(),
    };
    pages_dict.set("Kids", page_ids);
    pages_dict.set("Count", page_count);
    pages_dict.set("Type", "Pages");

    document.objects.insert(pages_object_id, Object::Dictionary(pages_dict));

    let catalog_object_id = match catalog_object {
        Some((id, Object::Dictionary(mut dict))) => {
            dict.set("Pages", pages_object_id);
            document.objects.insert(id, Object::Dictionary(dict));
            id
        }
        _ => {
            let id = (max_id, 0);
            max_id += 1;
            let mut dict = dictionary!();
            dict.set("Type", "Catalog");
            dict.set("Pages", pages_object_id);
            document.objects.insert(id, Object::Dictionary(dict));
            id
        }
    };

    // 残りの全 object を取り込む (pages / catalog 自体は上で設定済みなので除外)
    for (id, obj) in documents_objects {
        if id == pages_object_id || id == catalog_object_id {
            continue;
        }
        document.objects.insert(id, obj);
    }

    document.trailer.set("Root", catalog_object_id);
    document.max_id = document.objects.keys().map(|(id, _)| *id).max().unwrap_or(max_id);
    document.renumber_objects();
    document.compress();

    let mut buf: Vec<u8> = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        document
            .save_to(&mut cursor)
            .map_err(|e| ImoocsError::Internal(format!("lopdf save: {e}")))?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_result() -> SlideFetchResult {
        SlideFetchResult {
            local_pdf_path: PathBuf::from("/tmp/slide.pdf"),
            size_bytes: 123,
            page_count: 4,
            fetched_at: "2026-04-24T00:00:00Z".into(),
            from_cache: false,
        }
    }

    #[test]
    fn classify_ok_fills_all_metadata() {
        let out = classify_fetch_outcome(Ok(ok_result()));
        assert_eq!(out.status, FetchStatus::Ok);
        assert_eq!(
            out.local_pdf_path.as_deref(),
            Some(std::path::Path::new("/tmp/slide.pdf"))
        );
        assert_eq!(out.size_bytes, Some(123));
        assert_eq!(out.page_count, Some(4));
        assert_eq!(out.fetched_at.as_deref(), Some("2026-04-24T00:00:00Z"));
        assert!(!out.from_cache);
    }

    #[test]
    fn classify_auth_error_maps_to_skipped() {
        let err = ImoocsError::Auth {
            reason: "Google session required".into(),
            hint: None,
        };
        let out = classify_fetch_outcome(Err(err));
        assert_eq!(out.status, FetchStatus::Skipped);
        assert!(out.local_pdf_path.is_none());
        assert!(out.size_bytes.is_none());
        assert!(out.fetched_at.is_none());
    }

    #[test]
    fn classify_network_error_maps_to_failed() {
        let err = ImoocsError::Network("dns lookup failed".into());
        let out = classify_fetch_outcome(Err(err));
        assert_eq!(out.status, FetchStatus::Failed);
        assert!(out.local_pdf_path.is_none());
    }

    #[test]
    fn classify_ok_with_zero_page_count_keeps_page_count_none() {
        let mut r = ok_result();
        r.page_count = 0;
        let out = classify_fetch_outcome(Ok(r));
        assert_eq!(out.status, FetchStatus::Ok);
        assert!(out.page_count.is_none(), "zero page count should not be emitted");
    }
}
