//! `imoocs-browser`: agent-browser CLI を子プロセスとして spawn するラッパ。
//!
//! Phase A1 のスコープ:
//! - `AgentBrowser` (薄い process spawner)
//! - `BatchBuilder` (batch --json のコマンド列を組み立てる)
//! - `BrowserSession` (`--session-name imoocs` 固定のセッション抽象)
//! - `BrowserOps` trait + `FakeBrowserSession` (テスト容易性)
//! - `Snapshot` (a11y tree パース)
//!
//! 設計上の制約:
//! - imoocs-types のみ依存。imoocs-core / imoocs-cli への参照禁止 (循環依存防止)
//! - `secrecy::SecretString` 経由でのみ password を扱う

mod batch;
pub mod commands;
mod error;
mod install;
mod ops;
mod process;
mod session;
mod snapshot;

pub use batch::{BatchBuilder, BatchCommand, BatchOutcome, BatchResponse, LoadKind, Target};
pub use error::BrowserError;
pub use install::discover_binary;
pub use ops::{BrowserOps, FakeBrowserSession};
pub use process::AgentBrowser;
pub use session::BrowserSession;
pub use snapshot::{RefInfo, Snapshot, SnapshotOpts};
