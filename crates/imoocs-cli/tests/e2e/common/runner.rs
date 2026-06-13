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

/// agent-browser (PATH 探索) と keyring (Secret Service = D-Bus) を実際に使う
/// 認証系テスト用に、ホストのサービス系 env だけ引き継ぐ env キーの一覧。
pub const HOST_SERVICE_ENV_KEYS: &[&str] = &["PATH", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"];

/// `imoocs_in` + ホストのサービス系 env (PATH / D-Bus) を引き継いだコマンド。
/// auth login や drive / slides など agent-browser・keyring に触れるテストで使う。
pub fn imoocs_in_with_host_services(xdg: &TempXdg) -> Command {
    let mut cmd = imoocs_in(xdg);
    for key in HOST_SERVICE_ENV_KEYS {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd
}
