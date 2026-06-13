//! agent-browser バイナリの PATH 探索。
//!
//! `imoocs-cli` 側にも同等の検出ロジックがあるが、
//! `imoocs-core` から呼べる経路として `imoocs-browser` にも置く。
//! いずれこちらに完全集約予定。

use std::env;
use std::path::PathBuf;

/// PATH 上の agent-browser バイナリを探す。見つからなければ `None`。
pub fn discover_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        "agent-browser.exe"
    } else {
        "agent-browser"
    };
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var).find_map(|dir| {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            Some(candidate)
        } else {
            None
        }
    })
}
