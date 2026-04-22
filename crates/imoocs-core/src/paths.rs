//! XDG directory resolution (physically split by purpose — see plan §CLI Design Principles #6).
//!
//! - `config` (`$XDG_CONFIG_HOME/imoocs/`): portable, fine to commit to dotfiles.
//! - `data` (`$XDG_DATA_HOME/imoocs/`): credentials, written with umask 0o077.
//! - `cache` (`$XDG_CACHE_HOME/imoocs/`): cookies and slide PDFs; safe to delete.

use std::path::PathBuf;

use etcetera::{choose_base_strategy, BaseStrategy};

use crate::error::{ImoocsError, Result};

const APP_NAME: &str = "imoocs";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let strategy = choose_base_strategy()
            .map_err(|e| ImoocsError::Internal(format!("cannot resolve XDG base directories: {e}")))?;
        Ok(Self {
            config_dir: strategy.config_dir().join(APP_NAME),
            data_dir: strategy.data_dir().join(APP_NAME),
            cache_dir: strategy.cache_dir().join(APP_NAME),
        })
    }

    /// Convenience: `Paths` is `Clone`, this alias makes call sites read nicely.
    pub fn clone_paths(&self) -> Self {
        self.clone()
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.data_dir.join("credentials.toml")
    }

    pub fn cookies_file(&self) -> PathBuf {
        self.cache_dir.join("cookies.json")
    }

    pub fn slides_dir(&self) -> PathBuf {
        self.cache_dir.join("slides")
    }

    pub fn drive_dir(&self) -> PathBuf {
        self.cache_dir.join("drive")
    }
}
