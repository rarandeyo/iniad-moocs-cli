use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::output::{self, OutputMode};

#[derive(Debug, Parser)]
#[command(
    name = "imoocs",
    version,
    about = "Unofficial CLI for INIAD MOOCs, designed for AI agents.",
    infer_subcommands = true,
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Global options")]
pub struct GlobalArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputMode::Json, env = "IMOOCS_FORMAT", global = true)]
    pub format: OutputMode,

    /// Disable progress output to stderr.
    #[arg(long, env = "IMOOCS_NO_PROGRESS", global = true)]
    pub no_progress: bool,

    /// Suppress non-essential stderr.
    #[arg(long, short = 'q', env = "IMOOCS_QUIET", global = true)]
    pub quiet: bool,

    /// Verbose tracing to stderr.
    #[arg(long, env = "IMOOCS_DEBUG", global = true)]
    pub debug: bool,

    /// Override year (default: latest; resolved from MOOCs redirect).
    #[arg(long, env = "IMOOCS_YEAR", global = true)]
    pub year: Option<u32>,

    /// Auto-confirm write operations. Required for `assignment submit`.
    #[arg(long, short = 'y', env = "IMOOCS_YES", global = true)]
    pub yes: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show CLI version.
    Version,
    /// Report environment diagnosis (auth / config / cache / network).
    Doctor,
}

pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Version => {
            let data = serde_json::json!({
                "name": "imoocs",
                "version": env!("CARGO_PKG_VERSION"),
            });
            output::emit_success::<serde_json::Value>(data, cli.global.format);
            Ok(ExitCode::from(0))
        }
        Command::Doctor => {
            use imoocs_core::paths::Paths;
            use imoocs_core::schemas::DoctorReport;
            let paths = Paths::discover()?;
            let report = DoctorReport {
                version: env!("CARGO_PKG_VERSION").to_string(),
                moocs_authenticated: false,
                google_authenticated: false,
                config_dir: paths.config_dir,
                data_dir: paths.data_dir,
                cache_dir: paths.cache_dir,
                username: None,
            };
            output::emit_success(report, cli.global.format);
            Ok(ExitCode::from(0))
        }
    }
}
