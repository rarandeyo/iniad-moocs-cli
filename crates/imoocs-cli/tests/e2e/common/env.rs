use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

/// 1 つの e2e テストに割り当てる XDG 隔離 tempdir。
///
/// レイアウト:
///   <root>/home          (HOME)
///   <root>/config        (XDG_CONFIG_HOME)
///   <root>/data          (XDG_DATA_HOME)
///   <root>/cache         (XDG_CACHE_HOME)
///   <root>/state         (XDG_STATE_HOME)
///
/// `Paths::discover()` (etcetera) が Linux でこれらを参照するので、各テストの
/// config / cookies / drafts は完全に分離される。
pub struct TempXdg {
    _root: TempDir,
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
}

impl TempXdg {
    pub fn new() -> Self {
        let root = TempDir::new().expect("create tempdir for TempXdg");
        let path = root.path().to_path_buf();
        let make = |name: &str| -> PathBuf {
            let p = path.join(name);
            fs::create_dir_all(&p).expect("create xdg subdir");
            p
        };
        Self {
            home: make("home"),
            config: make("config"),
            data: make("data"),
            cache: make("cache"),
            state: make("state"),
            _root: root,
        }
    }

    /// `<XDG_CONFIG_HOME>/imoocs/config.toml` を書いてパスを返す。
    pub fn write_config(&self, body: &str) -> PathBuf {
        self.write_imoocs_config_file("config.toml", body)
    }

    /// `<XDG_CONFIG_HOME>/imoocs/course-drive-folders.toml` を書いてパスを返す。
    pub fn write_drive_folders(&self, body: &str) -> PathBuf {
        self.write_imoocs_config_file("course-drive-folders.toml", body)
    }

    fn write_imoocs_config_file(&self, name: &str, body: &str) -> PathBuf {
        let dir = self.config.join("imoocs");
        fs::create_dir_all(&dir).expect("create imoocs config dir");
        let path = dir.join(name);
        fs::write(&path, body).unwrap_or_else(|e| panic!("write {name}: {e}"));
        path
    }

    /// XDG_STATE_HOME 配下の drafts ディレクトリ。
    pub fn drafts_dir(&self) -> PathBuf {
        self.state.join("imoocs").join("drafts")
    }
}

impl Default for TempXdg {
    fn default() -> Self {
        Self::new()
    }
}
