//! Walking Skeleton (3 本): outside-in で risky な末端を 1 本ずつ貫通させる。
//!
//! 1. **配管**: binary が起動して `--version` が `imoocs <ver>` を吐く
//!    (TEST_LIST 1.1)
//! 2. **`auth *` text-only 契約**: `--format json` を渡しても auth status は
//!    text + exit 2 にフォールバックする (TEST_LIST 2.2)
//! 3. **confirm モード stage 契約**: `[assignment] confirm = "confirm"` 設定下で
//!    `submit` がネットワークを叩かず `$XDG_STATE_HOME/imoocs/drafts/` に
//!    draft を書き出す (TEST_LIST 4.2)
//!
//! 配管テストは XDG に触れない。残り 2 本は `TempXdg` 配下で動くので、
//! 開発者の実 cookies / drafts は読み書きされない。

use predicates::prelude::*;

use super::common::{assert_success_envelope, imoocs, imoocs_in, TempXdg, CONFIG_CONFIRM};

#[test]
fn version_flag_prints_imoocs_prefix() {
    // 1.1: `imoocs --version` → exit 0, stdout が "imoocs " で始まる
    imoocs()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("imoocs "));
}

#[test]
fn auth_status_ignores_format_json_and_exits_with_code_2_when_logged_out() {
    // 2.2: `auth *` は契約として text-only。`--format json` を渡しても
    // 人間向けの status 行が出力され、構造化された signal は exit code
    // (2 = AUTH_EXPIRED) のみ。
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .args(["--format", "json", "auth", "status"])
        .assert()
        .code(2);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        !stdout.trim_start().starts_with('{'),
        "auth status with --format json must NOT emit a JSON envelope, got:\n{stdout}"
    );
}

#[test]
fn submit_in_confirm_mode_stages_draft_to_xdg_state_home() {
    // 4.2: `[assignment] confirm = "confirm"` で `submit` は HTTP を叩かず
    // XDG_STATE_HOME に draft を書き、staged=true / submitted=false を返す。
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_CONFIRM);

    let course = "CS101";
    let problem = "prob-a";
    let pid = "p1";
    let value = "hello-walking-skeleton";
    let data = format!(r#"{{"{pid}":"{value}"}}"#);

    let assert = imoocs_in(&xdg)
        // --year を明示しないと resolve_key が resolve_latest_year で
        // ネットワークに出るので、Walking Skeleton では env で固定する。
        .env("IMOOCS_YEAR", "2026")
        .args([
            "--format",
            "json",
            "assignment",
            "submit",
            course,
            problem,
            "--data",
            &data,
        ])
        .assert()
        .success();

    let stdout = assert.get_output().stdout.clone();
    let data_value = assert_success_envelope(&stdout);
    assert_eq!(
        data_value.get("staged").and_then(|v| v.as_bool()),
        Some(true),
        "envelope.data.staged should be true:\n{data_value:#}"
    );
    assert_eq!(
        data_value.get("submitted").and_then(|v| v.as_bool()),
        Some(false),
        "envelope.data.submitted should be false (HTTP not yet hit):\n{data_value:#}"
    );

    // year は CLI が IMOOCS_YEAR=2026 から拾うはず → ファイル名は
    // `2026-CS101-prob-a.json` で確定するが、過剰な結合を避けるため
    // 「.json が 1 つ存在 + 中身に answer が含まれる」だけ検証する。
    let dir = xdg.drafts_dir();
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("drafts dir {dir:?} should exist after submit: {e}"))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "exactly one draft should be staged, got {} entries in {dir:?}",
        entries.len()
    );
    let draft_path = entries[0].path();
    let draft_body = std::fs::read_to_string(&draft_path).expect("read draft");
    let draft_json: serde_json::Value = serde_json::from_str(&draft_body).expect("draft is valid JSON");
    assert_eq!(
        draft_json
            .get("answers")
            .and_then(|a| a.get(pid))
            .and_then(|v| v.as_str()),
        Some(value),
        "draft should contain the submitted answer:\n{draft_json:#}"
    );
}
