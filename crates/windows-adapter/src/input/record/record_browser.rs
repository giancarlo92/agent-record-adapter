// Part of Agent Record Adapter (see NOTICE).
//! Browser windows.
//!
//! A recorded web action needs the page it happened on, so the recorder reads
//! its address, scheme included, into the window's `url`.
//! Chromium also builds the accessibility tree of a page only once a client
//! asks for it: the first hit test on a fresh page returns the bare browser
//! widget, and a moment later the element under the pointer.

use std::time::Duration;

use uiautomation::UIAutomation;
use uiautomation::patterns::UIValuePattern;
use uiautomation::types::Handle;

use crate::tree::element::UIAElement;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::{PropertyOutcome, PropertyValue, read_one};

const BROWSERS: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "brave.exe",
    "vivaldi.exe",
    "opera.exe",
];

const CHROMIUM_WIDGET: &str = "Chrome_WidgetWin_1";

/// How many times a hit on the bare Chromium widget is read again, and how
/// long apart, while the page builds its accessibility tree.
pub(super) const CHROMIUM_RETRIES: usize = 4;
pub(super) const CHROMIUM_RETRY_GAP: Duration = Duration::from_millis(150);

/// Whether an application is a browser whose address the recorder reads.
pub(super) fn is_browser(app: &str) -> bool {
    BROWSERS.iter().any(|name| app.eq_ignore_ascii_case(name))
}

/// Whether a top-level window is a browser's own frame (where its tabs and
/// address bar are), not a dialog of the browser's process such as the file
/// picker, which has no page and a large tree that is slow to search.
pub(super) fn is_browser_frame(root: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    if root == 0 {
        return false;
    }
    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(root as _, class.as_mut_ptr(), class.len() as i32) };
    length > 0 && String::from_utf16_lossy(&class[..length as usize]) == CHROMIUM_WIDGET
}

/// The address of the page that holds `element`: its nearest Chromium
/// document ancestor, whose value is the page's whole address. This is the
/// tab that received the action, whatever the window's title says (the tabs of
/// one web app often share a title).
pub(super) fn page_of(walker: &uiautomation::UITreeWalker, element: &UIAElement) -> Option<String> {
    const DOCUMENT: i32 = 50030;
    const MAX_STEPS: usize = 80;
    let mut current = element.0.clone();
    for _ in 0..MAX_STEPS {
        let is_document = current
            .get_control_type()
            .is_ok_and(|kind| kind as i32 == DOCUMENT);
        if is_document && current.get_framework_id().ok().as_deref() == Some("Chrome") {
            let url = current
                .get_pattern::<UIValuePattern>()
                .and_then(|pattern| pattern.get_value())
                .ok()
                .map(|url| url.trim().to_owned())
                .filter(|url| url.contains("://"));
            if url.is_some() {
                return url;
            }
        }
        current = walker.get_parent(&current).ok()?;
    }
    None
}

/// Whether a hit is Chromium's own unnamed widget instead of an element of
/// the page.
pub(super) fn is_bare_chromium(class: Option<&str>, named: bool) -> bool {
    !named && class == Some(CHROMIUM_WIDGET)
}

/// The address of the page the browser window `root` shows, if it has one.
/// The address bar hides the scheme (`127.0.0.1:4200/auth/login` for an
/// `http://` page), so the page's own document, which holds the whole
/// address, is read first.
pub(super) fn address(client: &UIAutomation, root: isize) -> Option<String> {
    let window = client.element_from_handle(Handle::from(root)).ok()?;
    let page = crate::actions::chromium_input::active_document(&window, client)
        .and_then(|document| document.get_pattern::<UIValuePattern>().ok())
        .and_then(|pattern| pattern.get_value().ok())
        .map(|url| url.trim().to_owned())
        .filter(|url| url.contains("://"));
    if page.is_some() {
        return page;
    }
    // The address bar, by its class: its name is in the browser's language.
    let bar = crate::actions::chromium_input::omnibox(root as _, client)?;
    match read_one(&UIAElement::from(bar), TreeProperty::Value) {
        PropertyOutcome::Known(PropertyValue::Text(text)) if !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "record_browser_tests.rs"]
mod tests;
