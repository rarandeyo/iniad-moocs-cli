use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use imoocs_core::{
    api,
    config::Config,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::{AssignmentKey, DerivedStatus, Lang},
    scrape::url::{self, MoocsPath},
    session::Session,
    ImoocsError,
};
use serde_json::{json, Value};

use crate::cli::GlobalArgs;
use crate::commands::confirm::{self, DestructiveAction};
use crate::output;

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum LangArg {
    Ja,
    En,
}

impl From<LangArg> for Lang {
    fn from(l: LangArg) -> Lang {
        match l {
            LangArg::Ja => Lang::Ja,
            LangArg::En => Lang::En,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum StatusFilter {
    Pending,
    Submitted,
    Closed,
    Graded,
    Network,
    Error,
    NonPublic,
    Open,
    All,
}

#[derive(Debug, Subcommand)]
pub enum AssignmentCommand {
    /// List all assignments in a course (by crawling lessons/pages).
    #[command(visible_alias = "ls")]
    List {
        course_id: String,
        /// Filter by lesson id (only assignments whose page is under this lesson).
        #[arg(long)]
        lesson: Option<String>,
        /// Filter by derived status. `pending` = open かつ未入力。`open` = 派生前の
        /// AssignmentStatus::Open に対応 (Pending/Submitted を合わせたもの)。`all` は無フィルタ。
        #[arg(long, value_enum, default_value_t = StatusFilter::All)]
        status: StatusFilter,
    },
    /// Show a single assignment's status, fields (typed), and current answers.
    Show {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        #[arg(long, value_enum, default_value_t = LangArg::Ja)]
        lang: LangArg,
        /// Resolve the assignment from a lesson/page MOOCs URL.
        /// The first `.problem-container` on the page is chosen; error if the page has 0 or > 1 assignments.
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// Save a draft answer without finalising. Accepts JSON `{pid: value}`.
    Answer {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        #[arg(long)]
        data: String,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// Finalise the submission (PUT /answers with `force=true`).
    Submit {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        #[arg(long)]
        data: Option<String>,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// Upload a file answer to a specific pid.
    Upload {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        /// The problem field id for the file.
        #[arg(long)]
        pid: String,
        /// Local file path to upload. Marked conditionally-required so clap's
        /// positional-ordering assert doesn't fire against the Option<String>
        /// course/problem positionals above; a runtime check below ensures it
        /// is still present.
        #[arg(required_unless_present = "url")]
        file: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
}

pub async fn run(global: &GlobalArgs, cmd: AssignmentCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths)?;

    match cmd {
        AssignmentCommand::List {
            course_id,
            lesson,
            status,
        } => {
            let year = match global.year {
                Some(y) => y,
                None => match api::resolve_latest_year(&session).await {
                    Ok(y) => y,
                    Err(err) => return Ok(emit_err(err)),
                },
            };
            match api::list_course_assignments(&session, year, &course_id).await {
                Ok(mut v) => {
                    if let Some(lid) = lesson.as_deref() {
                        v.retain(|a| a.lesson_id.as_deref() == Some(lid));
                    }
                    v.retain(|a| keep_by_status(a.derived_status, status));
                    output::emit_success(v, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        AssignmentCommand::Show {
            course_id,
            problem_id,
            lang,
            url,
        } => {
            let key = match resolve_key(&session, global.year, course_id, problem_id, url.as_deref()).await {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            match api::assignments::get_assignment_detail(&session, &key, lang.into()).await {
                Ok(v) => {
                    output::emit_success(v, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        AssignmentCommand::Answer {
            course_id,
            problem_id,
            data,
            url,
        } => {
            let key = match resolve_key(&session, global.year, course_id, problem_id, url.as_deref()).await {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            let parsed = match parse_data(&data) {
                Ok(p) => p,
                Err(e) => return Ok(emit_err(e)),
            };
            match api::put_answers(&session, &key, parsed, false).await {
                Ok(v) => {
                    output::emit_success(v, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        AssignmentCommand::Submit {
            course_id,
            problem_id,
            data,
            url,
        } => {
            let key = match resolve_key(&session, global.year, course_id, problem_id, url.as_deref()).await {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            let parsed = match data {
                Some(raw) => match parse_data(&raw) {
                    Ok(p) => p,
                    Err(e) => return Ok(emit_err(e)),
                },
                None => HashMap::new(),
            };
            let cfg = match Config::load(&session.paths.config_file()) {
                Ok(c) => c,
                Err(e) => return Ok(emit_err(e)),
            };
            let force = match confirm::resolve_force(
                &cfg,
                &DestructiveAction::Submit {
                    course: &key.course_id,
                    problem: &key.problem_id,
                },
            ) {
                Ok(f) => f,
                Err(e) => return Ok(emit_err(e)),
            };
            match api::put_answers(&session, &key, parsed, force).await {
                Ok(v) => {
                    output::emit_success(v, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        AssignmentCommand::Upload {
            course_id,
            problem_id,
            pid,
            file,
            force,
            url,
        } => {
            let file = match file {
                Some(p) => p,
                None => {
                    return Ok(emit_err(ImoocsError::Validation(
                        "`assignment upload` requires a file path".into(),
                    )));
                }
            };
            let key = match resolve_key(&session, global.year, course_id, problem_id, url.as_deref()).await {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            let effective_force = if force {
                let cfg = match Config::load(&session.paths.config_file()) {
                    Ok(c) => c,
                    Err(e) => return Ok(emit_err(e)),
                };
                let filename = file.file_name().and_then(|s| s.to_str()).unwrap_or("file");
                match confirm::resolve_force(&cfg, &DestructiveAction::UploadForce { pid: &pid, filename }) {
                    Ok(f) => f,
                    Err(e) => return Ok(emit_err(e)),
                }
            } else {
                false
            };
            match api::post_file(&session, &key, &pid, &file, effective_force).await {
                Ok(()) => {
                    output::emit_success(
                        json!({ "ok": true, "pid": pid, "finalised": effective_force }),
                        global.format,
                    );
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
    }
}

/// Resolve an AssignmentKey from either explicit positional args or a URL.
/// URLs must point to a lesson page; the unique `.problem-container[data-problem]`
/// on that page determines `problem_id`.
async fn resolve_key(
    session: &Session,
    global_year: Option<u32>,
    course_id: Option<String>,
    problem_id: Option<String>,
    url: Option<&str>,
) -> std::result::Result<AssignmentKey, ImoocsError> {
    if let Some(u) = url {
        let (year, course_id, lesson_id, page_id) = match url::parse(u) {
            Some(MoocsPath::Page {
                year,
                course_id,
                lesson_id,
                page_id,
            }) => (year, course_id, lesson_id, Some(page_id)),
            Some(MoocsPath::Lesson {
                year,
                course_id,
                lesson_id,
            }) => (year, course_id, lesson_id, None),
            _ => {
                return Err(ImoocsError::Validation(format!(
                    "URL does not point to a lesson/page: {u}"
                )))
            }
        };
        let lc = api::get_lesson_page(session, year, &course_id, &lesson_id, page_id.as_deref()).await?;
        let problems = lc.assignments;
        match problems.len() {
            0 => Err(ImoocsError::NotFound {
                what: format!("no `.problem-container` on page {u}"),
            }),
            1 => Ok(AssignmentKey {
                year,
                course_id,
                problem_id: problems.into_iter().next().unwrap(),
            }),
            n => Err(ImoocsError::Validation(format!(
                "page has {n} assignments; pass --problem-id explicitly (one of: {list})",
                list = problems.join(", ")
            ))),
        }
    } else {
        let year = match global_year {
            Some(y) => y,
            None => api::resolve_latest_year(session).await?,
        };
        Ok(AssignmentKey {
            year,
            course_id: course_id.expect("clap guarantees course_id when --url is missing"),
            problem_id: problem_id.expect("clap guarantees problem_id when --url is missing"),
        })
    }
}

fn parse_data(raw: &str) -> std::result::Result<HashMap<String, Value>, ImoocsError> {
    let body = if raw == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| ImoocsError::Validation(format!("cannot read stdin: {e}")))?;
        buf
    } else if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|e| ImoocsError::Validation(format!("cannot read {path}: {e}")))?
    } else {
        raw.to_string()
    };
    let v: Value = serde_json::from_str(body.trim())
        .map_err(|e| ImoocsError::Validation(format!("invalid JSON in --data: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| ImoocsError::Validation("--data must be a JSON object mapping pid -> value".into()))?;
    Ok(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

fn keep_by_status(d: DerivedStatus, filter: StatusFilter) -> bool {
    match filter {
        StatusFilter::All => true,
        StatusFilter::Pending => d == DerivedStatus::Pending,
        StatusFilter::Submitted => d == DerivedStatus::Submitted,
        StatusFilter::Closed => d == DerivedStatus::Closed,
        StatusFilter::Graded => d == DerivedStatus::Graded,
        StatusFilter::Network => d == DerivedStatus::Network,
        StatusFilter::Error => d == DerivedStatus::Error,
        StatusFilter::NonPublic => d == DerivedStatus::NonPublic,
        // `--status open` = Pending or Submitted（サーバ側 status==Open の 2 派生）
        StatusFilter::Open => matches!(d, DerivedStatus::Pending | DerivedStatus::Submitted),
    }
}

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}
