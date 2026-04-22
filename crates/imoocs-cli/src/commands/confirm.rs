

use std::io::IsTerminal;

use dialoguer::{theme::ColorfulTheme, Confirm};
use imoocs_core::config::{Config, ConfirmMode};
use imoocs_core::ImoocsError;

use super::auth::map_dialoguer_err;

pub enum DestructiveAction<'a> {
    Submit { course: &'a str, problem: &'a str },
    UploadForce { pid: &'a str, filename: &'a str },
}

impl DestructiveAction<'_> {
    fn prompt_text(&self) -> String {
        match self {
            DestructiveAction::Submit { course, problem } => {
                format!("Finalise submission for {course}/{problem}? This cannot be undone.")
            }
            DestructiveAction::UploadForce { pid, filename } => {
                format!("Finalise upload of {filename} as {pid}? This overwrites the previous submission.")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceDecision {
    Force,
    Draft,
    ConfigMissing,
}

/// Pure decision table. `prompt_answer` is what the TTY prompt returned
/// (caller handles the actual `dialoguer` call); `None` for non-TTY.
pub fn decide_force(mode: Option<ConfirmMode>, is_tty: bool, prompt_answer: Option<bool>) -> ForceDecision {
    match mode {
        None => ForceDecision::ConfigMissing,
        Some(ConfirmMode::Auto) => ForceDecision::Force,
        Some(ConfirmMode::Confirm) => {
            if is_tty {
                match prompt_answer {
                    Some(true) => ForceDecision::Force,
                    _ => ForceDecision::Draft,
                }
            } else {
                ForceDecision::Draft
            }
        }
    }
}

/// Runs the prompt where needed and returns the effective `force` flag.
/// Emits a stderr notice when `confirm` downgrades a destructive call to a
/// draft save, so operators understand why the server state didn't change.
pub fn resolve_force(cfg: &Config, action: &DestructiveAction) -> Result<bool, ImoocsError> {
    let mode = cfg.assignment.as_ref().and_then(|a| a.confirm);
    let is_tty = std::io::stdin().is_terminal();

    let prompt_answer = if matches!(mode, Some(ConfirmMode::Confirm)) && is_tty {
        let ans = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(action.prompt_text())
            .default(false)
            .interact()
            .map_err(map_dialoguer_err)?;
        Some(ans)
    } else {
        None
    };

    match decide_force(mode, is_tty, prompt_answer) {
        ForceDecision::Force => Ok(true),
        ForceDecision::Draft => {
            if is_tty {
                eprintln!("[imoocs] declined; saved as draft (force=false).");
            } else {
                eprintln!("[imoocs] non-interactive: saved as draft (force=false). Run from a TTY to finalise.");
            }
            Ok(false)
        }
        ForceDecision::ConfigMissing => Err(ImoocsError::Validation(
            "config `assignment.confirm` is not set. Run `imoocs setup`, or add \
             `[assignment]\\nconfirm = \"auto\"` (or `\"confirm\"`) to your config.toml."
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_is_error() {
        assert_eq!(decide_force(None, true, None), ForceDecision::ConfigMissing);
        assert_eq!(decide_force(None, false, Some(true)), ForceDecision::ConfigMissing);
    }

    #[test]
    fn auto_always_forces() {
        assert_eq!(decide_force(Some(ConfirmMode::Auto), true, None), ForceDecision::Force);
        assert_eq!(decide_force(Some(ConfirmMode::Auto), false, None), ForceDecision::Force);
        assert_eq!(
            decide_force(Some(ConfirmMode::Auto), false, Some(false)),
            ForceDecision::Force,
            "auto ignores any interactive answer"
        );
    }

    #[test]
    fn confirm_tty_yes_forces() {
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), true, Some(true)),
            ForceDecision::Force
        );
    }

    #[test]
    fn confirm_tty_no_drafts() {
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), true, Some(false)),
            ForceDecision::Draft
        );
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), true, None),
            ForceDecision::Draft,
            "missing prompt answer on TTY also drafts"
        );
    }

    #[test]
    fn confirm_non_tty_drafts() {
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), false, None),
            ForceDecision::Draft
        );
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), false, Some(true)),
            ForceDecision::Draft,
            "non-TTY never promotes, regardless of answer (agent protection)"
        );
    }
}
