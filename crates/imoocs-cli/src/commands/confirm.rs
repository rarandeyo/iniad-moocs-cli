//! `assignment submit` / `upload` / `push` のゲート判定。
//!
//! - `submit` / `upload` は [`resolve_submit_gate`] を通って
//!   `SubmitGate::Direct`（auto モード、即サーバ確定）か
//!   `SubmitGate::Stage`（confirm モード、ローカル draft に stage）に分岐する。
//!   どちらも TTY 有無には依存しない。
//! - `push` は [`resolve_push_gate`] が先に config / TTY / 対話プロンプトを
//!   検査し、合意が取れた場合のみ `true` を返す。

use std::io::{ErrorKind, IsTerminal};

use dialoguer::{theme::ColorfulTheme, Confirm};
use imoocs_core::config::{Config, ConfirmMode};
use imoocs_core::ImoocsError;

use super::auth::map_dialoguer_err;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitGate {
    /// auto モード: `submit` / `upload` は即サーバ確定。
    Direct,
    /// confirm モード: `submit` / `upload` はローカル draft に stage される。
    Stage,
}

/// `push` 時にユーザに見せる stage サマリ。
pub struct PushAction<'a> {
    pub course: &'a str,
    pub problem: &'a str,
    pub answer_pids: &'a [String],
    pub file_pids: &'a [(String, String)],
}

impl PushAction<'_> {
    fn prompt_text(&self) -> String {
        // 表示崩れ (TTY overwrite 失敗) を避けるため、1 行に収まる長さに抑える。
        // 詳細 (file 名・pid 一覧) は別途 stderr に事前出力するので、prompt 本体は
        // 「対象 + 何が送られるかの要約」だけ。
        let summary = match (self.answer_pids.is_empty(), self.file_pids.is_empty()) {
            (true, true) => "empty".to_string(),
            (false, true) => format!("answers={}", self.answer_pids.len()),
            (true, false) => format!("files={}", self.file_pids.len()),
            (false, false) => format!(
                "answers={} files={}",
                self.answer_pids.len(),
                self.file_pids.len()
            ),
        };
        format!(
            "Push {course}/{problem}? [{summary}]",
            course = self.course,
            problem = self.problem,
        )
    }

    /// 確認 prompt の前に stderr に出す詳細サマリ (pid 一覧など)。
    pub fn detail_text(&self) -> String {
        let answers = if self.answer_pids.is_empty() {
            "  answers: -".to_string()
        } else {
            format!("  answers: {}", self.answer_pids.join(", "))
        };
        let files = if self.file_pids.is_empty() {
            "  files:   -".to_string()
        } else {
            let pids: Vec<String> = self
                .file_pids
                .iter()
                .map(|(pid, name)| format!("{pid}={name}"))
                .collect();
            format!("  files:   {}", pids.join(", "))
        };
        format!("Push target: {course}/{problem}\n{answers}\n{files}", course = self.course, problem = self.problem)
    }
}

/// submit/upload モード分岐の pure fn。
pub fn decide_submit_gate(mode: Option<ConfirmMode>) -> Result<SubmitGate, ImoocsError> {
    match mode {
        None => Err(ImoocsError::Validation(
            "config `assignment.confirm` is not set. Run `imoocs setup`, or add \
             `[assignment]\\nconfirm = \"auto\"` (or `\"confirm\"`) to your config.toml."
                .into(),
        )),
        Some(ConfirmMode::Auto) => Ok(SubmitGate::Direct),
        Some(ConfirmMode::Confirm) => Ok(SubmitGate::Stage),
    }
}

/// `push` の TTY / config 先頭チェックだけ抽出した pure fn (テスト用)。
/// `Ok(true)` は「TTY でプロンプトを出す段階まで進んで良い」を意味する。
pub fn decide_push_precheck(mode: Option<ConfirmMode>, is_tty: bool) -> Result<(), ImoocsError> {
    if mode.is_none() {
        return Err(ImoocsError::Validation(
            "config `assignment.confirm` is not set. Run `imoocs setup`, or add \
             `[assignment]\\nconfirm = \"auto\"` (or `\"confirm\"`) to your config.toml."
                .into(),
        ));
    }
    if !is_tty {
        return Err(ImoocsError::Validation(
            "`assignment push` must be run from a TTY (interactive shell). \
             Draft is retained; re-run from your terminal to finalise."
                .into(),
        ));
    }
    Ok(())
}

/// Config から submit/upload のゲート判定を取り出す。
pub fn resolve_submit_gate(cfg: &Config) -> Result<SubmitGate, ImoocsError> {
    let mode = cfg.assignment.as_ref().and_then(|a| a.confirm);
    decide_submit_gate(mode)
}

/// `push` のゲート判定。config missing / 非 TTY で早期に Validation エラーを返し、
/// TTY では dialoguer プロンプトで `y` を押した場合のみ `Ok(true)` を返す。
/// `n` / EOF / Ctrl-C 等は Validation エラーにする (draft は呼び出し側で保持)。
pub fn resolve_push_gate(cfg: &Config, action: &PushAction) -> Result<bool, ImoocsError> {
    let mode = cfg.assignment.as_ref().and_then(|a| a.confirm);
    let is_tty = std::io::stdin().is_terminal();
    decide_push_precheck(mode, is_tty)?;

    // 詳細サマリ (pid 一覧) は prompt とは別に stderr に出す。
    // こうすると Confirm の prompt 自体を短くできて TTY overwrite が崩れない。
    eprintln!("{}", action.detail_text());

    let ans = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(action.prompt_text())
        .default(false)
        .interact()
        .map_err(|e| match &e {
            // EOF (Ctrl-D) / 割り込み / TTY 消失はユーザ由来のキャンセルとして扱う。
            // Internal (exit 5) にするとバグ扱いになるので Validation に倒す。
            dialoguer::Error::IO(io_err)
                if matches!(
                    io_err.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::Interrupted | ErrorKind::BrokenPipe
                ) =>
            {
                ImoocsError::Validation(
                    "Push cancelled (prompt interrupted). Draft is retained; \
                     re-run `imoocs assignment push` when ready."
                        .into(),
                )
            }
            _ => map_dialoguer_err(e),
        })?;
    if ans {
        Ok(true)
    } else {
        Err(ImoocsError::Validation(
            "Push cancelled. Draft is retained; re-run `imoocs assignment push` when ready.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_submit_gate_missing_is_error() {
        assert!(decide_submit_gate(None).is_err());
    }

    #[test]
    fn decide_submit_gate_auto_is_direct() {
        assert_eq!(decide_submit_gate(Some(ConfirmMode::Auto)).unwrap(), SubmitGate::Direct);
    }

    #[test]
    fn decide_submit_gate_confirm_is_stage() {
        // confirm モードは TTY/agent 問わず常に Stage (agent safety の key)
        assert_eq!(
            decide_submit_gate(Some(ConfirmMode::Confirm)).unwrap(),
            SubmitGate::Stage
        );
    }

    #[test]
    fn decide_push_precheck_missing_is_error() {
        assert!(decide_push_precheck(None, true).is_err());
        assert!(decide_push_precheck(None, false).is_err());
    }

    #[test]
    fn decide_push_precheck_non_tty_is_error() {
        // auto でも confirm でも、push は TTY 必須
        assert!(decide_push_precheck(Some(ConfirmMode::Auto), false).is_err());
        assert!(decide_push_precheck(Some(ConfirmMode::Confirm), false).is_err());
    }

    #[test]
    fn decide_push_precheck_tty_with_config_passes() {
        assert!(decide_push_precheck(Some(ConfirmMode::Auto), true).is_ok());
        assert!(decide_push_precheck(Some(ConfirmMode::Confirm), true).is_ok());
    }

    #[test]
    fn push_action_prompt_contains_summary() {
        let answer_pids = vec!["p1".to_string(), "p2".to_string()];
        let file_pids = vec![("html".to_string(), "report.html".to_string())];
        let action = PushAction {
            course: "CS101",
            problem: "prob-a",
            answer_pids: &answer_pids,
            file_pids: &file_pids,
        };
        // Phase C-11 で prompt は短縮: 識別子 + answers/files count だけ。
        let prompt = action.prompt_text();
        assert!(prompt.contains("CS101/prob-a"), "prompt should contain target: {prompt}");
        assert!(prompt.contains("answers=2"), "prompt should contain answers count: {prompt}");
        assert!(prompt.contains("files=1"), "prompt should contain files count: {prompt}");
        // 詳細 (pid 一覧 + ファイル名) は detail_text で stderr に出す。
        let detail = action.detail_text();
        assert!(detail.contains("CS101/prob-a"));
        assert!(detail.contains("p1"));
        assert!(detail.contains("p2"));
        assert!(detail.contains("html=report.html"));
    }
}
