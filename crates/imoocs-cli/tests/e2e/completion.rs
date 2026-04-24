//! completion install (TEST_LIST 章 8、3 件)
//!
//! XDG 隔離下で fish completion を書き込む / 同じ → up-to-date / 改ざん →
//! exit 3 / `--force` で再書込 を検証する。bash/zsh の per-shell 検出は
//! 既存 unit `commands/completion.rs:210-237` でカバー済なので、e2e は host
//! shell から独立した fish 1 種に絞る。

use super::common::{imoocs_in, TempXdg};

fn fish_completion_path(xdg: &TempXdg) -> std::path::PathBuf {
    xdg.config.join("fish/completions/imoocs.fish")
}

#[test]
fn install_writes_fish_completion_to_xdg_config_dir() {
    // 8.1: 新規書込 → exit 0 + stdout に "✓ wrote" + 中身に
    // `complete -c imoocs` (clap_complete fish スクリプトのマーカー)
    let xdg = TempXdg::new();
    let assert = imoocs_in(&xdg)
        .args(["completion", "install", "--shell", "fish"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("✓ wrote"),
        "stdout should report fresh write:\n{stdout}"
    );

    let path = fish_completion_path(&xdg);
    assert!(path.exists(), "fish completion file should exist at {path:?}");
    let body = std::fs::read_to_string(&path).expect("read completion");
    assert!(
        body.contains("complete -c imoocs"),
        "fish completion script should contain `complete -c imoocs`: head=\n{}",
        body.lines().take(5).collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn install_twice_with_unchanged_file_reports_up_to_date() {
    // 8.2: 同 commit を 2 回 install → 2 回目は exit 0 + "already up to date"
    let xdg = TempXdg::new();
    imoocs_in(&xdg)
        .args(["completion", "install", "--shell", "fish"])
        .assert()
        .success();

    let assert = imoocs_in(&xdg)
        .args(["completion", "install", "--shell", "fish"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("already up to date"),
        "stdout should mention `already up to date`:\n{stdout}"
    );
}

#[test]
fn install_with_modified_file_requires_force_to_overwrite() {
    // 8.3: 1 回 install → ファイル改ざん → 再 install で exit 3 +
    // "different content" / "--force" メッセージ → `--force` で exit 0 + 復元
    let xdg = TempXdg::new();
    imoocs_in(&xdg)
        .args(["completion", "install", "--shell", "fish"])
        .assert()
        .success();

    let path = fish_completion_path(&xdg);
    let original = std::fs::read_to_string(&path).expect("read original completion");
    std::fs::write(&path, "# manual edit by user\n").expect("overwrite completion");

    // --force 無しで再 install → exit 3 + "different content"
    let assert = imoocs_in(&xdg)
        .args(["completion", "install", "--shell", "fish"])
        .assert()
        .code(3);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("different content") || stderr.contains("--force"),
        "stderr should mention conflict + --force option:\n{stderr}"
    );
    // ファイルはまだ手動編集の中身のまま
    let after_block = std::fs::read_to_string(&path).expect("read after blocked install");
    assert_eq!(
        after_block, "# manual edit by user\n",
        "file should not have been overwritten without --force"
    );

    // --force 付きで再 install → exit 0 + 元の content に戻る
    imoocs_in(&xdg)
        .args(["completion", "install", "--shell", "fish", "--force"])
        .assert()
        .success();
    let after_force = std::fs::read_to_string(&path).expect("read after force install");
    assert_eq!(
        after_force, original,
        "completion file should be restored to canonical content after --force"
    );
}
