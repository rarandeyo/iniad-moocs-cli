//! assignment push 契約 (TEST_LIST 章 5)
//!
//! 5.1 のみ assert_cmd で書ける (非 TTY 起動)。5.2 (PTY + draft 無し → exit 4)
//! と 5.3 (PTY + n → exit 3) は PTY (rexpect) 必須なので別セッションで追加する。

use super::common::{assert_failure_envelope, imoocs_in, TempXdg, CONFIG_CONFIRM};

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
            "CS101",
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

    // push を非 TTY で叩く → TTY エラー (commands/confirm.rs:82-87 由来)
    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", "2026")
        .args(["assignment", "push", "CS101", "prob-a"])
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
    assert_eq!(
        count_drafts(),
        1,
        "draft must be retained after non-TTY push failure"
    );
}
