//! destructive (本番 MOOCs サーバ書き込み) テスト (TEST_LIST 章 9、3 件)
//!
//! 3 重 opt-in:
//! 1. `#[ignore]` — cargo test 既定では skip。`-- --ignored` で初めて run
//! 2. `IMOOCS_E2E_ALLOW_DESTRUCTIVE=1`
//! 3. `IMOOCS_E2E_USERNAME` / `_PASSWORD` / `_YEAR` / `_COURSE_ID` /
//!    `_PROBLEM_ID` / `_PROBLEM_PID` / `_PAGE_URL`
//!    (`_PAGE_URL` は課題ページのフル URL。submit の `--url` 必須化に伴い追加)
//!
//! submit value は `unique_marker()` (timestamp_nanos + uuid v4) で実行ごと
//! 完全ユニーク → 過去実行や別 marker と区別可能。
//!
//! 実行: `IMOOCS_E2E_ALLOW_DESTRUCTIVE=1 IMOOCS_E2E_USERNAME=... ... \`
//!       `cargo test -p imoocs-cli --test e2e -- --ignored`
//!
//! Linux gating は main.rs の `#[cfg(target_os = "linux")] mod destructive;`
//! 側に集約 (clippy::duplicated_attributes 回避)。

use rexpect::process::wait::WaitStatus;
use serde_json::Value;

use super::common::pty::imoocs_pty_in_with_env;
use super::common::{
    assert_success_envelope, imoocs_in_with_host_services, unique_marker, TempXdg, CONFIG_CONFIRM,
    HOST_SERVICE_ENV_KEYS,
};
use crate::require_env;

const CONFIG_AUTO: &str = "[assignment]\nconfirm = \"auto\"\n";

/// destructive 共通の前置: opt-in 3 つを確認、auth login で session を仕込む。
/// 失敗時は `[skip]` 出力 + None を返し、テスト本体は早期 return する。
fn ensure_destructive_xdg() -> Option<TempXdg> {
    if std::env::var("IMOOCS_E2E_ALLOW_DESTRUCTIVE").ok().as_deref() != Some("1") {
        eprintln!("[skip] destructive: IMOOCS_E2E_ALLOW_DESTRUCTIVE != 1");
        return None;
    }
    let user = match std::env::var("IMOOCS_E2E_USERNAME") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[skip] destructive: IMOOCS_E2E_USERNAME not set");
            return None;
        }
    };
    let pass = match std::env::var("IMOOCS_E2E_PASSWORD") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[skip] destructive: IMOOCS_E2E_PASSWORD not set");
            return None;
        }
    };
    let xdg = TempXdg::new();
    // write 系は agent-browser daemon を経由するため、PATH (binary 発見) と
    // D-Bus 系 (keyring) はホストから引き継ぐ。
    let out = imoocs_in_with_host_services(&xdg)
        .args(["auth", "login", "--username", &user, "--password-stdin"])
        .write_stdin(pass)
        .output()
        .expect("run auth login");
    let exit = out.status.code().unwrap_or(-1);
    if exit != 0 {
        eprintln!(
            "[skip] destructive: auth login failed with exit {exit}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    Some(xdg)
}

#[test]
#[ignore]
fn destructive_auth_login_then_status_returns_exit_0() {
    // 9.1: auth login --username X --password-stdin → exit 0
    // 続けて auth status → exit 0
    let Some(xdg) = ensure_destructive_xdg() else {
        return;
    };
    imoocs_in_with_host_services(&xdg)
        .args(["auth", "status"])
        .assert()
        .success();
}

#[test]
#[ignore]
fn destructive_confirm_submit_then_pty_push_y_round_trips_to_server() {
    // 9.2: confirm モード + submit (unique marker) → PTY で push y →
    // exit 0 + draft 削除 + assignment show で currentValue 一致を再確認
    let Some(xdg) = ensure_destructive_xdg() else {
        return;
    };
    let year = require_env!("IMOOCS_E2E_YEAR");
    let course = require_env!("IMOOCS_E2E_COURSE_ID");
    let problem = require_env!("IMOOCS_E2E_PROBLEM_ID");
    let pid = require_env!("IMOOCS_E2E_PROBLEM_PID");
    let page_url = require_env!("IMOOCS_E2E_PAGE_URL");

    xdg.write_config(CONFIG_CONFIRM);
    let marker = unique_marker();
    let data_arg = format!(r#"{{"{pid}":"{marker}"}}"#);

    // Stage (assert_cmd 経由、HTTP 不要)
    imoocs_in_with_host_services(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args([
            "--format",
            "json",
            "assignment",
            "submit",
            "--url",
            &page_url,
            "--problem-id",
            &problem,
            "--data",
            &data_arg,
        ])
        .assert()
        .success();

    // PTY で push 起動 (引数なしで全 draft 送信) + プロンプトに `y` 送信。
    // push は agent-browser daemon を経由するため、ホストのサービス系 env も渡す。
    let host_envs: Vec<(String, String)> = HOST_SERVICE_ENV_KEYS
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect();
    let mut extra_env: Vec<(&str, &str)> = vec![("IMOOCS_YEAR", &year)];
    extra_env.extend(host_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let mut session = imoocs_pty_in_with_env(
        &xdg,
        &["assignment", "push"],
        &extra_env,
        // push は daemon navigate + XHR 完了待ちを含むため余裕を持たせる
        120_000,
    )
    .expect("spawn pty for push");
    let push_prompt = format!(r"Push {course}/{problem}\?");
    session.exp_regex(&push_prompt).expect("see push prompt");
    session.send_line("y").expect("send y");

    let status = session.process.wait().expect("wait child");
    match status {
        WaitStatus::Exited(_, 0) => {}
        other => panic!("expected exit 0 after `y`, got {other:?}"),
    }

    // draft 削除確認
    let dir = xdg.drafts_dir();
    let remaining = std::fs::read_dir(&dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(remaining, 0, "draft must be removed after successful push");

    // 本サーバへの round-trip 確認: assignment show で currentValue がマーカー
    let show = imoocs_in_with_host_services(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args(["--format", "json", "assignment", "show", &course, &problem])
        .assert()
        .success();
    let data = assert_success_envelope(&show.get_output().stdout);
    let fields = data
        .get("fields")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("assignment.show should return data.fields array: {data:#}"));
    let target = fields
        .iter()
        .find(|f| f.get("pid").and_then(Value::as_str) == Some(pid.as_str()))
        .unwrap_or_else(|| panic!("no field with pid={pid}: {fields:#?}"));
    assert_eq!(
        target.get("currentValue").and_then(Value::as_str),
        Some(marker.as_str()),
        "currentValue should match the marker we just pushed: {target:#}"
    );
}

#[test]
#[ignore]
fn destructive_auto_submit_round_trips_to_server() {
    // 9.3: auto モード + submit (unique marker) → exit 0 +
    // envelope.data.submitted=true + assignment show で currentValue 一致
    let Some(xdg) = ensure_destructive_xdg() else {
        return;
    };
    let year = require_env!("IMOOCS_E2E_YEAR");
    let course = require_env!("IMOOCS_E2E_COURSE_ID");
    let problem = require_env!("IMOOCS_E2E_PROBLEM_ID");
    let pid = require_env!("IMOOCS_E2E_PROBLEM_PID");
    let page_url = require_env!("IMOOCS_E2E_PAGE_URL");

    xdg.write_config(CONFIG_AUTO);
    let marker = unique_marker();
    let data_arg = format!(r#"{{"{pid}":"{marker}"}}"#);

    // Direct submit (agent-browser 経由で即サーバ確定)
    let assert = imoocs_in_with_host_services(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args([
            "--format",
            "json",
            "assignment",
            "submit",
            "--url",
            &page_url,
            "--problem-id",
            &problem,
            "--data",
            &data_arg,
        ])
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_eq!(
        data.get("submitted").and_then(Value::as_bool),
        Some(true),
        "auto submit should return submitted=true: {data:#}"
    );

    // 再確認
    let show = imoocs_in_with_host_services(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args(["--format", "json", "assignment", "show", &course, &problem])
        .assert()
        .success();
    let show_data = assert_success_envelope(&show.get_output().stdout);
    let fields = show_data
        .get("fields")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("assignment.show should return fields: {show_data:#}"));
    let target = fields
        .iter()
        .find(|f| f.get("pid").and_then(Value::as_str) == Some(pid.as_str()))
        .unwrap_or_else(|| panic!("no field with pid={pid}: {fields:#?}"));
    assert_eq!(
        target.get("currentValue").and_then(Value::as_str),
        Some(marker.as_str()),
        "currentValue should match the marker we just submitted: {target:#}"
    );
}
