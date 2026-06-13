//! Google Slides の pubembed を取得して 1 つの PDF にマージする。
//!
//! 旧 SVG 抽出経路 (`SVG_ESCAPED_RE` + svg2pdf) は色付き背景 / 日本語フォントの
//! レンダリングが不安定だったため削除。Chrome 印刷 (`pdf` コマンド) も pubembed の
//! print CSS がビューアを隠して黒 1 色の PDF になるため使えない (D-3 実機検証)。
//!
//! 採用経路: 各 slide を agent-browser で `?slide=id.p<N>` クエリ付き個別 navigate
//! → screenshot (PNG 1280x720) → JPEG 変換 → `lopdf` の DCTDecode Image XObject
//! として 16:9 ページ (720x405pt) に埋め込んだ単一 PDF を合成する。
//!
//! cache は `$XDG_CACHE_HOME/imoocs/slides/<sha1(embedUrl)>.pdf` に 24h TTL。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use lopdf::{dictionary, Document, Object, Stream};
use sha1::{Digest, Sha1};
use tracing::{debug, info, warn};

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
    _session: &Session,
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

    // Google session が切れていても auth-vault profile + SAML chain で自動回復を試みる
    // (daemon 再起動後は cookie restore では復活しないため)。回復不能なら Auth エラー。
    let binary = super::agent_binary()?;
    imoocs_browser::commands::auth_google::ensure_google_session(&binary)
        .await
        .map_err(|e| ImoocsError::Auth {
            reason: format!("Google session required for slide download (auto-recovery failed: {e})"),
            hint: Some("run `imoocs auth login-google`".into()),
        })?;

    info!(%embed_url, "fetching slide screenshots via agent-browser (戦略 A': per-slide screenshot + lopdf)");

    // 一時 dir に各 slide PNG を保存し、終了時にクリーンアップする
    let tmp_dir = std::env::temp_dir().join(format!("imoocs-slides-{}", std::process::id()));
    fs::create_dir_all(&tmp_dir)?;

    let result = fetch_and_merge(&binary, embed_url, &tmp_dir, &cache_path, paths).await;

    // dump_dir があれば中間 PNG をコピー (result の成否に関わらず保全)
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
    let png_paths = imoocs_browser::commands::slides::fetch_slide_screenshots(binary, embed_url, tmp_dir)
        .await
        .map_err(super::map_browser_err)?;
    if png_paths.is_empty() {
        return Err(ImoocsError::Parse(
            "no slides fetched from pubembed (DOM may have changed)".into(),
        ));
    }

    fs::create_dir_all(paths.slides_dir())?;
    images_to_pdf(&png_paths, cache_path)?;

    let size = fs::metadata(cache_path)?.len();
    Ok(SlideFetchResult {
        local_pdf_path: cache_path.to_path_buf(),
        size_bytes: size,
        page_count: png_paths.len() as u32,
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

/// 16:9 のページサイズ (10in x 5.625in @72dpi)。Google Slides 標準と同じ。
const PAGE_W: f32 = 720.0;
const PAGE_H: f32 = 405.0;

/// PNG 群 (1280x720 screenshot) を JPEG (quality 85) に変換し、1 page 1 画像の
/// PDF として `output` に書く。JPEG は DCTDecode の Image XObject として
/// そのまま埋め込むので再圧縮されない。
fn images_to_pdf(input_paths: &[PathBuf], output: &Path) -> Result<()> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::with_capacity(input_paths.len());

    for input in input_paths {
        let img = image::open(input)
            .map_err(|e| ImoocsError::Parse(format!("failed to read screenshot {}: {e}", input.display())))?;
        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();
        let mut jpeg_bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 85);
        encoder
            .encode_image(&rgb)
            .map_err(|e| ImoocsError::Parse(format!("failed to encode JPEG: {e}")))?;

        let mut img_stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w,
                "Height" => h,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg_bytes,
        );
        // JPEG (DCTDecode) に Flate を重ねると壊れるので圧縮させない
        img_stream.allows_compression = false;
        let img_id = doc.add_object(img_stream);

        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => img_id },
        });
        let content = format!("q {PAGE_W} 0 0 {PAGE_H} 0 0 cm /Im0 Do Q");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), PAGE_W.into(), PAGE_H.into()],
        });
        kids.push(page_id.into());
    }

    let count = kids.len() as u32;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    doc.save(output)
        .map_err(|e| ImoocsError::Io(std::io::Error::other(format!("PDF save: {e}"))))?;
    Ok(())
}
