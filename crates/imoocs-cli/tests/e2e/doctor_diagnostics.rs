//! doctor / config の診断 (TEST_LIST 章 3)
//!
//! 旧 `tests/diagnostics.rs` の 3 件 (3.2-3.4) を common helper 経由に書き直して
//! 移植 + 3.1 (clean XDG での doctor) を新規追加。旧ファイルはこの commit で
//! 削除される。

use serde_json::Value;

use super::common::{assert_failure_envelope, assert_success_envelope, imoocs_in, TempXdg};

#[test]
fn doctor_with_clean_xdg_reports_unauthenticated_and_exits_2() {
    // 3.1: 認証情報も config も無い状態で doctor → exit 2 (AUTH_EXPIRED) +
    // success envelope (data.moocsAuthenticated=false)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg).args(["doctor", "--format", "json"]).assert().code(2);
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_eq!(
        data.get("moocsAuthenticated").and_then(Value::as_bool),
        Some(false),
        "expected moocsAuthenticated=false in clean XDG, got: {data:#}"
    );
}

#[test]
fn doctor_fails_on_invalid_config_with_exit_5() {
    // 3.2: config TOML 不正 → exit 5 + envelope success:false +
    // error.message に "config toml parse error" を含む
    // (移植元: 旧 tests/diagnostics.rs L30 doctor_fails_on_invalid_config)
    let xdg = TempXdg::new();
    xdg.write_config("not = [valid\n");
    let assert = imoocs_in(&xdg).args(["doctor", "--format", "json"]).assert().code(5);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert!(
        view.message.contains("config toml parse error"),
        "expected `config toml parse error` in message, got: {view:?}"
    );
}

#[test]
fn doctor_fails_on_invalid_drive_folder_toml_with_exit_5() {
    // 3.3: course-drive-folders.toml 不正 → exit 5 +
    // "course-drive-folders.toml parse error"
    // (移植元: 旧 diagnostics.rs L43 doctor_fails_on_invalid_drive_folder_toml)
    let xdg = TempXdg::new();
    xdg.write_config("[assignment]\nconfirm = \"confirm\"\n");
    xdg.write_drive_folders("driveRootFolderId = 123\n");
    let assert = imoocs_in(&xdg).args(["doctor", "--format", "json"]).assert().code(5);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert!(
        view.message.contains("course-drive-folders.toml parse error"),
        "expected `course-drive-folders.toml parse error` in message, got: {view:?}"
    );
}

#[test]
fn auth_status_surfaces_invalid_config_on_stderr_without_fake_summary() {
    // 3.4: config 不正で auth status → exit 5 + stderr に "認証状態確認失敗" +
    // "config toml parse error"。stdout に "MOOCs login" を含まない
    // (fake summary を出さない契約)
    // (移植元: 旧 diagnostics.rs L68 auth_status_surfaces_invalid_config)
    let xdg = TempXdg::new();
    xdg.write_config("not = [valid\n");
    let assert = imoocs_in(&xdg).args(["auth", "status"]).assert().code(5);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stderr.contains("認証状態確認失敗"),
        "stderr should contain `認証状態確認失敗`:\n{stderr}"
    );
    assert!(
        stderr.contains("config toml parse error"),
        "stderr should contain `config toml parse error`:\n{stderr}"
    );
    assert!(
        !stdout.contains("MOOCs login"),
        "auth status must not render fake summary on config errors:\n{stdout}"
    );
}
