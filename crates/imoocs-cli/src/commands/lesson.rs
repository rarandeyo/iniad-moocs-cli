use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api,
    api::slides::fetch_slide_pdf,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::Embed,
    scrape::url::{self, MoocsPath},
    session::Session,
    ImoocsError,
};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum LessonCommand {
    /// Show a lesson's page content (markdown body + embeds).
    Show {
        /// Course id (e.g. `INI301`). Ignored when `--url` is given.
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        /// Lesson id (e.g. `DS-00`, `AI-01`). Ignored when `--url` is given.
        #[arg(required_unless_present = "url")]
        lesson_id: Option<String>,
        /// Page id (e.g. `01`, `atnd`, `exercise`). Default: first page.
        #[arg(long)]
        page: Option<String>,
        /// Resolve course/lesson/page from a MOOCs URL instead of positional args.
        #[arg(long, conflicts_with_all = ["course_id", "lesson_id", "page"])]
        url: Option<String>,
        /// Download embedded Google Slides as PDFs and include `localPdfPath`.
        #[arg(long)]
        fetch_slides: bool,
        /// Force re-download even if the slide cache is fresh (implies --fetch-slides).
        #[arg(long)]
        no_cache: bool,
        /// Expand each on-page assignment into an AssignmentDetail and return
        /// `{lesson, assignments: [AssignmentDetail, ...]}`.
        #[arg(long)]
        with_assignments: bool,
        /// Language for expanded assignments (when --with-assignments is set).
        #[arg(long, value_enum, default_value = "ja")]
        lang: super::assignment::LangArg,
    },
}

struct Target {
    year: u32,
    course_id: String,
    lesson_id: String,
    page_id: Option<String>,
}

pub async fn run(global: &GlobalArgs, cmd: LessonCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths.clone_paths())?;

    match cmd {
        LessonCommand::Show {
            course_id,
            lesson_id,
            page,
            url,
            fetch_slides,
            no_cache,
            with_assignments,
            lang,
        } => {
            let target = match resolve_target(&session, global.year, course_id, lesson_id, page, url.as_deref()).await {
                Ok(t) => t,
                Err(e) => return Ok(emit_err(e)),
            };
            let fetch = fetch_slides || no_cache;
            if with_assignments {
                let result = api::get_lesson_with_assignments(
                    &session,
                    target.year,
                    &target.course_id,
                    &target.lesson_id,
                    target.page_id.as_deref(),
                    lang.into(),
                )
                .await;
                let mut with = match result {
                    Ok(w) => w,
                    Err(e) => return Ok(emit_err(e)),
                };
                if fetch {
                    apply_fetch_slides(&session, &paths, &mut with.lesson.embeds, no_cache).await;
                }
                output::emit_success(with, global.format);
                Ok(ExitCode::from(0))
            } else {
                match api::get_lesson_page(
                    &session,
                    target.year,
                    &target.course_id,
                    &target.lesson_id,
                    target.page_id.as_deref(),
                )
                .await
                {
                    Ok(mut content) => {
                        if fetch {
                            apply_fetch_slides(&session, &paths, &mut content.embeds, no_cache).await;
                        }
                        output::emit_success(content, global.format);
                        Ok(ExitCode::from(0))
                    }
                    Err(e) => Ok(emit_err(e)),
                }
            }
        }
    }
}

async fn resolve_target(
    session: &Session,
    global_year: Option<u32>,
    course_id: Option<String>,
    lesson_id: Option<String>,
    page: Option<String>,
    url: Option<&str>,
) -> std::result::Result<Target, ImoocsError> {
    if let Some(u) = url {
        return match url::parse(u) {
            Some(MoocsPath::Lesson { year, course_id, lesson_id }) => Ok(Target {
                year,
                course_id,
                lesson_id,
                page_id: None,
            }),
            Some(MoocsPath::Page { year, course_id, lesson_id, page_id }) => Ok(Target {
                year,
                course_id,
                lesson_id,
                page_id: Some(page_id),
            }),
            _ => Err(ImoocsError::Validation(format!(
                "URL does not point to a lesson or page: {u}"
            ))),
        };
    }
    let year = match global_year {
        Some(y) => y,
        None => api::resolve_latest_year(session).await?,
    };
    Ok(Target {
        year,
        course_id: course_id.expect("clap guarantees course_id when --url is missing"),
        lesson_id: lesson_id.expect("clap guarantees lesson_id when --url is missing"),
        page_id: page,
    })
}

async fn apply_fetch_slides(
    session: &Session,
    paths: &Paths,
    embeds: &mut [Embed],
    no_cache: bool,
) {
    for embed in embeds.iter_mut() {
        if let Embed::GoogleSlides {
            embed_url,
            local_pdf_path,
            size_bytes,
            page_count,
            fetched_at,
            ..
        } = embed
        {
            match fetch_slide_pdf(session, paths, embed_url, no_cache).await {
                Ok(res) => {
                    *local_pdf_path = Some(res.local_pdf_path);
                    *size_bytes = Some(res.size_bytes);
                    if res.page_count > 0 {
                        *page_count = Some(res.page_count);
                    }
                    *fetched_at = Some(res.fetched_at);
                }
                Err(e) => {
                    tracing::warn!(error = %e, embed = %embed_url, "failed to fetch slide");
                }
            }
        }
    }
}

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}

