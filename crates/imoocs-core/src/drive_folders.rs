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
    #[serde(default)]
    pub drive_folder_id: String,
    #[serde(default)]
    pub drive_folder_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_at: Option<String>,
    pub match_strategy: MatchStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MatchStrategy {
    Exact,
    Partial,
    UserConfirmed,
    Unresolved,
}

impl CourseDriveFolderEntry {
    pub fn is_resolved(&self) -> bool {
        self.match_strategy != MatchStrategy::Unresolved && !self.drive_folder_id.is_empty()
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
driveRootFolderId = "FAKE_DRIVE_ROOT_HIST_REDACT0001"

[[courses]]
year = 2026
courseId = "INI301"
name = "機械学習と人工知能"
driveFolderId = "abc123"
driveFolderUrl = "https://drive.google.com/drive/folders/abc123"
matchedAt = "2026-04-23"
matchStrategy = "exact"

[[courses]]
year = 2026
courseId = "INI302"
name = "データサイエンス入門"
driveFolderId = ""
driveFolderUrl = ""
matchStrategy = "unresolved"
"#;

    #[test]
    fn parses_full_sample() {
        let cdf: CourseDriveFolders = toml::from_str(FULL_SAMPLE).unwrap();
        assert_eq!(cdf.drive_root_folder_id, "FAKE_DRIVE_ROOT_HIST_REDACT0001");
        assert_eq!(cdf.courses.len(), 2);
        assert_eq!(cdf.courses[0].course_id, "INI301");
        assert_eq!(cdf.courses[0].match_strategy, MatchStrategy::Exact);
        assert_eq!(cdf.courses[0].matched_at.as_deref(), Some("2026-04-23"));
        assert!(cdf.courses[0].is_resolved());
        assert_eq!(cdf.courses[1].match_strategy, MatchStrategy::Unresolved);
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
    fn user_confirmed_entry_with_id_is_resolved() {
        let toml = r#"
driveRootFolderId = "root"

[[courses]]
year = 2026
courseId = "INI401"
name = "x"
driveFolderId = "fid"
driveFolderUrl = "https://drive.google.com/drive/folders/fid"
matchStrategy = "user-confirmed"
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
driveFolderId = "fid"
driveFolderUrl = "u"
matchStrategy = "exact"
futureFlag = true
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
