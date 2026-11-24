// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn names_are_short_and_plain() {
    assert!(valid_name("bank-login").is_ok());
    assert!(valid_name("mail.work_2").is_ok());
    assert!(valid_name("").is_err());
    assert!(valid_name("two words").is_err());
    assert!(valid_name(&"x".repeat(65)).is_err());
}

#[test]
fn an_error_never_carries_the_secret() {
    let error = AppError::Adapter(
        crate::AdapterError::new(crate::ErrorCode::ActionFailed, "could not set s3cret! here")
            .with_details(json!({ "expected": "s3cret!" })),
    );
    let AppError::Adapter(clean) = redact_error(error, "s3cret!") else {
        panic!("still an adapter error");
    };
    assert!(!clean.message.contains("s3cret!"));
    assert!(!clean.details.unwrap().to_string().contains("s3cret!"));
}
