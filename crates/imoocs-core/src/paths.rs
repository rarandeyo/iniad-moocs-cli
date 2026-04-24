//! XDG ディレクトリの解決 (用途別に物理的に分離 — plan §CLI Design Principles #6 参照)。
//!
//! - `config` (`$XDG_CONFIG_HOME/imoocs/`): portable、dotfiles に含めて OK。
//! - `data` (`$XDG_DATA_HOME/imoocs/`): credential 類、umask 0o077 で書き込む。
//! - `cache` (`$XDG_CACHE_HOME/imoocs/`): cookie と Drive ダウンロード。削除しても構わない。
//! - `state` (`$XDG_STATE_HOME/imoocs/`): draft (未 push の提出物) 等の「揮発でない実行状態」。
//!   Apple/Windows で XDG の state 概念がないプラットフォームでは `data_dir/state` に fallback。
//!
//! スライド PDF の既定は `/tmp/imoocs/slides/` (`DEFAULT_SLIDES_OUT_DIR` 参照) だが、
//! `config.toml [slides] out_dir` または `imoocs slide fetch --out-dir` で
//! リダイレクト可能。詳細は `resolve_slides_out_dir`。

use std::path::{Path, PathBuf};

use etcetera::{choose_base_strategy, BaseStrategy};

use crate::error::{ImoocsError, Result};

const APP_NAME: &str = "imoocs";

pub const DEFAULT_SLIDES_OUT_DIR: &str = "tmp";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    slides_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let strategy = choose_base_strategy()
            .map_err(|e| ImoocsError::Internal(format!("cannot resolve XDG base directories: {e}")))?;
        let config_dir = strategy.config_dir().join(APP_NAME);
        let data_dir = strategy.data_dir().join(APP_NAME);
        let cache_dir = strategy.cache_dir().join(APP_NAME);
        // XDG: $XDG_STATE_HOME/imoocs (既定 $HOME/.local/state/imoocs)
        // Apple/Windows は state 概念がないので data_dir 配下の state/ に fallback。
        let state_dir = strategy
            .state_dir()
            .map(|d| d.join(APP_NAME))
            .unwrap_or_else(|| data_dir.join("state"));
        // いったん XDG cache のデフォルトを設定しておき、CLI 層が config と
        // flag を読んだ後に `with_slides_dir` で上書きする
        let slides_dir = cache_dir.join("slides");
        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
            state_dir,
            slides_dir,
        })
    }

    pub fn clone_paths(&self) -> Self {
        self.clone()
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn course_drive_folders_file(&self) -> PathBuf {
        self.config_dir.join("course-drive-folders.toml")
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

    pub fn drafts_dir(&self) -> PathBuf {
        self.state_dir.join("drafts")
    }

    pub fn with_slides_dir(mut self, dir: PathBuf) -> Self {
        self.slides_dir = dir;
        self
    }
}

/// `slides.out_dir` の値 (config / CLI 由来) を絶対パスに解決する。
///
/// 許容する値:
/// - `"cache"` → `<cache_dir>/slides`
/// - `"tmp"`   → `/tmp/imoocs/slides`
/// - 任意の絶対パス (そのまま使う)
///
/// それ以外は Validation エラー。
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
            state_dir: PathBuf::from("/state"),
            slides_dir: PathBuf::from("/cache/slides"),
        };
        let p = p.with_slides_dir(PathBuf::from("/tmp/imoocs/slides"));
        assert_eq!(p.slides_dir(), PathBuf::from("/tmp/imoocs/slides"));
    }

    #[test]
    fn drafts_dir_is_under_state_dir() {
        let p = Paths {
            config_dir: PathBuf::from("/c"),
            data_dir: PathBuf::from("/d"),
            cache_dir: PathBuf::from("/cache"),
            state_dir: PathBuf::from("/home/u/.local/state/imoocs"),
            slides_dir: PathBuf::from("/cache/slides"),
        };
        assert_eq!(p.drafts_dir(), PathBuf::from("/home/u/.local/state/imoocs/drafts"));
    }
}
