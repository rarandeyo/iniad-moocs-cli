use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::commands;
use crate::output::OutputMode;

#[derive(Debug, Parser)]
#[command(
    name = "imoocs",
    version,
    about = "AI agent 向けに設計された INIAD MOOCs の非公式 CLI。",
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
    /// 出力フォーマット。デフォルトは `text`。人間向けの verb (`doctor`,
    /// `setup`) は読みやすいサマリを出力し、`--format json` で pretty JSON
    /// envelope に切り替わる。agent 向けの verb はこの flag に関わらず常に
    /// pretty JSON envelope を吐く。`auth *` は text 専用で、この flag を
    /// 無視する — 状態は exit code (構造化が必要なら
    /// `imoocs doctor --format json`) で判断する。
    #[arg(long, value_enum, default_value_t = OutputMode::Text, env = "IMOOCS_FORMAT", global = true)]
    pub format: OutputMode,

    /// stderr への進捗出力を無効化する。
    #[arg(long, env = "IMOOCS_NO_PROGRESS", global = true)]
    pub no_progress: bool,

    /// 重要度の低い stderr 出力を抑制する。
    #[arg(long, short = 'q', env = "IMOOCS_QUIET", global = true)]
    pub quiet: bool,

    /// stderr に詳細な trace を出す。
    #[arg(long, env = "IMOOCS_DEBUG", global = true)]
    pub debug: bool,

    /// year を明示指定する (デフォルトは最新; MOOCs の redirect から解決)。
    #[arg(long, env = "IMOOCS_YEAR", global = true)]
    pub year: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// CLI の version を表示する。
    Version,
    /// 環境診断 (auth / config / cache) を報告する。
    Doctor,
    /// 認証関連のサブコマンド。
    Auth {
        #[command(subcommand)]
        cmd: commands::auth::AuthCommand,
    },
    /// コース関連のサブコマンド (list / show)。
    #[command(visible_alias = "c")]
    Course {
        #[command(subcommand)]
        cmd: commands::course::CourseCommand,
    },
    /// lesson 関連のサブコマンド (show)。
    #[command(visible_alias = "l")]
    Lesson {
        #[command(subcommand)]
        cmd: commands::lesson::LessonCommand,
    },
    /// 課題関連のサブコマンド (list / show / answer / submit / upload)。
    #[command(visible_alias = "a")]
    Assignment {
        #[command(subcommand)]
        cmd: commands::assignment::AssignmentCommand,
    },
    /// スライド関連のサブコマンド (fetch)。
    #[command(visible_alias = "s")]
    Slide {
        #[command(subcommand)]
        cmd: commands::slide::SlideCommand,
    },
    /// Google Drive 関連のサブコマンド (list / fetch)。
    #[command(visible_alias = "d")]
    Drive {
        #[command(subcommand)]
        cmd: commands::drive::DriveCommand,
    },
    /// MOOCs の URL を開き、種類に応じた envelope
    /// (course / lesson-with-assignments / …) を返す。
    Open(commands::open::OpenArgs),
    /// 初期セットアップウィザード: MOOCs ログイン、Google SSO、最終診断。
    Setup(commands::setup::SetupArgs),
    /// shell completion script を stdout に出力する。
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
        Command::Setup(args) => commands::setup::run(&cli.global, args).await,
        Command::Completion { shell } => commands::completion::run(shell),
    }
}
