//! Fetch a Google Slides pubembed, extract embedded SVGs, and merge them into a
//! single PDF cached under `$XDG_CACHE_HOME/imoocs/slides/<sha1(embedUrl)>.pdf`.
//!
//! SVG extraction approach adapted from moocs-collect `src/repository/slide.rs:56-113`.

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
use crate::session::Session;

const SLIDES_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Regex from moocs-collect: matches the escape-encoded <svg>...</svg>
/// sequences that Google Slides embeds inside its JS init payload.
static SVG_ESCAPED_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\\x3csvg[\s\S]*?\\x3c\\/svg\\x3e").unwrap());

/// Regex to find `xlink:href="https://..."` image refs inside an SVG.
static XLINK_HTTPS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"xlink:href="(https://[^"]+)""#).unwrap());

#[derive(Debug)]
pub struct SlideFetchResult {
    pub local_pdf_path: PathBuf,
    pub size_bytes: u64,
    pub page_count: u32,
    pub fetched_at: String,
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

/// Same as `fetch_slide_pdf` but, if `dump_dir` is provided, also writes the
/// intermediate raw SVGs and the raw pubembed HTML there — useful for debugging
/// blank-PDF issues without re-implementing the auth flow.
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

    // Inline any https: image references into base64 data URIs so svg2pdf can
    // rasterise them (otherwise the output PDF comes out blank for image-heavy
    // slides).
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
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age > SLIDES_CACHE_TTL {
        return Ok(None);
    }
    Ok(Some(SlideFetchResult {
        local_pdf_path: path.clone(),
        size_bytes: meta.len(),
        // We don't re-count pages from cache for speed; agents read the PDF directly.
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

/// Extract `<svg>...</svg>` strings from the escaped JS literals inside the
/// pubembed response, and normalise the common escapes.
fn extract_svgs(body: &str) -> Vec<String> {
    SVG_ESCAPED_RE
        .find_iter(body)
        .map(|m| m.as_str().to_string())
        .map(|s| s.replace(r"\/", "/"))
        .filter_map(|s| unicode_escape::decode(&s).ok())
        .collect()
}

/// Replace every `xlink:href="https://..."` in the SVGs with a base64 data URI
/// whose content is the fetched resource. Shares the download across SVGs that
/// reference the same URL. Failures are warned and leave the original URL.
async fn inline_image_refs(session: &Session, svgs: &[String]) -> Result<Vec<String>> {
    // Collect all unique URLs across slides.
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

    // Fetch in parallel.
    type FetchOutcome = std::result::Result<(Vec<u8>, Option<String>), String>;
    let results: Vec<(String, FetchOutcome)> =
        stream::iter(urls.into_iter())
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

    // Rewrite each SVG replacing matching URLs.
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
        // Sanitize common aliases
        return match ct {
            "image/jpeg" | "image/jpg" => "image/jpeg",
            "image/png" => "image/png",
            "image/gif" => "image/gif",
            "image/webp" => "image/webp",
            "image/svg+xml" => "image/svg+xml",
            _ => {
                // Fall back to magic sniffing
                sniff_mime(bytes)
            }
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

/// Compose multiple SVG pages into a single multi-page PDF.
///
/// Uses `svg2pdf::to_pdf` (each SVG → single-page PDF bytes), then merges with
/// `lopdf` using the canonical "renumber + harvest pages" approach.
fn svgs_to_pdf(svgs: &[String]) -> Result<Vec<u8>> {
    use svg2pdf::{usvg, ConversionOptions, PageOptions};

    let mut opts = usvg::Options::default();
    opts.fontdb_mut().load_system_fonts();

    let mut per_slide_pdfs: Vec<Vec<u8>> = Vec::with_capacity(svgs.len());
    for (i, svg) in svgs.iter().enumerate() {
        let tree = usvg::Tree::from_str(svg, &opts).map_err(|e| {
            ImoocsError::Parse(format!("usvg parse failed on slide {i}: {e}"))
        })?;
        let bytes = svg2pdf::to_pdf(&tree, ConversionOptions::default(), PageOptions::default())
            .map_err(|e| ImoocsError::Internal(format!("svg2pdf conversion failed on slide {i}: {e}")))?;
        per_slide_pdfs.push(bytes);
    }

    if per_slide_pdfs.len() == 1 {
        return Ok(per_slide_pdfs.into_iter().next().unwrap());
    }

    merge_pdfs_lopdf(&per_slide_pdfs)
}

/// Merge multiple PDF byte-blobs into a single PDF using lopdf. Based on the
/// canonical upstream example (lopdf/examples/merge.rs).
fn merge_pdfs_lopdf(inputs: &[Vec<u8>]) -> Result<Vec<u8>> {
    use std::collections::BTreeMap;

    use lopdf::{dictionary, Document, Object, ObjectId};

    // Load each doc and renumber objects so they don't collide.
    let mut max_id: u32 = 1;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();

    for (i, bytes) in inputs.iter().enumerate() {
        let mut doc = Document::load_mem(bytes)
            .map_err(|e| ImoocsError::Internal(format!("lopdf load slide {i}: {e}")))?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        documents_pages.extend(doc.get_pages().into_values().map(|object_id| {
            let value = doc.get_object(object_id).cloned().unwrap_or(Object::Null);
            (object_id, value)
        }));
        documents_objects.extend(doc.objects);
    }

    let mut document = Document::with_version("1.5");

    // Find Catalog/Pages objects from any input; we'll recreate the central Pages entry.
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;
    for (object_id, object) in documents_objects.iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => catalog_object = Some((
                catalog_object.map_or(*object_id, |(id, _)| id),
                object.clone(),
            )),
            b"Pages" => {
                if let Ok(dict) = object.as_dict() {
                    let mut dict = dict.clone();
                    dict.set("Parent", pages_object.as_ref().map_or(*object_id, |(id, _)| *id));
                    if let Some((_, Object::Dictionary(prev))) = pages_object.clone() {
                        // Merge Kids and Count
                        dict.extend(&prev);
                    }
                    pages_object = Some((
                        pages_object.map_or(*object_id, |(id, _)| id),
                        Object::Dictionary(dict),
                    ));
                }
            }
            _ => {}
        }
    }

    let (pages_object_id, pages_object_value) = match pages_object {
        Some(p) => p,
        None => return Err(ImoocsError::Internal("no Pages object across merged PDFs".into())),
    };

    // Update all page dicts to point to the new Pages parent.
    for (object_id, object) in documents_pages.iter() {
        if let Ok(dict) = object.as_dict() {
            let mut dict = dict.clone();
            dict.set("Parent", pages_object_id);
            documents_objects.insert(*object_id, Object::Dictionary(dict));
        }
    }

    // Collect page ids for the Pages dict.
    let page_ids: Vec<Object> = documents_pages.keys().map(|id| Object::Reference(*id)).collect();
    let page_count = page_ids.len() as i64;

    let mut pages_dict = match pages_object_value {
        Object::Dictionary(d) => d,
        _ => dictionary!(),
    };
    pages_dict.set("Kids", page_ids);
    pages_dict.set("Count", page_count);
    pages_dict.set("Type", "Pages");

    document
        .objects
        .insert(pages_object_id, Object::Dictionary(pages_dict));

    // Catalog
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

    // Absorb all other objects (except the pages/catalog themselves, which we've set above).
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
