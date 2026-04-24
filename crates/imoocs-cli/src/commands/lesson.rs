use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::LessonWithAssignments,
    scrape::url::{self, MoocsPath},
    session::Session,
    ImoocsError,
};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum LessonCommand {
    /// lesson ページの内容 (markdown 本文 + embed + 各課題の AssignmentDetail) を
    /// まとめて返す。デフォルトで課題展開とスライド PDF 取得を行い、必要なら
    /// `--no-assignments` / `--no-fetch-slides` で抑制できる。スライド取得は
    /// best-effort なので、Google SSO 切れやネットワーク障害でも exit は 0 の
    /// ままで `embeds[*].fetchStatus` に `skipped` / `failed` が入る。
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
        /// 各課題の AssignmentDetail 展開をスキップし、`assignments` を空配列で返す。
        /// ページ本文 (markdown + embeds + assignment ID リスト) だけが欲しいときに使う。
        #[arg(long)]
        no_assignments: bool,
        /// 埋め込み Google Slides の PDF 取得をスキップする。`embeds[*].localPdfPath`
        /// は `null`、`fetchStatus` は埋まらない (None)。
        #[arg(long)]
        no_fetch_slides: bool,
        /// スライド cache が有効でも強制的に再取得する。
        /// `--no-fetch-slides` が指定されたときは無視される。
        #[arg(long)]
        no_cache: bool,
        /// 展開された課題の言語 (`--no-assignments` 指定時は無視される)。
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
            no_assignments,
            no_fetch_slides,
            no_cache,
            lang,
        } => {
            let target = match resolve_target(&session, global.year, course_id, lesson_id, page, url.as_deref()).await {
                Ok(t) => t,
                Err(e) => return Ok(emit_err(e)),
            };
            let fetched = match fetch_payload(&session, &target, no_assignments, lang.into()).await {
                Ok(w) => w,
                Err(e) => return Ok(emit_err(e)),
            };
            let mut with = fetched;
            if !no_fetch_slides {
                super::populate_slide_pdfs(&session, &paths, &mut with.lesson.embeds, no_cache).await;
            }
            output::emit_success(with, global.format);
            Ok(ExitCode::from(0))
        }
    }
}

async fn fetch_payload(
    session: &Session,
    target: &Target,
    no_assignments: bool,
    lang: imoocs_core::schemas::Lang,
) -> std::result::Result<LessonWithAssignments, ImoocsError> {
    if no_assignments {
        let lesson = api::get_lesson_page(
            session,
            target.year,
            &target.course_id,
            &target.lesson_id,
            target.page_id.as_deref(),
        )
        .await?;
        Ok(LessonWithAssignments {
            lesson,
            assignments: Vec::new(),
        })
    } else {
        api::get_lesson_with_assignments(
            session,
            target.year,
            &target.course_id,
            &target.lesson_id,
            target.page_id.as_deref(),
            lang,
        )
        .await
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
