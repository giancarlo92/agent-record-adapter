// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! Pointer clicks, keys and addresses for a Chromium browser the person
//! already uses (Chrome, Edge), with their own profile and session, in the
//! background, with no DevTools endpoint.
//!
//! - **Click.** An accessibility click on a page reaches only the element's
//!   own click handler: components that listen to the pointer (a date
//!   picker's button opened on `mousedown`, a menu group opened from its
//!   header) do not answer. The browser window takes mouse messages sent to
//!   it while it is behind others and turns them into trusted pointer events
//!   (move, down, up, click) at that point of the page, so the click goes
//!   there: at the part of the element that shows its name (a group's header),
//!   when the element is shown and that point is inside the window.
//! - **Keys.** Chromium drops keyboard messages while its window is not
//!   active. Telling the window it is active and has the focus, with the
//!   messages Windows itself would send (`WM_ACTIVATE`, `WM_SETFOCUS`), and
//!   taking that back after the key, lets the key reach the page's focused
//!   element. Windows' own foreground and keyboard focus never move: the
//!   person keeps typing where they were.
//! - **Address.** The address bar (`OmniboxViewViews`, whatever the browser's
//!   language) takes the address as its value, and Enter opens it in the same
//!   tab, with the tab's cookies and storage.

use std::time::{Duration, Instant};

use agent_desktop_core::{
    ActionStep, AdapterError, Deadline, ErrorCode, StepMechanism, WindowInfo,
};
use uiautomation::patterns::{UIScrollItemPattern, UIValuePattern};
use uiautomation::types::{Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetClientRect, HWND_BOTTOM, IsIconic, IsWindow,
    SMTO_ABORTIFHUNG, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SendMessageTimeoutW, SetWindowPos, ShowWindow, WA_ACTIVE, WA_INACTIVE, WM_ACTIVATE,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETFOCUS,
};

use crate::tree::automation::automation_client;
use crate::tree::element::UIAElement;

const CLICK_LABEL: &str = "PostMessage.PointerClick";
const MK_LBUTTON: u32 = 0x0001;
pub(crate) const CHROMIUM_FRAMEWORK: &str = "Chrome";
const CHROMIUM_WINDOW_CLASS: &str = "Chrome_WidgetWin_1";
const OMNIBOX_CLASS: &str = "OmniboxViewViews";
/// UIA_TextControlTypeId and UIA_DocumentControlTypeId.
const TEXT_CONTROL_TYPE: i32 = 50020;
const DOCUMENT_CONTROL_TYPE: i32 = 50030;
const SEND_TIMEOUT_MS: u32 = 2_000;
const MAX_ANCESTORS: usize = 96;
/// How many nodes under an element are read for the part to click.
const TEXT_NODES: usize = 200;
const NAVIGATE_TIMEOUT: Duration = Duration::from_secs(20);

fn send(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> bool {
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(
            hwnd,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut result,
        ) != 0
    }
}

/// Runs `deliver` while the browser window believes it is active and has the
/// focus; Windows' foreground is not touched.
pub(crate) fn while_active<T>(top: HWND, deliver: impl FnOnce() -> T) -> T {
    send(top, WM_ACTIVATE, WA_ACTIVE as usize, 0);
    send(top, WM_SETFOCUS, 0, 0);
    let delivered = deliver();
    send(top, WM_KILLFOCUS, 0, 0);
    send(top, WM_ACTIVATE, WA_INACTIVE as usize, 0);
    delivered
}

/// Whether a top-level window handle is a Chromium browser window, by its
/// window class (a Win32 read: it never waits on the app, as a UI Automation
/// call to a busy app's own provider can).
pub(crate) fn is_chromium_hwnd(hwnd: HWND) -> bool {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    len > 0 && String::from_utf16_lossy(&name[..len as usize]) == CHROMIUM_WINDOW_CLASS
}

/// Whether a top-level window is a Chromium browser window.
pub(crate) fn is_chromium_top(window: &WindowInfo) -> Option<HWND> {
    let raw: usize = window.id.strip_prefix("w-")?.parse().ok()?;
    let hwnd = raw as HWND;
    if raw == 0 || unsafe { IsWindow(hwnd) } == 0 {
        return None;
    }
    is_chromium_hwnd(hwnd).then_some(hwnd)
}

/// Sends `keys` (down, char, up messages) to a Chromium window, active for
/// that moment only.
pub(crate) fn send_keys(top: HWND, keys: &[(u32, usize, isize)]) -> bool {
    while_active(top, || {
        keys.iter()
            .all(|(message, wparam, lparam)| send(top, *message, *wparam, *lparam))
    })
}

pub(crate) fn top_window(element: &UIElement, client: &UIAutomation) -> Option<HWND> {
    let walker = client.get_control_view_walker().ok()?;
    let mut current = element.clone();
    for _ in 0..MAX_ANCESTORS {
        if let Ok(handle) = current.get_native_window_handle() {
            let hwnd: windows::Win32::Foundation::HWND = handle.into();
            if !hwnd.0.is_null() {
                let root = unsafe { GetAncestor(hwnd.0 as HWND, GA_ROOT) };
                if !root.is_null() {
                    return Some(root);
                }
            }
        }
        current = walker.get_parent(&current).ok()?;
    }
    None
}

fn inside_document(element: &UIElement, client: &UIAutomation) -> bool {
    let Ok(walker) = client.get_control_view_walker() else {
        return false;
    };
    let mut current = element.clone();
    for _ in 0..MAX_ANCESTORS {
        if current.get_control_type().ok().map(|kind| kind as i32) == Some(DOCUMENT_CONTROL_TYPE) {
            return true;
        }
        match walker.get_parent(&current) {
            Ok(parent) => current = parent,
            Err(_) => return false,
        }
    }
    false
}

/// The part of the element a person would click: the text that shows its
/// name (a menu group's header); else its first visible text in document
/// order (the header of a group, a list item or a card that has no name of
/// its own, never the middle of the group, which may be one of its links once
/// it is open); else the element.
fn name_part(element: &UIElement, client: &UIAutomation) -> UIElement {
    let shown = |part: &UIElement| {
        part.get_bounding_rectangle()
            .is_ok_and(|r| r.get_width() > 0 && r.get_height() > 0)
            && !part.is_offscreen().unwrap_or(false)
    };
    let Ok(walker) = client.get_raw_view_walker() else {
        return element.clone();
    };
    let name = element.get_name().unwrap_or_default();
    let name = name.trim();
    let mut first_text: Option<UIElement> = None;
    // Document order, depth first, a bounded number of nodes.
    let mut stack = Vec::new();
    if let Ok(child) = walker.get_first_child(element) {
        stack.push(child);
    }
    let mut read = 0;
    while let Some(node) = stack.pop() {
        read += 1;
        if read > TEXT_NODES {
            break;
        }
        if let Ok(next) = walker.get_next_sibling(&node) {
            stack.push(next);
        }
        let is_text =
            node.get_control_type().ok().map(|kind| kind as i32) == Some(TEXT_CONTROL_TYPE);
        if is_text && shown(&node) {
            let text = node.get_name().unwrap_or_default();
            let text = text.trim();
            if !name.is_empty() && text == name {
                return node;
            }
            if first_text.is_none() && !text.is_empty() {
                first_text = Some(node.clone());
                if name.is_empty() {
                    break;
                }
            }
        }
        if let Ok(child) = walker.get_first_child(&node) {
            stack.push(child);
        }
    }
    first_text.unwrap_or_else(|| element.clone())
}

/// The part of a checkbox or radio button a person would click: the control
/// itself when it has a real box, else (a component library's visually hidden
/// input, 1 px wide) the nearest container around it that has one.
fn control_part(element: &UIElement, client: &UIAutomation) -> UIElement {
    let size = |part: &UIElement| {
        part.get_bounding_rectangle()
            .map(|r| (r.get_width(), r.get_height()))
            .unwrap_or((0, 0))
    };
    // A control clipped to nothing (PrimeNG's hidden input) still reports a
    // box, often beside the drawn one, and says it is offscreen while the
    // page around it is in view: the box around it is clicked instead.
    let (width, height) = size(element);
    if width >= 6 && height >= 6 && !element.is_offscreen().unwrap_or(false) {
        return element.clone();
    }
    let Ok(walker) = client.get_control_view_walker() else {
        return element.clone();
    };
    let mut current = element.clone();
    for _ in 0..3 {
        let Ok(parent) = walker.get_parent(&current) else {
            break;
        };
        let (width, height) = size(&parent);
        if width >= 8 && height >= 8 {
            return parent;
        }
        current = parent;
    }
    element.clone()
}

/// A trusted pointer click on a web element of a Chromium browser, as mouse
/// messages to its window, at the part a person would click (`name_part`).
/// `None` when the element is not Chromium page content, is a form control
/// (those keep their accessibility actions first, see `pointer_click_control`),
/// has no shown point, or the window did not take the messages: the caller goes
/// on to its other ways.
pub(crate) fn pointer_click(
    element: &UIAElement,
    deadline: Deadline,
) -> Result<Option<ActionStep>, AdapterError> {
    use uiautomation::types::ControlType;
    if element.0.get_framework_id().ok().as_deref() != Some(CHROMIUM_FRAMEWORK)
        || matches!(
            element.0.get_control_type().ok(),
            Some(
                ControlType::CheckBox
                    | ControlType::RadioButton
                    | ControlType::Edit
                    | ControlType::ComboBox
                    | ControlType::Slider
                    | ControlType::Spinner
            )
        )
    {
        return Ok(None);
    }
    click_part(element, deadline, name_part)
}

/// The element of the page that has keyboard focus in the Chromium window
/// `top`, read through accessibility; Chromium keeps it while the window is in
/// the background. The page's document when no control has the focus.
pub(crate) fn page_focus(top: HWND) -> Option<UIAElement> {
    let client = automation_client().ok()?;
    let root = client
        .element_from_handle(Handle::from(top as isize))
        .ok()?;
    let condition = client
        .create_property_condition(UIProperty::HasKeyboardFocus, Variant::from(true), None)
        .ok()?;
    let found = root.find_first(TreeScope::Descendants, &condition).ok()?;
    Some(UIAElement::from(found))
}

/// The same pointer click on a checkbox or radio button, at its visible box
/// (`control_part`): for a component library's control whose real input is
/// hidden and whose state changes only on the click of the box around it.
pub(crate) fn pointer_click_control(
    element: &UIAElement,
    deadline: Deadline,
) -> Result<Option<ActionStep>, AdapterError> {
    if element.0.get_framework_id().ok().as_deref() != Some(CHROMIUM_FRAMEWORK) {
        return Ok(None);
    }
    click_part(element, deadline, control_part)
}

/// After `value` was written into a web field of a Chromium browser: its last
/// character typed again as keys (Backspace, then the character) to the
/// browser window, active for that moment only. Some components take only
/// typed input: PrimeNG's calendar parses its text only after a key went down,
/// and forgets a written date when the field loses focus. The field ends with
/// the same text; if it does not, the value is written again. `false` when
/// nothing was typed (not page content, not the focused field).
pub(crate) fn retype_last_char(element: &UIAElement, value: &str) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VkKeyScanW;
    let Some(last) = value.chars().last() else {
        return false;
    };
    if element.0.get_framework_id().ok().as_deref() != Some(CHROMIUM_FRAMEWORK)
        || !element.0.has_keyboard_focus().unwrap_or(false)
        || element.0.is_password().unwrap_or(false)
    {
        return false;
    }
    let Ok(client) = automation_client() else {
        return false;
    };
    if !inside_document(&element.0, &client) {
        return false;
    }
    let Some(top) = top_window(&element.0, &client) else {
        return false;
    };
    let mut units = [0u16; 2];
    let mut keys = crate::actions::posted_keys::key_messages(0x08, Some(0x08), false);
    for unit in last.encode_utf16(&mut units) {
        let scanned = unsafe { VkKeyScanW(*unit) };
        let vk = if scanned == -1 {
            0
        } else {
            (scanned & 0xFF) as u16
        };
        keys.extend(crate::actions::posted_keys::key_messages(
            vk,
            Some(*unit),
            false,
        ));
    }
    let sent = send_keys(top, &keys);
    std::thread::sleep(Duration::from_millis(80));
    if let Ok(pattern) = element.0.get_pattern::<UIValuePattern>()
        && pattern.get_value().is_ok_and(|now| now != value)
    {
        let _ = pattern.set_value(value);
    }
    tracing::debug!(sent, "set-value: last character typed again");
    sent
}

fn click_part(
    element: &UIAElement,
    deadline: Deadline,
    aim: fn(&UIElement, &UIAutomation) -> UIElement,
) -> Result<Option<ActionStep>, AdapterError> {
    let Ok(client) = automation_client() else {
        return Ok(None);
    };
    if !inside_document(&element.0, &client) {
        return Ok(None);
    }
    let Some(top) = top_window(&element.0, &client) else {
        return Ok(None);
    };
    show_behind_if_minimized(top);
    if element.0.is_offscreen().unwrap_or(false)
        && let Ok(pattern) = element.0.get_pattern::<UIScrollItemPattern>()
    {
        let _ = pattern.scroll_into_view();
        std::thread::sleep(Duration::from_millis(150));
    }
    let part = aim(&element.0, &client);
    let Ok(rect) = part.get_bounding_rectangle() else {
        return Ok(None);
    };
    if rect.get_width() <= 0 || rect.get_height() <= 0 {
        return Ok(None);
    }
    let point = POINT {
        x: rect.get_left() + rect.get_width() / 2,
        y: rect.get_top() + rect.get_height() / 2,
    };
    deadline.ensure_remaining()?;
    tracing::debug!(part = %part.get_name().unwrap_or_default(), "pointer click");
    if !press_at(top, point) {
        return Ok(None);
    }
    Ok(Some(
        ActionStep::succeeded(CLICK_LABEL)
            .with_mechanism(StepMechanism::SemanticApi)
            .with_verified(false),
    ))
}

/// A minimized window has nowhere to click and its elements are offscreen (a
/// browser lays its page out at no size): it is restored behind the person's
/// windows, without being activated, so the person's own window stays in
/// front. Also used before a window of any app is read (`adapter.rs`).
pub(crate) fn show_behind_if_minimized(top: HWND) {
    if unsafe { IsIconic(top) } == 0 {
        return;
    }
    // An always-on-top window keeps floating (see `behind::is_topmost`).
    let floating = crate::actions::behind::is_topmost(top);
    let behind = |window: HWND| unsafe {
        if floating {
            return 1;
        }
        SetWindowPos(
            window,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
    behind(top);
    unsafe { ShowWindow(top, SW_SHOWNOACTIVATE) };
    behind(top);
    tracing::debug!("the window was minimized; restored behind the person's windows");
    // The page is laid out again at the window's size.
    std::thread::sleep(Duration::from_millis(400));
}

/// A left click at a point of the screen, as mouse messages to the browser
/// window `top`. `false` when the point is outside its client area or the
/// window did not take them.
fn press_at(top: HWND, point: POINT) -> bool {
    // The screen's hit test answers with whatever window lies on top there (the
    // person's own, while the browser is behind it), so the point is checked
    // against the browser window itself: the point must be inside the window's
    // client area.
    let mut client_point = point;
    if unsafe { ScreenToClient(top, &mut client_point) } == 0 {
        return false;
    }
    let mut area: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetClientRect(top, &mut area) } == 0
        || client_point.x < 0
        || client_point.y < 0
        || client_point.x >= area.right
        || client_point.y >= area.bottom
    {
        tracing::debug!("pointer click: the point is outside the browser window");
        return false;
    }
    let at = (((client_point.y as u16) as isize) << 16) | (client_point.x as u16) as isize;
    // The pointer comes from the window's corner first: a click at the very
    // point of the previous one would otherwise count as its second click
    // (a double click) and not toggle what the first one opened.
    send(top, WM_MOUSEMOVE, 0, 0)
        && send(top, WM_MOUSEMOVE, 0, at)
        && send(top, WM_LBUTTONDOWN, MK_LBUTTON as usize, at)
        && {
            std::thread::sleep(Duration::from_millis(40));
            send(top, WM_LBUTTONUP, 0, at)
        }
}

fn omnibox(top: HWND, client: &UIAutomation) -> Option<UIElement> {
    let root = client
        .element_from_handle(Handle::from(top as isize))
        .ok()?;
    let condition = client
        .create_property_condition(UIProperty::ClassName, Variant::from(OMNIBOX_CLASS), None)
        .ok()?;
    root.find_first(TreeScope::Descendants, &condition).ok()
}

/// The document of the tab a Chromium window shows: the one shown; else,
/// in a window behind others (whose documents may all say they are off the
/// screen), the one whose title begins the window's title.
pub(crate) fn active_document(root: &UIElement, client: &UIAutomation) -> Option<UIElement> {
    let condition = client
        .create_property_condition(
            UIProperty::ControlType,
            Variant::from(DOCUMENT_CONTROL_TYPE),
            None,
        )
        .ok()?;
    let documents: Vec<UIElement> = root
        .find_all(TreeScope::Descendants, &condition)
        .ok()?
        .into_iter()
        .filter(|document| document.get_framework_id().ok().as_deref() == Some(CHROMIUM_FRAMEWORK))
        .filter(|document| {
            document
                .get_pattern::<UIValuePattern>()
                .and_then(|pattern| pattern.get_value())
                .is_ok_and(|url| url.contains("://"))
        })
        .collect();
    if let Some(shown) = documents
        .iter()
        .find(|document| document.is_offscreen().ok() == Some(false))
    {
        return Some(shown.clone());
    }
    let title = root.get_name().unwrap_or_default();
    let mut titled = documents.iter().filter(|document| {
        let name = document.get_name().unwrap_or_default();
        !name.is_empty() && title.starts_with(name.as_str())
    });
    match (titled.next(), titled.next()) {
        (Some(one), None) => Some(one.clone()),
        _ => (documents.len() == 1).then(|| documents[0].clone()),
    }
}

/// The address of the page the window shows (its active tab's document).
fn page_address(top: HWND, client: &UIAutomation) -> Option<String> {
    let root = client
        .element_from_handle(Handle::from(top as isize))
        .ok()?;
    active_document(&root, client)?
        .get_pattern::<UIValuePattern>()
        .and_then(|pattern| pattern.get_value())
        .ok()
}

/// Two addresses compared as the browser shows them: case, a trailing slash
/// and %-escapes (a space is %20 in the tab's address) aside.
fn same_address(a: &str, b: &str) -> bool {
    let clean = |url: &str| {
        let bytes = url.trim().trim_end_matches('/').as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let hex = |at: usize| (bytes[at] as char).to_digit(16);
            if bytes[i] == b'%'
                && i + 2 < bytes.len()
                && let (Some(high), Some(low)) = (hex(i + 1), hex(i + 2))
            {
                decoded.push((high * 16 + low) as u8);
                i += 3;
            } else {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&decoded).to_lowercase()
    };
    clean(a) == clean(b)
}

/// Opens `url` in the tab a Chromium window shows, through its address bar
/// and Enter, keeping the tab's session.
pub(crate) fn navigate(
    window: &WindowInfo,
    url: &str,
    deadline: Deadline,
) -> Result<serde_json::Value, AdapterError> {
    let Some(top) = is_chromium_top(window) else {
        return Err(AdapterError::new(
            ErrorCode::ActionNotSupported,
            "navigate works on a Chromium browser window (Chrome, Edge)",
        ));
    };
    show_behind_if_minimized(top);
    // The address bar is clicked and typed into as window messages, the browser
    // told it is active: a guard gives the person their window back (see
    // `focus_guard::during_input`).
    let input = crate::actions::focus_guard::during_input(
        Some(window.pid.get()),
        crate::actions::focus_guard::PersonFront::now(),
        800,
    );
    let client = automation_client()?;
    let Some(bar) = omnibox(top, &client) else {
        return Err(AdapterError::new(
            ErrorCode::ElementNotFound,
            "The browser window has no address bar (an app window?)",
        ));
    };
    let from = page_address(top, &client);
    // Enter goes to what has the focus inside the browser, which after a page
    // loads is the page: a pointer click on the address bar gives it the focus
    // there (Windows' foreground does not move), as a person clicks it first.
    if let Ok(rect) = bar.get_bounding_rectangle()
        && rect.get_width() > 0
        && rect.get_height() > 0
    {
        let point = POINT {
            x: rect.get_left() + rect.get_width() / 2,
            y: rect.get_top() + rect.get_height() / 2,
        };
        let _ = press_at(top, point);
        std::thread::sleep(Duration::from_millis(100));
    }
    let pattern = bar.get_pattern::<UIValuePattern>().map_err(|error| {
        AdapterError::new(
            ErrorCode::ActionFailed,
            format!("The address bar takes no value: {error}"),
        )
    })?;
    pattern.set_value(url).map_err(|error| {
        AdapterError::new(
            ErrorCode::ActionFailed,
            format!("The address bar refused the address: {error}"),
        )
    })?;
    // Delete first: the address bar completes a typed address inline with one
    // it saw before (/dashboard becomes /dashboard/almacen/...), and Enter
    // would open that one.
    let mut keys = crate::actions::posted_keys::key_messages(0x2E, None, true);
    keys.extend(crate::actions::posted_keys::key_messages(
        0x0D,
        Some(0x0D),
        false,
    ));
    if !send_keys(top, &keys) {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The browser window did not take Enter",
        ));
    }
    // The address and Enter are in: while the page loads, the browser may ask
    // for the foreground, and it is given back to the person at once.
    let _guard = input.input_sent();
    let until = Instant::now() + deadline.remaining().min(NAVIGATE_TIMEOUT);
    let mut now = None;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(250));
        now = page_address(top, &client);
        if now.as_deref().is_some_and(|now| same_address(now, url)) {
            std::thread::sleep(Duration::from_millis(500));
            return Ok(serde_json::json!({
                "window": window.id, "from": from, "url": now, "reached": true, "in_app": false,
            }));
        }
    }
    Ok(serde_json::json!({
        "window": window.id, "from": from, "url": now, "reached": false, "in_app": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::same_address;

    #[test]
    fn addresses_compare_as_the_browser_shows_them() {
        assert!(same_address(
            "file:///C:/a%20b/x.html",
            "file:///C:/a b/x.html"
        ));
        assert!(same_address(
            "http://127.0.0.1:4200/usuarios/",
            "http://127.0.0.1:4200/usuarios"
        ));
        assert!(!same_address(
            "http://127.0.0.1:4200/roles",
            "http://127.0.0.1:4200/usuarios"
        ));
    }
}
