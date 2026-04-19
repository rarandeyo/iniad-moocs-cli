use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use imoocs_core::{
    api,
    envelope::ErrorDetail,
    paths::Paths,
    session::Session,
};

use crate::cli::GlobalArgs;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum LessonCommand {
    /// Show a lesson's page content (markdown body + embeds).
    Show {
        /// Course id (e.g. `INI301`).
        course_id: String,
        /// Lesson id (e.g. `DS-00`, `AI-01`).
        lesson_id: String,
        /// Page id (e.g. `01`, `atnd`, `exercise`). Default: first page.
        #[arg(long)]
        page: Option<String>,
    },
}

pub async fn run(global: &GlobalArgs, cmd: LessonCommand) -> Result<ExitCode> {
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
        LessonCommand::Show {
            course_id,
            lesson_id,
            page,
        } => {
            match api::get_lesson_page(&session, year, &course_id, &lesson_id, page.as_deref()).await {
                Ok(content) => {
                    output::emit_success(content, global.format);
                    Ok(ExitCode::from(0))
                }
                Err(err) => {
                    output::emit_failure::<serde_json::Value>(&ErrorDetail::from_error(&err));
                    Ok(ExitCode::from(err.exit_code().as_u8()))
                }
            }
        }
    }
}
