use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api::slides::fetch_slide_pdf_with_dump, envelope::ErrorDetail, paths::Paths, session::Session, ImoocsError,
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum SlideCommand {
    /// Google Slides pubembed を取得し、統合 PDF を cache directory に書き、
    /// ローカル path を返す。
    Fetch {
        /// lesson ページの iframe `src` (通常は `docs.google.com/.../pubembed`)。
        embed_url: String,
        /// この呼び出しに限り、スライド cache directory を上書きする。
        /// `cache`, `tmp`, または絶対パスを受け付ける。未指定時は
        /// `config.toml [slides] out_dir` → 組み込みデフォルト (`tmp`) の順に fallback。
        #[arg(long)]
        out_dir: Option<String>,
        /// cache が新しくても強制的に再取得する。
        #[arg(long)]
        no_cache: bool,
        /// デバッグ用: pubembed の生 HTML と抽出した SVG をこの directory に dump する。
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

    match cmd {
        SlideCommand::Fetch {
            embed_url,
            out_dir,
            no_cache,
            dump_svgs,
        } => {
            let paths = match super::apply_slides_config(paths, out_dir.as_deref()) {
                Ok(p) => p,
                Err(e) => return Ok(emit_err(e)),
            };
            let session = Session::new(paths.clone_paths())?;
            match fetch_slide_pdf_with_dump(&session, &paths, &embed_url, no_cache, dump_svgs.as_deref()).await {
                Ok(res) => {
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
