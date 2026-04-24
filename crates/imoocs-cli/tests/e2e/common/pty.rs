//! PTY (pseudo-terminal) wrapper for rexpect-based tests. Linux/macOS only.
//!
//! `dialoguer::Confirm` (commands/confirm.rs:106) needs a real terminal to
//! read single keystrokes and to emit ANSI cursor / clear sequences. Plain
//! `assert_cmd` runs the child non-TTY, which is fine for the safety
//! contract test (5.1) but not for the prompt round-trip (5.2 / 5.3).
//!
//! `rexpect::session::PtySession` spawns the binary on a Unix PTY and
//! exposes `exp_regex` / `send_line` plus the underlying `Wait::wait` for
//! exit-code retrieval.

#![cfg(unix)]

use rexpect::error::Error;
use rexpect::session::{spawn_command, PtySession};

use super::env::TempXdg;

/// Spawn `imoocs <args>` on a PTY with XDG redirected to `xdg`.
/// Convenience for tests that don't need extra env.
pub fn imoocs_pty_in(xdg: &TempXdg, args: &[&str], timeout_ms: u64) -> Result<PtySession, Error> {
    imoocs_pty_in_with_env(xdg, args, &[], timeout_ms)
}

/// Spawn `imoocs <args>` on a PTY with XDG redirected to `xdg` plus extra
/// `(KEY, VALUE)` env pairs (e.g. `IMOOCS_YEAR=2026` to bypass
/// `resolve_latest_year`). Mirrors `runner::imoocs_in` but uses a PTY
/// backend instead of pipes.
///
/// `timeout_ms` is the per-`exp_*` deadline; default callers to a few
/// seconds because dialoguer occasionally takes a beat to repaint.
pub fn imoocs_pty_in_with_env(
    xdg: &TempXdg,
    args: &[&str],
    extra_env: &[(&str, &str)],
    timeout_ms: u64,
) -> Result<PtySession, Error> {
    // CARGO_BIN_EXE_imoocs は cargo が test ターゲットに食わせる compile-time
    // env なので env!() で解決する (std::env::var は子プロセスに継承されない)。
    let bin = env!("CARGO_BIN_EXE_imoocs");

    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .env_clear()
        .env("HOME", &xdg.home)
        .env("XDG_CONFIG_HOME", &xdg.config)
        .env("XDG_DATA_HOME", &xdg.data)
        .env("XDG_CACHE_HOME", &xdg.cache)
        .env("XDG_STATE_HOME", &xdg.state)
        .env("SHELL", "/bin/fish")
        // dialoguer の ColorfulTheme が ANSI escape を吐くので、テストで
        // expect_regex する文字列が紛れる。`TERM=dumb` で抑制を試みる。
        .env("TERM", "dumb")
        .env("RUST_BACKTRACE", "0");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    spawn_command(cmd, Some(timeout_ms))
}
