//! `imoocs setup` — 初期セットアップのファサード。
//!
//! `auth login` → `auth login-google` → `confirmMode` → `completionInstall`
//! を順次呼び出し、単一の `SetupReport` envelope として結果を報告する。
//! 自前の state / dotfile は生成せず、既存の discrete verbs と等価な副作用
//! だけ残す。

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use imoocs_core::{
    config::{AssignmentConfig, Config, ConfirmMode},
    envelope::ErrorDetail,
    paths::Paths,
    ImoocsError,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Value};

use crate::cli::GlobalArgs;
use crate::commands::auth;
use crate::commands::auth::map_dialoguer_err;
use crate::output::{self, OutputMode};

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// Override the username (otherwise read from config or prompted).
    #[arg(long, short = 'u', env = "IMOOCS_USERNAME")]
    pub username: Option<String>,

    /// Read the password from stdin (for CI / agents). Overrides keyring.
    #[arg(long)]
    pub password_stdin: bool,

    /// Skip the Google SAML login step (useful outside INIAD network or when
    /// slide fetching is not needed).
    #[arg(long)]
    pub skip_google: bool,

    /// shell completion を XDG 標準パスに配置する (未指定かつ対話環境なら確認プロンプト)。
    #[arg(long)]
    pub install_completion: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    pub steps: Vec<StepReport>,
    pub all_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_steps: Option<NextSteps>,
}

/// README の Quick start 後半 (skill 導入 → Drive フォルダ紐付け) を
/// agent が機械可読な形で拾えるようにまとめたもの。
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NextSteps {
    pub skill_installs: Vec<SkillInstallHint>,
    pub drive_setup_command: String,
}

impl NextSteps {
    fn recommended() -> Self {
        let repo = "rarandeyo/iniad-moocs-cli";
        Self {
            skill_installs: vec![
                SkillInstallHint::new(repo, "imoocs"),
                SkillInstallHint::new(repo, "imoocs-drive-setup"),
            ],
            drive_setup_command: "/imoocs-drive-setup".into(),
        }
    }
}

/// `gh skill install` への誘導。`--agent` / `--scope` は指定せず、コマンド側
/// の対話プロンプトでユーザに選ばせる。
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallHint {
    pub command: String,
    pub repo: String,
    pub skill: String,
}

impl SkillInstallHint {
    fn new(repo: &str, skill: &str) -> Self {
        Self {
            command: format!("gh skill install {repo} {skill}"),
            repo: repo.into(),
            skill: skill.into(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    pub step: String,
    pub status: String, // "ok" | "skipped" | "error"
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetail>,
}

impl StepReport {
    fn ok(step: &str, details: Value) -> Self {
        Self {
            step: step.into(),
            status: "ok".into(),
            details,
            error: None,
        }
    }

    fn skipped(step: &str, reason: &str) -> Self {
        Self {
            step: step.into(),
            status: "skipped".into(),
            details: json!({ "reason": reason }),
            error: None,
        }
    }

    fn error(step: &str, err: &ImoocsError) -> Self {
        Self {
            step: step.into(),
            status: "error".into(),
            details: Value::Null,
            error: Some(ErrorDetail::from_error(err)),
        }
    }
}

pub async fn run(global: &GlobalArgs, args: SetupArgs) -> Result<ExitCode> {
    let text_mode = matches!(global.format, OutputMode::Text);
    let mut steps: Vec<StepReport> = Vec::new();
    let mut failure: Option<ImoocsError> = None;

    if text_mode {
        eprintln!("[1/4] INIAD MOOCs ログイン ...");
    }
    tracing::info!("setup: step 1/4 authLogin");
    match auth::do_login(args.username.clone(), args.password_stdin).await {
        Ok(outcome) => {
            if text_mode {
                eprintln!("  ✓ authenticated as {}", outcome.username);
            }
            steps.push(StepReport::ok("authLogin", json!({ "username": outcome.username })));
        }
        Err(err) => {
            if text_mode {
                eprintln!("  ✗ {err}");
            }
            steps.push(StepReport::error("authLogin", &err));
            failure = Some(err);
        }
    }

    if failure.is_some() {
        steps.push(StepReport::skipped("authLoginGoogle", "prior step failed"));
    } else if args.skip_google {
        if text_mode {
            eprintln!("[2/4] Google SSO ... skipped (--skip-google)");
        }
        steps.push(StepReport::skipped("authLoginGoogle", "--skip-google"));
    } else {
        if text_mode {
            eprintln!("[2/4] Google SSO セッション取得 ...");
        }
        tracing::info!("setup: step 2/4 authLoginGoogle");
        match auth::do_login_google().await {
            Ok(username) => {
                if text_mode {
                    eprintln!("  ✓ google session ready for {username}");
                }
                steps.push(StepReport::ok("authLoginGoogle", json!({ "username": username })));
            }
            Err(err) => {
                if text_mode {
                    eprintln!("  ✗ {err}");
                }
                steps.push(StepReport::error("authLoginGoogle", &err));
                failure = Some(err);
            }
        }
    }

    if failure.is_some() {
        steps.push(StepReport::skipped("confirmMode", "prior step failed"));
    } else {
        if text_mode {
            eprintln!("[3/4] 提出モード (assignment.confirm) ...");
        }
        tracing::info!("setup: step 3/4 confirmMode");
        match ensure_confirm_mode(text_mode) {
            Ok(ConfirmModeOutcome::AlreadySet(mode)) => {
                steps.push(StepReport::skipped(
                    "confirmMode",
                    &format!("already set to {}", mode_str(mode)),
                ));
            }
            Ok(ConfirmModeOutcome::Configured(mode)) => {
                if text_mode {
                    eprintln!("  ✓ confirm = {}", mode_str(mode));
                }
                steps.push(StepReport::ok("confirmMode", json!({ "confirm": mode_str(mode) })));
            }
            Err(err) => {
                if text_mode {
                    eprintln!("  ✗ {err}");
                }
                steps.push(StepReport::error("confirmMode", &err));
                failure = Some(err);
            }
        }
    }

    if failure.is_some() {
        steps.push(StepReport::skipped("completionInstall", "prior step failed"));
    } else {
        let should_install = decide_completion_install(args.install_completion, text_mode);
        if !should_install {
            steps.push(StepReport::skipped("completionInstall", "not requested"));
        } else {
            if text_mode {
                eprintln!("[4/4] shell completion の自動配置 ...");
            }
            tracing::info!("setup: step 4/4 completionInstall");
            match crate::commands::completion::do_install(None, false) {
                Ok(outcome) => {
                    let marker = if outcome.wrote { "wrote" } else { "up to date" };
                    if text_mode {
                        eprintln!("  ✓ {} → {} ({marker})", outcome.shell.name(), outcome.path.display());
                    }
                    steps.push(StepReport::ok(
                        "completionInstall",
                        json!({
                            "shell": outcome.shell.name(),
                            "path": outcome.path.display().to_string(),
                            "wrote": outcome.wrote,
                        }),
                    ));
                }
                Err(err) => {
                    if text_mode {
                        eprintln!("  ✗ {err}");
                    }
                    // dotfile 補助的なので setup 全体は継続する (failure には載せない)
                    steps.push(StepReport::error("completionInstall", &err));
                }
            }
        }
    }

    let all_ok = failure.is_none();
    let next_steps = if all_ok { Some(NextSteps::recommended()) } else { None };
    let report = SetupReport {
        steps,
        all_ok,
        next_steps,
    };

    if let Some(err) = failure {
        // failure envelope: これまでの step 情報を error.details に載せ、
        // agent が stderr を parse しなくても最初に失敗した step を拾えるようにする
        let mut detail = ErrorDetail::from_error(&err);
        detail.details = Some(serde_json::to_value(&report).unwrap_or(Value::Null));
        output::emit_failure::<Value>(&detail);
        Ok(ExitCode::from(err.exit_code().as_u8()))
    } else {
        output::emit_success_text(report, global.format, render_report);
        Ok(ExitCode::from(0))
    }
}

fn render_report(r: &SetupReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let total = r.steps.len();
    for (idx, step) in r.steps.iter().enumerate() {
        let tag = format!("[{}/{}]", idx + 1, total);
        let (symbol, suffix) = match step.status.as_str() {
            "ok" => ("✓", step_ok_suffix(&step.step, &step.details)),
            "skipped" => {
                let reason = step.details.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                let symbol = if reason.starts_with("already") { "✓" } else { "⚠" };
                (symbol, format!("skipped: {reason}"))
            }
            _ => ("✗", String::new()),
        };
        if suffix.is_empty() {
            let _ = writeln!(out, "{tag} {:<18} {symbol}", step.step);
        } else {
            let _ = writeln!(out, "{tag} {:<18} {symbol}  {suffix}", step.step);
        }
    }
    let _ = writeln!(out);
    if let Some(next) = &r.next_steps {
        let _ = writeln!(out, "Setup complete.");
        let _ = writeln!(out);
        let _ = writeln!(out, "次のステップ (詳細は README の Quick start):");
        let _ = writeln!(out, "  1. エージェント skill を 2 つ導入");
        for hint in &next.skill_installs {
            let _ = writeln!(out, "       $ {}", hint.command);
        }
        let _ = writeln!(out, "  2. agent 内で Drive フォルダを紐付け");
        let _ = writeln!(out, "       {}", next.drive_setup_command);
        let _ = writeln!(out, "  3. 完了後 `imoocs doctor` を再実行して全項目 ✓ を確認");
        let _ = write!(out, "       $ imoocs doctor");
    } else {
        let _ = write!(out, "Setup complete. `imoocs course list` から始められます。");
    }
    out
}

fn step_ok_suffix(step: &str, details: &Value) -> String {
    match step {
        "authLogin" | "authLoginGoogle" => details
            .get("username")
            .and_then(|v| v.as_str())
            .map(|u| format!("({u})"))
            .unwrap_or_default(),
        "confirmMode" => details
            .get("confirm")
            .and_then(|v| v.as_str())
            .map(|m| format!("(confirm={m})"))
            .unwrap_or_default(),
        "completionInstall" => {
            let shell = details.get("shell").and_then(|v| v.as_str()).unwrap_or("-");
            let wrote = details.get("wrote").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if wrote { "wrote" } else { "up to date" };
            format!("({shell}, {marker})")
        }
        _ => String::new(),
    }
}

/// `install_completion` フラグ、および対話環境かを見て completion install を実行するか決める。
/// - flag が指定されていれば無条件で実行
/// - `--format json` のような非 text モードでは skip (dotfile 相当の副作用は agent に任せない)
/// - 非対話 tty (pipe / CI) でも skip
/// - text + tty 時は Confirm プロンプトでユーザに聞く (default Yes)
fn decide_completion_install(flag: bool, text_mode: bool) -> bool {
    if flag {
        return true;
    }
    if !text_mode {
        return false;
    }
    use std::io::IsTerminal as _;
    if !std::io::stdin().is_terminal() {
        return false;
    }
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("shell 補完を XDG 標準パスに配置しますか？")
        .default(true)
        .interact()
        .unwrap_or(false)
}

enum ConfirmModeOutcome {
    AlreadySet(ConfirmMode),
    Configured(ConfirmMode),
}

fn mode_str(m: ConfirmMode) -> &'static str {
    match m {
        ConfirmMode::Auto => "auto",
        ConfirmMode::Confirm => "confirm",
    }
}

/// Interactively asks the user to pick `auto` / `confirm` if the config
/// doesn't have one yet. If already set, returns the existing value.
fn ensure_confirm_mode(text_mode: bool) -> std::result::Result<ConfirmModeOutcome, ImoocsError> {
    let paths = Paths::discover()?;
    let cfg_path = paths.config_file();
    let mut cfg = Config::load(&cfg_path)?;

    if let Some(existing) = cfg.assignment.as_ref().and_then(|a| a.confirm) {
        return Ok(ConfirmModeOutcome::AlreadySet(existing));
    }

    let items = [
        "confirm — 慎重: submit/upload はローカル draft に stage だけ。確定は TTY で `imoocs assignment push`",
        "auto    — 信頼: submit/upload で即 force=true 確定 (摩擦なし)",
    ];
    if text_mode {
        eprintln!("  提出時の挙動を選んでください (後から config.toml で変更可)\n  ↑↓ で選択 / Enter で決定");
    }
    let idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("assignment.confirm")
        .items(&items)
        .default(0)
        .interact()
        .map_err(map_dialoguer_err)?;
    let mode = if idx == 0 {
        ConfirmMode::Confirm
    } else {
        ConfirmMode::Auto
    };

    cfg.assignment = Some(AssignmentConfig { confirm: Some(mode) });
    cfg.save(&cfg_path)?;
    Ok(ConfirmModeOutcome::Configured(mode))
}
