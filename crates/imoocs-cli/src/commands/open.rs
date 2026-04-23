use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use imoocs_core::{
    api,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::{Lang, OpenResult},
    scrape::url::{self, MoocsPath},
    session::Session,
    ImoocsError,
};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Args)]
pub struct OpenArgs {
    /// MOOCs URL (例: lesson ページやコース概要)。
    pub url: String,
    /// URL が lesson で Google Slides の embed が含まれているときに PDF も取得する。
    #[arg(long)]
    pub fetch_slides: bool,
    /// スライド PDF を強制再取得する。
    #[arg(long)]
    pub no_cache: bool,
    /// 課題を展開するときの問題文の言語。
    #[arg(long, value_enum, default_value = "ja")]
    pub lang: super::assignment::LangArg,
}

pub async fn run(global: &GlobalArgs, args: OpenArgs) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let paths = match super::apply_slides_config(paths, None) {
        Ok(p) => p,
        Err(e) => return Ok(emit_err(e)),
    };
    let session = Session::new(paths.clone_paths())?;

    let path = match url::parse(&args.url) {
        Some(p) => p,
        None => {
            let err = ImoocsError::Validation(format!("not a MOOCs URL I can route: {url}", url = args.url));
            return Ok(emit_err(err));
        }
    };

    match path {
        MoocsPath::CoursesIndex => {
            let year = match api::resolve_latest_year(&session).await {
                Ok(y) => y,
                Err(e) => return Ok(emit_err(e)),
            };
            match api::get_course_list(&session, Some(year)).await {
                Ok(list) => {
                    output::emit_success(OpenResult::Courses { year, courses: list }, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        MoocsPath::Year(year) => match api::get_course_list(&session, Some(year)).await {
            Ok(list) => {
                output::emit_success(OpenResult::Courses { year, courses: list }, global.format);
                Ok(ExitCode::from(0))
            }
            Err(e) => Ok(emit_err(e)),
        },
        MoocsPath::Course { year, course_id } => match api::get_course_detail(&session, year, &course_id).await {
            Ok(detail) => {
                output::emit_success(OpenResult::Course(detail), global.format);
                Ok(ExitCode::from(0))
            }
            Err(e) => Ok(emit_err(e)),
        },
        MoocsPath::Lesson { .. } | MoocsPath::Page { .. } => {
            let (year, course_id, lesson_id, page_id) = match path {
                MoocsPath::Lesson {
                    year,
                    course_id,
                    lesson_id,
                } => (year, course_id, lesson_id, None),
                MoocsPath::Page {
                    year,
                    course_id,
                    lesson_id,
                    page_id,
                } => (year, course_id, lesson_id, Some(page_id)),
                _ => unreachable!(),
            };
            let lang: Lang = args.lang.into();
            let result =
                api::get_lesson_with_assignments(&session, year, &course_id, &lesson_id, page_id.as_deref(), lang)
                    .await;
            let mut with = match result {
                Ok(w) => w,
                Err(e) => return Ok(emit_err(e)),
            };
            if args.fetch_slides {
                use imoocs_core::api::slides::fetch_slide_pdf;
                use imoocs_core::schemas::Embed;
                for embed in with.lesson.embeds.iter_mut() {
                    if let Embed::GoogleSlides {
                        embed_url,
                        local_pdf_path,
                        size_bytes,
                        page_count,
                        fetched_at,
                        ..
                    } = embed
                    {
                        if let Ok(res) = fetch_slide_pdf(&session, &paths, embed_url, args.no_cache).await {
                            *local_pdf_path = Some(res.local_pdf_path);
                            *size_bytes = Some(res.size_bytes);
                            if res.page_count > 0 {
                                *page_count = Some(res.page_count);
                            }
                            *fetched_at = Some(res.fetched_at);
                        }
                    }
                }
            }
            output::emit_success(OpenResult::Lesson(with), global.format);
            Ok(ExitCode::from(0))
        }
    }
}

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}
