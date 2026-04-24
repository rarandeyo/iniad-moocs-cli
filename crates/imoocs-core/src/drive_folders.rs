//! `$XDG_CONFIG_HOME/imoocs/course-drive-folders.toml` を型付きで読むためのモジュール。
//!
//! 書き込みは `imoocs-drive-setup` skill が担当するため、この CLI からは
//! 読み取り専用の `load()` のみを提供する。

use std::fs;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ImoocsError, Result};
use crate::schemas::{DriveFoldersSummary, Year};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CourseDriveFolders {
    pub drive_root_folder_id: String,
    #[serde(default)]
    pub courses: Vec<CourseDriveFolderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CourseDriveFolderEntry {
    pub year: Year,
    pub course_id: String,
    pub name: String,
    /// 1 コースに紐付く Drive フォルダ群。0 件なら未解決、複数なら 1:N
    /// (例: COT101「概論Ⅰ + 基礎演習Ⅰ」)。複数コースが同一フォルダを共有する
    /// N:1 ケース (例: HII201/UX104/UX108 の「デザイン理論」) は、各 entry の
    /// `drive_folders` に同じ `id` を書くだけで表現する。
    #[serde(default)]
    pub drive_folders: Vec<DriveFolderRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_at: Option<String>,
    pub match_strategy: MatchStrategy,
    /// `match_strategy = "unresolved"` の理由分類。再走時の挙動を分けるために
    /// 使う (例: `Deferred` は次回 Drive 検索で埋まる見込み、`NotOffered` は
    /// 再走時もスキップ)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_reason: Option<UnresolvedReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriveFolderRef {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MatchStrategy {
    Exact,
    Partial,
    UserConfirmed,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum UnresolvedReason {
    /// 当該年度に Drive フォルダが用意されたら埋まる見込み (典型: 学期途中で追加)。
    Deferred,
    /// 当該年度は未開講と判断 (履修登録には残るが資料が出ない / 出さない)。
    NotOffered,
    /// 教員側でフォルダが作られていない (要連絡)。
    PendingFolder,
    /// 候補が複数 / 不明で次回ユーザ判断待ち。
    NeedsUserInput,
}

impl CourseDriveFolderEntry {
    pub fn is_resolved(&self) -> bool {
        self.match_strategy != MatchStrategy::Unresolved && !self.drive_folders.is_empty()
    }
}

impl CourseDriveFolders {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        let parsed: Self = toml::from_str(&raw)
            .map_err(|e| ImoocsError::Parse(format!("course-drive-folders.toml parse error: {e}")))?;
        Ok(Some(parsed))
    }

    pub fn summary(&self) -> DriveFoldersSummary {
        let total = self.courses.len();
        let resolved = self.courses.iter().filter(|c| c.is_resolved()).count();
        DriveFoldersSummary {
            total,
            resolved,
            unresolved: total - resolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_SAMPLE: &str = r#"
driveRootFolderId = "FAKE_DRIVE_ROOT_ID_SAMPLE_0001"

[[courses]]
year = 2026
courseId = "INI301"
name = "機械学習と人工知能"
matchedAt = "2026-04-23"
matchStrategy = "exact"
[[courses.driveFolders]]
id = "FAKE_DRIVE_FOLDER_ID_SAMPLE_0001"
url = "https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_SAMPLE_0001"

[[courses]]
year = 2026
courseId = "INI302"
name = "データサイエンス入門"
matchStrategy = "unresolved"
unresolvedReason = "not-offered"
"#;

    #[test]
    fn parses_full_sample() {
        let cdf: CourseDriveFolders = toml::from_str(FULL_SAMPLE).unwrap();
        assert_eq!(cdf.drive_root_folder_id, "FAKE_DRIVE_ROOT_ID_SAMPLE_0001");
        assert_eq!(cdf.courses.len(), 2);
        assert_eq!(cdf.courses[0].course_id, "INI301");
        assert_eq!(cdf.courses[0].match_strategy, MatchStrategy::Exact);
        assert_eq!(cdf.courses[0].matched_at.as_deref(), Some("2026-04-23"));
        assert_eq!(cdf.courses[0].drive_folders.len(), 1);
        assert_eq!(cdf.courses[0].drive_folders[0].id, "FAKE_DRIVE_FOLDER_ID_SAMPLE_0001");
        assert!(cdf.courses[0].is_resolved());
        assert_eq!(cdf.courses[1].match_strategy, MatchStrategy::Unresolved);
        assert_eq!(cdf.courses[1].unresolved_reason, Some(UnresolvedReason::NotOffered));
        assert!(cdf.courses[1].drive_folders.is_empty());
        assert!(!cdf.courses[1].is_resolved());
    }

    #[test]
    fn summary_counts_resolved_vs_unresolved() {
        let cdf: CourseDriveFolders = toml::from_str(FULL_SAMPLE).unwrap();
        let s = cdf.summary();
        assert_eq!(s.total, 2);
        assert_eq!(s.resolved, 1);
        assert_eq!(s.unresolved, 1);
    }

    #[test]
    fn match_strategy_roundtrip_kebab_case() {
        let variants = [
            ("\"exact\"", MatchStrategy::Exact),
            ("\"partial\"", MatchStrategy::Partial),
            ("\"user-confirmed\"", MatchStrategy::UserConfirmed),
            ("\"unresolved\"", MatchStrategy::Unresolved),
        ];
        for (repr, expected) in variants {
            let got: MatchStrategy = serde_json::from_str(repr).unwrap();
            assert_eq!(got, expected);
            let back = serde_json::to_string(&expected).unwrap();
            assert_eq!(back, repr);
        }
    }

    #[test]
    fn unresolved_reason_roundtrip_kebab_case() {
        let variants = [
            ("\"deferred\"", UnresolvedReason::Deferred),
            ("\"not-offered\"", UnresolvedReason::NotOffered),
            ("\"pending-folder\"", UnresolvedReason::PendingFolder),
            ("\"needs-user-input\"", UnresolvedReason::NeedsUserInput),
        ];
        for (repr, expected) in variants {
            let got: UnresolvedReason = serde_json::from_str(repr).unwrap();
            assert_eq!(got, expected);
            let back = serde_json::to_string(&expected).unwrap();
            assert_eq!(back, repr);
        }
    }

    #[test]
    fn one_to_many_entry_keeps_all_folders() {
        let toml = r#"
driveRootFolderId = "root"

[[courses]]
year = 2026
courseId = "COT101"
name = "コンピュータ・サイエンス概論 I & 演習 I"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_FOLDER_GAIRON_I"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_GAIRON_I"
[[courses.driveFolders]]
id = "FAKE_FOLDER_KISO_ENSHU_I"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_KISO_ENSHU_I"
"#;
        let cdf: CourseDriveFolders = toml::from_str(toml).unwrap();
        assert_eq!(cdf.courses[0].drive_folders.len(), 2);
        assert_eq!(cdf.courses[0].drive_folders[1].id, "FAKE_FOLDER_KISO_ENSHU_I");
        assert!(cdf.courses[0].is_resolved());
    }

    #[test]
    fn many_to_one_shares_folder_id_across_entries() {
        let toml = r#"
driveRootFolderId = "root"

[[courses]]
year = 2026
courseId = "HII201"
name = "デザイン理論：UX基礎"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_FOLDER_DESIGN_THEORY"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_DESIGN_THEORY"

[[courses]]
year = 2026
courseId = "UX104"
name = "デザイン理論 III"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_FOLDER_DESIGN_THEORY"
url = "https://drive.google.com/drive/folders/FAKE_FOLDER_DESIGN_THEORY"
"#;
        let cdf: CourseDriveFolders = toml::from_str(toml).unwrap();
        assert_eq!(cdf.courses.len(), 2);
        assert_eq!(cdf.courses[0].drive_folders[0].id, cdf.courses[1].drive_folders[0].id);
    }

    #[test]
    fn empty_drive_folders_means_unresolved_for_is_resolved() {
        let toml = r#"
driveRootFolderId = "root"

[[courses]]
year = 2026
courseId = "CV101"
name = "地理情報システム"
matchStrategy = "unresolved"
unresolvedReason = "not-offered"
"#;
        let cdf: CourseDriveFolders = toml::from_str(toml).unwrap();
        assert!(!cdf.courses[0].is_resolved());
    }

    #[test]
    fn user_confirmed_entry_with_folders_is_resolved() {
        let toml = r#"
driveRootFolderId = "root"

[[courses]]
year = 2026
courseId = "INI401"
name = "x"
matchStrategy = "user-confirmed"
[[courses.driveFolders]]
id = "FAKE_DRIVE_FOLDER_ID_USER_CONFIRMED_0001"
url = "https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_USER_CONFIRMED_0001"
"#;
        let cdf: CourseDriveFolders = toml::from_str(toml).unwrap();
        assert!(cdf.courses[0].is_resolved());
        assert_eq!(cdf.summary().resolved, 1);
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compat() {
        let toml = r#"
driveRootFolderId = "root"
someFutureField = "ignored"

[[courses]]
year = 2026
courseId = "INI301"
name = "x"
matchStrategy = "exact"
futureFlag = true
[[courses.driveFolders]]
id = "FAKE_DRIVE_FOLDER_ID_FORWARD_COMPAT_0001"
url = "https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_FORWARD_COMPAT_0001"
"#;
        let cdf: CourseDriveFolders = toml::from_str(toml).expect("unknown fields must not error");
        assert_eq!(cdf.courses.len(), 1);
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        let path = std::path::PathBuf::from(format!(
            "/tmp/imoocs-test-missing-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        assert!(!path.exists(), "test precondition: path must not exist");
        let got = CourseDriveFolders::load(&path).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn parse_failure_maps_to_parse_error() {
        let broken = "driveRootFolderId = 123\n";
        let err = toml::from_str::<CourseDriveFolders>(broken)
            .map_err(|e| ImoocsError::Parse(format!("course-drive-folders.toml parse error: {e}")))
            .unwrap_err();
        assert!(matches!(err, ImoocsError::Parse(_)));
    }
}
