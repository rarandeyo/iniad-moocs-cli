//! Persistent config at `$XDG_CONFIG_HOME/imoocs/config.toml`.
//!
//! Holds non-sensitive preferences. Secrets go to keyring + cookies.json (cache).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ImoocsError, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slides: Option<SlidesConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlidesConfig {
    /// Where to store downloaded slide PDFs. Accepts:
    /// - `"cache"` → `$XDG_CACHE_HOME/imoocs/slides/`
    /// - `"tmp"`   → `/tmp/imoocs/slides/` (default; auto-cleaned by the OS)
    /// - absolute path (e.g. `"/home/me/slides"`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_dir: Option<String>,
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
