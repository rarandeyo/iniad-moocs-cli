use std::io;
use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate, Shell};

use crate::cli::Cli;

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
    Nushell,
}

impl ShellArg {
    fn to_shell(self) -> Option<Shell> {
        match self {
            ShellArg::Bash => Some(Shell::Bash),
            ShellArg::Zsh => Some(Shell::Zsh),
            ShellArg::Fish => Some(Shell::Fish),
            ShellArg::Powershell => Some(Shell::PowerShell),
            ShellArg::Elvish => Some(Shell::Elvish),
            // Nushell は clap_complete_nushell の専用 generator を使う (ここでは未対応)
            ShellArg::Nushell => None,
        }
    }
}

pub fn run(shell: ShellArg) -> Result<ExitCode> {
    let Some(shell) = shell.to_shell() else {
        eprintln!("nushell completions are not supported in this build");
        return Ok(ExitCode::from(3));
    };
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
    Ok(ExitCode::from(0))
}
