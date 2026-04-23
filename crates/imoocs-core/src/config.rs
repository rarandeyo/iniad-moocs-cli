//! `$XDG_CONFIG_HOME/imoocs/config.toml` に保存する永続設定。
//!
//! 機微でない preferences のみ保持する。secret は keyring と cookies.json (cache) に置く。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ImoocsError, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slides: Option<SlidesConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<AssignmentConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlidesConfig {
    /// ダウンロードしたスライド PDF の保存先。以下を受け付ける:
    /// - `"cache"` → `$XDG_CACHE_HOME/imoocs/slides/`
    /// - `"tmp"`   → `/tmp/imoocs/slides/` (デフォルト。OS 側で自動クリーンアップされる)
    /// - 絶対パス (例: `"/home/me/slides"`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssignmentConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<ConfirmMode>,
}

/// `assignment submit` / `assignment upload --force` の確定挙動を制御する。
/// `Auto` は無条件に `force=true` を送る。`Confirm` は対話プロンプトで人間が
/// `y` と答えた場合のみ `force=true` を送る。ユーザが拒否する、または
/// 非対話で実行された場合、CLI は API を呼ばずに Validation エラーで中断する
/// (draft 保存はされない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfirmMode {
    Auto,
    Confirm,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| ImoocsError::Parse(format!("config toml parse error: {e}")))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| ImoocsError::Internal(format!("config toml serialize error: {e}")))?;
        fs::write(path, body)?;
        Ok(())
    }

    pub fn clear(path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_mode_roundtrip_auto() {
        let cfg: Config = toml::from_str("[assignment]\nconfirm = \"auto\"\n").unwrap();
        assert_eq!(cfg.assignment.unwrap().confirm, Some(ConfirmMode::Auto));
    }

    #[test]
    fn confirm_mode_roundtrip_confirm() {
        let cfg: Config = toml::from_str("[assignment]\nconfirm = \"confirm\"\n").unwrap();
        assert_eq!(cfg.assignment.unwrap().confirm, Some(ConfirmMode::Confirm));
    }

    #[test]
    fn confirm_mode_unknown_value_is_error() {
        let err = toml::from_str::<Config>("[assignment]\nconfirm = \"yolo\"\n");
        assert!(err.is_err(), "unknown confirm value must be a parse error");
    }

    #[test]
    fn assignment_absent_loads_as_none() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.assignment.is_none());
    }

    #[test]
    fn assignment_serialises_back_to_lowercase() {
        let cfg = Config {
            assignment: Some(AssignmentConfig {
                confirm: Some(ConfirmMode::Confirm),
            }),
            ..Config::default()
        };
        let body = toml::to_string(&cfg).unwrap();
        assert!(body.contains("confirm = \"confirm\""));
    }
}
