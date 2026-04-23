//! agent skill (`imoocs`, `imoocs-drive-setup`) の検出。
//!
//! 戦略:
//! 1. `gh skill list --json name` を試す (gh extension `gh-skill` が入っている場合)
//! 2. 失敗したら `~/.claude/skills/<name>/SKILL.md` の存在で判定
//! 3. いずれも取れなければ `Unknown` を返す (warn 止まり、exit code は汚さない)
//!
//! ユーザが skill を手動配置したケースでは gh 経由では検出できないが、
//! その場合でも filesystem fallback が救うことが多い。両方取れないときは
//! ⚠ 表示で済ませ、ユーザに手動確認を促す。

use std::path::PathBuf;
use std::process::Command;

use imoocs_core::schemas::{SkillDetectionMethod, SkillDetectionReport};
use serde::Deserialize;

const IMOOCS_SKILL: &str = "imoocs";
const DRIVE_SETUP_SKILL: &str = "imoocs-drive-setup";

pub fn detect_skills() -> SkillDetectionReport {
    if let Some(via_gh) = detect_via_gh() {
        return via_gh;
    }
    if let Some(via_fs) = detect_via_filesystem() {
        return via_fs;
    }
    SkillDetectionReport {
        method: SkillDetectionMethod::Unknown,
        imoocs: false,
        imoocs_drive_setup: false,
    }
}

#[derive(Deserialize)]
struct GhSkillEntry {
    #[serde(default)]
    name: Option<String>,
}

fn detect_via_gh() -> Option<SkillDetectionReport> {
    let output = Command::new("gh")
        .args(["skill", "list", "--json", "name"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let entries: Vec<GhSkillEntry> = serde_json::from_slice(&output.stdout).ok()?;
    let names: Vec<String> = entries.into_iter().filter_map(|e| e.name).collect();
    Some(SkillDetectionReport {
        method: SkillDetectionMethod::Gh,
        imoocs: names.iter().any(|n| has_suffix_component(n, IMOOCS_SKILL)),
        imoocs_drive_setup: names.iter().any(|n| has_suffix_component(n, DRIVE_SETUP_SKILL)),
    })
}

/// `gh skill list` が返す name は `owner/repo/skill` か単に `skill` のどちらか
/// 判別しきれないので、末尾コンポーネントだけ比較する。
fn has_suffix_component(name: &str, target: &str) -> bool {
    name.rsplit('/').next().map(|s| s == target).unwrap_or(false) || name == target
}

fn detect_via_filesystem() -> Option<SkillDetectionReport> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let skills_root = home.join(".claude").join("skills");
    if !skills_root.exists() {
        return None;
    }
    Some(SkillDetectionReport {
        method: SkillDetectionMethod::Filesystem,
        imoocs: skill_installed(&skills_root, IMOOCS_SKILL),
        imoocs_drive_setup: skill_installed(&skills_root, DRIVE_SETUP_SKILL),
    })
}

fn skill_installed(root: &std::path::Path, name: &str) -> bool {
    root.join(name).join("SKILL.md").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_suffix_component_matches_bare_name() {
        assert!(has_suffix_component("imoocs", "imoocs"));
    }

    #[test]
    fn has_suffix_component_matches_owner_repo_skill() {
        assert!(has_suffix_component("rarandeyo/iniad-moocs-cli/imoocs", "imoocs"));
        assert!(has_suffix_component(
            "rarandeyo/iniad-moocs-cli/imoocs-drive-setup",
            "imoocs-drive-setup"
        ));
    }

    #[test]
    fn has_suffix_component_rejects_partial() {
        assert!(!has_suffix_component("imoocs-drive-setup", "imoocs"));
        assert!(!has_suffix_component("my-imoocs", "imoocs"));
    }
}
