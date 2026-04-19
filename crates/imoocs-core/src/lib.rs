//! imoocs-core — core library for the imoocs CLI.
//!
//! Provides:
//! - `auth`: login flows for MOOCs (Keycloak) and Google Workspace (SAML)
//! - `session`: authenticated HTTP session with cookie jar, CSRF cache, auto re-login
//! - `api`: typed wrappers for /assignments/... endpoints
//! - `scrape`: HTML scrapers (courses list, lessons, pages, problem form, slides iframe)
//! - `schemas`: serde+schemars types, stable JSON envelope
//! - `util`: small helpers (html, stdout, paths)

pub mod envelope;
pub mod error;
pub mod paths;
pub mod schemas;
pub mod util;

pub use envelope::{Envelope, ErrorDetail};
pub use error::{ExitCode, ImoocsError, Result};
