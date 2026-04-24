//! lesson show のデフォルト = 課題展開 + best-effort slide PDF 取得契約
//! (TEST_LIST 章 10、3 件)
//!
//! `362b402` で `lesson show` のデフォルトが反転 (旧 opt-in `--with-assignments`
//! `--fetch-slides` 廃止 → `--no-assignments` / `--no-fetch-slides` の opt-out)、
//! Embed::GoogleSlides に `fetchStatus = ok|skipped|failed` フィールドが追加された
//! 契約変更を回帰検出する。
//!
//! env: IMOOCS_E2E_USERNAME / _PASSWORD / _YEAR / _LESSON_URL
//! 10.x すべて MOOCs 認証必要。10.3 は Google SSO 未ログイン (auth login-google を
//! 呼ばない) 状態を TempXdg 隔離で作る。
//!
//! Linux gating は main.rs の `#[cfg(target_os = "linux")] mod lesson_best_effort;`
//! 側に集約 (clippy::duplicated_attributes 回避)。

use serde_json::Value;

use super::common::{assert_success_envelope, imoocs_in, TempXdg};
use crate::require_env;

/// MOOCs ログイン済 + Google SSO 未ログインの TempXdg を作る。
/// env 未設定や login 失敗時は None を返し、テスト本体は早期 return する。
fn ensure_moocs_logged_in_xdg() -> Option<TempXdg> {
    let user = match std::env::var("IMOOCS_E2E_USERNAME") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[skip] lesson_best_effort: IMOOCS_E2E_USERNAME not set");
            return None;
        }
    };
    let pass = match std::env::var("IMOOCS_E2E_PASSWORD") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[skip] lesson_best_effort: IMOOCS_E2E_PASSWORD not set");
            return None;
        }
    };
    let xdg = TempXdg::new();
    let out = imoocs_in(&xdg)
        .args(["auth", "login", "--username", &user, "--password-stdin"])
        .write_stdin(pass)
        .output()
        .expect("run auth login");
    let exit = out.status.code().unwrap_or(-1);
    if exit != 0 {
        eprintln!(
            "[skip] lesson_best_effort: auth login failed with exit {exit}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    Some(xdg)
}

#[test]
fn lesson_show_default_returns_lesson_with_assignments_shape() {
    // 10.1: `lesson show --url <lesson>` (デフォルト) → exit 0 +
    // envelope.data に `lesson` と `assignments` 両方 (LessonWithAssignments)
    let Some(xdg) = ensure_moocs_logged_in_xdg() else {
        return;
    };
    let year = require_env!("IMOOCS_E2E_YEAR");
    let url = require_env!("IMOOCS_E2E_LESSON_URL");

    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args(["--format", "json", "lesson", "show", "--url", &url])
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert!(
        data.get("lesson").is_some(),
        "data.lesson should exist (LessonWithAssignments shape):\n{data:#}"
    );
    assert!(
        data.get("assignments").and_then(Value::as_array).is_some(),
        "data.assignments should be an array (even when empty):\n{data:#}"
    );
}

#[test]
fn lesson_show_with_no_fetch_slides_omits_fetch_status_field() {
    // 10.2: --no-fetch-slides → embeds[*].fetchStatus がすべて省略 (None)
    // (skip_serializing_if で出力に現れない、schema.md L58-62 の契約)
    let Some(xdg) = ensure_moocs_logged_in_xdg() else {
        return;
    };
    let year = require_env!("IMOOCS_E2E_YEAR");
    let url = require_env!("IMOOCS_E2E_LESSON_URL");

    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args(["--format", "json", "lesson", "show", "--url", &url, "--no-fetch-slides"])
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    let embeds = data
        .get("lesson")
        .and_then(|l| l.get("embeds"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("data.lesson.embeds should be an array:\n{data:#}"));
    for embed in embeds {
        assert!(
            embed.get("fetchStatus").is_none(),
            "embed should not have fetchStatus when --no-fetch-slides:\n{embed:#}"
        );
    }
}

#[test]
fn lesson_show_without_google_sso_records_skipped_fetch_status() {
    // 10.3: Google SSO 未ログイン (TempXdg は MOOCs login のみ、auth login-google を
    // 呼んでいないので google.com cookies が無い) で lesson show →
    // **exit 0** (best-effort 維持の核心契約) +
    // 少なくとも 1 つの google-slides embed が fetchStatus="skipped"
    //
    // 注意: IMOOCS_E2E_LESSON_URL は **slide 埋め込みのあるページ** にすること。
    // slide が無いページだと skipped が 1 つも観察できず assertion で落ちる。
    let Some(xdg) = ensure_moocs_logged_in_xdg() else {
        return;
    };
    let year = require_env!("IMOOCS_E2E_YEAR");
    let url = require_env!("IMOOCS_E2E_LESSON_URL");

    let assert = imoocs_in(&xdg)
        .env("IMOOCS_YEAR", &year)
        .args(["--format", "json", "lesson", "show", "--url", &url, "--no-cache"])
        .assert()
        .success(); // exit 0 維持こそが best-effort 契約の本丸
    let data = assert_success_envelope(&assert.get_output().stdout);
    let embeds = data
        .get("lesson")
        .and_then(|l| l.get("embeds"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("data.lesson.embeds should be an array:\n{data:#}"));
    let skipped_count = embeds
        .iter()
        .filter(|e| {
            let is_slides = e.get("type").and_then(Value::as_str) == Some("google-slides");
            let is_skipped = e.get("fetchStatus").and_then(Value::as_str) == Some("skipped");
            is_slides && is_skipped
        })
        .count();
    assert!(
        skipped_count > 0,
        "at least one google-slides embed should have fetchStatus=\"skipped\" \
         without Google SSO; verify IMOOCS_E2E_LESSON_URL points to a page with \
         google-slides embeds. embeds:\n{embeds:#?}"
    );
}
