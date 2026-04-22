//! XDG directory resolution (physically split by purpose — see plan §CLI Design Principles #6).
//!
//! - `config` (`$XDG_CONFIG_HOME/imoocs/`): portable, fine to commit to dotfiles.
//! - `data` (`$XDG_DATA_HOME/imoocs/`): credentials, written with umask 0o077.
//! - `cache` (`$XDG_CACHE_HOME/imoocs/`): cookies and Drive downloads; safe to delete.
//!
//! Slide PDFs default to `/tmp/imoocs/slides/` (see `DEFAULT_SLIDES_OUT_DIR`)
//! but can be redirected via `config.toml [slides] out_dir` or the
//! `imoocs slide fetch --out-dir` flag. See `resolve_slides_out_dir`.

use std::path::{Path, PathBuf};

use etcetera::{choose_base_strategy, BaseStrategy};

use crate::error::{ImoocsError, Result};

const APP_NAME: &str = "imoocs";

/// Default value for `slides.out_dir` when neither config nor CLI flag sets it.
pub const DEFAULT_SLIDES_OUT_DIR: &str = "tmp";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    slides_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let strategy = choose_base_strategy()
            .map_err(|e| ImoocsError::Internal(format!("cannot resolve XDG base directories: {e}")))?;
        let config_dir = strategy.config_dir().join(APP_NAME);
        let data_dir = strategy.data_dir().join(APP_NAME);
        let cache_dir = strategy.cache_dir().join(APP_NAME);
        // Start with the XDG cache default; the CLI layer overrides this via
        // `with_slides_dir` after reading config + CLI flags.
        let slides_dir = cache_dir.join("slides");
        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
            slides_dir,
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
        self.slides_dir.clone()
    }

    pub fn drive_dir(&self) -> PathBuf {
        self.cache_dir.join("drive")
    }

    /// Builder-style override for the slide PDF destination.
    pub fn with_slides_dir(mut self, dir: PathBuf) -> Self {
        self.slides_dir = dir;
        self
    }
}

/// Resolve a `slides.out_dir` value (from config or CLI) into an absolute path.
///
/// Accepted values:
/// - `"cache"` → `<cache_dir>/slides`
/// - `"tmp"`   → `/tmp/imoocs/slides`
/// - any absolute path, used as-is
///
/// Anything else is a validation error.
pub fn resolve_slides_out_dir(value: &str, cache_dir: &Path) -> Result<PathBuf> {
    match value {
        "cache" => Ok(cache_dir.join("slides")),
        "tmp" => Ok(PathBuf::from("/tmp/imoocs/slides")),
        p if Path::new(p).is_absolute() => Ok(PathBuf::from(p)),
        _ => Err(ImoocsError::Validation(format!(
            "slides out_dir must be 'cache', 'tmp', or an absolute path (got {value:?})"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_value_maps_under_cache_dir() {
        let got = resolve_slides_out_dir("cache", Path::new("/home/u/.cache/imoocs")).unwrap();
        assert_eq!(got, PathBuf::from("/home/u/.cache/imoocs/slides"));
    }

    #[test]
    fn tmp_value_maps_to_tmp_slides() {
        let got = resolve_slides_out_dir("tmp", Path::new("/home/u/.cache/imoocs")).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/imoocs/slides"));
    }

    #[test]
    fn absolute_path_passes_through() {
        let got = resolve_slides_out_dir("/srv/slides", Path::new("/irrelevant")).unwrap();
        assert_eq!(got, PathBuf::from("/srv/slides"));
    }

    #[test]
    fn relative_path_is_rejected() {
        let err = resolve_slides_out_dir("./slides", Path::new("/irrelevant")).unwrap_err();
        assert!(matches!(err, ImoocsError::Validation(_)));
    }

    #[test]
    fn with_slides_dir_overrides_default() {
        let p = Paths {
            config_dir: PathBuf::from("/c"),
            data_dir: PathBuf::from("/d"),
            cache_dir: PathBuf::from("/cache"),
            slides_dir: PathBuf::from("/cache/slides"),
        };
        let p = p.with_slides_dir(PathBuf::from("/tmp/imoocs/slides"));
        assert_eq!(p.slides_dir(), PathBuf::from("/tmp/imoocs/slides"));
    }
}
