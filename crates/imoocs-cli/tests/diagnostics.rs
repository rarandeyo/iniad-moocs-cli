use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("imoocs-cli-{label}-{}-{unique}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("config/imoocs")).expect("create config dir");
    root
}

fn run_imoocs(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_imoocs"))
        .args(args)
        .env("HOME", root)
        .env("SHELL", "/bin/fish")
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .expect("run imoocs")
}

#[test]
fn doctor_fails_on_invalid_config() {
    let root = temp_root("doctor-bad-config");
    fs::write(root.join("config/imoocs/config.toml"), "not = [valid\n").expect("write config");

    let output = run_imoocs(&root, &["doctor", "--format", "json"]);
    assert_eq!(output.status.code(), Some(5));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"success\": false"), "stdout was: {stdout}");
    assert!(stdout.contains("config toml parse error"), "stdout was: {stdout}");
}

#[test]
fn doctor_fails_on_invalid_drive_folder_toml() {
    let root = temp_root("doctor-bad-drive");
    fs::write(
        root.join("config/imoocs/config.toml"),
        "[assignment]\nconfirm = \"confirm\"\n",
    )
    .expect("write config");
    fs::write(
        root.join("config/imoocs/course-drive-folders.toml"),
        "driveRootFolderId = 123\n",
    )
    .expect("write drive folders");

    let output = run_imoocs(&root, &["doctor", "--format", "json"]);
    assert_eq!(output.status.code(), Some(5));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"success\": false"), "stdout was: {stdout}");
    assert!(
        stdout.contains("course-drive-folders.toml parse error"),
        "stdout was: {stdout}"
    );
}

#[test]
fn auth_status_surfaces_invalid_config() {
    let root = temp_root("auth-status-bad-config");
    fs::write(root.join("config/imoocs/config.toml"), "not = [valid\n").expect("write config");

    let output = run_imoocs(&root, &["auth", "status"]);
    assert_eq!(output.status.code(), Some(5));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stderr.contains("認証状態確認失敗"), "stderr was: {stderr}");
    assert!(stderr.contains("config toml parse error"), "stderr was: {stderr}");
    assert!(
        !stdout.contains("MOOCs login"),
        "auth status should not render a fake status summary on config errors: {stdout}"
    );
}
