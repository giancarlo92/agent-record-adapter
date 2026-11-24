// Part of Agent Record Adapter (see NOTICE).
use super::*;
use serde_json::json;

fn secret(app: Option<&str>, site: Option<&str>, any_field: bool) -> StoredSecret {
    StoredSecret {
        secret: "s3cret!".into(),
        app: app.map(Into::into),
        site: site.map(Into::into),
        any_field,
    }
}

fn field<'a>(
    states: &'a [String],
    app: Option<&'a str>,
    address: Option<&'a str>,
) -> FieldFacts<'a> {
    FieldFacts {
        role: "textfield",
        states,
        app,
        address,
    }
}

#[test]
fn a_secret_goes_into_a_password_field_of_its_app_and_site() {
    let secure = vec!["secure".to_string()];
    let saved = secret(Some("chrome.exe"), Some("login.example.com"), false);
    let ok = field(
        &secure,
        Some("chrome.exe"),
        Some("https://login.example.com/session?x=1"),
    );
    assert_eq!(refusal(&saved, &ok), None);
}

#[test]
fn a_plain_field_is_refused_unless_the_secret_allows_any_field() {
    let plain: Vec<String> = Vec::new();
    let target = field(&plain, Some("chrome.exe"), None);
    assert!(refusal(&secret(None, None, false), &target).is_some());
    assert_eq!(refusal(&secret(None, None, true), &target), None);
}

#[test]
fn another_app_or_site_is_refused() {
    let secure = vec!["secure".to_string()];
    let saved = secret(Some("chrome.exe"), Some("example.com"), false);
    assert!(
        refusal(
            &saved,
            &field(&secure, Some("notepad.exe"), Some("example.com"))
        )
        .is_some()
    );
    let phishing = field(
        &secure,
        Some("chrome.exe"),
        Some("https://example.com.evil.test/login"),
    );
    assert!(
        refusal(&saved, &phishing)
            .unwrap()
            .contains("example.com.evil.test")
    );
    let unknown = field(&secure, Some("chrome.exe"), None);
    assert!(
        refusal(&saved, &unknown)
            .unwrap()
            .contains("could not be read")
    );
}

#[test]
fn hosts_are_read_from_any_address_shape() {
    assert_eq!(
        host("https://User@Login.Example.com:8443/a?b#c"),
        "login.example.com"
    );
    assert_eq!(host("example.com/path"), "example.com");
    assert!(host_matches("https://a.example.com/x", "example.com"));
    assert!(host_matches("example.com", "https://example.com/"));
    assert!(!host_matches("https://notexample.com", "example.com"));
    assert!(!host_matches("https://example.com", ""));
}

#[test]
fn the_secret_is_removed_from_any_result() {
    let mut value = json!({
        "post_state": { "value": "s3cret!" },
        "details": ["typed s3cret! into the field", 3],
        "other": "kept"
    });
    redact(&mut value, "s3cret!");
    let text = value.to_string();
    assert!(!text.contains("s3cret!"), "{text}");
    assert!(text.contains("[secret]"));
    assert_eq!(value["other"], "kept");
}

#[test]
fn a_local_file_has_no_site() {
    assert_eq!(host("file:///C:/Users/me/login.html"), "");
    assert_eq!(host("C:/Users/me/login.html"), "");
    assert_eq!(host(r"C:\Users\me\login.html"), "");
    assert_eq!(
        host("https://login.example.com:8443/x"),
        "login.example.com"
    );
}
