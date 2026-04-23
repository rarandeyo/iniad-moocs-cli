//! imoocs-core — imoocs CLI のコアライブラリ。
//!
//! 提供モジュール:
//! - `auth`: MOOCs (Keycloak) と Google Workspace (SAML) の login flow
//! - `session`: cookie jar / CSRF cache / 自動 re-login 付きの認証 HTTP session
//! - `api`: `/assignments/...` などに対する型付きラッパー
//! - `scrape`: HTML scraper 群 (コース一覧、lesson、page、problem form、slide iframe)
//! - `schemas`: serde + schemars の型定義、安定 JSON envelope
//! - `util`: 小さめのヘルパー (html / stdout / paths)

pub mod api;
pub mod auth;
pub mod config;
pub mod envelope;
pub mod error;
pub mod keyring;
pub mod paths;
pub mod schemas;
pub mod scrape;
pub mod session;
pub mod util;

pub use envelope::{Envelope, ErrorDetail};
pub use error::{ExitCode, ImoocsError, Result};
pub use session::Session;
