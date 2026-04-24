//! Plan R6: PTY (rexpect) ハーネスが実機で動くかの検証実験。
//!
//! 章 5.2 / 5.3 / 9.2 の本番 PTY テストに着手する前に、`spawn_command` +
//! `exp_string` + `wait` の round-trip が通ることだけを確認する 1 本。
//! 通らない場合は rexpect の代わりに直接 `nix::pty` を使うか、別の戦略
//! (CLI 側に `--prompt-stub` 経路を足す等) を検討する。
//!
//! `--version` を選んだ理由: 副作用ゼロ、HTTP 不要、stdout に決定論的な
//! `imoocs <ver>` 1 行を吐くだけ。dialoguer に依存しないので「PTY 自体が
//! 動くか」と「dialoguer が PTY で何を吐くか」を切り分けられる。

#![cfg(target_os = "linux")]

use rexpect::error::Error;

use super::common::{pty::imoocs_pty_in, TempXdg};

#[test]
fn pty_round_trip_for_version_works_and_exits_0() {
    let xdg = TempXdg::new();
    let mut session = imoocs_pty_in(&xdg, &["--version"], 3_000).expect("spawn pty session");

    // PTY は stdout/stderr を 1 本に merge する。`imoocs --version` は stdout
    // に "imoocs <ver>" を出すだけ。
    session.exp_string("imoocs ").expect("read `imoocs ` prefix");

    // EOF まで待つ → wait で exit status を取る。
    let status = session.process.wait().expect("child wait");
    use rexpect::process::wait::WaitStatus;
    match status {
        WaitStatus::Exited(_, 0) => {}
        other => panic!("expected exit 0, got {other:?}"),
    }
}

/// rexpect が timeout 系で返すエラー型は `Error::Timeout` 等。これは
/// `Result<_, Error>` を `match` で扱う公式パターンの確認用。
/// (本番テストで `expect("...")` が落ちたとき何が見えるかを早めに把握しておく。)
#[allow(dead_code)]
fn _error_inspection_pattern(err: Error) {
    match err {
        Error::Timeout { expected, .. } => {
            eprintln!("rexpect timeout while expecting {expected:?}");
        }
        other => eprintln!("rexpect other error: {other:?}"),
    }
}
