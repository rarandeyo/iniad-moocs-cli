//! Google Slides の pubembed を取得して 1 つの PDF にマージする (Phase D-3 戦略 A)。
//!
//! 旧 SVG 抽出経路 (`SVG_ESCAPED_RE` + svg2pdf) は色付き背景 / 日本語フォントの
//! レンダリングが不安定だったため削除。代わりに pubembed の各 slide を
//! agent-browser で `?slide=id.p<N>` クエリ付き個別 navigate → Chrome 印刷 pdf
//! → `lopdf` でマージする戦略を採用する。
//!
//! cache は `$XDG_CACHE_HOME/imoocs/slides/<sha1(embedUrl)>.pdf` に 24h TTL。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use lopdf::{Document, Object, ObjectId};
use sha1::{Digest, Sha1};
use tracing::{debug, info, warn};

use crate::auth::is_logged_in_google;
use crate::error::{ImoocsError, Result};
use crate::paths::Paths;
use crate::schemas::FetchStatus;
use crate::session::Session;

const SLIDES_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

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
/// `tracing::warn!` を 1 行出す。
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
/// (各 slide の単一 PDF) をそのディレクトリにもコピーする。
pub async fn fetch_slide_pdf_with_dump(
    session: &Session,
    paths: &Paths,
    embed_url: &str,
    no_cache: bool,
    dump_dir: Option<&Path>,
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

    info!(%embed_url, "fetching slide PDFs via agent-browser (戦略 A: per-slide pdf + lopdf merge)");
    let binary = super::agent_binary()?;

    // 一時 dir に各 slide PDF を保存し、終了時にクリーンアップする
    let tmp_dir = std::env::temp_dir().join(format!("imoocs-slides-{}", std::process::id()));
    fs::create_dir_all(&tmp_dir)?;

    let result = fetch_and_merge(&binary, embed_url, &tmp_dir, &cache_path, paths).await;

    // dump_dir があれば中間 PDF をコピー (result の成否に関わらず保全)
    if let Some(dir) = dump_dir {
        let _ = fs::create_dir_all(dir);
        if let Ok(entries) = fs::read_dir(&tmp_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_name() {
                    let _ = fs::copy(entry.path(), dir.join(name));
                }
            }
        }
    }

    // 一時 dir を削除
    let _ = fs::remove_dir_all(&tmp_dir);

    result
}

async fn fetch_and_merge(
    binary: &Path,
    embed_url: &str,
    tmp_dir: &Path,
    cache_path: &Path,
    paths: &Paths,
) -> Result<SlideFetchResult> {
    let pdf_paths = imoocs_browser::commands::slides::fetch_slide_pdfs(binary, embed_url, tmp_dir)
        .await
        .map_err(super::map_browser_err)?;
    if pdf_paths.is_empty() {
        return Err(ImoocsError::Parse(
            "no slides fetched from pubembed (DOM may have changed)".into(),
        ));
    }

    fs::create_dir_all(paths.slides_dir())?;
    merge_pdfs(&pdf_paths, cache_path)?;

    let size = fs::metadata(cache_path)?.len();
    Ok(SlideFetchResult {
        local_pdf_path: cache_path.to_path_buf(),
        size_bytes: size,
        page_count: pdf_paths.len() as u32,
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

fn reuse_cache_if_fresh(path: &Path) -> Result<Option<SlideFetchResult>> {
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
        local_pdf_path: path.to_path_buf(),
        size_bytes: meta.len(),
        // cache 再利用時は page count を数え直さない (速度優先)。
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

/// lopdf を使った PDF マージ。bookmarks は付けず、単に Pages を 1 つに連結する。
/// 公式 examples/merge.rs をベースに bookmark layering を削った簡易版。
fn merge_pdfs(input_paths: &[PathBuf], output: &Path) -> Result<()> {
    let mut max_id = 1u32;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut document = Document::with_version("1.5");

    for input in input_paths {
        let mut doc = Document::load(input).map_err(|e| {
            ImoocsError::Parse(format!("failed to load slide PDF {}: {e}", input.display()))
        })?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        let pages = doc.get_pages();
        for object_id in pages.into_values() {
            if let Ok(obj) = doc.get_object(object_id).map(|o| o.to_owned()) {
                documents_pages.insert(object_id, obj);
            }
        }
        documents_objects.extend(doc.objects);
    }

    // Catalog / Pages 用に先頭の参照を保持し、それ以外のオブジェクトはそのまま入れる
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects.into_iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                if catalog_object.is_none() {
                    catalog_object = Some((object_id, object));
                }
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref existing)) = pages_object {
                        if let Ok(old) = existing.as_dict() {
                            dictionary.extend(old);
                        }
                    }
                    let id = pages_object.as_ref().map(|(i, _)| *i).unwrap_or(object_id);
                    pages_object = Some((id, Object::Dictionary(dictionary)));
                }
            }
            // Page は後で documents_pages から処理する。Outline 系は今回非対応。
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                document.objects.insert(object_id, object);
            }
        }
    }

    let (catalog_id, catalog_object) = catalog_object
        .ok_or_else(|| ImoocsError::Parse("no Catalog object in slide PDFs".into()))?;
    let (page_id, page_object) = pages_object
        .ok_or_else(|| ImoocsError::Parse("no Pages object in slide PDFs".into()))?;

    // Pages 再構築: Count + Kids を全 Page で更新
    if let Ok(dict) = page_object.as_dict() {
        let mut dict = dict.clone();
        dict.set("Count", documents_pages.len() as u32);
        dict.set(
            "Kids",
            documents_pages
                .keys()
                .map(|&id| Object::Reference(id))
                .collect::<Vec<_>>(),
        );
        document.objects.insert(page_id, Object::Dictionary(dict));
    }

    // 各 Page に新しい Parent を設定
    for (object_id, object) in documents_pages.iter() {
        if let Ok(dict) = object.as_dict() {
            let mut d = dict.clone();
            d.set("Parent", page_id);
            document.objects.insert(*object_id, Object::Dictionary(d));
        }
    }

    // Catalog 再構築 (Pages 参照を貼り直し、Outlines は削除)
    if let Ok(dict) = catalog_object.as_dict() {
        let mut d = dict.clone();
        d.set("Pages", page_id);
        d.remove(b"Outlines");
        document.objects.insert(catalog_id, Object::Dictionary(d));
    }

    document.trailer.set("Root", catalog_id);
    document.max_id = document.objects.len() as u32;
    document.renumber_objects();

    document
        .save(output)
        .map_err(|e| ImoocsError::Io(std::io::Error::other(format!("PDF save: {e}"))))?;
    Ok(())
}
