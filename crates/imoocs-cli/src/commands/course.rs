use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{api, envelope::ErrorDetail, paths::Paths, session::Session};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum CourseCommand {
    /// 指定 year のコース一覧を返す (デフォルトは最新)。
    #[command(visible_alias = "ls")]
    List,
    /// コースの lesson tree (sidebar から) を表示する。
    Show {
        /// コース id (例: `INI301`)。
        course_id: String,
    },
}

pub async fn run(global: &GlobalArgs, cmd: CourseCommand) -> Result<ExitCode> {
    let paths = Paths::discover()?;
    let session = Session::new(paths)?;

    let year = match global.year {
        Some(y) => y,
        None => match api::resolve_latest_year(&session).await {
            Ok(y) => y,
            Err(err) => {
                output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
                return Ok(ExitCode::from(err.exit_code().as_u8()));
            }
        },
    };

    match cmd {
        CourseCommand::List => match api::get_course_list(&session, Some(year)).await {
            Ok(list) => {
                output::emit_success(list, global.format);
                Ok(ExitCode::from(0))
            }
            Err(err) => {
                output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
                Ok(ExitCode::from(err.exit_code().as_u8()))
            }
        },
        CourseCommand::Show { course_id } => match api::get_course_detail(&session, year, &course_id).await {
            Ok(detail) => {
                output::emit_success(detail, global.format);
                Ok(ExitCode::from(0))
            }
            Err(err) => {
                output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
                Ok(ExitCode::from(err.exit_code().as_u8()))
            }
        },
    }
}
