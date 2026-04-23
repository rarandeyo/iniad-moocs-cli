//! 認証付きの HTTP session: 永続化された cookie jar と CSRF cache を持つ reqwest クライアント。
//!
//! - 起動時に `$XDG_CACHE_HOME/imoocs/cookies.json` から cookie を読み込む
//! - login / 書き込み系 API の成功後、cookie を同ファイルへ書き戻す
//! - `meta[name="csrf-token"]` の値を session 単位でキャッシュする (419/422 時は再取得)
//! - UA は moocs-collect に倣って Chrome 124 on Windows を名乗る

use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use reqwest::Client;
use reqwest_cookie_store::CookieStoreMutex;
use tokio::sync::RwLock;

use crate::error::{ImoocsError, Result};
use crate::paths::Paths;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0";

pub const MOOCS_BASE: &str = "https://moocs.iniad.org";

pub struct Session {
    pub client: Client,
    pub cookies: Arc<CookieStoreMutex>,
    pub paths: Paths,
    csrf_token: Arc<RwLock<Option<String>>>,
}

impl Session {
    pub fn new(paths: Paths) -> Result<Self> {
        let cookie_path = paths.cookies_file();
        let cookies = load_cookie_jar(&cookie_path)?;
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(Arc::clone(&cookies))
            .gzip(true)
            .build()?;

        Ok(Self {
            client,
            cookies,
            paths,
            csrf_token: Arc::new(RwLock::new(None)),
        })
    }

    pub fn save_cookies(&self) -> Result<()> {
        save_cookie_jar(&self.cookies, &self.paths.cookies_file())
    }

    pub fn clear_cookies(&self) -> Result<()> {
        {
            let mut store = self
                .cookies
                .lock()
                .map_err(|_| ImoocsError::Internal("cookie store mutex poisoned".into()))?;
            store.clear();
        }
        let path = self.paths.cookies_file();
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub async fn csrf_token(&self) -> Option<String> {
        self.csrf_token.read().await.clone()
    }

    pub async fn set_csrf_token(&self, token: Option<String>) {
        *self.csrf_token.write().await = token;
    }

    /// reqwest が `request_url` 向けのリクエストで実際に送る cookie `name` の値を返す。
    /// jar の request-matching ルール (domain / path / secure / 期限) を通すので、
    /// SAPISIDHASH の計算元として使えば server が wire 上で見る値と完全一致する。
    pub fn cookie_value_for(&self, request_url: &reqwest::Url, name: &str) -> Option<String> {
        let store = self.cookies.lock().ok()?;
        let value = store
            .matches(request_url)
            .into_iter()
            .find(|c| c.name() == name)
            .map(|c| c.value().to_string());
        value
    }
}

fn load_cookie_jar(path: &Path) -> Result<Arc<CookieStoreMutex>> {
    if !path.exists() {
        return Ok(Arc::new(CookieStoreMutex::new(cookie_store::CookieStore::default())));
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let store =
        cookie_store::serde::json::load(reader).map_err(|e| ImoocsError::Parse(format!("cookie jar load: {e}")))?;
    Ok(Arc::new(CookieStoreMutex::new(store)))
}

fn save_cookie_jar(jar: &Arc<CookieStoreMutex>, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    {
        let store = jar
            .lock()
            .map_err(|_| ImoocsError::Internal("cookie store mutex poisoned".into()))?;
        cookie_store::serde::json::save(&store, &mut writer)
            .map_err(|e| ImoocsError::Internal(format!("cookie jar save: {e}")))?;
    }
    set_file_mode_0600(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode_0600(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn moocs_url(path: &str) -> String {
    format!("{MOOCS_BASE}{path}")
}

pub fn cookie_path_repr(paths: &Paths) -> PathBuf {
    paths.cookies_file()
}
