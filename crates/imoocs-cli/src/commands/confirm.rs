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
    Cancelled,
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
                    _ => ForceDecision::Cancelled,
                }
            } else {
                ForceDecision::Cancelled
            }
        }
    }
}

/// Runs the prompt where needed and returns the effective `force` flag.
/// Returns a Validation error without calling the API when the user declines
/// the confirmation prompt or when running non-interactively under
/// `confirm` mode — the server is left untouched.
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
        ForceDecision::Cancelled => {
            let msg = if is_tty {
                "Confirmation declined. Nothing was sent to the server."
            } else {
                "Confirmation required but running non-interactively. Run from a TTY, \
                 or set `assignment.confirm = \"auto\"` in config.toml to let agents finalise."
            };
            Err(ImoocsError::Validation(msg.into()))
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
    fn confirm_tty_no_cancels() {
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), true, Some(false)),
            ForceDecision::Cancelled
        );
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), true, None),
            ForceDecision::Cancelled,
            "missing prompt answer on TTY also cancels"
        );
    }

    #[test]
    fn confirm_non_tty_cancels() {
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), false, None),
            ForceDecision::Cancelled
        );
        assert_eq!(
            decide_force(Some(ConfirmMode::Confirm), false, Some(true)),
            ForceDecision::Cancelled,
            "non-TTY never promotes, regardless of answer (agent protection)"
        );
    }
}
