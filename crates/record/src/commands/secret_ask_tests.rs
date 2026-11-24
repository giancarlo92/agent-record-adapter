// Part of Agent Record Adapter (see NOTICE).
use super::*;
use crate::AdapterError;
use crate::{ActionOps, InputOps, ObservationOps, RecordOps, SystemOps};

/// An adapter whose own window answers with a fixed outcome.
struct Answers(Result<Option<&'static str>, ErrorCode>);
impl ObservationOps for Answers {}
impl ActionOps for Answers {}
impl SystemOps for Answers {}
impl InputOps for Answers {}
impl RecordOps for Answers {
    fn ask_secret(&self, title: &str, _message: &str) -> Result<Option<String>, AdapterError> {
        assert_eq!(title, TITLE);
        match &self.0 {
            Ok(answer) => Ok(answer.map(str::to_string)),
            Err(code) => Err(AdapterError::new(code.clone(), "failed")),
        }
    }
}

#[test]
fn the_platform_window_answer_is_returned() {
    let given = ask(&Answers(Ok(Some("typed"))), "message").unwrap();
    assert_eq!(given.as_deref(), Some("typed"));
}

#[test]
fn a_cancelled_window_gives_nothing() {
    assert_eq!(ask(&Answers(Ok(None)), "message").unwrap(), None);
}

#[test]
fn a_failing_window_is_an_error_not_a_fallback() {
    assert!(ask(&Answers(Err(ErrorCode::Internal)), "message").is_err());
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn without_a_window_or_dialog_tool_it_says_how_to_save_one() {
    let error = ask(&Answers(Err(ErrorCode::PlatformNotSupported)), "message").unwrap_err();
    assert!(error.to_string().contains("secret set"));
}
