use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Confirm;
use imoocs_core::{config::Config, keyring, paths::Paths, ImoocsError};

use crate::cli::GlobalArgs;
use crate::commands::apply_slides_config;

#[derive(Debug, Args)]
pub struct ResetArgs {
    /// 消すスコープ。複数指定可 (`--scope auth --scope cache` / `--scope auth,cache`)。
    /// 未指定時は `all` と同等。`--scope` を付ける場合は 1 つ以上値が必須。
    #[arg(long, short = 's', value_enum, num_args = 1.., value_delimiter = ',')]
    scope: Vec<Scope>,

    /// 確認プロンプトを skip する。非 TTY 環境では必須。
    #[arg(long, short = 'y')]
    yes: bool,

    /// 消さずに対象リストのみ表示する。
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Scope {
    /// keyring credential と `cookies.json`。
    Auth,
    /// `config.toml` と `course-drive-folders.toml`。
    Config,
    /// `cookies.json` / `drive/` / `slides_dir` (safe root 内のみ)。
    Cache,
    /// `state_dir/drafts/` 配下。
    Drafts,
    /// 上記すべて。
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ResolvedScope {
    Auth,
    Config,
    Cache,
    Drafts,
}

impl ResolvedScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Config => "config",
            Self::Cache => "cache",
            Self::Drafts => "drafts",
        }
    }
}

#[derive(Debug)]
enum TargetKind {
    Keyring { username: String },
    File(PathBuf),
    Dir(PathBuf),
    Unsafe { path: PathBuf, reason: &'static str },
}

#[derive(Debug)]
struct Target {
    scope: ResolvedScope,
    label: String,
    kind: TargetKind,
    exists: bool,
}

pub async fn run(_global: &GlobalArgs, args: ResetArgs) -> anyhow::Result<ExitCode> {
    match run_inner(args).await {
        Ok(code) => Ok(code),
        Err(err) => Ok(ExitCode::from(err.exit_code().as_u8())),
    }
}

async fn run_inner(args: ResetArgs) -> std::result::Result<ExitCode, ImoocsError> {
    let scopes = effective_scopes(&args.scope);
    let paths = Paths::discover()?;

    // 壊れた config 自体を reset で復旧する経路を塞がないため、config load と
    // slides_dir 解決はどちらも best-effort で先へ進める。
    let cfg = Config::load(&paths.config_file()).unwrap_or_default();
    let paths = apply_slides_config(paths.clone(), None).unwrap_or(paths);

    let targets = collect_targets(&paths, &cfg, &scopes);
    print_plan(&targets);

    if args.dry_run {
        println!();
        println!("[dry-run] nothing removed.");
        return Ok(ExitCode::from(0));
    }

    confirm_or_bail(args.yes)?;

    let (removed, skipped, errors) = execute(&targets);

    println!();
    if errors > 0 {
        println!("Done with errors. {removed} removed, {skipped} skipped (not present), {errors} failed.");
        return Err(ImoocsError::Internal(format!("reset: {errors} item(s) failed")));
    }
    println!("Done. {removed} removed, {skipped} skipped (not present).");
    Ok(ExitCode::from(0))
}

fn effective_scopes(specified: &[Scope]) -> BTreeSet<ResolvedScope> {
    if specified.is_empty() || specified.contains(&Scope::All) {
        return [
            ResolvedScope::Auth,
            ResolvedScope::Config,
            ResolvedScope::Cache,
            ResolvedScope::Drafts,
        ]
        .into_iter()
        .collect();
    }
    specified
        .iter()
        .filter_map(|s| match s {
            Scope::Auth => Some(ResolvedScope::Auth),
            Scope::Config => Some(ResolvedScope::Config),
            Scope::Cache => Some(ResolvedScope::Cache),
            Scope::Drafts => Some(ResolvedScope::Drafts),
            Scope::All => None,
        })
        .collect()
}

/// `slides.out_dir` は任意の絶対パスを受け付けるため、app 管理下の既定 2 パスに
/// 限り `remove_dir_all` を許可する。ユーザが共有フォルダを指定していた場合の
/// 巻き込み削除を防ぐ。
fn is_safe_slides_dir(dir: &Path, cache_dir: &Path) -> bool {
    dir == cache_dir.join("slides") || dir == Path::new("/tmp/imoocs/slides")
}

fn collect_targets(paths: &Paths, cfg: &Config, scopes: &BTreeSet<ResolvedScope>) -> Vec<Target> {
    let mut out = Vec::new();
    if scopes.contains(&ResolvedScope::Auth) {
        if let Some(u) = cfg.username.as_deref() {
            out.push(Target {
                scope: ResolvedScope::Auth,
                label: format!("keyring credential (user: {u})"),
                kind: TargetKind::Keyring {
                    username: u.to_string(),
                },
                // delete_credential が NoEntry を吸収するため存在確認はスキップ。
                exists: true,
            });
        }
        out.push(file_target(ResolvedScope::Auth, paths.cookies_file()));
    }
    if scopes.contains(&ResolvedScope::Config) {
        out.push(file_target(ResolvedScope::Config, paths.config_file()));
        out.push(file_target(ResolvedScope::Config, paths.course_drive_folders_file()));
    }
    if scopes.contains(&ResolvedScope::Cache) {
        out.push(file_target(ResolvedScope::Cache, paths.cookies_file()));
        out.push(dir_target(ResolvedScope::Cache, paths.drive_dir()));
        let slides = paths.slides_dir();
        if is_safe_slides_dir(&slides, &paths.cache_dir) {
            out.push(dir_target(ResolvedScope::Cache, slides));
        } else {
            out.push(Target {
                scope: ResolvedScope::Cache,
                label: format!("{} (user-configured slides.out_dir)", slides.display()),
                exists: slides.exists(),
                kind: TargetKind::Unsafe {
                    path: slides,
                    reason: "outside <cache_dir>/slides and /tmp/imoocs/slides — refusing to recursively delete a user-chosen directory",
                },
            });
        }
    }
    if scopes.contains(&ResolvedScope::Drafts) {
        out.push(dir_target(ResolvedScope::Drafts, paths.drafts_dir()));
    }
    out
}

fn file_target(scope: ResolvedScope, path: PathBuf) -> Target {
    let exists = path.exists();
    Target {
        scope,
        label: path.display().to_string(),
        kind: TargetKind::File(path),
        exists,
    }
}

fn dir_target(scope: ResolvedScope, path: PathBuf) -> Target {
    let exists = path.exists();
    Target {
        scope,
        label: path.display().to_string(),
        kind: TargetKind::Dir(path),
        exists,
    }
}

fn print_plan(targets: &[Target]) {
    println!("The following will be removed:");
    let mut current: Option<ResolvedScope> = None;
    for t in targets {
        if Some(t.scope) != current {
            println!();
            println!("  {} scope", t.scope.as_str());
            current = Some(t.scope);
        }
        match &t.kind {
            TargetKind::Unsafe { reason, .. } => {
                println!("    ! {} (skip: {reason})", t.label);
            }
            _ => {
                let mark = if t.exists { "✓" } else { "·" };
                let suffix = if t.exists { "" } else { "  (not present)" };
                println!("    {mark} {}{suffix}", t.label);
            }
        }
    }
}

fn confirm_or_bail(yes: bool) -> std::result::Result<(), ImoocsError> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        eprintln!("reset: refusing to run in non-interactive mode without --yes (hint: pass --yes or --dry-run)");
        return Err(ImoocsError::Validation(
            "reset requires --yes in non-interactive mode".into(),
        ));
    }
    println!();
    let ok = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Proceed?")
        .default(false)
        .interact()
        .map_err(|e| ImoocsError::Internal(format!("confirm prompt: {e}")))?;
    if !ok {
        eprintln!("Aborted.");
        return Err(ImoocsError::Validation("reset aborted by user".into()));
    }
    Ok(())
}

fn execute(targets: &[Target]) -> (usize, usize, usize) {
    let mut removed = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;
    // keyring 削除が失敗したら `cfg.username` を失うと retry で entry を特定でき
    // なくなるため、config scope は auth が clean に終わった場合のみ実行する。
    let mut auth_failed = false;
    for t in targets {
        if t.scope == ResolvedScope::Config && auth_failed {
            eprintln!(
                "! skipping {} to preserve username for keyring retry (auth cleanup failed)",
                t.label
            );
            skipped += 1;
            continue;
        }
        match &t.kind {
            TargetKind::Keyring { username } => match keyring::delete_credential(username) {
                Ok(()) => removed += 1,
                Err(e) => {
                    eprintln!("! keyring entry for {username}: {e} (skipping config deletion to preserve username)");
                    auth_failed = true;
                    errors += 1;
                }
            },
            TargetKind::File(p) => {
                if !p.exists() {
                    skipped += 1;
                    continue;
                }
                match fs::remove_file(p) {
                    Ok(()) => removed += 1,
                    Err(e) => {
                        eprintln!("! remove_file {}: {e} (continuing)", p.display());
                        errors += 1;
                    }
                }
            }
            TargetKind::Dir(p) => {
                if !p.exists() {
                    skipped += 1;
                    continue;
                }
                match fs::remove_dir_all(p) {
                    Ok(()) => removed += 1,
                    Err(e) => {
                        eprintln!("! remove_dir_all {}: {e} (continuing)", p.display());
                        errors += 1;
                    }
                }
            }
            TargetKind::Unsafe { path, reason } => {
                eprintln!(
                    "! refusing to delete {}: {reason}. Remove it yourself if you really want to.",
                    path.display()
                );
                skipped += 1;
            }
        }
    }
    (removed, skipped, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_scopes_empty_is_all() {
        let s = effective_scopes(&[]);
        assert_eq!(s.len(), 4);
        assert!(s.contains(&ResolvedScope::Auth));
        assert!(s.contains(&ResolvedScope::Config));
        assert!(s.contains(&ResolvedScope::Cache));
        assert!(s.contains(&ResolvedScope::Drafts));
    }

    #[test]
    fn effective_scopes_all_includes_everything() {
        let s = effective_scopes(&[Scope::All]);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn effective_scopes_multiple_deduped() {
        let s = effective_scopes(&[Scope::Auth, Scope::Cache, Scope::Auth]);
        assert_eq!(s.len(), 2);
        assert!(s.contains(&ResolvedScope::Auth));
        assert!(s.contains(&ResolvedScope::Cache));
        assert!(!s.contains(&ResolvedScope::Config));
    }

    #[test]
    fn effective_scopes_single() {
        let s = effective_scopes(&[Scope::Drafts]);
        assert_eq!(s.len(), 1);
        assert!(s.contains(&ResolvedScope::Drafts));
    }

    #[test]
    fn safe_slides_dir_accepts_cache_and_tmp() {
        let cache = Path::new("/home/u/.cache/imoocs");
        assert!(is_safe_slides_dir(&cache.join("slides"), cache));
        assert!(is_safe_slides_dir(Path::new("/tmp/imoocs/slides"), cache));
    }

    #[test]
    fn safe_slides_dir_rejects_arbitrary_paths() {
        let cache = Path::new("/home/u/.cache/imoocs");
        assert!(!is_safe_slides_dir(Path::new("/home/u/Documents/slides"), cache));
        assert!(!is_safe_slides_dir(Path::new("/"), cache));
        assert!(!is_safe_slides_dir(Path::new("/srv/slides"), cache));
    }
}
