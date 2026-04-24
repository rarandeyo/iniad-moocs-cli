use std::collections::HashMap;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use imoocs_core::{
    api,
    config::Config,
    drafts::Draft,
    envelope::ErrorDetail,
    paths::Paths,
    schemas::{AnswerResult, AssignmentKey, DerivedStatus, Lang, PushResult, StagedResult, UploadResult},
    scrape::url::{self, MoocsPath},
    session::Session,
    ImoocsError,
};
use serde_json::{json, Value};

use crate::cli::GlobalArgs;
use crate::commands::confirm::{self, PushAction, SubmitGate};
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
    /// コース内の全課題を列挙する (lesson/page を巡回して収集)。
    #[command(visible_alias = "ls")]
    List {
        course_id: String,
        /// lesson id でフィルタする (該当 lesson 配下のページの課題のみ)。
        #[arg(long)]
        lesson: Option<String>,
        /// derived status でフィルタする。`pending` = open かつ未入力、
        /// `open` = 派生前の AssignmentStatus::Open (Pending/Submitted の合算)、
        /// `all` は無フィルタ。
        #[arg(long, value_enum, default_value_t = StatusFilter::All)]
        status: StatusFilter,
    },
    /// 単一課題の status / 型付き field / 現在の answer を表示する。
    Show {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        #[arg(long, value_enum, default_value_t = LangArg::Ja)]
        lang: LangArg,
        /// lesson / page の MOOCs URL から課題を解決する。
        /// ページ上の最初の `.problem-container` を採用する。ページに課題が 0 個または 2 個以上ある場合はエラー。
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// 答案を記録する。`assignment.confirm = "auto"` では即サーバ確定
    /// (PUT /answers, `force=true`)、`"confirm"` ではローカル draft に stage
    /// だけ行い、確定は `imoocs assignment push` が担う。
    Submit {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        /// 提出する答案。JSON `{pid: value}` / `@path` / `-` (stdin) を受け付ける。
        #[arg(long)]
        data: String,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// ファイル答案を記録する。`auto` では即サーバ確定
    /// (POST /file/<pid>?force=true)、`confirm` では draft の
    /// `files[pid]` に絶対パスを stage する。
    Upload {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        /// ファイル答案用の problem field id。
        #[arg(long)]
        pid: String,
        /// upload するローカルファイル。conditionally-required にしているのは、
        /// 上の Option<String> な course_id / problem_id positional に対して
        /// clap の positional 順序 assert が発火するのを避けるため。
        /// 未指定で `--url` もない場合は runtime で検査する。
        #[arg(required_unless_present = "url")]
        file: Option<PathBuf>,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// ローカル draft をサーバに確定送信する (stage した答案を finalise)。
    /// TTY 必須。`put_answers(force=true)` → 各 `post_file(force=true)` を
    /// 順次叩き、全部成功したら draft を削除する。途中失敗は draft を残し
    /// API_ERROR / NETWORK_ERROR で止める (再実行で resume できる)。
    Push {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// ローカルに stage された draft の閲覧 / 削除。
    Drafts {
        #[command(subcommand)]
        cmd: DraftsCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DraftsCommand {
    /// stage されている draft の一覧 (DraftSummary[])。
    #[command(visible_alias = "ls")]
    List,
    /// 単一 draft の中身をそのまま出す。無ければ NOT_FOUND。
    Show {
        #[arg(required_unless_present = "url")]
        course_id: Option<String>,
        #[arg(required_unless_present = "url")]
        problem_id: Option<String>,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id"])]
        url: Option<String>,
    },
    /// draft を削除する。`<course> <problem>` / `--url` で単一、`--all` で全削除。
    Clear {
        course_id: Option<String>,
        problem_id: Option<String>,
        #[arg(long, conflicts_with_all = ["course_id", "problem_id", "all"])]
        url: Option<String>,
        /// drafts_dir 内のすべての draft を削除する。
        #[arg(long, conflicts_with_all = ["course_id", "problem_id", "url"])]
        all: bool,
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
        AssignmentCommand::Submit {
            course_id,
            problem_id,
            data,
            url,
        } => run_submit(&session, global, course_id, problem_id, data, url).await,
        AssignmentCommand::Upload {
            course_id,
            problem_id,
            pid,
            file,
            url,
        } => run_upload(&session, global, course_id, problem_id, pid, file, url).await,
        AssignmentCommand::Push {
            course_id,
            problem_id,
            url,
        } => run_push(&session, global, course_id, problem_id, url).await,
        AssignmentCommand::Drafts { cmd } => run_drafts(&session, global, cmd).await,
    }
}

async fn run_submit(
    session: &Session,
    global: &GlobalArgs,
    course_id: Option<String>,
    problem_id: Option<String>,
    data: String,
    url: Option<String>,
) -> Result<ExitCode> {
    let key = match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await {
        Ok(k) => k,
        Err(e) => return Ok(emit_err(e)),
    };
    let parsed = match parse_data(&data) {
        Ok(p) => p,
        Err(e) => return Ok(emit_err(e)),
    };
    let cfg = match Config::load(&session.paths.config_file()) {
        Ok(c) => c,
        Err(e) => return Ok(emit_err(e)),
    };
    let gate = match confirm::resolve_submit_gate(&cfg) {
        Ok(g) => g,
        Err(e) => return Ok(emit_err(e)),
    };

    match gate {
        SubmitGate::Direct => match api::put_answers(session, &key, parsed, true).await {
            Ok(v) => {
                output::emit_success(v, global.format);
                Ok(ExitCode::from(0))
            }
            Err(e) => Ok(emit_err(e)),
        },
        SubmitGate::Stage => {
            let drafts_dir = session.paths.drafts_dir();
            let mut draft = match Draft::load_or_new(&drafts_dir, &key) {
                Ok(d) => d,
                Err(e) => return Ok(emit_err(e)),
            };
            draft.answers = parsed;
            draft.answers_staged = true;
            let draft_path = match draft.save(&drafts_dir) {
                Ok(p) => p,
                Err(e) => return Ok(emit_err(e)),
            };
            let result = StagedResult {
                staged: true,
                submitted: false,
                draft_path,
                year: draft.year,
                course_id: draft.course_id.clone(),
                problem_id: draft.problem_id.clone(),
                answers: draft.answers.clone(),
                files: draft.files.clone(),
                hint: push_hint(&draft.course_id, &draft.problem_id),
            };
            output::emit_success(result, global.format);
            Ok(ExitCode::from(0))
        }
    }
}

async fn run_upload(
    session: &Session,
    global: &GlobalArgs,
    course_id: Option<String>,
    problem_id: Option<String>,
    pid: String,
    file: Option<PathBuf>,
    url: Option<String>,
) -> Result<ExitCode> {
    let file = match file {
        Some(p) => p,
        None => {
            return Ok(emit_err(ImoocsError::Validation(
                "`assignment upload` requires a file path".into(),
            )));
        }
    };
    let key = match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await {
        Ok(k) => k,
        Err(e) => return Ok(emit_err(e)),
    };
    let cfg = match Config::load(&session.paths.config_file()) {
        Ok(c) => c,
        Err(e) => return Ok(emit_err(e)),
    };
    let gate = match confirm::resolve_submit_gate(&cfg) {
        Ok(g) => g,
        Err(e) => return Ok(emit_err(e)),
    };

    match gate {
        SubmitGate::Direct => match api::post_file(session, &key, &pid, &file, true).await {
            Ok(()) => {
                let result = UploadResult {
                    ok: true,
                    pid,
                    staged: false,
                    submitted: true,
                    draft_path: None,
                };
                output::emit_success(result, global.format);
                Ok(ExitCode::from(0))
            }
            Err(e) => Ok(emit_err(e)),
        },
        SubmitGate::Stage => {
            let abs_file = match file.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return Ok(emit_err(ImoocsError::Validation(format!(
                        "cannot resolve file path {path}: {e}",
                        path = file.display()
                    ))));
                }
            };
            let drafts_dir = session.paths.drafts_dir();
            let mut draft = match Draft::load_or_new(&drafts_dir, &key) {
                Ok(d) => d,
                Err(e) => return Ok(emit_err(e)),
            };
            draft.files.insert(pid.clone(), abs_file);
            let draft_path = match draft.save(&drafts_dir) {
                Ok(p) => p,
                Err(e) => return Ok(emit_err(e)),
            };
            let result = UploadResult {
                ok: true,
                pid,
                staged: true,
                submitted: false,
                draft_path: Some(draft_path),
            };
            output::emit_success(result, global.format);
            Ok(ExitCode::from(0))
        }
    }
}

async fn run_push(
    session: &Session,
    global: &GlobalArgs,
    course_id: Option<String>,
    problem_id: Option<String>,
    url: Option<String>,
) -> Result<ExitCode> {
    let cfg = match Config::load(&session.paths.config_file()) {
        Ok(c) => c,
        Err(e) => return Ok(emit_err(e)),
    };
    // config 未設定 / 非 TTY は resolve_key (ネットワーク) より先に弾く。
    let mode = cfg.assignment.as_ref().and_then(|a| a.confirm);
    let is_tty = std::io::stdin().is_terminal();
    if let Err(e) = confirm::decide_push_precheck(mode, is_tty) {
        return Ok(emit_err(e));
    }

    let key = match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await {
        Ok(k) => k,
        Err(e) => return Ok(emit_err(e)),
    };
    let drafts_dir = session.paths.drafts_dir();
    let draft = match Draft::load(&drafts_dir, &key) {
        Ok(Some(d)) => d,
        Ok(None) => {
            return Ok(emit_err(ImoocsError::NotFound {
                what: format!(
                    "no draft staged for {course}/{problem}. Run `imoocs assignment submit` or `upload` first.",
                    course = key.course_id,
                    problem = key.problem_id
                ),
            }));
        }
        Err(e) => return Ok(emit_err(e)),
    };
    let draft_path = Draft::path_for(&drafts_dir, &key);

    let mut answer_pids: Vec<String> = draft.answers.keys().cloned().collect();
    answer_pids.sort();
    let mut file_pids: Vec<(String, String)> = draft
        .files
        .iter()
        .map(|(pid, path)| {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file").to_string();
            (pid.clone(), name)
        })
        .collect();
    file_pids.sort_by(|a, b| a.0.cmp(&b.0));

    let action = PushAction {
        course: &key.course_id,
        problem: &key.problem_id,
        answer_pids: &answer_pids,
        file_pids: &file_pids,
    };
    if let Err(e) = confirm::resolve_push_gate(&cfg, &action) {
        return Ok(emit_err(e));
    }

    // upload 単独で作られた draft は `answers_staged = false` のまま。
    // その場合 `put_answers` をスキップしないと `{}` で既存 answers を wipe する。
    let answer_result: Option<AnswerResult> = if draft.answers_staged {
        match api::put_answers(session, &key, draft.answers.clone(), true).await {
            Ok(v) => Some(v),
            Err(e) => return Ok(emit_err(decorate_push_err(e, &draft_path))),
        }
    } else {
        None
    };

    let mut files_submitted: Vec<String> = Vec::new();
    let mut files_sorted: Vec<(&String, &PathBuf)> = draft.files.iter().collect();
    files_sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (pid, path) in files_sorted {
        match api::post_file(session, &key, pid, path, true).await {
            Ok(()) => files_submitted.push(pid.clone()),
            Err(e) => return Ok(emit_err(decorate_push_err(e, &draft_path))),
        }
    }

    if let Err(e) = Draft::remove(&drafts_dir, &key) {
        return Ok(emit_err(e));
    }

    let effective_answer_pids = if draft.answers_staged { answer_pids } else { Vec::new() };
    let result = PushResult {
        pushed: true,
        submitted: true,
        year: draft.year,
        course_id: draft.course_id.clone(),
        problem_id: draft.problem_id.clone(),
        answers_submitted_pids: effective_answer_pids,
        files_submitted_pids: files_submitted,
        status: answer_result.map(|r| r.status),
    };
    output::emit_success(result, global.format);
    Ok(ExitCode::from(0))
}

async fn run_drafts(session: &Session, global: &GlobalArgs, cmd: DraftsCommand) -> Result<ExitCode> {
    let drafts_dir = session.paths.drafts_dir();
    match cmd {
        DraftsCommand::List => match Draft::list(&drafts_dir) {
            Ok(v) => {
                output::emit_success(v, global.format);
                Ok(ExitCode::from(0))
            }
            Err(e) => Ok(emit_err(e)),
        },
        DraftsCommand::Show {
            course_id,
            problem_id,
            url,
        } => {
            let key = match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            match Draft::load(&drafts_dir, &key) {
                Ok(Some(d)) => {
                    output::emit_success(d, global.format);
                    Ok(ExitCode::from(0))
                }
                Ok(None) => Ok(emit_err(ImoocsError::NotFound {
                    what: format!(
                        "no draft staged for {course}/{problem}",
                        course = key.course_id,
                        problem = key.problem_id
                    ),
                })),
                Err(e) => Ok(emit_err(e)),
            }
        }
        DraftsCommand::Clear {
            course_id,
            problem_id,
            url,
            all,
        } => {
            if all {
                let mut removed = 0usize;
                if drafts_dir.exists() {
                    let entries = match std::fs::read_dir(&drafts_dir) {
                        Ok(e) => e,
                        Err(e) => return Ok(emit_err(ImoocsError::Io(e))),
                    };
                    for entry in entries {
                        let entry = match entry {
                            Ok(e) => e,
                            Err(e) => return Ok(emit_err(ImoocsError::Io(e))),
                        };
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("json") {
                            if let Err(e) = std::fs::remove_file(&path) {
                                return Ok(emit_err(ImoocsError::Io(e)));
                            }
                            removed += 1;
                        }
                    }
                }
                output::emit_success(json!({ "cleared": "all", "removed": removed }), global.format);
                Ok(ExitCode::from(0))
            } else {
                // clap の Option<String> 4 つをそのまま resolve_key に渡すと
                // 引数なし実行で `.expect()` が panic するので、先に runtime で弾く。
                if url.is_none() && (course_id.is_none() || problem_id.is_none()) {
                    return Ok(emit_err(ImoocsError::Validation(
                        "`assignment drafts clear` requires `--all`, `--url <url>`, or both `<course> <problem>` positional args"
                            .into(),
                    )));
                }
                let key = match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await {
                    Ok(k) => k,
                    Err(e) => return Ok(emit_err(e)),
                };
                match Draft::remove(&drafts_dir, &key) {
                    Ok(existed) => {
                        output::emit_success(
                            json!({
                                "cleared": format!("{}/{}", key.course_id, key.problem_id),
                                "existed": existed,
                            }),
                            global.format,
                        );
                        Ok(ExitCode::from(0))
                    }
                    Err(e) => Ok(emit_err(e)),
                }
            }
        }
    }
}

fn push_hint(course: &str, problem: &str) -> String {
    format!("Draft staged locally. Run `imoocs assignment push {course} {problem}` from your TTY to finalise.")
}

/// push 実行中に出たエラーに「draft を残しているので再実行で resume できる」注記を足す。
/// Io はローカル fs の失敗 (stage 済みファイルが移動/削除された等) なので Validation
/// (exit 3) に倒す。そのままだと Internal (exit 5) になって bug 扱いの印象を与える。
/// Reqwest は transparent variant で message を書き換えられないので Network に畳む。
fn decorate_push_err(err: ImoocsError, draft_path: &Path) -> ImoocsError {
    let resume = format!(
        " Draft retained at {}. Re-run `imoocs assignment push` to resume.",
        draft_path.display()
    );
    match err {
        ImoocsError::Api(msg) => ImoocsError::Api(format!("{msg}{resume}")),
        ImoocsError::Network(msg) => ImoocsError::Network(format!("{msg}{resume}")),
        ImoocsError::Reqwest(req_err) => ImoocsError::Network(format!("{req_err}{resume}")),
        ImoocsError::Io(io_err) => ImoocsError::Validation(format!(
            "local file I/O failed during push: {io_err}.{resume} \
             Check that staged file paths still exist."
        )),
        other => other,
    }
}

/// positional の `course_id` / `problem_id` または `--url` から
/// [`AssignmentKey`] を解決する。URL は lesson page を指す必要があり、
/// `.problem-container[data-problem]` が 1 つしかない場合のみ
/// `problem_id` を自動推定する。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_path() -> PathBuf {
        PathBuf::from("/tmp/d.json")
    }

    #[test]
    fn decorate_api_keeps_code_and_appends_resume() {
        let decorated = decorate_push_err(ImoocsError::Api("boom".into()), &draft_path());
        match decorated {
            ImoocsError::Api(msg) => {
                assert!(msg.contains("boom"));
                assert!(msg.contains("Draft retained at /tmp/d.json"));
                assert!(msg.contains("Re-run `imoocs assignment push`"));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn decorate_network_keeps_code() {
        let decorated = decorate_push_err(ImoocsError::Network("down".into()), &draft_path());
        assert!(matches!(decorated, ImoocsError::Network(ref m) if m.contains("down") && m.contains("Draft retained")));
    }

    #[test]
    fn decorate_io_maps_to_validation() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let decorated = decorate_push_err(ImoocsError::Io(io), &draft_path());
        match decorated {
            ImoocsError::Validation(msg) => {
                assert!(msg.contains("local file I/O failed"));
                assert!(msg.contains("Draft retained"));
                assert!(msg.contains("staged file paths"));
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn decorate_unrelated_passes_through() {
        let decorated = decorate_push_err(ImoocsError::NotFound { what: "x".into() }, &draft_path());
        assert!(matches!(decorated, ImoocsError::NotFound { .. }));
    }
}
