use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api,
    envelope::ErrorDetail,
    paths::Paths,
    scrape::url::{self, MoocsPath},
    session::Session,
    ImoocsError,
};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum LessonCommand {
    /// lesson ページの内容 (markdown 本文 + embed) を表示する。
    Show {
        /// コース id (例: `INI301`)。`--url` を指定した場合は無視される。
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        /// lesson id (例: `DS-00`, `AI-01`)。`--url` を指定した場合は無視される。
        #[arg(required_unless_present = "url")]
        lesson_id: Option<String>,
        /// ページ id (例: `01`, `atnd`, `exercise`)。デフォルトは最初のページ。
        #[arg(long)]
        page: Option<String>,
        /// positional 引数の代わりに MOOCs URL から course/lesson/page を解決する。
        #[arg(long, conflicts_with_all = ["course_id", "lesson_id", "page"])]
        url: Option<String>,
        /// 埋め込み Google Slides を PDF として取得し `localPdfPath` を結果に入れる。
        #[arg(long)]
        fetch_slides: bool,
        /// スライド cache が有効でも強制的に再取得する (--fetch-slides が有効になる)。
        #[arg(long)]
        no_cache: bool,
        /// ページ上の各課題を AssignmentDetail に展開し
        /// `{lesson, assignments: [AssignmentDetail, ...]}` を返す。
        #[arg(long)]
        with_assignments: bool,
        /// 展開された課題の言語 (--with-assignments 指定時のみ有効)。
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
    let paths = match super::apply_slides_config(paths, None) {
        Ok(p) => p,
        Err(e) => return Ok(emit_err(e)),
    };
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
                    if let Err(e) =
                        super::populate_slide_pdfs(&session, &paths, &mut with.lesson.embeds, no_cache).await
                    {
                        return Ok(emit_err(e));
                    }
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
                            if let Err(e) =
                                super::populate_slide_pdfs(&session, &paths, &mut content.embeds, no_cache).await
                            {
                                return Ok(emit_err(e));
                            }
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
            Some(MoocsPath::Lesson {
                year,
                course_id,
                lesson_id,
            }) => Ok(Target {
                year,
                course_id,
                lesson_id,
                page_id: None,
            }),
            Some(MoocsPath::Page {
                year,
                course_id,
                lesson_id,
                page_id,
            }) => Ok(Target {
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

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}
