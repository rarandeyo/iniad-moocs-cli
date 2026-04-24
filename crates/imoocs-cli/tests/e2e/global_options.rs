//! グローバルオプションの env 上書き (TEST_LIST 章 7、2 件)
//!
//! 7.1 は env 不要 (version の JSON envelope を flag vs env で比較)。
//! 7.2 は実 HTTP + keyring 必要 (`IMOOCS_E2E_KEYRING_BOOTSTRAPPED=1`)。
//! env 未設定時は require_env! で skip。

use serde_json::Value;

use super::common::{assert_failure_envelope, imoocs, imoocs_in, TempXdg};
use crate::require_env;

#[test]
fn version_envelope_is_identical_for_format_flag_and_env() {
    // 7.1: --format json flag と IMOOCS_FORMAT=json env が同じ stdout を出す。
    // (global flag と env が同じ意味であることの三角測量)
    let xdg = TempXdg::new();

    let from_flag = imoocs_in(&xdg)
        .args(["--format", "json", "version"])
        .assert()
        .success();
    let envelope_flag: Value =
        serde_json::from_slice(&from_flag.get_output().stdout).expect("envelope from flag");

    let from_env = imoocs_in(&xdg)
        .env("IMOOCS_FORMAT", "json")
        .arg("version")
        .assert()
        .success();
    let envelope_env: Value =
        serde_json::from_slice(&from_env.get_output().stdout).expect("envelope from env");

    assert_eq!(
        envelope_flag, envelope_env,
        "flag and env should produce identical envelopes"
    );
}

#[test]
fn course_list_with_year_2099_returns_failure_envelope() {
    // 7.2: --year 2099 で course list → API_ERROR or NOT_FOUND
    // 実 HTTP 必要なので IMOOCS_E2E_KEYRING_BOOTSTRAPPED=1 を opt-in に要求。
    // 実環境の keyring/cookies を引き継ぐ必要があるので bare imoocs() を使う
    // (XDG 隔離は破れるが read-only なので副作用は無い)。
    let _ = require_env!("IMOOCS_E2E_KEYRING_BOOTSTRAPPED");

    let assert = imoocs()
        .args(["--year", "2099", "--format", "json", "course", "list"])
        .assert();
    let output = assert.get_output();
    let exit_code = output.status.code().expect("exit code");
    assert_ne!(
        exit_code, 0,
        "year=2099 should fail, got exit 0:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let view = assert_failure_envelope(&output.stdout);
    let valid_codes = ["API_ERROR", "NOT_FOUND"];
    assert!(
        valid_codes.contains(&view.code.as_str()),
        "expected API_ERROR or NOT_FOUND for non-existent year 2099, got: {view:?}"
    );
}
