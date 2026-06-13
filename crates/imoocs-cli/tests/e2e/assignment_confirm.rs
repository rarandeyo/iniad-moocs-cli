//! assignment confirm モード stage 契約 (TEST_LIST 章 4)
//!
//! 4.2 は walking_skeleton.rs で済み。残り 4 件 (4.1, 4.3-4.5)。
//! HTTP 不要、env 不要 (year は IMOOCS_YEAR=2026 で固定)。
//!
//! 4.3-4.4 は draft が 1 件だけ作られて answer が読み戻せることを共通の
//! `assert_single_draft_with_answer` で検証。4.1 と 4.5 は失敗 envelope の
//! `error.code` と `error.message` を assert する純粋なエラーテスト。

use serde_json::Value;

use super::common::{assert_failure_envelope, assert_success_envelope, imoocs_in, TempXdg, CONFIG_CONFIRM};

/// 4.2-4.4 で共通: stage された draft が `staged=true` を返し、
/// `<XDG_STATE_HOME>/imoocs/drafts/` に JSON 1 件だけ存在し、その中身に
/// 期待する answer が入っていることを assert する。
fn assert_single_draft_with_answer(xdg: &TempXdg, expected_pid: &str, expected_value: &str, data: &Value) {
    assert_eq!(
        data.get("staged").and_then(Value::as_bool),
        Some(true),
        "envelope.data.staged should be true:\n{data:#}"
    );
    let dir = xdg.drafts_dir();
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("drafts dir {dir:?} should exist: {e}"))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly 1 draft, got {} in {dir:?}",
        entries.len()
    );
    let body = std::fs::read_to_string(entries[0].path()).expect("read draft");
    let json: Value = serde_json::from_str(&body).expect("draft is JSON");
    assert_eq!(
        json.get("answers")
            .and_then(|a| a.get(expected_pid))
            .and_then(Value::as_str),
        Some(expected_value),
        "draft.answers[{expected_pid:?}] should be {expected_value:?}:\n{json:#}"
    );
}

#[test]
fn submit_without_confirm_config_emits_validation_error_with_hint() {
    // 4.1: config 未配置 (assignment.confirm = None) → exit 3 +
    // VALIDATION_ERROR + message に "assignment.confirm" を含む
    // (decide_submit_gate(None) -> Err、commands/confirm.rs:140-143 由来)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", "2026")
        .args([
            "assignment",
            "submit",
            "--url",
            "https://moocs.iniad.org/courses/2026/CS101/L1/P1",
            "--problem-id",
            "prob-a",
            "--data",
            "{}",
        ])
        .assert()
        .code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("assignment.confirm"),
        "message should mention `assignment.confirm`: {view:?}"
    );
}

#[test]
fn submit_with_data_at_path_stages_draft_from_file() {
    // 4.3: `--data @<path>` で file 経由のデータも 4.2 と同じく stage される
    // (parse_data の @ 接頭辞経路、commands/assignment.rs:656)
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);
    let pid = "p2";
    let value = "from-file";
    let data_file = xdg.home.join("answer.json");
    std::fs::write(&data_file, format!(r#"{{"{pid}":"{value}"}}"#)).expect("write answer.json");
    let at_arg = format!("@{}", data_file.display());

    let assert = imoocs_in(&xdg)
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
            &at_arg,
        ])
        .assert()
        .success();

    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_single_draft_with_answer(&xdg, pid, value, &data);
}

#[test]
fn submit_with_data_dash_reads_payload_from_stdin() {
    // 4.4: `--data -` で stdin 経由のデータも stage される
    // (parse_data の "-" 経路、commands/assignment.rs:650)
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);
    let pid = "p3";
    let value = "from-stdin";
    let payload = format!(r#"{{"{pid}":"{value}"}}"#);

    let assert = imoocs_in(&xdg)
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
            "-",
        ])
        .write_stdin(payload)
        .assert()
        .success();

    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_single_draft_with_answer(&xdg, pid, value, &data);
}

#[test]
fn submit_with_invalid_json_payload_emits_validation_error() {
    // 4.5: `--data 'not-json'` → exit 3 + VALIDATION_ERROR +
    // "invalid JSON in --data" を含むメッセージ
    // (parse_data の serde_json::from_str エラー、commands/assignment.rs:661)
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);
    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", "2026")
        .args([
            "assignment",
            "submit",
            "--url",
            "https://moocs.iniad.org/courses/2026/CS101/L1/P1",
            "--problem-id",
            "prob-a",
            "--data",
            "not-json",
        ])
        .assert()
        .code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("invalid JSON in --data"),
        "expected `invalid JSON in --data`: {view:?}"
    );
}
