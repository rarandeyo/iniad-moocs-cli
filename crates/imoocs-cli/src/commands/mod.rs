pub mod assignment;
pub mod auth;
pub mod completion;
pub mod confirm;
pub mod course;
pub mod doctor;
pub mod drive;
pub mod lesson;
pub mod open;
pub mod setup;
pub mod slide;
pub mod version;

use std::future::Future;

use imoocs_core::api::slides::{fetch_best_effort, FetchOutcome};
use imoocs_core::config::Config;
use imoocs_core::paths::{resolve_slides_out_dir, Paths, DEFAULT_SLIDES_OUT_DIR};
use imoocs_core::schemas::Embed;
use imoocs_core::session::Session;
use imoocs_core::Result;

/// 有効な slide-PDF の保存先を `Paths` に重ねる。
///
/// 優先順位 (高 → 低): CLI flag (`--out-dir`), `config.toml [slides] out_dir`,
/// 組み込みデフォルト (`DEFAULT_SLIDES_OUT_DIR` = `"tmp"`)。
pub fn apply_slides_config(paths: Paths, cli_override: Option<&str>) -> Result<Paths> {
    let cfg = Config::load(&paths.config_file())?;
    let value = cli_override
        .or_else(|| cfg.slides.as_ref().and_then(|s| s.out_dir.as_deref()))
        .unwrap_or(DEFAULT_SLIDES_OUT_DIR);
    let dir = resolve_slides_out_dir(value, &paths.cache_dir)?;
    Ok(paths.with_slides_dir(dir))
}

/// `Embed::GoogleSlides` を best-effort で PDF 取得し、結果 (成功/skip/failed) を
/// 各埋め込みに書き戻す。`fetch_best_effort` が内部で warn + skip するので
/// 呼び出し側は `Result` を畳む必要がない。Google SSO 未ログインや一時的な
/// ネットワーク障害でも呼び出し元は exit 0 で先に進める。
pub(crate) async fn populate_slide_pdfs(
    session: &Session,
    paths: &Paths,
    embeds: &mut [Embed],
    no_cache: bool,
) {
    populate_slide_pdfs_with(embeds, |embed_url| async move {
        fetch_best_effort(session, paths, &embed_url, no_cache).await
    })
    .await
}

async fn populate_slide_pdfs_with<F, Fut>(embeds: &mut [Embed], mut fetch: F)
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = FetchOutcome>,
{
    for embed in embeds.iter_mut() {
        if let Embed::GoogleSlides {
            embed_url,
            local_pdf_path,
            size_bytes,
            page_count,
            fetched_at,
            fetch_status,
            ..
        } = embed
        {
            let out = fetch(embed_url.clone()).await;
            *local_pdf_path = out.local_pdf_path;
            *size_bytes = out.size_bytes;
            *page_count = out.page_count;
            *fetched_at = out.fetched_at;
            *fetch_status = Some(out.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use imoocs_core::schemas::FetchStatus;

    use super::*;

    fn slides_embed(url: &str) -> Embed {
        Embed::GoogleSlides {
            embed_url: url.into(),
            export_pdf_url: "https://example.invalid/export.pdf".into(),
            export_pptx_url: "https://example.invalid/export.pptx".into(),
            local_pdf_path: None,
            size_bytes: None,
            page_count: None,
            fetched_at: None,
            fetch_status: None,
        }
    }

    fn iframe_embed() -> Embed {
        Embed::Iframe {
            src: "https://example.invalid/frame".into(),
        }
    }

    fn ok_outcome(path: &str, pages: u32) -> FetchOutcome {
        FetchOutcome {
            status: FetchStatus::Ok,
            local_pdf_path: Some(PathBuf::from(path)),
            size_bytes: Some(42),
            page_count: if pages > 0 { Some(pages) } else { None },
            fetched_at: Some("2026-04-24T00:00:00Z".into()),
            from_cache: false,
        }
    }

    fn skipped_outcome() -> FetchOutcome {
        FetchOutcome {
            status: FetchStatus::Skipped,
            local_pdf_path: None,
            size_bytes: None,
            page_count: None,
            fetched_at: None,
            from_cache: false,
        }
    }

    fn failed_outcome() -> FetchOutcome {
        FetchOutcome {
            status: FetchStatus::Failed,
            local_pdf_path: None,
            size_bytes: None,
            page_count: None,
            fetched_at: None,
            from_cache: false,
        }
    }

    #[tokio::test]
    async fn populate_slide_pdfs_applies_ok_outcome() {
        let mut embeds = vec![iframe_embed(), slides_embed("slide-a")];

        populate_slide_pdfs_with(&mut embeds, |embed_url| {
            let name = embed_url;
            async move { ok_outcome(&format!("/tmp/{name}.pdf"), 3) }
        })
        .await;

        assert!(matches!(embeds[0], Embed::Iframe { .. }));
        match &embeds[1] {
            Embed::GoogleSlides {
                local_pdf_path,
                size_bytes,
                page_count,
                fetched_at,
                fetch_status,
                ..
            } => {
                assert_eq!(local_pdf_path.as_ref(), Some(&PathBuf::from("/tmp/slide-a.pdf")));
                assert_eq!(*size_bytes, Some(42));
                assert_eq!(*page_count, Some(3));
                assert_eq!(fetched_at.as_deref(), Some("2026-04-24T00:00:00Z"));
                assert_eq!(*fetch_status, Some(FetchStatus::Ok));
            }
            other => panic!("expected google slides, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn populate_slide_pdfs_records_skipped_and_failed_without_propagating() {
        let mut embeds = vec![slides_embed("slide-a"), slides_embed("slide-b"), slides_embed("slide-c")];

        populate_slide_pdfs_with(&mut embeds, |embed_url| {
            let name = embed_url;
            async move {
                match name.as_str() {
                    "slide-a" => ok_outcome("/tmp/slide-a.pdf", 1),
                    "slide-b" => skipped_outcome(),
                    _ => failed_outcome(),
                }
            }
        })
        .await;

        match &embeds[0] {
            Embed::GoogleSlides {
                fetch_status,
                local_pdf_path,
                ..
            } => {
                assert_eq!(*fetch_status, Some(FetchStatus::Ok));
                assert!(local_pdf_path.is_some());
            }
            other => panic!("expected google slides, got {other:?}"),
        }
        match &embeds[1] {
            Embed::GoogleSlides {
                fetch_status,
                local_pdf_path,
                size_bytes,
                fetched_at,
                ..
            } => {
                assert_eq!(*fetch_status, Some(FetchStatus::Skipped));
                assert!(local_pdf_path.is_none());
                assert!(size_bytes.is_none());
                assert!(fetched_at.is_none());
            }
            other => panic!("expected google slides, got {other:?}"),
        }
        match &embeds[2] {
            Embed::GoogleSlides {
                fetch_status,
                local_pdf_path,
                ..
            } => {
                assert_eq!(*fetch_status, Some(FetchStatus::Failed));
                assert!(local_pdf_path.is_none());
            }
            other => panic!("expected google slides, got {other:?}"),
        }
    }
}
