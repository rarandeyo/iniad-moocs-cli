//! `imoocs skill install [--user|--project]` — embedded SKILL.md と reference を
//! Claude Code が発見できる場所にコピーする。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};
use include_dir::{include_dir, Dir};
use serde::Serialize;
use schemars::JsonSchema;

use crate::cli::GlobalArgs;
use crate::output;

/// リポジトリの `skills/imoocs/` を丸ごとバイナリに埋め込む。
static SKILL_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../skills/imoocs");

const SKILL_NAME: &str = "imoocs";

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Install the embedded skill into ~/.claude/skills/imoocs (or ./.claude/skills/imoocs).
    Install(InstallArgs),
    /// Remove the installed skill directory.
    Uninstall(ScopeArgs),
    /// Show where the skill would be installed.
    Status(ScopeArgs),
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    #[command(flatten)]
    pub scope: ScopeArgs,
    /// Overwrite existing files if they differ.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ScopeArgs {
    /// Install into user-global `~/.claude/skills/imoocs/` (default).
    #[arg(long, conflicts_with = "project")]
    pub user: bool,
    /// Install into project-local `./.claude/skills/imoocs/`.
    #[arg(long, conflicts_with = "user")]
    pub project: bool,
}

impl ScopeArgs {
    fn target_dir(&self) -> std::io::Result<PathBuf> {
        if self.project {
            let cwd = std::env::current_dir()?;
            Ok(cwd.join(".claude").join("skills").join(SKILL_NAME))
        } else {
            // Default = user
            let home =
                std::env::var_os("HOME").ok_or_else(|| std::io::Error::other("HOME not set"))?;
            Ok(PathBuf::from(home)
                .join(".claude")
                .join("skills")
                .join(SKILL_NAME))
        }
    }
    fn scope_label(&self) -> &'static str {
        if self.project {
            "project"
        } else {
            "user"
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct InstallReport {
    scope: String,
    target_dir: PathBuf,
    files: Vec<PathBuf>,
    skipped: Vec<PathBuf>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UninstallReport {
    scope: String,
    target_dir: PathBuf,
    removed: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StatusReport {
    scope: String,
    target_dir: PathBuf,
    installed: bool,
    skill_md_size: Option<u64>,
}

pub async fn run(global: &GlobalArgs, cmd: SkillCommand) -> Result<ExitCode> {
    match cmd {
        SkillCommand::Install(args) => install(global, args),
        SkillCommand::Uninstall(scope) => uninstall(global, scope),
        SkillCommand::Status(scope) => status(global, scope),
    }
}

fn install(global: &GlobalArgs, args: InstallArgs) -> Result<ExitCode> {
    let target = args.scope.target_dir()?;
    fs::create_dir_all(&target)?;
    let mut written: Vec<PathBuf> = Vec::new();
    let mut skipped: Vec<PathBuf> = Vec::new();
    extract_dir(&SKILL_DIR, &target, args.force, &mut written, &mut skipped)?;
    output::emit_success(
        InstallReport {
            scope: args.scope.scope_label().to_string(),
            target_dir: target,
            files: written,
            skipped,
        },
        global.format,
    );
    Ok(ExitCode::from(0))
}

fn uninstall(global: &GlobalArgs, scope: ScopeArgs) -> Result<ExitCode> {
    let target = scope.target_dir()?;
    let removed = if target.exists() {
        fs::remove_dir_all(&target)?;
        true
    } else {
        false
    };
    output::emit_success(
        UninstallReport {
            scope: scope.scope_label().to_string(),
            target_dir: target,
            removed,
        },
        global.format,
    );
    Ok(ExitCode::from(0))
}

fn status(global: &GlobalArgs, scope: ScopeArgs) -> Result<ExitCode> {
    let target = scope.target_dir()?;
    let skill_md = target.join("SKILL.md");
    let size = fs::metadata(&skill_md).ok().map(|m| m.len());
    output::emit_success(
        StatusReport {
            scope: scope.scope_label().to_string(),
            installed: size.is_some(),
            skill_md_size: size,
            target_dir: target,
        },
        global.format,
    );
    Ok(ExitCode::from(0))
}

fn extract_dir(
    dir: &Dir<'_>,
    dest: &Path,
    force: bool,
    written: &mut Vec<PathBuf>,
    skipped: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::File(f) => {
                let rel = f.path();
                let out = dest.join(rel);
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                let content = f.contents();
                let should_write = !out.exists()
                    || force
                    || fs::read(&out).map(|existing| existing != content).unwrap_or(true);
                if should_write {
                    fs::write(&out, content)?;
                    written.push(out);
                } else {
                    skipped.push(out);
                }
            }
            include_dir::DirEntry::Dir(d) => {
                extract_dir(d, dest, force, written, skipped)?;
            }
        }
    }
    Ok(())
}
