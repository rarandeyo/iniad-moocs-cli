//! `imoocs drive list|fetch` — Drive folder/file access via the session's SAML cookie.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api::drive::{fetch_drive_file, list_drive_folder},
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
    /// List items in a Drive folder by scraping `window['_DRIVE_ivd']`.
    ///
    /// Accepts `/drive/folders/<id>` URLs or a bare folder ID.
    List {
        /// `/drive/folders/<id>` URL or folder ID.
        target: String,
    },
    /// Download a single Drive file into the cache.
    ///
    /// Accepts `/file/d/<id>/...` URLs, `/uc?export=download&id=<id>` URLs,
    /// `drive.usercontent.google.com/download?id=<id>` URLs, or a bare file ID.
    Fetch {
        /// `/file/d/<id>/(view|preview)?`, `/uc?...&id=<id>`, or file ID.
        target: String,
        /// Copy the downloaded file to this path in addition to caching.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Force re-download, ignoring the 24h cache.
        #[arg(long)]
        no_cache: bool,
    },
}

pub async fn run(global: &GlobalArgs, cmd: DriveCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths.clone_paths())?;

    match cmd {
        DriveCommand::List { target } => {
            let folder_id = match parse_drive_target(&target) {
                DriveTarget::Folder(id) | DriveTarget::Ambiguous(id) => id,
                DriveTarget::File(_) => {
                    return Ok(emit_err(ImoocsError::Validation(
                        "target looks like a Drive FILE URL; use `imoocs drive fetch` instead"
                            .into(),
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
        DriveCommand::Fetch {
            target,
            out,
            no_cache,
        } => {
            let file_id = match parse_drive_target(&target) {
                DriveTarget::File(id) | DriveTarget::Ambiguous(id) => id,
                DriveTarget::Folder(_) => {
                    return Ok(emit_err(ImoocsError::Validation(
                        "target looks like a Drive FOLDER URL; use `imoocs drive list` instead"
                            .into(),
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
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DriveTarget {
    File(String),
    Folder(String),
    /// A bare ID (no URL) — can't tell file vs folder, caller decides by command.
    Ambiguous(String),
    Unrecognized,
}

// All patterns exclude `#` from the id capture so fragment anchors
// (e.g. `/file/d/<id>#foo`) don't contaminate the fileId.
static FILE_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https://drive\.google\.com/file/d/([^/?#]+)").unwrap()
});
static FOLDER_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https://drive\.google\.com/drive/folders/([^/?#]+)").unwrap()
});
// `(?:[^#]*&)?id=` makes the prefix optional so both `uc?id=X` (id-first) and
// `uc?export=download&id=X` (id-last) are accepted.
static UC_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https://drive\.google\.com/uc\?(?:[^#]*&)?id=([^&#]+)").unwrap()
});
static USERCONTENT_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https://drive\.usercontent\.google\.com/download\?(?:[^#]*&)?id=([^&#]+)")
        .unwrap()
});
static BARE_ID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9_-]{25,64}$").unwrap());

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
