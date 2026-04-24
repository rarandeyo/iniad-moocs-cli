//! slides_dir の既定 `/tmp/imoocs/slides` 経由で開発者環境を触らないよう、
//! 各テストは config.toml に `[slides] out_dir = "cache"` を書いて slides_dir を
//! tempdir の `<XDG_CACHE_HOME>/imoocs/slides` に閉じ込める。config を消す scope
//! テストを 2 度繰り返すと DEFAULT にフォールバックするため、冪等性テストは
//! config 非依存な `--scope drafts` で行う。
//! `credentials_file()` (`data_dir/credentials.toml`) は現状未使用なので reset
//! の対象外。運用開始時は auth scope のテストを拡張すること。

use std::fs;
use std::path::PathBuf;

use crate::common::env::TempXdg;
use crate::common::runner::imoocs_in;

/// env_clear で DBUS が切れて keyring 操作が失敗する環境を想定し、username は
/// 書かない (keyring 対象ゼロ)。`keyring_failure_preserves_config_for_retry` の
/// 方で `CONFIG_WITH_USERNAME` を使い、keyring 失敗経路を検証する。
const CONFIG_BODY: &str = r#"[slides]
out_dir = "cache"

[assignment]
confirm = "auto"
"#;

const CONFIG_WITH_USERNAME: &str = r#"username = "s1f102301392"

[slides]
out_dir = "cache"

[assignment]
confirm = "auto"
"#;

const COURSE_DRIVE_BODY: &str = r#"[[course]]
year = 2026
course_id = "INI301"
folder_id = "abc"
"#;

fn setup_state(xdg: &TempXdg) {
    xdg.write_config(CONFIG_BODY);
    xdg.write_drive_folders(COURSE_DRIVE_BODY);

    let cookies = cookies_path(xdg);
    fs::create_dir_all(cookies.parent().unwrap()).unwrap();
    fs::write(&cookies, "[]").unwrap();

    let drive = drive_path(xdg);
    fs::create_dir_all(&drive).unwrap();
    fs::write(drive.join("INI301.toml"), "").unwrap();

    let slides = slides_path(xdg);
    fs::create_dir_all(&slides).unwrap();
    fs::write(slides.join("foo.pdf"), "").unwrap();

    let drafts = xdg.drafts_dir();
    fs::create_dir_all(&drafts).unwrap();
    fs::write(drafts.join("draft.json"), "{}").unwrap();
}

fn config_toml_path(xdg: &TempXdg) -> PathBuf {
    xdg.config.join("imoocs").join("config.toml")
}

fn course_drive_path(xdg: &TempXdg) -> PathBuf {
    xdg.config.join("imoocs").join("course-drive-folders.toml")
}

fn cookies_path(xdg: &TempXdg) -> PathBuf {
    xdg.cache.join("imoocs").join("cookies.json")
}

fn drive_path(xdg: &TempXdg) -> PathBuf {
    xdg.cache.join("imoocs").join("drive")
}

fn slides_path(xdg: &TempXdg) -> PathBuf {
    xdg.cache.join("imoocs").join("slides")
}

#[test]
fn dry_run_removes_nothing() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    let assert = imoocs_in(&xdg).args(["reset", "--dry-run"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("[dry-run] nothing removed."),
        "expected dry-run marker in stdout: {stdout}"
    );
    assert!(config_toml_path(&xdg).exists());
    assert!(course_drive_path(&xdg).exists());
    assert!(cookies_path(&xdg).exists());
    assert!(drive_path(&xdg).exists());
    assert!(slides_path(&xdg).exists());
    assert!(xdg.drafts_dir().exists());
}

#[test]
fn scope_cache_only_removes_cache() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg)
        .args(["reset", "--scope", "cache", "--yes"])
        .assert()
        .success();
    assert!(!cookies_path(&xdg).exists());
    assert!(!drive_path(&xdg).exists());
    assert!(!slides_path(&xdg).exists());
    assert!(config_toml_path(&xdg).exists());
    assert!(course_drive_path(&xdg).exists());
    assert!(xdg.drafts_dir().exists());
}

#[test]
fn scope_config_removes_config_files() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg)
        .args(["reset", "--scope", "config", "--yes"])
        .assert()
        .success();
    assert!(!config_toml_path(&xdg).exists());
    assert!(!course_drive_path(&xdg).exists());
    assert!(cookies_path(&xdg).exists());
    assert!(drive_path(&xdg).exists());
    assert!(xdg.drafts_dir().exists());
}

#[test]
fn scope_drafts_removes_drafts() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg)
        .args(["reset", "--scope", "drafts", "--yes"])
        .assert()
        .success();
    assert!(!xdg.drafts_dir().exists());
    assert!(config_toml_path(&xdg).exists());
    assert!(cookies_path(&xdg).exists());
}

#[test]
fn scope_all_removes_everything() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg)
        .args(["reset", "--scope", "all", "--yes"])
        .assert()
        .success();
    assert!(!config_toml_path(&xdg).exists());
    assert!(!course_drive_path(&xdg).exists());
    assert!(!cookies_path(&xdg).exists());
    assert!(!drive_path(&xdg).exists());
    assert!(!slides_path(&xdg).exists());
    assert!(!xdg.drafts_dir().exists());
}

#[test]
fn no_scope_defaults_to_all() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg).args(["reset", "--yes"]).assert().success();
    assert!(!config_toml_path(&xdg).exists());
    assert!(!cookies_path(&xdg).exists());
    assert!(!xdg.drafts_dir().exists());
}

#[test]
fn non_tty_without_yes_exits_3() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    let assert = imoocs_in(&xdg).args(["reset"]).assert().failure();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refusing to run in non-interactive mode"),
        "expected refuse message, got: {stderr}"
    );
    assert!(config_toml_path(&xdg).exists());
    assert!(cookies_path(&xdg).exists());
    assert!(xdg.drafts_dir().exists());
}

#[test]
fn idempotent_drafts_twice() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg)
        .args(["reset", "--scope", "drafts", "--yes"])
        .assert()
        .success();
    assert!(!xdg.drafts_dir().exists());
    imoocs_in(&xdg)
        .args(["reset", "--scope", "drafts", "--yes"])
        .assert()
        .success();
}

#[test]
fn scope_auth_without_username_succeeds() {
    let xdg = TempXdg::new();
    imoocs_in(&xdg)
        .args(["reset", "--scope", "auth", "--yes"])
        .assert()
        .success();
}

#[test]
fn scope_csv_multiple() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    imoocs_in(&xdg)
        .args(["reset", "--scope", "cache,drafts", "--yes"])
        .assert()
        .success();
    assert!(!cookies_path(&xdg).exists());
    assert!(!drive_path(&xdg).exists());
    assert!(!slides_path(&xdg).exists());
    assert!(!xdg.drafts_dir().exists());
    assert!(config_toml_path(&xdg).exists());
    assert!(course_drive_path(&xdg).exists());
}

#[test]
fn keyring_failure_preserves_config_for_retry() {
    // env_clear で DBUS が切れて keyring backend が Err を返す状況を利用し、
    // auth 失敗時に config.toml が維持される (= username で keyring retry 可能)
    // ことを検証する。
    let xdg = TempXdg::new();
    xdg.write_config(CONFIG_WITH_USERNAME);
    xdg.write_drive_folders(COURSE_DRIVE_BODY);
    let cookies = cookies_path(&xdg);
    fs::create_dir_all(cookies.parent().unwrap()).unwrap();
    fs::write(&cookies, "[]").unwrap();
    fs::create_dir_all(xdg.drafts_dir()).unwrap();
    fs::write(xdg.drafts_dir().join("d.json"), "{}").unwrap();

    let assert = imoocs_in(&xdg)
        .args(["reset", "--scope", "all", "--yes"])
        .assert()
        .failure();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(5));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("keyring entry for s1f102301392"),
        "expected keyring warning, got: {stderr}"
    );
    assert!(
        stderr.contains("preserve username"),
        "expected preservation notice, got: {stderr}"
    );
    assert!(!cookies_path(&xdg).exists());
    assert!(!xdg.drafts_dir().exists());
    assert!(
        config_toml_path(&xdg).exists(),
        "config.toml must be preserved when keyring cleanup fails"
    );
    assert!(
        course_drive_path(&xdg).exists(),
        "course-drive-folders.toml must be preserved when keyring cleanup fails"
    );
}

#[test]
fn bare_scope_without_value_is_rejected() {
    let xdg = TempXdg::new();
    setup_state(&xdg);
    let assert = imoocs_in(&xdg).args(["reset", "--scope", "--yes"]).assert().failure();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(2));
    assert!(config_toml_path(&xdg).exists());
    assert!(cookies_path(&xdg).exists());
    assert!(xdg.drafts_dir().exists());
}

#[test]
fn cache_scope_refuses_unsafe_slides_dir() {
    let xdg = TempXdg::new();
    let user_slides = xdg.home.join("Documents").join("slides");
    fs::create_dir_all(&user_slides).unwrap();
    let important = user_slides.join("important.pdf");
    fs::write(&important, "USER DATA — MUST NOT BE DELETED").unwrap();

    let cfg_body = format!("[slides]\nout_dir = \"{}\"\n", user_slides.display());
    xdg.write_config(&cfg_body);

    let cookies = cookies_path(&xdg);
    fs::create_dir_all(cookies.parent().unwrap()).unwrap();
    fs::write(&cookies, "[]").unwrap();
    fs::create_dir_all(drive_path(&xdg)).unwrap();
    fs::write(drive_path(&xdg).join("INI301.toml"), "").unwrap();

    let assert = imoocs_in(&xdg)
        .args(["reset", "--scope", "cache", "--yes"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("refusing to delete") && stderr.contains("Documents"),
        "expected refuse notice for unsafe slides_dir, got: {stderr}"
    );
    assert!(!cookies_path(&xdg).exists());
    assert!(!drive_path(&xdg).exists());
    assert!(
        important.exists(),
        "user Documents/slides/important.pdf must be preserved"
    );
    assert!(user_slides.exists());
}

#[test]
fn reset_config_scope_works_with_malformed_config() {
    let xdg = TempXdg::new();
    let config_path = config_toml_path(&xdg);
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, "this is not = valid == toml [[[\n").unwrap();

    imoocs_in(&xdg)
        .args(["reset", "--scope", "config", "--yes"])
        .assert()
        .success();

    assert!(!config_toml_path(&xdg).exists());
}

#[test]
fn dry_run_works_with_malformed_config() {
    let xdg = TempXdg::new();
    let config_path = config_toml_path(&xdg);
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, "garbage ==\n").unwrap();

    imoocs_in(&xdg).args(["reset", "--dry-run"]).assert().success();
    assert!(config_toml_path(&xdg).exists());
}
