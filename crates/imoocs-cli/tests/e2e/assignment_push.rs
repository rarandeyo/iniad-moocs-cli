//! assignment push 契約 (TEST_LIST 章 5)
//!
//! 5.1 は assert_cmd 経由 (非 TTY 起動)。5.2 / 5.3 は PTY (rexpect) 経由。
//! `y` 確定の本送信は 9.2 destructive で扱う。

use super::common::{assert_failure_envelope, imoocs_in, TempXdg, CONFIG_CONFIRM};

#[cfg(target_os = "linux")]
use rexpect::process::wait::WaitStatus;

#[cfg(target_os = "linux")]
use super::common::pty::imoocs_pty_in_with_env;

#[test]
fn push_in_non_tty_mode_with_staged_draft_exits_3_and_keeps_draft() {
    // 5.1: assert_cmd は子プロセスを非 TTY で起動するので、config OK でも
    // push は TTY チェックで exit 3 + "must be run from a TTY" を返す。
    // draft はサーバ送信されず残存する (agent safety の本丸契約)。
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);

    // 4.2 と同等の stage で draft を 1 件作る。
    imoocs_in(&xdg)
        .env("IMOOCS_YEAR", "2026")
        .args([
            "--format",
            "json",
            "assignment",
            "submit",
            "--url",
            "https://moocs.iniad.org/courses/2026/CS101/L1/P1",
            "--problem-id",
            "prob-a",
            "--data",
            r#"{"p1":"hello"}"#,
        ])
        .assert()
        .success();

    let dir = xdg.drafts_dir();
    let count_drafts = || -> usize {
        std::fs::read_dir(&dir)
            .map(|it| {
                it.filter_map(Result::ok)
                    .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(count_drafts(), 1, "stage should produce exactly 1 draft");

    // push を非 TTY で叩く (引数なしで全 draft 対象) → TTY エラー
    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", "2026")
        .args(["assignment", "push"])
        .assert()
        .code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("must be run from a TTY"),
        "expected `must be run from a TTY`: {view:?}"
    );

    // 重要: TTY エラーでも draft は残存していなければならない
    // (agent safety の核: サーバに何も送らず、ユーザが TTY から再 push できる)
    assert_eq!(count_drafts(), 1, "draft must be retained after non-TTY push failure");
}

#[cfg(target_os = "linux")]
#[test]
fn push_in_pty_with_no_staged_draft_exits_4_not_found() {
    // 5.2: PTY 起動 + draft 無し → TTY チェックは通過、resolve_key も通過、
    // Draft::load が None を返すので exit 4 (NOT_FOUND) +
    // "no draft staged" を message に含む
    // (commands/assignment.rs:399 由来)
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);

    // Phase C-10: push は引数なしで全 draft 一括送信。draft 0 件なら NOT_FOUND。
    let session = imoocs_pty_in_with_env(
        &xdg,
        &["assignment", "push"],
        &[("IMOOCS_YEAR", "2026")],
        5_000,
    )
    .expect("spawn pty");

    // プロンプトは出ずに即終了するはずなので EOF まで待つ。
    let status = session.process.wait().expect("wait child");
    match status {
        WaitStatus::Exited(_, 4) => {}
        other => panic!("expected exit 4 (NOT_FOUND), got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn push_in_pty_with_n_response_cancels_and_keeps_draft() {
    // 5.3: PTY + draft あり + プロンプトに `n` → exit 3 + draft 残存
    // (resolve_push_gate L130-132 の "Push cancelled" Validation エラー)
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);

    // 4.2 と同等の stage で draft 1 件作る (assert_cmd 経由)
    imoocs_in(&xdg)
        .env("IMOOCS_YEAR", "2026")
        .args([
            "--format",
            "json",
            "assignment",
            "submit",
            "--url",
            "https://moocs.iniad.org/courses/2026/CS101/L1/P1",
            "--problem-id",
            "prob-a",
            "--data",
            r#"{"p1":"hello"}"#,
        ])
        .assert()
        .success();

    let dir = xdg.drafts_dir();
    let count_drafts = || -> usize {
        std::fs::read_dir(&dir)
            .map(|it| {
                it.filter_map(Result::ok)
                    .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(count_drafts(), 1, "stage should produce 1 draft");

    // PTY で push 起動 (引数なし) → "Push CS101/prob-a?" プロンプトを expect → "n" 送信
    let mut session = imoocs_pty_in_with_env(
        &xdg,
        &["assignment", "push"],
        &[("IMOOCS_YEAR", "2026")],
        5_000,
    )
    .expect("spawn pty");

    // Phase C-11: prompt は短縮されて `Push CS101/prob-a? [answers=1]` 形式。
    // ANSI escape が混じる可能性があるので regex で。
    session.exp_regex(r"Push CS101/prob-a\?").expect("see push prompt");
    session.send_line("n").expect("send n");

    let status = session.process.wait().expect("wait child");
    match status {
        WaitStatus::Exited(_, 3) => {}
        other => panic!("expected exit 3 (cancelled), got {other:?}"),
    }

    // draft 残存 (push 拒否で送信されない)
    assert_eq!(count_drafts(), 1, "draft must be retained after `n` response");
}
