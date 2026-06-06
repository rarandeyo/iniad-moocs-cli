//! 配管・契約 (TEST_LIST 章 1, 2)
//!
//! 副作用なし、env 不要。clap レイヤと envelope の untagged enum 形を押さえる。
//! 1.1 と 2.2 は walking_skeleton.rs で済んでいるので、残り 7 件をここに置く。

use serde_json::Value;

use super::common::{assert_failure_envelope, assert_success_envelope, imoocs, imoocs_in, TempXdg};

/// 12 サブコマンド (`cli.rs` の `Command` enum と同期)。
const ALL_SUBCOMMANDS: &[&str] = &[
    "version",
    "doctor",
    "auth",
    "course",
    "lesson",
    "assignment",
    "slide",
    "drive",
    "open",
    "reset",
    "setup",
    "completion",
];

#[test]
fn help_lists_all_twelve_subcommands() {
    // 1.2: `imoocs --help` → exit 0, stdout に 12 サブコマンド名すべて
    let assert = imoocs().arg("--help").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    for sub in ALL_SUBCOMMANDS {
        assert!(
            stdout.contains(sub),
            "help should mention `{sub}`:\n--- stdout ---\n{stdout}"
        );
    }
}

#[test]
fn unknown_subcommand_fails_with_clap_error_on_stderr() {
    // 1.3: `imoocs unknown-cmd` → exit ≠ 0 (clap), stderr に "unrecognized" / "unknown"
    let assert = imoocs().arg("definitely-not-a-real-cmd").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("unrecognized") || stderr.contains("unknown"),
        "clap should reject unknown subcommand on stderr:\n{stderr}"
    );
}

#[test]
fn version_subcommand_emits_json_envelope_even_in_text_mode() {
    // 1.4: `imoocs version` → 常に JSON envelope (--format text でも JSON)
    // emit_success が mode を無視する契約 (output.rs:62)。
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg).arg("version").assert().success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_eq!(data.get("name").and_then(Value::as_str), Some("imoocs"));
    let version = data
        .get("version")
        .and_then(Value::as_str)
        .expect("envelope.data.version");
    assert!(!version.is_empty(), "version should not be empty: {data:#}");
}

#[test]
fn invalid_format_value_is_clap_error() {
    // 1.5: `imoocs --format yaml version` → exit ≠ 0 (clap value_enum)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg).args(["--format", "yaml", "version"]).assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("invalid value") || stderr.contains("possible values"),
        "clap should reject invalid --format on stderr:\n{stderr}"
    );
}

#[test]
fn failure_envelope_is_a_single_json_object() {
    // 2.1: 失敗 envelope の stdout が single JSON object である。
    // config 未配置 → assignment.confirm == None → exit 3 / VALIDATION_ERROR
    // (commands/confirm.rs:140-143 decide_submit_gate(None) -> Err)
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
    assert_eq!(
        view.code, "VALIDATION_ERROR",
        "expected VALIDATION_ERROR for missing assignment.confirm config, got {view:?}"
    );
}

#[test]
fn success_envelope_has_no_error_key() {
    // 2.3: success envelope に `error` キーが無い (untagged enum / SuccessFlag 由来)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg).arg("version").assert().success();
    let envelope: Value = serde_json::from_slice(&assert.get_output().stdout).expect("envelope is JSON");
    assert!(
        envelope.get("error").is_none(),
        "success envelope must not contain `error`:\n{envelope:#}"
    );
    assert_eq!(
        envelope.get("success").and_then(Value::as_bool),
        Some(true),
        "success envelope must have `success: true`:\n{envelope:#}"
    );
}

#[test]
fn failure_envelope_has_no_data_key() {
    // 2.4: failure envelope に `data` キーが無い (FailureFlag 由来)
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
    let envelope: Value = serde_json::from_slice(&assert.get_output().stdout).expect("envelope is JSON");
    assert!(
        envelope.get("data").is_none(),
        "failure envelope must not contain `data`:\n{envelope:#}"
    );
    assert_eq!(
        envelope.get("success").and_then(Value::as_bool),
        Some(false),
        "failure envelope must have `success: false`:\n{envelope:#}"
    );
}
