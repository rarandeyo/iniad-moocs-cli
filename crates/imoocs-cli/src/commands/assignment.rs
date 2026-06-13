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
use indicatif::{ProgressBar, ProgressStyle};
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
    ///
    /// `--url` は必須: 課題ページ URL があれば lesson/page を解決する
    /// ための 15 秒の course-list 走査を skip できる。同じページに課題が複数あれば
    /// `--problem-id` で 1 つに絞る。
    Submit {
        /// 課題ページ URL (例: `https://moocs.iniad.org/courses/2026/INI301/AI-10/09`)。
        #[arg(long)]
        url: String,
        /// 提出する答案。JSON `{pid: value}` / `@path` / `-` (stdin) を受け付ける。
        #[arg(long)]
        data: String,
        /// ページに `.problem-container` が複数あるとき、対象を絞り込む。
        #[arg(long)]
        problem_id: Option<String>,
    },
    /// ファイル答案を記録する。`auto` では即サーバ確定
    /// (POST /file/<pid>?force=true)、`confirm` では draft の
    /// `files[pid]` に絶対パスを stage する。
    ///
    /// `--url` は必須。
    Upload {
        /// 課題ページ URL。
        #[arg(long)]
        url: String,
        /// ファイル答案用の problem field id。
        #[arg(long)]
        pid: String,
        /// upload するローカルファイル。
        #[arg(long, short = 'f')]
        file: PathBuf,
        /// ページに `.problem-container` が複数あるとき、対象を絞り込む。
        #[arg(long)]
        problem_id: Option<String>,
    },
    /// ローカル draft をサーバに確定送信する (stage した答案を finalise)。
    /// TTY 必須。`put_answers(force=true)` → 各 `post_file(force=true)` を
    /// 順次叩き、全部成功したら draft を削除する。途中失敗は draft を残し
    /// API_ERROR / NETWORK_ERROR で止める (再実行で resume できる)。
    ///
    /// デフォルトは **stage 済みの全 draft を一括送信**。
    /// `--url` を渡すと該当 draft 1 つだけ送信する。
    Push {
        /// 特定 draft の課題ページ URL。省略すると全 draft を順次送信する。
        #[arg(long)]
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
            let (key, _page_url) = match resolve_key(&session, global.year, course_id, problem_id, url.as_deref()).await
            {
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
        AssignmentCommand::Submit { url, data, problem_id } => {
            run_submit(&session, global, url, data, problem_id).await
        }
        AssignmentCommand::Upload {
            url,
            pid,
            file,
            problem_id,
        } => run_upload(&session, global, url, pid, file, problem_id).await,
        AssignmentCommand::Push { url } => run_push(&session, global, url).await,
        AssignmentCommand::Drafts { cmd } => run_drafts(&session, global, cmd).await,
    }
}

async fn run_submit(
    session: &Session,
    global: &GlobalArgs,
    url: String,
    data: String,
    problem_id: Option<String>,
) -> Result<ExitCode> {
    // network より前に config / payload を validate する (early-fail で network を節約)。
    let cfg = match Config::load(&session.paths.config_file()) {
        Ok(c) => c,
        Err(e) => return Ok(emit_err(e)),
    };
    let gate = match confirm::resolve_submit_gate(&cfg) {
        Ok(g) => g,
        Err(e) => return Ok(emit_err(e)),
    };
    let parsed = match parse_data(&data) {
        Ok(p) => p,
        Err(e) => return Ok(emit_err(e)),
    };
    let (key, page_url_hint) = match resolve_key(session, global.year, None, problem_id, Some(url.as_str())).await {
        Ok(k) => k,
        Err(e) => return Ok(emit_err(e)),
    };

    match gate {
        SubmitGate::Direct => {
            let page_url = match page_url_hint {
                Some(u) => u,
                None => match resolve_page_url(session, &key).await {
                    Ok(u) => u,
                    Err(e) => return Ok(emit_err(e)),
                },
            };
            let n = parsed.len();
            let spinner = make_spinner(format!("Submitting {}/{} ({n} pid)", key.course_id, key.problem_id));
            let res = api::put_answers(session, &key, &page_url, parsed, true).await;
            match res {
                Ok(v) => {
                    spinner.finish_and_clear();
                    let course = key.course_id.clone();
                    let problem = key.problem_id.clone();
                    output::emit_success_text(v, global.format, move |r| {
                        format!("✓ {course}/{problem} submitted (answers={n}, status={:?})", r.status)
                    });
                    Ok(ExitCode::from(0))
                }
                Err(e) => {
                    spinner.abandon_with_message(format!("✗ {}/{} submit failed", key.course_id, key.problem_id));
                    Ok(emit_err(e))
                }
            }
        }
        SubmitGate::Stage => {
            let drafts_dir = session.paths.drafts_dir();
            let mut draft = match Draft::load_or_new(&drafts_dir, &key) {
                Ok(d) => d,
                Err(e) => return Ok(emit_err(e)),
            };
            draft.answers = parsed;
            draft.answers_staged = true;
            // push が agent-browser に渡す navigate 先として保存。
            if let Some(ref u) = page_url_hint {
                draft.page_url = u.clone();
            }
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
            output::emit_success_text(result, global.format, |r| {
                format!(
                    "✓ {}/{} staged (answers={}, files={}) — `imoocs assignment push` で確定",
                    r.course_id,
                    r.problem_id,
                    r.answers.len(),
                    r.files.len(),
                )
            });
            Ok(ExitCode::from(0))
        }
    }
}

async fn run_upload(
    session: &Session,
    global: &GlobalArgs,
    url: String,
    pid: String,
    file: PathBuf,
    problem_id: Option<String>,
) -> Result<ExitCode> {
    let cfg = match Config::load(&session.paths.config_file()) {
        Ok(c) => c,
        Err(e) => return Ok(emit_err(e)),
    };
    let gate = match confirm::resolve_submit_gate(&cfg) {
        Ok(g) => g,
        Err(e) => return Ok(emit_err(e)),
    };
    let (key, page_url_hint) = match resolve_key(session, global.year, None, problem_id, Some(url.as_str())).await {
        Ok(k) => k,
        Err(e) => return Ok(emit_err(e)),
    };

    match gate {
        SubmitGate::Direct => {
            let page_url = match page_url_hint {
                Some(u) => u,
                None => match resolve_page_url(session, &key).await {
                    Ok(u) => u,
                    Err(e) => return Ok(emit_err(e)),
                },
            };
            let filename = file.file_name().and_then(|s| s.to_str()).unwrap_or("file").to_string();
            let spinner = make_spinner(format!(
                "Uploading {}/{} {pid} ({filename})",
                key.course_id, key.problem_id
            ));
            let res = api::post_file(session, &key, &page_url, &pid, &file, true).await;
            match res {
                Ok(()) => {
                    spinner.finish_and_clear();
                    let result = UploadResult {
                        ok: true,
                        pid: pid.clone(),
                        staged: false,
                        submitted: true,
                        draft_path: None,
                    };
                    let course = key.course_id.clone();
                    let problem = key.problem_id.clone();
                    output::emit_success_text(result, global.format, move |r| {
                        format!("✓ {course}/{problem} uploaded {} ({filename})", r.pid)
                    });
                    Ok(ExitCode::from(0))
                }
                Err(e) => {
                    spinner.abandon_with_message(format!("✗ {}/{} {pid} upload failed", key.course_id, key.problem_id));
                    Ok(emit_err(e))
                }
            }
        }
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
            // push が agent-browser に渡す navigate 先として保存。
            if let Some(ref u) = page_url_hint {
                draft.page_url = u.clone();
            }
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
            let course = key.course_id.clone();
            let problem = key.problem_id.clone();
            output::emit_success_text(result, global.format, move |r| {
                format!(
                    "✓ {course}/{problem} staged {} — `imoocs assignment push` で確定",
                    r.pid
                )
            });
            Ok(ExitCode::from(0))
        }
    }
}

/// `--url` で 1 つだけ送信、引数なしで **全 draft 一括送信**。
async fn run_push(session: &Session, global: &GlobalArgs, url: Option<String>) -> Result<ExitCode> {
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

    let drafts_dir = session.paths.drafts_dir();

    // 対象 draft を決定する。`--url` 指定なら該当 1 件、無しなら全件。
    let target_keys: Vec<AssignmentKey> = match url {
        Some(u) => {
            let (key, _hint) = match resolve_key(session, global.year, None, None, Some(u.as_str())).await {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            vec![key]
        }
        None => match Draft::list(&drafts_dir) {
            Ok(summaries) => summaries
                .into_iter()
                .map(|s| AssignmentKey {
                    year: s.year,
                    course_id: s.course_id,
                    problem_id: s.problem_id,
                })
                .collect(),
            Err(e) => return Ok(emit_err(e)),
        },
    };

    if target_keys.is_empty() {
        return Ok(emit_err(ImoocsError::NotFound {
            what: "no draft staged. Run `imoocs assignment submit` or `upload` first.".into(),
        }));
    }

    // 順次 push。1 件失敗したらそこで停止 (resume は再実行で対応)。
    let total = target_keys.len();
    let mut results: Vec<PushResult> = Vec::with_capacity(total);
    for (i, key) in target_keys.iter().enumerate() {
        let label = format!("[{}/{}] {}/{}", i + 1, total, key.course_id, key.problem_id);
        match push_one_draft(session, &cfg, &drafts_dir, key, &label).await {
            Ok(r) => results.push(r),
            Err(e) => return Ok(emit_err(e)),
        }
    }
    // text モードなら 1 行サマリ、json モードなら従来の envelope。
    output::emit_success_text(results, global.format, |rs| {
        rs.iter()
            .map(|r| {
                let status = r.status.map(|s| format!(", status={s:?}")).unwrap_or_default();
                format!(
                    "✓ {}/{} (answers={}, files={}{})",
                    r.course_id,
                    r.problem_id,
                    r.answers_submitted_pids.len(),
                    r.files_submitted_pids.len(),
                    status,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    Ok(ExitCode::from(0))
}

/// 単一 draft の push (put_answers + 各 post_file)。成功したら draft を削除する。
/// `label` は stderr に表示する prefix (例: `[1/2] INI301/ai-10-free`)。
async fn push_one_draft(
    session: &Session,
    cfg: &Config,
    drafts_dir: &Path,
    key: &AssignmentKey,
    label: &str,
) -> std::result::Result<PushResult, ImoocsError> {
    let draft = Draft::load(drafts_dir, key)?.ok_or_else(|| ImoocsError::NotFound {
        what: format!(
            "no draft staged for {course}/{problem}. Run `imoocs assignment submit` or `upload` first.",
            course = key.course_id,
            problem = key.problem_id
        ),
    })?;
    let draft_path = Draft::path_for(drafts_dir, key);

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
    confirm::resolve_push_gate(cfg, &action)?;

    // draft.page_url が保存されていればそれを使う (= list skip)。
    // 旧形式の draft で空文字列なら resolve_page_url に fallback (15s)。
    let page_url = if draft.page_url.is_empty() {
        resolve_page_url(session, key)
            .await
            .map_err(|e| decorate_push_err(e, &draft_path))?
    } else {
        draft.page_url.clone()
    };

    // upload 単独で作られた draft は `answers_staged = false` のまま。
    // その場合 `put_answers` をスキップしないと `{}` で既存 answers を wipe する。
    let answer_result: Option<AnswerResult> = if draft.answers_staged {
        let spinner = make_spinner(format!("{label} submitting answers ({} pid)", answer_pids.len()));
        let r = api::put_answers(session, key, &page_url, draft.answers.clone(), true)
            .await
            .map_err(|e| decorate_push_err(e, &draft_path));
        match r {
            Ok(v) => {
                spinner.finish_with_message(format!("{label} answers ✓"));
                Some(v)
            }
            Err(e) => {
                spinner.abandon_with_message(format!("{label} answers ✗"));
                return Err(e);
            }
        }
    } else {
        None
    };

    let mut files_submitted: Vec<String> = Vec::new();
    let mut files_sorted: Vec<(&String, &PathBuf)> = draft.files.iter().collect();
    files_sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (pid, path) in files_sorted {
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
        let spinner = make_spinner(format!("{label} uploading {pid} ({filename})"));
        let r = api::post_file(session, key, &page_url, pid, path, true)
            .await
            .map_err(|e| decorate_push_err(e, &draft_path));
        match r {
            Ok(()) => {
                spinner.finish_with_message(format!("{label} {pid} ✓"));
                files_submitted.push(pid.clone());
            }
            Err(e) => {
                spinner.abandon_with_message(format!("{label} {pid} ✗"));
                return Err(e);
            }
        }
    }

    Draft::remove(drafts_dir, key)?;

    let effective_answer_pids = if draft.answers_staged { answer_pids } else { Vec::new() };
    Ok(PushResult {
        pushed: true,
        submitted: true,
        year: draft.year,
        course_id: draft.course_id.clone(),
        problem_id: draft.problem_id.clone(),
        answers_submitted_pids: effective_answer_pids,
        files_submitted_pids: files_submitted,
        status: answer_result.map(|r| r.status),
    })
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
            let (key, _page_url) = match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await
            {
                Ok(k) => k,
                Err(e) => return Ok(emit_err(e)),
            };
            match Draft::load(&drafts_dir, &key) {
                Ok(Some(d)) => {
                    // drafts show は内容閲覧なので text モードでも JSON 出力する
                    // (簡素化対象は「実行結果サマリ」だけで、データ閲覧はそのまま)
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
                output::emit_success_text(json!({ "cleared": "all", "removed": removed }), global.format, |_| {
                    format!("✓ Cleared {removed} draft(s)")
                });
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
                let (key, _page_url) =
                    match resolve_key(session, global.year, course_id, problem_id, url.as_deref()).await {
                        Ok(k) => k,
                        Err(e) => return Ok(emit_err(e)),
                    };
                match Draft::remove(&drafts_dir, &key) {
                    Ok(existed) => {
                        let course = key.course_id.clone();
                        let problem = key.problem_id.clone();
                        output::emit_success_text(
                            json!({
                                "cleared": format!("{course}/{problem}"),
                                "existed": existed,
                            }),
                            global.format,
                            move |_| {
                                if existed {
                                    format!("✓ Cleared {course}/{problem}")
                                } else {
                                    format!("- No draft for {course}/{problem}")
                                }
                            },
                        );
                        Ok(ExitCode::from(0))
                    }
                    Err(e) => Ok(emit_err(e)),
                }
            }
        }
    }
}

/// ぐるぐる回る spinner を生成する。stderr に出力 (stdout の JSON envelope を汚さない)。
/// TTY でない場合は indicatif が自動で no-op になる。
fn make_spinner(message: String) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner:.cyan} {msg}").unwrap_or(ProgressStyle::default_spinner()));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb.set_message(message);
    pb
}

fn push_hint(_course: &str, _problem: &str) -> String {
    // push は引数なしで全 draft 一括送信。`--url` で個別指定。
    "Draft staged locally. Run `imoocs assignment push` from your TTY to finalise.".into()
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
/// [`AssignmentKey`] と (URL 経路の場合) 課題ページ URL を解決する。
///
/// `page_url` を返すのは高速化のため: write 系 (`put_answers` /
/// `post_file`) が agent-browser navigate 先として使う。URL 経路 (= ユーザが
/// `--url` で lesson/page を渡した) では既に lesson_id/page_id が確定するので
/// 直接組み立てて `Some` で返し、`list_course_assignments` (15s) を skip できる。
/// positional 経路では `None` を返し、呼び出し側で `resolve_page_url` の list
/// フォールバックに委ねる。
async fn resolve_key(
    session: &Session,
    global_year: Option<u32>,
    course_id: Option<String>,
    problem_id: Option<String>,
    url: Option<&str>,
) -> std::result::Result<(AssignmentKey, Option<String>), ImoocsError> {
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
        // `--problem-id` がユーザ指定で渡されているとき、URL が `/<lesson>/<page>` 形式なら
        // ネットワークを叩かずに直接 AssignmentKey + page_url を構築する。これは:
        // - submit/upload を高速化 (= page fetch 1 回ぶん約 1 秒節約)
        // - e2e / オフラインでの stage モード動作を可能にする
        if let (Some(pid), Some(page_id_str)) = (problem_id.clone(), page_id.clone()) {
            let key = AssignmentKey {
                year,
                course_id: course_id.clone(),
                problem_id: pid,
            };
            let page_url = format!("https://moocs.iniad.org/courses/{year}/{course_id}/{lesson_id}/{page_id_str}");
            return Ok((key, Some(page_url)));
        }
        // 自動推定経路: page を fetch して `.problem-container` の id を読む。
        let lc = api::get_lesson_page(session, year, &course_id, &lesson_id, page_id.as_deref()).await?;
        let resolved_page_id = lc.page_id.clone();
        let problems = lc.assignments;
        // user が --problem-id を指定したなら、ページ内の id 集合に存在することだけ確認して採用。
        if let Some(pid) = problem_id {
            if !problems.iter().any(|p| p == &pid) {
                return Err(ImoocsError::NotFound {
                    what: format!(
                        "problem_id `{pid}` not on page {u} (found: {list})",
                        list = problems.join(", ")
                    ),
                });
            }
            let key = AssignmentKey {
                year,
                course_id: course_id.clone(),
                problem_id: pid,
            };
            let page_url = format!("https://moocs.iniad.org/courses/{year}/{course_id}/{lesson_id}/{resolved_page_id}");
            return Ok((key, Some(page_url)));
        }
        match problems.len() {
            0 => Err(ImoocsError::NotFound {
                what: format!("no `.problem-container` on page {u}"),
            }),
            1 => {
                let key = AssignmentKey {
                    year,
                    course_id: course_id.clone(),
                    problem_id: problems.into_iter().next().unwrap(),
                };
                let page_url =
                    format!("https://moocs.iniad.org/courses/{year}/{course_id}/{lesson_id}/{resolved_page_id}");
                Ok((key, Some(page_url)))
            }
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
        let key = AssignmentKey {
            year,
            course_id: course_id.expect("clap guarantees course_id when --url is missing"),
            problem_id: problem_id.expect("clap guarantees problem_id when --url is missing"),
        };
        Ok((key, None))
    }
}

/// write 系 (`put_answers` / `post_file`) は agent-browser navigate に
/// 課題ページ URL が必須。`list_course_assignments` で逆引きして lesson_id + page_id
/// を埋め、`/courses/<year>/<course>/<lesson>/<page>` を返す。
///
/// list は course 全体を走査するので 5〜10 秒掛かるが、submit/upload は頻度が
/// 低いので許容範囲。`resolve_key` で URL 経路を通った場合 (= 既に lesson/page を
/// 知っている) でも、現状は `AssignmentKey` がそれを保持しないので再取得になる。
/// 将来 `resolve_key` を struct 返却に変えるなら短縮可能。
async fn resolve_page_url(session: &Session, key: &AssignmentKey) -> std::result::Result<String, ImoocsError> {
    let list = api::list_course_assignments(session, key.year, &key.course_id).await?;
    let found = list
        .iter()
        .find(|a| a.problem_id == key.problem_id)
        .ok_or_else(|| ImoocsError::NotFound {
            what: format!(
                "problem_id `{}` not found in course `{}` (year {}); double-check the id or pass --url",
                key.problem_id, key.course_id, key.year
            ),
        })?;
    let lesson_id = found.lesson_id.as_deref().ok_or_else(|| {
        ImoocsError::Internal(format!(
            "list_course_assignments did not return lesson_id for problem `{}`",
            key.problem_id
        ))
    })?;
    let page_id = &found.page_id;
    Ok(format!(
        "https://moocs.iniad.org/courses/{year}/{course}/{lesson}/{page}",
        year = key.year,
        course = key.course_id,
        lesson = lesson_id,
        page = page_id,
    ))
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
