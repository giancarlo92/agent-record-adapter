// Part of Agent Record Adapter (see NOTICE).
//! Where secrets live: the system's own secret store (Windows Credential
//! Manager, the macOS Keychain, the Secret Service on Linux), one entry per
//! name, holding the value and its binding. A small index next to the user's
//! home lists the names and bindings, never the values, so `secret list` can
//! show them: the stores cannot all be enumerated.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::commands::secret_policy::StoredSecret;

const SERVICE: &str = "desktop-recorder-mcp";
const INDEX_DIR: &str = ".desktop-recorder-mcp";
const INDEX_FILE: &str = "secrets.json";

/// One name in the index: what `secret list` shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    #[serde(default)]
    pub any_field: bool,
    pub saved_ms: u64,
}

fn entry(name: &str) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE, name).map_err(store_error)
}

fn store_error(error: keyring::Error) -> AppError {
    AppError::Internal(format!("The system's secret store refused: {error}"))
}

pub fn save(name: &str, secret: &StoredSecret) -> Result<(), AppError> {
    let payload = serde_json::to_string(secret)?;
    entry(name)?.set_password(&payload).map_err(store_error)
}

/// The stored secret, or `None` when nothing is saved under that name.
pub fn load(name: &str) -> Result<Option<StoredSecret>, AppError> {
    match entry(name)?.get_password() {
        Ok(payload) => Ok(Some(serde_json::from_str(&payload)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(store_error(error)),
    }
}

/// Removes a secret; `false` when there was none.
pub fn remove(name: &str) -> Result<bool, AppError> {
    match entry(name)?.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(store_error(error)),
    }
}

/// The index file for a home folder.
pub fn index_path(home: &Path) -> PathBuf {
    home.join(INDEX_DIR).join(INDEX_FILE)
}

pub fn default_index() -> Result<PathBuf, AppError> {
    crate::refs::home_dir()
        .map(|home| index_path(&home))
        .ok_or_else(|| AppError::Internal("HOME directory not found".into()))
}

pub fn read_index(path: &Path) -> Vec<SecretInfo> {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

pub fn write_index(path: &Path, entries: &[SecretInfo]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(entries)?)?;
    Ok(())
}

/// The index with one name added or replaced.
pub fn with_entry(mut entries: Vec<SecretInfo>, info: SecretInfo) -> Vec<SecretInfo> {
    entries.retain(|entry| entry.name != info.name);
    entries.push(info);
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

#[cfg(test)]
#[path = "secret_store_tests.rs"]
mod tests;
