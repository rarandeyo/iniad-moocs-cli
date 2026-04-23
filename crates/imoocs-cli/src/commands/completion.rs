use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use etcetera::{choose_base_strategy, BaseStrategy};
use imoocs_core::ImoocsError;

use crate::cli::Cli;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
}

impl ShellArg {
    fn to_shell(self) -> Shell {
        match self {
            ShellArg::Bash => Shell::Bash,
            ShellArg::Zsh => Shell::Zsh,
            ShellArg::Fish => Shell::Fish,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            ShellArg::Bash => "bash",
            ShellArg::Zsh => "zsh",
            ShellArg::Fish => "fish",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum CompletionCommand {
    /// 補完スクリプトを stdout に出力する (任意の場所へのリダイレクト用途)。
    Generate {
        #[arg(value_enum)]
        shell: ShellArg,
    },
    /// 補完スクリプトを XDG 標準パスに自動配置する。shell 未指定時は `$SHELL` から検出。
    Install {
        /// shell を明示指定する (未指定時は `$SHELL` から自動検出)。
        #[arg(long, short = 's', value_enum)]
        shell: Option<ShellArg>,
        /// 既存ファイルの内容が異なる場合に上書きする。
        #[arg(long)]
        force: bool,
    },
}

pub fn run(cmd: CompletionCommand) -> Result<ExitCode> {
    match cmd {
        CompletionCommand::Generate { shell } => Ok(generate_completion(shell)),
        CompletionCommand::Install { shell, force } => Ok(install(shell, force)),
    }
}

fn generate_completion(shell: ShellArg) -> ExitCode {
    // clap_complete::generate は writer の io::Error を unwrap するので、
    // `| head` 等で SIGPIPE を食らうと panic する。先に Vec に出して、
    // BrokenPipe は静かに無視する。
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    let mut buf: Vec<u8> = Vec::new();
    generate(shell.to_shell(), &mut cmd, name, &mut buf);

    use std::io::Write as _;
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    match handle.write_all(&buf) {
        Ok(()) => ExitCode::from(0),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::from(0),
        Err(e) => {
            eprintln!("failed to write completion: {e}");
            ExitCode::from(5)
        }
    }
}

#[derive(Debug)]
pub(crate) struct InstallOutcome {
    pub shell: ShellArg,
    pub path: PathBuf,
    pub wrote: bool,
}

fn install(shell_arg: Option<ShellArg>, force: bool) -> ExitCode {
    match do_install(shell_arg, force) {
        Ok(outcome) => {
            if outcome.wrote {
                println!(
                    "✓ wrote {} completion to {}",
                    outcome.shell.name(),
                    outcome.path.display()
                );
            } else {
                println!(
                    "✓ {} completion already up to date: {}",
                    outcome.shell.name(),
                    outcome.path.display()
                );
            }
            print_post_install_notes(&outcome);
            ExitCode::from(0)
        }
        Err(err) => {
            eprintln!("✗ completion install 失敗: {err}");
            if let Some(hint) = err.hint() {
                eprintln!("  hint: {hint}");
            }
            ExitCode::from(err.exit_code().as_u8())
        }
    }
}

fn print_post_install_notes(outcome: &InstallOutcome) {
    if outcome.shell == ShellArg::Zsh {
        let parent = outcome
            .path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        eprintln!();
        eprintln!("zsh では fpath にこのディレクトリを足してください (`~/.zshrc` で `compinit` の前):");
        eprintln!("  fpath=({parent} $fpath)");
        eprintln!("  rm -f ~/.zcompdump*   # 既存 cache を破棄");
    }
}

pub(crate) fn do_install(shell_arg: Option<ShellArg>, force: bool) -> std::result::Result<InstallOutcome, ImoocsError> {
    let shell = match shell_arg {
        Some(s) => s,
        None => detect_shell_from_env()?,
    };
    let path = completion_target_path(shell)?;

    let mut buf: Vec<u8> = Vec::new();
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell.to_shell(), &mut cmd, name, &mut buf);

    if let Ok(existing) = std::fs::read(&path) {
        if existing == buf {
            return Ok(InstallOutcome {
                shell,
                path,
                wrote: false,
            });
        }
        if !force {
            return Err(ImoocsError::Validation(format!(
                "{} already exists with different content; pass --force to overwrite",
                path.display()
            )));
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &buf)?;

    Ok(InstallOutcome {
        shell,
        path,
        wrote: true,
    })
}

pub(crate) fn detect_shell_from_env() -> std::result::Result<ShellArg, ImoocsError> {
    let s = std::env::var("SHELL").map_err(|_| {
        ImoocsError::Validation(
            "cannot detect shell: SHELL environment variable is not set; pass --shell <bash|zsh|fish>".into(),
        )
    })?;
    parse_shell_name(&s).ok_or_else(|| {
        ImoocsError::Validation(format!(
            "unsupported shell {s:?} (supported: bash, zsh, fish); pass --shell <bash|zsh|fish> to override"
        ))
    })
}

fn parse_shell_name(shell_path: &str) -> Option<ShellArg> {
    let basename = Path::new(shell_path).file_name().and_then(|n| n.to_str()).unwrap_or("");
    match basename {
        "bash" => Some(ShellArg::Bash),
        "zsh" => Some(ShellArg::Zsh),
        "fish" => Some(ShellArg::Fish),
        _ => None,
    }
}

pub(crate) fn completion_target_path(shell: ShellArg) -> std::result::Result<PathBuf, ImoocsError> {
    let strategy = choose_base_strategy()
        .map_err(|e| ImoocsError::Internal(format!("cannot resolve XDG base directories: {e}")))?;
    let path = match shell {
        ShellArg::Fish => strategy.config_dir().join("fish/completions/imoocs.fish"),
        ShellArg::Bash => strategy.data_dir().join("bash-completion/completions/imoocs"),
        ShellArg::Zsh => strategy.data_dir().join("zsh/site-functions/_imoocs"),
    };
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_shell_paths() {
        assert_eq!(parse_shell_name("/bin/bash"), Some(ShellArg::Bash));
        assert_eq!(parse_shell_name("/usr/bin/zsh"), Some(ShellArg::Zsh));
        assert_eq!(parse_shell_name("/usr/local/bin/fish"), Some(ShellArg::Fish));
        assert_eq!(parse_shell_name("fish"), Some(ShellArg::Fish));
    }

    #[test]
    fn rejects_unsupported_shells() {
        assert_eq!(parse_shell_name("/bin/sh"), None);
        assert_eq!(parse_shell_name("/usr/bin/dash"), None);
        assert_eq!(parse_shell_name("/usr/bin/pwsh"), None);
        assert_eq!(parse_shell_name(""), None);
    }

    #[test]
    fn completion_target_path_tails_are_xdg_canonical() {
        let p = completion_target_path(ShellArg::Fish).unwrap();
        assert!(p.ends_with("fish/completions/imoocs.fish"), "got {p:?}");
        let p = completion_target_path(ShellArg::Bash).unwrap();
        assert!(p.ends_with("bash-completion/completions/imoocs"), "got {p:?}");
        let p = completion_target_path(ShellArg::Zsh).unwrap();
        assert!(p.ends_with("zsh/site-functions/_imoocs"), "got {p:?}");
    }
}
