use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use imoocs_core::{
    api,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::{AssignmentKey, Lang},
    session::Session,
    ImoocsError,
};
use serde_json::{json, Value};

use crate::cli::GlobalArgs;
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

#[derive(Debug, Subcommand)]
pub enum AssignmentCommand {
    /// List all assignments in a course (by crawling lessons/pages).
    #[command(visible_alias = "ls")]
    List {
        course_id: String,
    },
    /// Show a single assignment's status, fields (typed), and current answers.
    Show {
        course_id: String,
        problem_id: String,
        /// Language variant of the problem statement.
        #[arg(long, value_enum, default_value_t = LangArg::Ja)]
        lang: LangArg,
    },
    /// Save a draft answer without finalising. Accepts JSON `{pid: value}`.
    Answer {
        course_id: String,
        problem_id: String,
        /// JSON inline (`--data '{"p1": "x"}'`), `@file`, or `-` (stdin).
        #[arg(long)]
        data: String,
    },
    /// Finalise the submission (PUT /answers with `force=true`).
    Submit {
        course_id: String,
        problem_id: String,
        /// Optional data to set before submitting. Same format as `answer --data`.
        #[arg(long)]
        data: Option<String>,
    },
    /// Upload a file answer to a specific pid.
    Upload {
        course_id: String,
        problem_id: String,
        /// The problem field id for the file.
        #[arg(long)]
        pid: String,
        /// Local file path to upload.
        file: PathBuf,
        /// Force=true (finalise after upload).
        #[arg(long)]
        force: bool,
    },
}

pub async fn run(global: &GlobalArgs, cmd: AssignmentCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths)?;

    let year = match global.year {
        Some(y) => y,
        None => match api::resolve_latest_year(&session).await {
            Ok(y) => y,
            Err(err) => return Ok(emit_err(err)),
        },
    };

    match cmd {
        AssignmentCommand::List { course_id } => {
            match api::list_course_assignments(&session, year, &course_id).await {
                Ok(v) => {
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
        } => {
            let key = AssignmentKey {
                year,
                course_id,
                problem_id,
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
        } => {
            let key = AssignmentKey {
                year,
                course_id,
                problem_id,
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
        } => {
            if !global.yes {
                let err = ImoocsError::Validation(
                    "`assignment submit` requires --yes (force=true finalises the submission)"
                        .into(),
                );
                return Ok(emit_err(err));
            }
            let key = AssignmentKey {
                year,
                course_id,
                problem_id,
            };
            let parsed = match data {
                Some(raw) => match parse_data(&raw) {
                    Ok(p) => p,
                    Err(e) => return Ok(emit_err(e)),
                },
                None => HashMap::new(),
            };
            match api::put_answers(&session, &key, parsed, true).await {
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
        } => {
            let key = AssignmentKey {
                year,
                course_id,
                problem_id,
            };
            match api::post_file(&session, &key, &pid, &file, force).await {
                Ok(()) => {
                    output::emit_success(json!({ "ok": true, "pid": pid }), global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
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
        std::fs::read_to_string(path)
            .map_err(|e| ImoocsError::Validation(format!("cannot read {path}: {e}")))?
    } else {
        raw.to_string()
    };
    let v: Value = serde_json::from_str(body.trim())
        .map_err(|e| ImoocsError::Validation(format!("invalid JSON in --data: {e}")))?;
    let obj = v.as_object().ok_or_else(|| {
        ImoocsError::Validation("--data must be a JSON object mapping pid -> value".into())
    })?;
    Ok(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}
