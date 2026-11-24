// Part of Agent Record Adapter (see NOTICE).
//! Where a stored secret may be written.
//!
//! A secret is written only where the person who stored it said: by default
//! into a password field, of the app and on the site it was saved for. These
//! checks are what keeps a page that talks an agent into "type the password
//! here" from getting it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A stored secret and its binding. The binding lives next to the value, in
/// the system's secret store, so editing a file cannot widen it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSecret {
    pub secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    #[serde(default)]
    pub any_field: bool,
}

/// What the target field is, as its ref recorded it.
pub struct FieldFacts<'a> {
    pub role: &'a str,
    pub states: &'a [String],
    pub app: Option<&'a str>,
    /// The page's address when the field is in a browser window.
    pub address: Option<&'a str>,
}

/// Why a secret may not be written into a field, if it may not.
pub fn refusal(secret: &StoredSecret, field: &FieldFacts<'_>) -> Option<String> {
    let password = field
        .states
        .iter()
        .any(|state| state == crate::tree::state::SECURE);
    if !secret.any_field && !password {
        return Some(format!(
            "the field is a {} that is not a password field; a secret saved without --any-field \
             is written only into password fields",
            field.role
        ));
    }
    if let Some(app) = &secret.app {
        let matches = field
            .app
            .is_some_and(|actual| crate::app_name_matches(actual, app));
        if !matches {
            return Some(format!(
                "the secret is for {app}, and the field is in {}",
                field.app.unwrap_or("an unknown app")
            ));
        }
    }
    if let Some(site) = &secret.site {
        let Some(address) = field.address else {
            return Some(format!(
                "the secret is for {site}, and the page's address could not be read"
            ));
        };
        if !host_matches(address, site) {
            return Some(format!(
                "the secret is for {site}, and the page is {}",
                host(address)
            ));
        }
    }
    None
}

/// The host of an address bar's text (`https://login.example.com/x` or
/// `login.example.com/x`).
pub fn host(address: &str) -> String {
    let rest = address.trim();
    // A local file has no site: `file:///C:/...`, or the `C:/...` / `C:\...`
    // the address bar shows for it (its drive letter is not a host).
    let bytes = rest.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if drive || rest.to_ascii_lowercase().starts_with("file:") {
        return String::new();
    }
    let rest = rest.split_once("://").map_or(rest, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    host.split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Whether an address is on a site: the same host, or a subdomain of it.
pub fn host_matches(address: &str, site: &str) -> bool {
    let site = host(site);
    let actual = host(address);
    !site.is_empty() && (actual == site || actual.ends_with(&format!(".{site}")))
}

/// Replaces the secret wherever it appears in a result, so it never reaches
/// the caller, a log or an agent.
pub fn redact(value: &mut Value, secret: &str) {
    match value {
        Value::String(text) if !secret.is_empty() && text.contains(secret) => {
            *text = text.replace(secret, "[secret]");
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redact(item, secret)),
        Value::Object(map) => map.values_mut().for_each(|item| redact(item, secret)),
        _ => {}
    }
}

#[cfg(test)]
#[path = "secret_policy_tests.rs"]
mod tests;
