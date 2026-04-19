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
use crate::output;

pub async fn run(global: &GlobalArgs) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let cfg = Config::load(&paths.config_file()).unwrap_or_default();
    let session = Session::new(paths.clone_paths())?;
    let moocs_auth = is_logged_in_moocs(&session).await.unwrap_or(false);
    let google_auth = is_logged_in_google(&session).await.unwrap_or(false);

    let report = DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        moocs_authenticated: moocs_auth,
        google_authenticated: google_auth,
        config_dir: paths.config_dir,
        data_dir: paths.data_dir,
        cache_dir: paths.cache_dir,
        username: cfg.username,
    };
    output::emit_success(report, global.format);
    Ok(ExitCode::from(if moocs_auth { 0 } else { 2 }))
}
