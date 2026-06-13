//! assignment drafts (TEST_LIST 章 6)
//!
//! HTTP 不要、env 不要 (year は submit 側で IMOOCS_YEAR=2026 固定)。
//! 6.2 は confirm submit で 1 件 stage → drafts list で読み戻す統合検証。

use serde_json::Value;

use super::common::{assert_failure_envelope, assert_success_envelope, imoocs_in, TempXdg, CONFIG_CONFIRM};

#[test]
fn drafts_list_in_clean_xdg_returns_empty_array() {
    // 6.1: 何も stage してない状態 → exit 0, envelope.data == []
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .args(["--format", "json", "assignment", "drafts", "list"])
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    let arr = data.as_array().unwrap_or_else(|| panic!("expected array: {data:#}"));
    assert!(arr.is_empty(), "expected empty array, got: {data:#}");
}

#[test]
fn drafts_list_after_confirm_submit_returns_one_summary() {
    // 6.2: confirm submit で 1 件 stage → drafts list に DraftSummary 1 件
    // (DraftSummary shape: schema.md L173-181 = year, courseId, problemId,
    //  answerPids, filePids, updatedAt, path)
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);

    let course = "CS101";
    let problem = "prob-a";
    let pid = "p1";
    let value = "draft-for-list";
    let data_arg = format!(r#"{{"{pid}":"{value}"}}"#);

    // Stage 1 件
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
            problem,
            "--data",
            &data_arg,
        ])
        .assert()
        .success();
    let _ = course; // course is encoded in --url

    // List で 1 件読み戻す
    let assert = imoocs_in(&xdg)
        .args(["--format", "json", "assignment", "drafts", "list"])
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    let arr = data.as_array().unwrap_or_else(|| panic!("expected array: {data:#}"));
    assert_eq!(arr.len(), 1, "expected 1 summary, got: {data:#}");

    let summary = &arr[0];
    assert_eq!(summary.get("courseId").and_then(Value::as_str), Some(course));
    assert_eq!(summary.get("problemId").and_then(Value::as_str), Some(problem));
    assert_eq!(summary.get("year").and_then(Value::as_u64), Some(2026));
    let answer_pids = summary
        .get("answerPids")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("expected answerPids array: {summary:#}"));
    assert_eq!(answer_pids.len(), 1);
    assert_eq!(answer_pids[0].as_str(), Some(pid));
    assert!(
        summary.get("path").and_then(Value::as_str).is_some(),
        "DraftSummary should have a `path` field: {summary:#}"
    );
    assert!(
        summary.get("updatedAt").and_then(Value::as_str).is_some(),
        "DraftSummary should have an `updatedAt` field: {summary:#}"
    );
}

#[test]
fn drafts_clear_without_args_emits_validation_error_listing_options() {
    // 6.3: drafts clear (引数なし) → exit 3 + VALIDATION_ERROR +
    // "requires" / "--all" を含むメッセージ
    // (commands/assignment.rs:537-541 の runtime check 由来)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg).args(["assignment", "drafts", "clear"]).assert().code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("requires") && view.message.contains("--all"),
        "message should mention `requires` and `--all`: {view:?}"
    );
}
