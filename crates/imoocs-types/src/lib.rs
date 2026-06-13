//! `imoocs-types`: 中立な共有型のみを持つ薄いクレート。
//!
//! 目的:
//! - `imoocs-browser` と `imoocs-core` の間で「循環依存」を作らない
//! - password / credentials の memory 露出を `secrecy::SecretString` で最小化
//!
//! 設計方針:
//! - HTTP / scraper / process 依存は一切持たない (テスト容易性を優先)
//! - `Debug` 実装は `secrecy` のリダクションを尊重 (`***`)

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// INIAD MOOCs のログイン credentials。password は `SecretString` で
/// 包み、`Debug` 出力時は `***` にリダクトされる。
#[derive(Clone, Serialize, Deserialize, Zeroize)]
pub struct Credentials {
    /// INIAD アカウントの username (`s1f...`)。これは PII なので tracing
    /// では出さないが、`Debug` 出力では生表示される (誤入力デバッグ用)。
    pub username: String,
    /// 秘密の password。`SecretString` 経由でしか取れない。
    #[serde(skip)]
    #[zeroize(skip)] // SecretString は内部で zeroize する
    password: SecretString,
}

impl Credentials {
    /// Plain String から構築。呼び出し側で速やかに password 元バッファを `zeroize` すること。
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: SecretString::new(password.into().into_boxed_str()),
        }
    }

    /// 子プロセスの stdin 書き込み用に password bytes を一時的に取り出す。
    /// 返値の `&str` はライフタイム内でのみ参照可能で、関数を抜けると `SecretString`
    /// が `zeroize` で消す。
    pub fn password(&self) -> &str {
        self.password.expose_secret()
    }
}

/// `Debug` 出力では `password` が `***` になることを保証する。
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"***")
            .finish()
    }
}

/// MOOCs の言語切替パラメータ (`?lang=ja|en`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ja => "ja",
            Self::En => "en",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_debug_redacts_password() {
        let c = Credentials::new("s1f123", "secret-pw");
        let dbg = format!("{c:?}");
        assert!(dbg.contains("s1f123"));
        assert!(dbg.contains("***"));
        assert!(!dbg.contains("secret-pw"), "password leaked in Debug: {dbg}");
    }

    #[test]
    fn credentials_password_exposes_value() {
        let c = Credentials::new("u", "pw");
        assert_eq!(c.password(), "pw");
    }

    #[test]
    fn credentials_serialize_skips_password() {
        let c = Credentials::new("u", "secret-pw");
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"u\""));
        assert!(!json.contains("secret-pw"), "password leaked in JSON: {json}");
    }

    #[test]
    fn lang_serialization() {
        assert_eq!(Lang::Ja.as_str(), "ja");
        assert_eq!(Lang::En.as_str(), "en");
    }
}
