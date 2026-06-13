pub mod assignments;
pub mod drive;
pub mod moocs;
pub mod slides;

pub use assignments::{
    get_answers, get_assessment, get_assignment_detail, get_file, get_problem_html, get_status, post_file, put_answers,
};
pub use moocs::{
    get_course_detail, get_course_list, get_lesson_page, get_lesson_with_assignments, list_course_assignments,
    resolve_latest_year,
};

use std::path::PathBuf;

use crate::error::{ImoocsError, Result};

/// agent-browser バイナリの場所を解決する。write 系 / drive / slides が依存する。
pub(crate) fn agent_binary() -> Result<PathBuf> {
    imoocs_browser::discover_binary().ok_or_else(|| ImoocsError::Auth {
        reason: "agent-browser binary missing".into(),
        hint: Some("install agent-browser via `imoocs setup` or `npm i -g agent-browser`".into()),
    })
}

/// `imoocs_browser::BrowserError` をドメインエラーに変換する。
pub(crate) fn map_browser_err(err: imoocs_browser::BrowserError) -> ImoocsError {
    use imoocs_browser::BrowserError as E;
    match err {
        E::BinaryMissing => ImoocsError::Auth {
            reason: "agent-browser binary missing".into(),
            hint: Some("run `imoocs setup` to install agent-browser".into()),
        },
        E::AuthProfileMissing { name } => ImoocsError::Auth {
            reason: format!("auth profile `{name}` not found"),
            hint: Some("run `imoocs auth login` first".into()),
        },
        E::ChallengeRequired { current_url } => ImoocsError::Auth {
            reason: format!("auth challenge required at {current_url}"),
            hint: Some("complete the challenge interactively in a browser".into()),
        },
        E::CommandFailed(msg) => ImoocsError::Api(format!("agent-browser command failed: {msg}")),
        E::Spawn(e) => ImoocsError::Internal(format!("agent-browser spawn error: {e}")),
        E::NonZeroExit { code, stderr } => ImoocsError::Internal(format!("agent-browser exited {code:?}: {stderr}")),
        E::Json(e) => ImoocsError::Internal(format!("agent-browser JSON parse: {e}")),
        E::Internal(msg) => ImoocsError::Internal(msg),
    }
}
