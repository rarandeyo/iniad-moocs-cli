//! `imoocs drive` (list / search / fetch / folders) の E2E。
//!
//! 2 層構成:
//!
//! 1. **非認証** (常時実行): clap / target parse / envelope 契約。ネットワークにも
//!    agent-browser にも触れない。
//! 2. **認証** (`#[ignore]` + env opt-in): 実 Google Drive に対する read-only round-trip。
//!    agent-browser の DOM scrape / download 経路を実機で検証する。
//!    daemon は TempXdg の HOME 配下に新規に立つため、テストごとに
//!    auth login (MOOCs + Google SAML) からフルに回る。
//!
//! 認証系の実行例:
//! ```sh
//! IMOOCS_E2E_USERNAME=s1f10... IMOOCS_E2E_PASSWORD=... \
//! IMOOCS_E2E_DRIVE_FOLDER_ID=1nVajf... \
//! IMOOCS_E2E_DRIVE_FILE_ID=1m_kM4... \
//! IMOOCS_E2E_DRIVE_SEARCH_NAME=Classroom \
//! cargo test -p imoocs-cli --test e2e drive -- --ignored --nocapture
//! ```

use serde_json::Value;

use super::common::{
    assert_failure_envelope, assert_success_envelope, imoocs_in, imoocs_in_with_host_services, TempXdg,
};
use crate::require_env;

// ─── 非認証: clap / parse / envelope 契約 ────────────────────────────────────

#[test]
fn drive_list_with_file_url_is_validation_error() {
    // file URL を list に渡すと fetch への誘導込みの VALIDATION_ERROR (exit 3)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .args([
            "drive",
            "list",
            "https://drive.google.com/file/d/FAKE_DRIVE_FILE_ID_FOR_TESTS_0001/view",
        ])
        .assert()
        .code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("drive fetch"),
        "should redirect user to `drive fetch`: {view:?}"
    );
}

#[test]
fn drive_fetch_with_folder_url_is_validation_error() {
    // folder URL を fetch に渡すと list への誘導込みの VALIDATION_ERROR (exit 3)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .args([
            "drive",
            "fetch",
            "https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001",
        ])
        .assert()
        .code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("drive list"),
        "should redirect user to `drive list`: {view:?}"
    );
}

#[test]
fn drive_list_unrecognized_target_is_validation_error() {
    // URL でも bare ID でもない文字列は exit 3 + "cannot recognise"
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .args(["drive", "list", "http://example.com/not-a-drive-url"])
        .assert()
        .code(3);
    let view = assert_failure_envelope(&assert.get_output().stdout);
    assert_eq!(view.code, "VALIDATION_ERROR");
    assert!(
        view.message.contains("cannot recognise"),
        "expected target-parse error: {view:?}"
    );
}

#[test]
fn drive_folders_with_clean_xdg_reports_not_registered() {
    // course-drive-folders.toml 未配置 → exit 0 + 案内文 (text 契約)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg).args(["drive", "folders"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("No course-drive-folders.toml"),
        "expected registration hint, got:\n{stdout}"
    );
}

// ─── 認証: 実 Drive への read-only round-trip ───────────

/// 認証系共通の前置: credentials env を確認して auth login まで済ませた
/// TempXdg を返す。env 不足や login 失敗時は `[skip]` を出して None。
///
/// `imoocs_in` は `env_clear()` するため、agent-browser binary の発見
/// (`discover_binary` = PATH 探索) のために PATH だけ親から引き継ぐ。
fn ensure_drive_xdg() -> Option<TempXdg> {
    let user = match std::env::var("IMOOCS_E2E_USERNAME") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[skip] drive: IMOOCS_E2E_USERNAME not set");
            return None;
        }
    };
    let pass = match std::env::var("IMOOCS_E2E_PASSWORD") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[skip] drive: IMOOCS_E2E_PASSWORD not set");
            return None;
        }
    };
    let xdg = TempXdg::new();
    let out = drive_cmd(&xdg)
        .args(["auth", "login", "--username", &user, "--password-stdin"])
        .write_stdin(pass)
        .output()
        .expect("run auth login");
    let exit = out.status.code().unwrap_or(-1);
    if exit != 0 {
        eprintln!(
            "[skip] drive: auth login failed with exit {exit}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    Some(xdg)
}

/// agent-browser + keyring を実際に使うため、ホストのサービス系 env を
/// 引き継いだ `imoocs` コマンド (common::imoocs_in_with_host_services の別名)。
fn drive_cmd(xdg: &TempXdg) -> assert_cmd::Command {
    imoocs_in_with_host_services(xdg)
}

#[test]
#[ignore]
fn drive_list_real_folder_returns_items() {
    // 実フォルダの listing が agent-browser DOM scrape 経由で取れる
    let Some(xdg) = ensure_drive_xdg() else { return };
    let folder_id = require_env!("IMOOCS_E2E_DRIVE_FOLDER_ID");

    let assert = drive_cmd(&xdg)
        .args(["--format", "json", "drive", "list", &folder_id])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_eq!(data.get("folderId").and_then(Value::as_str), Some(folder_id.as_str()));
    let items = data
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("expected items array: {data:#}"));
    assert!(!items.is_empty(), "expected at least 1 item in folder: {data:#}");
    for item in items {
        assert!(item.get("id").and_then(Value::as_str).is_some(), "item.id: {item:#}");
        assert!(
            item.get("name").and_then(Value::as_str).is_some(),
            "item.name: {item:#}"
        );
        let kind = item.get("kind").and_then(Value::as_str);
        assert!(
            matches!(kind, Some("folder") | Some("file")),
            "item.kind should be folder|file: {item:#}"
        );
    }
}

#[test]
#[ignore]
fn drive_search_real_query_returns_folders_only() {
    // 実検索が grid scrape + client-side folder フィルタで folder のみ返す
    let Some(xdg) = ensure_drive_xdg() else { return };
    let name = require_env!("IMOOCS_E2E_DRIVE_SEARCH_NAME");

    let assert = drive_cmd(&xdg)
        .args(["--format", "json", "drive", "search", &name])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_eq!(data.get("query").and_then(Value::as_str), Some(name.as_str()));
    let items = data
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("expected items array: {data:#}"));
    assert!(!items.is_empty(), "expected at least 1 folder hit: {data:#}");
    for item in items {
        assert_eq!(
            item.get("kind").and_then(Value::as_str),
            Some("folder"),
            "search results must be folders only: {item:#}"
        );
        let item_name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        assert!(
            item_name.to_lowercase().contains(&name.to_lowercase()),
            "partial match filter should keep only names containing {name:?}: {item:#}"
        );
    }
}

#[test]
#[ignore]
fn drive_fetch_real_file_downloads_then_serves_from_cache() {
    // 実ファイルの download round-trip:
    // 1 回目: agent-browser daemon (download path 付き再起動 + session 自動回復) 経由で取得
    // 2 回目: 24h TTL cache から返る (fromCache: true)
    let Some(xdg) = ensure_drive_xdg() else { return };
    let file_id = require_env!("IMOOCS_E2E_DRIVE_FILE_ID");

    // 1 回目 (実 download)
    let assert = drive_cmd(&xdg)
        .args(["--format", "json", "drive", "fetch", &file_id, "--no-cache"])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();
    let data = assert_success_envelope(&assert.get_output().stdout);
    assert_eq!(data.get("fileId").and_then(Value::as_str), Some(file_id.as_str()));
    assert_eq!(data.get("fromCache").and_then(Value::as_bool), Some(false));
    let local_path = data
        .get("localPath")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("expected localPath: {data:#}"));
    let meta =
        std::fs::metadata(local_path).unwrap_or_else(|e| panic!("downloaded file should exist at {local_path}: {e}"));
    assert!(meta.len() > 0, "downloaded file must not be empty");
    assert_eq!(
        data.get("sizeBytes").and_then(Value::as_u64),
        Some(meta.len()),
        "sizeBytes should match the file on disk: {data:#}"
    );
    // filename は Content-Disposition 由来 (= fileId.bin の fallback ではない) を期待
    let filename = data.get("filename").and_then(Value::as_str).unwrap_or_default();
    assert!(
        !filename.is_empty() && filename != format!("{file_id}.bin"),
        "filename should come from the served download, got {filename:?}"
    );

    // 2 回目 (cache hit)
    let assert2 = drive_cmd(&xdg)
        .args(["--format", "json", "drive", "fetch", &file_id])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let data2 = assert_success_envelope(&assert2.get_output().stdout);
    assert_eq!(
        data2.get("fromCache").and_then(Value::as_bool),
        Some(true),
        "second fetch should be served from cache: {data2:#}"
    );
    assert_eq!(
        data2.get("localPath").and_then(Value::as_str),
        Some(local_path),
        "cache hit should return the same path: {data2:#}"
    );
}
