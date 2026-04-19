use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api::slides::fetch_slide_pdf_with_dump,
    envelope::ErrorDetail,
    paths::Paths,
    session::Session,
    ImoocsError,
};
use serde::Serialize;
use schemars::JsonSchema;

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum SlideCommand {
    /// Download a Google Slides pubembed and write a merged PDF to the cache
    /// (or a custom path via `--out`). Returns the local path.
    Fetch {
        /// The iframe `src` from a lesson page (usually `docs.google.com/.../pubembed`).
        embed_url: String,
        /// Copy the resulting PDF to this path in addition to caching.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Force re-download even when the cache is fresh.
        #[arg(long)]
        no_cache: bool,
        /// Debug: dump raw pubembed HTML and extracted SVGs under this directory.
        #[arg(long, hide = true)]
        dump_svgs: Option<PathBuf>,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FetchReport {
    embed_url: String,
    local_pdf_path: PathBuf,
    size_bytes: u64,
    page_count: u32,
    fetched_at: String,
    from_cache: bool,
}

pub async fn run(global: &GlobalArgs, cmd: SlideCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths.clone_paths())?;

    match cmd {
        SlideCommand::Fetch { embed_url, out, no_cache, dump_svgs } => {
            match fetch_slide_pdf_with_dump(
                &session,
                &paths,
                &embed_url,
                no_cache,
                dump_svgs.as_deref(),
            )
            .await
            {
                Ok(res) => {
                    if let Some(dest) = out {
                        if let Some(parent) = dest.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::copy(&res.local_pdf_path, &dest) {
                            return Ok(emit_err(ImoocsError::Io(e)));
                        }
                    }
                    output::emit_success(
                        FetchReport {
                            embed_url,
                            local_pdf_path: res.local_pdf_path,
                            size_bytes: res.size_bytes,
                            page_count: res.page_count,
                            fetched_at: res.fetched_at,
                            from_cache: res.from_cache,
                        },
                        global.format,
                    );
                    Ok(ExitCode::from(0))
                }
                Err(e) => Ok(emit_err(e)),
            }
        }
    }
}

fn emit_err(err: ImoocsError) -> ExitCode {
    let code = err.exit_code().as_u8();
    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
    ExitCode::from(code)
}
