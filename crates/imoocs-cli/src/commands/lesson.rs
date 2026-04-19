use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api,
    api::slides::fetch_slide_pdf,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::Embed,
    session::Session,
};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum LessonCommand {
    /// Show a lesson's page content (markdown body + embeds).
    Show {
        /// Course id (e.g. `INI301`).
        course_id: String,
        /// Lesson id (e.g. `DS-00`, `AI-01`).
        lesson_id: String,
        /// Page id (e.g. `01`, `atnd`, `exercise`). Default: first page.
        #[arg(long)]
        page: Option<String>,
        /// Download embedded Google Slides as PDFs and include `localPdfPath`.
        #[arg(long)]
        fetch_slides: bool,
        /// Force re-download even if the slide cache is fresh (implies --fetch-slides).
        #[arg(long)]
        no_cache: bool,
    },
}

pub async fn run(global: &GlobalArgs, cmd: LessonCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths.clone_paths())?;

    let year = match global.year {
        Some(y) => y,
        None => match api::resolve_latest_year(&session).await {
            Ok(y) => y,
            Err(err) => {
                output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
                return Ok(ExitCode::from(err.exit_code().as_u8()));
            }
        },
    };

    match cmd {
        LessonCommand::Show {
            course_id,
            lesson_id,
            page,
            fetch_slides,
            no_cache,
        } => {
            let fetch = fetch_slides || no_cache;
            match api::get_lesson_page(&session, year, &course_id, &lesson_id, page.as_deref()).await {
                Ok(mut content) => {
                    if fetch {
                        for embed in content.embeds.iter_mut() {
                            if let Embed::GoogleSlides {
                                embed_url,
                                local_pdf_path,
                                size_bytes,
                                page_count,
                                fetched_at,
                                ..
                            } = embed
                            {
                                match fetch_slide_pdf(&session, &paths, embed_url, no_cache).await {
                                    Ok(res) => {
                                        *local_pdf_path = Some(res.local_pdf_path);
                                        *size_bytes = Some(res.size_bytes);
                                        if res.page_count > 0 {
                                            *page_count = Some(res.page_count);
                                        }
                                        *fetched_at = Some(res.fetched_at);
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            error = %e,
                                            embed = %embed_url,
                                            "failed to fetch slide; embed URL preserved"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    output::emit_success(content, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(err) => {
                    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
                    Ok(ExitCode::from(err.exit_code().as_u8()))
                }
            }
        }
    }
}
