use std::fmt::Write as _;
use std::process::ExitCode;

use imoocs_core::{
    auth::{is_logged_in_google, is_logged_in_moocs},
    config::{Config, ConfirmMode},
    envelope::ErrorDetail,
    paths::Paths,
    schemas::{CompletionStatus, DoctorReport, SkillDetectionMethod},
    session::Session,
    ImoocsError,
};

use crate::cli::GlobalArgs;
use crate::commands::completion::{completion_target_path, detect_shell_from_env};
use crate::commands::drive;
use crate::output;
use crate::skills;

/// `imoocs doctor` の生データ生成。envelope emit を含まないので
/// `imoocs setup` 等のファサードから再利用できる。
pub async fn compute_report() -> std::result::Result<DoctorReport, ImoocsError> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file())?;
    let drive_folders = drive::compute_folders_report(&paths)?.map(|cdf| cdf.summary());
    let session = Session::new(paths.clone_paths())?;
    let moocs_auth = is_logged_in_moocs(&session).await?;
    let google_auth = is_logged_in_google(&session).await?;
    let confirm_mode = cfg.assignment.as_ref().and_then(|a| a.confirm);
    let completion = detect_completion_status();
    let skills = skills::detect_skills();

    let quick_start_complete = moocs_auth
        && google_auth
        && confirm_mode.is_some()
        && completion.as_ref().is_some_and(|c| c.installed)
        && drive_folders.as_ref().is_some_and(|s| s.total > 0 && s.unresolved == 0)
        && skills.imoocs
        && skills.imoocs_drive_setup;

    Ok(DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        moocs_authenticated: moocs_auth,
        google_authenticated: google_auth,
        config_dir: paths.config_dir,
        data_dir: paths.data_dir,
        cache_dir: paths.cache_dir,
        username: cfg.username,
        drive_folders,
        confirm_mode,
        completion,
        skills,
        quick_start_complete,
    })
}

pub async fn run(global: &GlobalArgs) -> anyhow::Result<ExitCode> {
    match compute_report().await {
        Ok(report) => {
            let moocs_auth = report.moocs_authenticated;
            output::emit_success_text(report, global.format, render);
            Ok(ExitCode::from(if moocs_auth { 0 } else { 2 }))
        }
        Err(err) => Ok(emit_err(err)),
    }
}

/// 現在の `$SHELL` に対応する completion の配置状況を調べる。
/// shell 検出不能 / 対応外なら `None` (warn 行で「未検出」と出す)。
fn detect_completion_status() -> Option<CompletionStatus> {
    let shell = detect_shell_from_env().ok()?;
    let path = completion_target_path(shell).ok()?;
    let installed = path.is_file();
    Some(CompletionStatus {
        shell: shell.name().to_string(),
        path,
        installed,
    })
}

fn render(r: &DoctorReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "imoocs {}", r.version);
    let mooc_user = r.username.as_deref().unwrap_or("-");
    let _ = writeln!(out, "  {} MOOCs login   ({mooc_user})", mark(r.moocs_authenticated));
    let _ = writeln!(out, "  {} Google SSO", mark(r.google_authenticated));
    match r.confirm_mode {
        Some(mode) => {
            let _ = writeln!(out, "  ✓ assignment.confirm = {}", confirm_str(mode));
        }
        None => {
            let _ = writeln!(
                out,
                "  ⚠ assignment.confirm 未設定 (`imoocs setup` を再実行 または config.toml を編集)"
            );
        }
    }
    match &r.completion {
        Some(c) if c.installed => {
            let _ = writeln!(out, "  ✓ completion 配置済 ({}) {}", c.shell, c.path.display());
        }
        Some(c) => {
            let _ = writeln!(
                out,
                "  ⚠ completion 未配置 ({}) — `imoocs completion install` で配置",
                c.shell
            );
        }
        None => {
            let _ = writeln!(
                out,
                "  ⚠ completion: shell 未検出 — `imoocs completion install --shell <name>` で指定"
            );
        }
    }
    match (r.skills.imoocs, r.skills.imoocs_drive_setup, r.skills.method) {
        (true, true, method) => {
            let _ = writeln!(out, "  ✓ skill 検出 ({} 経由)", method_str(method));
        }
        (_, _, SkillDetectionMethod::Unknown) => {
            let _ = writeln!(
                out,
                "  ⚠ skill 未検出 — `gh skill install rarandeyo/iniad-moocs-cli {{imoocs,imoocs-drive-setup}}` で追加"
            );
        }
        (im, ds, method) => {
            let _ = writeln!(
                out,
                "  ⚠ skill 一部未検出 ({} 経由): imoocs={}, imoocs-drive-setup={}",
                method_str(method),
                mark(im),
                mark(ds),
            );
        }
    }
    match &r.drive_folders {
        Some(s) if s.total > 0 && s.unresolved == 0 => {
            let _ = writeln!(out, "  ✓ Drive folders ({} courses)", s.total);
        }
        Some(s) if s.total == 0 => {
            let _ = writeln!(out, "  ⚠ Drive folders が空 (`/imoocs-drive-setup` で紐付け)");
        }
        Some(s) => {
            let _ = writeln!(
                out,
                "  ⚠ Drive folders 未解決 ({}/{}, `/imoocs-drive-setup` で紐付け)",
                s.unresolved, s.total
            );
        }
        None => {
            let _ = writeln!(out, "  ⚠ Drive folders 未登録 (`/imoocs-drive-setup` で紐付け)");
        }
    }
    let _ = writeln!(out, "Paths");
    let _ = writeln!(out, "  config  {}", r.config_dir.display());
    let _ = writeln!(out, "  data    {}", r.data_dir.display());
    let _ = writeln!(out, "  cache   {}", r.cache_dir.display());
    if r.quick_start_complete {
        let _ = write!(out, "Quick start: ✓ 全項目クリア");
    } else {
        let _ = write!(out, "Quick start: 未完了 — 上の ⚠ を解消してください");
    }
    out
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "✓"
    } else {
        "✗"
    }
}

fn confirm_str(mode: ConfirmMode) -> &'static str {
    match mode {
        ConfirmMode::Auto => "auto",
        ConfirmMode::Confirm => "confirm",
    }
}

fn method_str(method: SkillDetectionMethod) -> &'static str {
    match method {
        SkillDetectionMethod::Gh => "gh",
        SkillDetectionMethod::Filesystem => "filesystem",
        SkillDetectionMethod::Unknown => "unknown",
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
    use imoocs_core::schemas::{DriveFoldersSummary, SkillDetectionReport};
    use std::path::PathBuf;

    fn all_green() -> DoctorReport {
        DoctorReport {
            version: "test".into(),
            moocs_authenticated: true,
            google_authenticated: true,
            config_dir: PathBuf::from("/c"),
            data_dir: PathBuf::from("/d"),
            cache_dir: PathBuf::from("/cache"),
            username: Some("u".into()),
            drive_folders: Some(DriveFoldersSummary {
                total: 3,
                resolved: 3,
                unresolved: 0,
            }),
            confirm_mode: Some(ConfirmMode::Auto),
            completion: Some(CompletionStatus {
                shell: "fish".into(),
                path: PathBuf::from("/c/fish/completions/imoocs.fish"),
                installed: true,
            }),
            skills: SkillDetectionReport {
                method: SkillDetectionMethod::Gh,
                imoocs: true,
                imoocs_drive_setup: true,
            },
            quick_start_complete: true,
        }
    }

    /// `quick_start_complete` derive ロジックを `compute_report()` と同じ式で再現。
    /// フィールドを 1 つずつ false/None に落として roll-up が連動することを確認。
    fn derive(r: &DoctorReport) -> bool {
        r.moocs_authenticated
            && r.google_authenticated
            && r.confirm_mode.is_some()
            && r.completion.as_ref().is_some_and(|c| c.installed)
            && r.drive_folders
                .as_ref()
                .is_some_and(|s| s.total > 0 && s.unresolved == 0)
            && r.skills.imoocs
            && r.skills.imoocs_drive_setup
    }

    #[test]
    fn all_green_is_complete() {
        let r = all_green();
        assert!(derive(&r));
    }

    #[test]
    fn missing_confirm_mode_is_incomplete() {
        let mut r = all_green();
        r.confirm_mode = None;
        assert!(!derive(&r));
    }

    #[test]
    fn completion_not_installed_is_incomplete() {
        let mut r = all_green();
        if let Some(c) = r.completion.as_mut() {
            c.installed = false;
        }
        assert!(!derive(&r));
    }

    #[test]
    fn unresolved_drive_is_incomplete() {
        let mut r = all_green();
        r.drive_folders = Some(DriveFoldersSummary {
            total: 3,
            resolved: 2,
            unresolved: 1,
        });
        assert!(!derive(&r));
    }

    #[test]
    fn missing_skill_is_incomplete() {
        let mut r = all_green();
        r.skills.imoocs_drive_setup = false;
        assert!(!derive(&r));
    }

    #[test]
    fn render_all_green_shows_quick_start_line() {
        let r = all_green();
        let s = render(&r);
        assert!(s.contains("Quick start: ✓ 全項目クリア"));
    }

    #[test]
    fn render_warn_shows_incomplete_line() {
        let mut r = all_green();
        r.quick_start_complete = false;
        r.confirm_mode = None;
        let s = render(&r);
        assert!(s.contains("⚠ assignment.confirm 未設定"));
        assert!(s.contains("Quick start: 未完了"));
    }
}
