//! Thin wrapper around the `keyring` crate (v3).
//!
//! Service name: `imoocs`. Account: the username.

use keyring::Entry;

use crate::error::{ImoocsError, Result};

pub const SERVICE: &str = "imoocs";

pub fn entry(username: &str) -> Result<Entry> {
    Entry::new(SERVICE, username).map_err(|e| ImoocsError::Internal(format!("keyring entry: {e}")))
}

pub fn get_password(username: &str) -> Result<Option<String>> {
    let e = entry(username)?;
    match e.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(ImoocsError::Internal(format!("keyring get_password: {err}"))),
    }
}

pub fn set_password(username: &str, password: &str) -> Result<()> {
    let e = entry(username)?;
    e.set_password(password)
        .map_err(|err| ImoocsError::Internal(format!("keyring set_password: {err}")))
}

pub fn delete_credential(username: &str) -> Result<()> {
    let e = entry(username)?;
    match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(ImoocsError::Internal(format!("keyring delete_credential: {err}"))),
    }
}
