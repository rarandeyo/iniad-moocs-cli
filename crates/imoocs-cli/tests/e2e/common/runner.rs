use assert_cmd::Command;

use super::env::TempXdg;

/// env を一切 shape しない bare な `imoocs` 起動コマンド。
/// clap レイヤだけを assert したいテスト (`--version` / `--help`) で使う。
pub fn imoocs() -> Command {
    Command::cargo_bin("imoocs").expect("locate imoocs binary")
}

/// XDG を tempdir に向けた `imoocs` 起動コマンド。親 env はすべて clear して
/// 開発者の実 config / cookies / drafts を読み書きする事故を防ぐ。
pub fn imoocs_in(xdg: &TempXdg) -> Command {
    let mut cmd = imoocs();
    cmd.env_clear()
        .env("HOME", &xdg.home)
        .env("XDG_CONFIG_HOME", &xdg.config)
        .env("XDG_DATA_HOME", &xdg.data)
        .env("XDG_CACHE_HOME", &xdg.cache)
        .env("XDG_STATE_HOME", &xdg.state)
        // SHELL を空にすると completion 系のコードパスが意外な形で死ぬので
        // fish で固定する (テストの決定論性のため)。
        .env("SHELL", "/bin/fish")
        // panic を読みやすく。
        .env("RUST_BACKTRACE", "0");
    cmd
}
