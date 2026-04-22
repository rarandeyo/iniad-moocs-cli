use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::commands;
use crate::output::OutputMode;

#[derive(Debug, Parser)]
#[command(
    name = "imoocs",
    version,
    about = "Unofficial CLI for INIAD MOOCs, designed for AI agents.",
    infer_subcommands = true
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
    /// Output format. Default: `text`. Human-facing verbs (`doctor`,
    /// `setup`) render a human-readable summary; pass `--format json` to
    /// get a pretty JSON envelope instead. Agent-facing verbs always emit
    /// a pretty JSON envelope regardless of this flag. `auth *` is
    /// text-only and ignores this flag — use exit codes (and
    /// `imoocs doctor --format json` for structured state) instead.
    #[arg(long, value_enum, default_value_t = OutputMode::Text, env = "IMOOCS_FORMAT", global = true)]
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
    /// Course subcommands (list / show).
    #[command(visible_alias = "c")]
    Course {
        #[command(subcommand)]
        cmd: commands::course::CourseCommand,
    },
    /// Lesson subcommands (show).
    #[command(visible_alias = "l")]
    Lesson {
        #[command(subcommand)]
        cmd: commands::lesson::LessonCommand,
    },
    /// Assignment subcommands (list / show / answer / submit / upload).
    #[command(visible_alias = "a")]
    Assignment {
        #[command(subcommand)]
        cmd: commands::assignment::AssignmentCommand,
    },
    /// Slide subcommands (fetch).
    #[command(visible_alias = "s")]
    Slide {
        #[command(subcommand)]
        cmd: commands::slide::SlideCommand,
    },
    /// Google Drive subcommands (list / fetch).
    #[command(visible_alias = "d")]
    Drive {
        #[command(subcommand)]
        cmd: commands::drive::DriveCommand,
    },
    /// Open a MOOCs URL and return the appropriate envelope
    /// (course / lesson-with-assignments / …).
    Open(commands::open::OpenArgs),
    /// Agent Skill installer (`install` / `uninstall` / `status`).
    Skill {
        #[command(subcommand)]
        cmd: commands::skill::SkillCommand,
    },
    /// Print shell completion script to stdout.
    Completion {
        #[arg(value_enum)]
        shell: commands::completion::ShellArg,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Version => commands::version::run(&cli.global),
        Command::Doctor => commands::doctor::run(&cli.global).await,
        Command::Auth { cmd } => commands::auth::run(&cli.global, cmd).await,
        Command::Course { cmd } => commands::course::run(&cli.global, cmd).await,
        Command::Lesson { cmd } => commands::lesson::run(&cli.global, cmd).await,
        Command::Assignment { cmd } => commands::assignment::run(&cli.global, cmd).await,
        Command::Slide { cmd } => commands::slide::run(&cli.global, cmd).await,
        Command::Drive { cmd } => commands::drive::run(&cli.global, cmd).await,
        Command::Open(args) => commands::open::run(&cli.global, args).await,
        Command::Skill { cmd } => commands::skill::run(&cli.global, cmd).await,
        Command::Completion { shell } => commands::completion::run(shell),
    }
}
