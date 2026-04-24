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

use imoocs_core::api::slides::{fetch_slide_pdf, SlideFetchResult};
use imoocs_core::config::Config;
use imoocs_core::paths::{resolve_slides_out_dir, Paths, DEFAULT_SLIDES_OUT_DIR};
use imoocs_core::schemas::{Embed, FetchStatus};
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

pub(crate) async fn populate_slide_pdfs(
    session: &Session,
    paths: &Paths,
    embeds: &mut [Embed],
    no_cache: bool,
) -> Result<()> {
    populate_slide_pdfs_with(embeds, |embed_url| async move {
        fetch_slide_pdf(session, paths, &embed_url, no_cache).await
    })
    .await
}

async fn populate_slide_pdfs_with<F, Fut>(embeds: &mut [Embed], mut fetch: F) -> Result<()>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<SlideFetchResult>>,
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
            let res = fetch(embed_url.clone()).await?;
            *local_pdf_path = Some(res.local_pdf_path);
            *size_bytes = Some(res.size_bytes);
            if res.page_count > 0 {
                *page_count = Some(res.page_count);
            }
            *fetched_at = Some(res.fetched_at);
            *fetch_status = Some(FetchStatus::Ok);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use imoocs_core::ImoocsError;

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

    #[tokio::test]
    async fn populate_slide_pdfs_applies_fetch_results() {
        let mut embeds = vec![iframe_embed(), slides_embed("slide-a")];

        populate_slide_pdfs_with(&mut embeds, |embed_url| {
            let name = embed_url;
            async move {
                Ok(SlideFetchResult {
                    local_pdf_path: PathBuf::from(format!("/tmp/{name}.pdf")),
                    size_bytes: 42,
                    page_count: 3,
                    fetched_at: "2026-04-24T00:00:00Z".into(),
                    from_cache: false,
                })
            }
        })
        .await
        .expect("fetch succeeds");

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
    async fn populate_slide_pdfs_propagates_fetch_errors() {
        let mut embeds = vec![slides_embed("slide-a"), slides_embed("slide-b")];

        let err = populate_slide_pdfs_with(&mut embeds, |embed_url| {
            let name = embed_url;
            async move {
                if name == "slide-b" {
                    Err(ImoocsError::Network("network down".into()))
                } else {
                    Ok(SlideFetchResult {
                        local_pdf_path: PathBuf::from("/tmp/slide-a.pdf"),
                        size_bytes: 7,
                        page_count: 1,
                        fetched_at: "2026-04-24T00:00:00Z".into(),
                        from_cache: false,
                    })
                }
            }
        })
        .await
        .expect_err("second fetch should fail");

        assert!(matches!(err, ImoocsError::Network(_)));
        match &embeds[0] {
            Embed::GoogleSlides {
                local_pdf_path,
                size_bytes,
                fetch_status,
                ..
            } => {
                assert_eq!(local_pdf_path.as_ref(), Some(&PathBuf::from("/tmp/slide-a.pdf")));
                assert_eq!(*size_bytes, Some(7));
                assert_eq!(*fetch_status, Some(FetchStatus::Ok));
            }
            other => panic!("expected google slides, got {other:?}"),
        }
        match &embeds[1] {
            Embed::GoogleSlides {
                local_pdf_path,
                size_bytes,
                fetch_status,
                ..
            } => {
                assert!(local_pdf_path.is_none());
                assert!(size_bytes.is_none());
                assert!(fetch_status.is_none());
            }
            other => panic!("expected google slides, got {other:?}"),
        }
    }
}
