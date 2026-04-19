use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::commands;
use crate::output::OutputMode;

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
    /// Report environment diagnosis (auth / config / cache).
    Doctor,
    /// Authentication subcommands.
    Auth {
        #[command(subcommand)]
        cmd: commands::auth::AuthCommand,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Version => commands::version::run(&cli.global),
        Command::Doctor => commands::doctor::run(&cli.global).await,
        Command::Auth { cmd } => commands::auth::run(&cli.global, cmd).await,
    }
}
