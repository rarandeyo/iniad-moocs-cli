use std::fs;
use std::path::PathBuf;

use crate::common::env::TempXdg;
use crate::common::runner::imoocs_in;

const CONFIG_BODY: &str = r#"username = "s1f102301392"

[assignment]
confirm = "auto"
"#;

fn config_path(xdg: &TempXdg) -> PathBuf {
    xdg.config.join("imoocs").join("config.toml")
}

fn cookies_path(xdg: &TempXdg) -> PathBuf {
    xdg.cache.join("imoocs").join("cookies.json")
}

fn seed(xdg: &TempXdg) {
    xdg.write_config(CONFIG_BODY);
    let cookies = cookies_path(xdg);
    fs::create_dir_all(cookies.parent().unwrap()).unwrap();
    fs::write(&cookies, "[]").unwrap();
}

#[test]
fn logout_keeps_config_toml() {
    let xdg = TempXdg::new();
    seed(&xdg);
    imoocs_in(&xdg).args(["auth", "logout"]).assert().success();
    assert!(
        config_path(&xdg).exists(),
        "config.toml must be preserved by the new auth logout"
    );
}

#[test]
fn logout_removes_cookies_json() {
    let xdg = TempXdg::new();
    seed(&xdg);
    imoocs_in(&xdg).args(["auth", "logout"]).assert().success();
    assert!(
        !cookies_path(&xdg).exists(),
        "cookies.json must be cleared by auth logout"
    );
}

#[test]
fn keep_config_flag_is_removed() {
    let xdg = TempXdg::new();
    seed(&xdg);
    let assert = imoocs_in(&xdg)
        .args(["auth", "logout", "--keep-config"])
        .assert()
        .failure();
    let output = assert.get_output();
    assert_eq!(output.status.code(), Some(2));
}
