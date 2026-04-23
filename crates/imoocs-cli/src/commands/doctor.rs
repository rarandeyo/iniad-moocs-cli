use std::fmt::Write as _;
use std::process::ExitCode;

use anyhow::Result;
use imoocs_core::{
    auth::{is_logged_in_google, is_logged_in_moocs},
    config::Config,
    paths::Paths,
    schemas::DoctorReport,
    session::Session,
};

use crate::cli::GlobalArgs;
use crate::commands::drive;
use crate::output;

/// `imoocs doctor` の生データ生成。envelope emit を含まないので
/// `imoocs setup` 等のファサードから再利用できる。
pub async fn compute_report() -> Result<DoctorReport> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file()).unwrap_or_default();
    let session = Session::new(paths.clone_paths())?;
    let moocs_auth = is_logged_in_moocs(&session).await.unwrap_or(false);
    let google_auth = is_logged_in_google(&session).await.unwrap_or(false);
    let drive_folders = drive::compute_folders_report(&paths)
        .unwrap_or(None)
        .map(|cdf| cdf.summary());

    Ok(DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        moocs_authenticated: moocs_auth,
        google_authenticated: google_auth,
        config_dir: paths.config_dir,
        data_dir: paths.data_dir,
        cache_dir: paths.cache_dir,
        username: cfg.username,
        drive_folders,
    })
}

pub async fn run(global: &GlobalArgs) -> Result<ExitCode> {
    let report = compute_report().await?;
    let moocs_auth = report.moocs_authenticated;
    output::emit_success_text(report, global.format, render);
    Ok(ExitCode::from(if moocs_auth { 0 } else { 2 }))
}

fn render(r: &DoctorReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "imoocs {}", r.version);
    let mooc_user = r.username.as_deref().unwrap_or("-");
    let _ = writeln!(out, "  {} MOOCs login   ({mooc_user})", mark(r.moocs_authenticated));
    let _ = writeln!(out, "  {} Google SSO", mark(r.google_authenticated));
    match &r.drive_folders {
        Some(s) if s.unresolved == 0 => {
            let _ = writeln!(out, "  ✓ Drive folders ({} courses)", s.total);
        }
        Some(s) => {
            let _ = writeln!(
                out,
                "  ✓ Drive folders ({} courses, {} unresolved)",
                s.total, s.unresolved
            );
        }
        None => {
            let _ = writeln!(out, "  ✗ Drive folders not configured (run /imoocs-drive-setup)");
        }
    }
    let _ = writeln!(out, "Paths");
    let _ = writeln!(out, "  config  {}", r.config_dir.display());
    let _ = writeln!(out, "  data    {}", r.data_dir.display());
    let _ = write!(out, "  cache   {}", r.cache_dir.display());
    out
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "✓"
    } else {
        "✗"
    }
}
