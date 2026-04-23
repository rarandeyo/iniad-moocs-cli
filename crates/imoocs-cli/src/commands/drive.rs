//! `imoocs drive list|fetch|folders` — session の SAML cookie を使った Drive folder/file アクセス、
//! および `course-drive-folders.toml` の表示。

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api::drive::{fetch_drive_file, list_drive_folder},
    drive_folders::{CourseDriveFolders, MatchStrategy},
    envelope::ErrorDetail,
    paths::Paths,
    session::Session,
    ImoocsError,
};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum DriveCommand {
    /// Drive folder の中身を列挙する (`window['_DRIVE_ivd']` を scrape)。
    ///
    /// `/drive/folders/<id>` URL または folder id を受け付ける。
    List {
        /// `/drive/folders/<id>` URL か folder id。
        target: String,
    },
    /// 単一の Drive ファイルを cache にダウンロードする。
    ///
    /// `/file/d/<id>/...` / `/uc?export=download&id=<id>` /
    /// `drive.usercontent.google.com/download?id=<id>` URL、または file id を受け付ける。
    Fetch {
        /// `/file/d/<id>/(view|preview)?`, `/uc?...&id=<id>`, または file id。
        target: String,
        /// cache に加えて、このパスにダウンロードファイルをコピーする。
        #[arg(long)]
        out: Option<PathBuf>,
        /// 24h cache を無視して強制再取得する。
        #[arg(long)]
        no_cache: bool,
    },
    /// `course-drive-folders.toml` (履修コース ↔ Drive フォルダの対応) を表示する。
    ///
    /// `imoocs-drive-setup` skill が書き込む TOML を読み取り専用で表示するだけ。
    /// 編集はせず、対象ファイルが無ければその旨を案内する。
    Folders,
}

pub async fn run(global: &GlobalArgs, cmd: DriveCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;

    if let DriveCommand::Folders = cmd {
        return Ok(run_folders(global, &paths));
    }

    let session = Session::new(paths.clone_paths())?;

    match cmd {
        DriveCommand::List { target } => {
            let folder_id = match parse_drive_target(&target) {
                DriveTarget::Folder(id) | DriveTarget::Ambiguous(id) => id,
                DriveTarget::File(_) => {
                    return Ok(emit_err(ImoocsError::Validation(
                        "target looks like a Drive FILE URL; use `imoocs drive fetch` instead".into(),
                    )));
                }
                DriveTarget::Unrecognized => {
                    return Ok(emit_err(ImoocsError::Validation(format!(
                        "cannot recognise Drive target: {target}"
                    ))));
                }
            };
            match list_drive_folder(&session, &folder_id).await {
                Ok(listing) => {
                    output::emit_success(listing, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        DriveCommand::Fetch { target, out, no_cache } => {
            let file_id = match parse_drive_target(&target) {
                DriveTarget::File(id) | DriveTarget::Ambiguous(id) => id,
                DriveTarget::Folder(_) => {
                    return Ok(emit_err(ImoocsError::Validation(
                        "target looks like a Drive FOLDER URL; use `imoocs drive list` instead".into(),
                    )));
                }
                DriveTarget::Unrecognized => {
                    return Ok(emit_err(ImoocsError::Validation(format!(
                        "cannot recognise Drive target: {target}"
                    ))));
                }
            };
            match fetch_drive_file(&session, &paths, &file_id, no_cache).await {
                Ok(res) => {
                    if let Some(dest) = out {
                        if let Some(parent) = dest.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::copy(&res.local_path, &dest) {
                            return Ok(emit_err(ImoocsError::Io(e)));
                        }
                    }
                    output::emit_success(res, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
        DriveCommand::Folders => unreachable!("handled above"),
    }
}

/// `course-drive-folders.toml` を純粋に読み込む。`doctor` から再利用する
/// ためにコマンド本体と分離してある。ファイル未存在は `Ok(None)`、
/// パース失敗は `Err`。
pub fn compute_folders_report(paths: &Paths) -> Result<Option<CourseDriveFolders>, ImoocsError> {
    CourseDriveFolders::load(&paths.course_drive_folders_file())
}

fn run_folders(global: &GlobalArgs, paths: &Paths) -> ExitCode {
    match compute_folders_report(paths) {
        Ok(report) => {
            output::emit_success_text(report, global.format, render_folders);
            ExitCode::from(0)
        }
        Err(e) => emit_err(e),
    }
}

fn render_folders(report: &Option<CourseDriveFolders>) -> String {
    let Some(cdf) = report else {
        return "No course-drive-folders.toml registered. Run /imoocs-drive-setup in a MOOCs skill-enabled session."
            .to_string();
    };
    let mut out = String::new();
    let _ = writeln!(out, "Drive root: {}", cdf.drive_root_folder_id);
    if cdf.courses.is_empty() {
        let _ = write!(out, "(no courses registered)");
        return out;
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "year | courseId | name | strategy | url");
    for c in &cdf.courses {
        let strategy = strategy_label(c.match_strategy);
        let url = if c.drive_folder_url.is_empty() {
            "-"
        } else {
            c.drive_folder_url.as_str()
        };
        let _ = writeln!(
            out,
            "{} | {} | {} | {} | {}",
            c.year, c.course_id, c.name, strategy, url
        );
    }
    let s = cdf.summary();
    let _ = write!(
        out,
        "\n{} courses ({} resolved, {} unresolved)",
        s.total, s.resolved, s.unresolved
    );
    out
}

fn strategy_label(s: MatchStrategy) -> &'static str {
    match s {
        MatchStrategy::Exact => "exact",
        MatchStrategy::Partial => "partial",
        MatchStrategy::UserConfirmed => "user-confirmed",
        MatchStrategy::Unresolved => "unresolved",
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DriveTarget {
    File(String),
    Folder(String),
    /// 裸の id (URL ではない) — file か folder か判別不可のため caller のコマンドに委ねる。
    Ambiguous(String),
    Unrecognized,
}

// どの pattern も id のキャプチャから `#` を除外している。`#foo` のような
// fragment anchor が fileId に混ざらないようにするため
static FILE_URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https://drive\.google\.com/file/d/([^/?#]+)").unwrap());
static FOLDER_URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^https://drive\.google\.com/drive/folders/([^/?#]+)").unwrap());
// `(?:[^#]*&)?id=` で prefix を optional にすることで、`uc?id=X` (id 先頭) と
// `uc?export=download&id=X` (id 末尾) の両形式を受け付けられる
static UC_URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^https://drive\.google\.com/uc\?(?:[^#]*&)?id=([^&#]+)").unwrap());
static USERCONTENT_URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^https://drive\.usercontent\.google\.com/download\?(?:[^#]*&)?id=([^&#]+)").unwrap());
static BARE_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z0-9_-]{25,64}$").unwrap());

fn parse_drive_target(target: &str) -> DriveTarget {
    let t = target.trim();
    if let Some(c) = FOLDER_URL_RE.captures(t) {
        return DriveTarget::Folder(c[1].to_string());
    }
    if let Some(c) = FILE_URL_RE.captures(t) {
        return DriveTarget::File(c[1].to_string());
    }
    if let Some(c) = UC_URL_RE.captures(t) {
        return DriveTarget::File(c[1].to_string());
    }
    if let Some(c) = USERCONTENT_URL_RE.captures(t) {
        return DriveTarget::File(c[1].to_string());
    }
    if BARE_ID_RE.is_match(t) {
        return DriveTarget::Ambiguous(t.to_string());
    }
    DriveTarget::Unrecognized
}

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_view_url() {
        assert_eq!(
            parse_drive_target("https://drive.google.com/file/d/FAKE_DRIVE_FILE_ID_HIST_REDACT001/view?usp=drive_link"),
            DriveTarget::File("FAKE_DRIVE_FILE_ID_HIST_REDACT001".into())
        );
    }

    #[test]
    fn parses_folder_url() {
        assert_eq!(
            parse_drive_target("https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_HIST_REDACT1"),
            DriveTarget::Folder("FAKE_DRIVE_FOLDER_ID_HIST_REDACT1".into())
        );
    }

    #[test]
    fn parses_legacy_uc_url() {
        assert_eq!(
            parse_drive_target("https://drive.google.com/uc?export=download&id=1ABC_23"),
            DriveTarget::File("1ABC_23".into())
        );
    }

    #[test]
    fn parses_id_first_uc_url() {
        assert_eq!(
            parse_drive_target("https://drive.google.com/uc?id=1ABC_23&export=download"),
            DriveTarget::File("1ABC_23".into())
        );
    }

    #[test]
    fn parses_id_first_usercontent_url() {
        assert_eq!(
            parse_drive_target("https://drive.usercontent.google.com/download?id=1ABC_23"),
            DriveTarget::File("1ABC_23".into())
        );
    }

    #[test]
    fn file_url_fragment_does_not_leak_into_id() {
        assert_eq!(
            parse_drive_target("https://drive.google.com/file/d/1ABC_23/view#junk"),
            DriveTarget::File("1ABC_23".into())
        );
    }

    #[test]
    fn folder_url_fragment_does_not_leak_into_id() {
        assert_eq!(
            parse_drive_target("https://drive.google.com/drive/folders/1ABC_23#junk"),
            DriveTarget::Folder("1ABC_23".into())
        );
    }

    #[test]
    fn parses_usercontent_url() {
        assert_eq!(
            parse_drive_target("https://drive.usercontent.google.com/download?id=1XYZ&export=download&confirm=t"),
            DriveTarget::File("1XYZ".into())
        );
    }

    #[test]
    fn parses_bare_id_as_ambiguous() {
        assert_eq!(
            parse_drive_target("FAKE_DRIVE_FILE_ID_HIST_REDACT001"),
            DriveTarget::Ambiguous("FAKE_DRIVE_FILE_ID_HIST_REDACT001".into())
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_drive_target("http://example.com/"), DriveTarget::Unrecognized);
    }
}
