// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! A key pressed in one window as posted messages, for `press --window-id`.
//!
//! A real key press goes to whatever window has the person's keyboard, so this
//! build refused `press`. Many tasks still need one key in one window: Enter
//! to run a search box that has no button, Escape to close a popup, Tab or an
//! arrow to move inside a list. The window that holds the keyboard focus of
//! its own thread (the field the agent last wrote into, even while the window
//! is behind others) handles the messages a real key sends it: a key down, the
//! character it types, a key up. They are posted to that window only: the
//! person's keyboard, focus and foreground are never touched.
//!
//! A browser window's page gets the key through DevTools when the browser
//! has an endpoint (`launch --cdp`). Otherwise a Chromium window, which drops
//! keyboard messages while it is not active, is told it is active for that
//! moment (see `chromium_input`): the person's own browser and profile work
//! as they are, and Windows' foreground never moves.
//!
//! Letters and other characters go the same way (a key down, the character, a
//! key up, the virtual key read from the keyboard layout). A shortcut with
//! Ctrl, Alt or Shift holds its modifiers down in the target thread's own key
//! state while its messages are handled: this thread attaches to the target's
//! input (`AttachThreadInput`) and sets the state with `SetKeyboardState`, as
//! `GetKeyState` is what an app reads to know a modifier is down. Right after,
//! only the modifiers held here are put back: any other key that changed in
//! the meantime (the person's own, when the target is the window they use) is
//! left as it is now. Win shortcuts
//! belong to the system and are not pressed this way. When a key cannot be
//! posted, the original code runs (it needs the window in front).

use agent_desktop_core::Modifier;
use agent_desktop_core::{
    Action, ActionResult, ActionStep, AdapterError, Deadline, ErrorCode, KeyCombo, StepMechanism,
    WindowInfo,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{GetUpdateRect, InvalidateRect};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardState, MAPVK_VK_TO_VSC, MapVirtualKeyW, SetKeyboardState, VK_BACK, VK_CONTROL,
    VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_LCONTROL, VK_LEFT, VK_LMENU,
    VK_LSHIFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
    VkKeyScanW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GUITHREADINFO, GetAncestor, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow,
    PostMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_NULL,
    WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const LABEL: &str = "PostMessage.Key";
/// How much of a document's text is read to see that typed text arrived.
const DOCUMENT_TEXT_READ: i32 = 200_000;

/// After Space went to a page's focused radio button or checkbox: verified
/// when its state changed; else the control is clicked at its visible box (a
/// component library's control, like PrimeNG's, whose hidden input ignores
/// the key), and when even that leaves it unchanged the press fails, naming
/// the control, so a script does not go on as if it had been chosen.
fn space_on_toggle(
    action: &Action,
    element: &crate::tree::element::UIAElement,
    before: crate::actions::background::ControlState,
    key_step: ActionStep,
    deadline: Deadline,
) -> Result<ActionResult, AdapterError> {
    use std::time::Duration;
    if before.changed(element, Duration::from_millis(600)) {
        return Ok(
            ActionResult::from_execution(action, vec![key_step.with_verified(true)])
                .with_verified_delivery(),
        );
    }
    if let Some(click) = crate::actions::chromium_input::pointer_click_control(element, deadline)?
        && before.changed(element, Duration::from_millis(1200))
    {
        return Ok(
            ActionResult::from_execution(action, vec![key_step, click.with_verified(true)])
                .with_verified_delivery(),
        );
    }
    let name = element.0.get_name().unwrap_or_default();
    Err(AdapterError::new(
        ErrorCode::ActionFailed,
        format!("Space reached the page, but the focused option '{name}' did not change"),
    )
    .with_suggestion("Click the option instead (click its ref), then read it to confirm")
    .with_disposition(agent_desktop_core::DeliverySemantics::delivered_unverified()))
}

/// The virtual key for a key name with no modifier, and the character it
/// types (Enter types a carriage return, Tab a tab...), if any.
fn key(name: &str) -> Option<(u16, Option<u16>)> {
    let key = match name {
        "return" | "enter" => (VK_RETURN, Some(0x0D)),
        "escape" | "esc" => (VK_ESCAPE, Some(0x1B)),
        "tab" => (VK_TAB, Some(0x09)),
        "space" => (VK_SPACE, Some(0x20)),
        "delete" | "backspace" => (VK_BACK, Some(0x08)),
        "forwarddelete" => (VK_DELETE, None),
        "home" => (VK_HOME, None),
        "end" => (VK_END, None),
        "pageup" => (VK_PRIOR, None),
        "pagedown" => (VK_NEXT, None),
        "left" => (VK_LEFT, None),
        "right" => (VK_RIGHT, None),
        "up" => (VK_UP, None),
        "down" => (VK_DOWN, None),
        _ => {
            if let Some(number) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
                return Some((
                    (1..=12).contains(&number).then_some(VK_F1 + number - 1)?,
                    None,
                ));
            }
            return character_key(name).map(|(vk, character, _)| (vk, Some(character)));
        }
    };
    Some(key)
}

/// One printable character: its virtual key in the keyboard layout, the
/// character, and whether the layout needs Shift for it ("A", "?").
fn character_key(name: &str) -> Option<(u16, u16, bool)> {
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() || c.is_control() {
        return None;
    }
    let mut units = [0u16; 2];
    let encoded = c.encode_utf16(&mut units);
    if encoded.len() != 1 {
        return None;
    }
    let unit = encoded[0];
    let scanned = unsafe { VkKeyScanW(unit) };
    if scanned == -1 {
        return None;
    }
    let vk = (scanned & 0xFF) as u16;
    let shift = (scanned >> 8) & 1 == 1;
    Some((vk, unit, shift))
}

/// The modifiers of a combo that a target thread's key state can hold (Win cannot).
#[derive(Clone, Copy, Default)]
struct Held {
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Held {
    fn any(self) -> bool {
        self.ctrl || self.alt || self.shift
    }

    /// How many modifier keys are held (their releases end `combo_messages`).
    fn count(self) -> usize {
        usize::from(self.ctrl) + usize::from(self.alt) + usize::from(self.shift)
    }
}

fn held(combo: &KeyCombo) -> Option<Held> {
    let mut h = Held::default();
    for m in &combo.modifiers {
        match m {
            Modifier::Ctrl => h.ctrl = true,
            Modifier::Alt => h.alt = true,
            Modifier::Shift => h.shift = true,
            Modifier::Meta => return None,
        }
    }
    Some(h)
}

/// The messages of a combo: modifiers down, the key (with the character it
/// types: a control character with Ctrl, a system character with Alt), the key
/// up, modifiers up.
fn combo_messages(vk: u16, character: Option<u16>, h: Held) -> Vec<(u32, usize, isize)> {
    let alt = h.alt && !h.ctrl;
    let (down_msg, up_msg) = if alt {
        (WM_SYSKEYDOWN, WM_SYSKEYUP)
    } else {
        (WM_KEYDOWN, WM_KEYUP)
    };
    let context: isize = if alt { 1 << 29 } else { 0 };
    let lparams = |vk: u16, extended: bool| {
        let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as isize;
        let down = 1 | (scan << 16) | if extended { 1 << 24 } else { 0 } | context;
        (down, down | (1 << 30) | (1 << 31))
    };
    let mods: Vec<u16> = [(h.ctrl, VK_CONTROL), (h.alt, VK_MENU), (h.shift, VK_SHIFT)]
        .into_iter()
        .filter_map(|(on, vk)| on.then_some(vk))
        .collect();
    let mut out = Vec::new();
    for m in &mods {
        out.push((down_msg, *m as usize, lparams(*m, false).0));
    }
    let (down, up) = lparams(vk, is_extended(vk));
    out.push((down_msg, vk as usize, down));
    let lower = |c: u16| {
        if (0x41..=0x5A).contains(&c) {
            c | 0x20
        } else {
            c
        }
    };
    let typed = match character {
        // Ctrl with a letter types its control character (Ctrl+B is 0x02).
        Some(c) if h.ctrl && (0x61..=0x7A).contains(&lower(c)) => Some(lower(c) - 0x60),
        Some(_) if h.ctrl => None,
        Some(c) if h.shift && (0x61..=0x7A).contains(&c) => Some(c - 0x20),
        other => other,
    };
    if let Some(c) = typed {
        out.push((if alt { WM_SYSCHAR } else { WM_CHAR }, c as usize, down));
    }
    out.push((up_msg, vk as usize, up));
    for m in mods.iter().rev() {
        out.push((up_msg, *m as usize, lparams(*m, false).1));
    }
    out
}

/// Holds `h`'s modifiers down in `thread`'s key state while `deliver` runs and
/// the thread handles the messages, then puts those modifiers back as they
/// were (see `put_back`).
/// Whether `thread`'s input state can be shared with ours, so a modifier put in
/// it is seen by the app (an app in a sandbox refuses it).
fn can_share_input(thread: u32) -> bool {
    let me = unsafe { GetCurrentThreadId() };
    if thread == 0 || thread == me {
        return thread != 0;
    }
    let attached = unsafe { AttachThreadInput(me, thread, 1) } != 0;
    if attached {
        unsafe { AttachThreadInput(me, thread, 0) };
    }
    attached
}

fn with_modifiers<T>(thread: u32, h: Held, deliver: impl FnOnce() -> T) -> T {
    if !h.any() || thread == 0 {
        return deliver();
    }
    unsafe {
        let me = GetCurrentThreadId();
        let attached = me != thread && AttachThreadInput(me, thread, 1) != 0;
        let mut saved = [0u8; 256];
        let have = GetKeyboardState(saved.as_mut_ptr()) != 0;
        let mut state = saved;
        let mut changed: Vec<u16> = Vec::new();
        for (on, keys) in [
            (h.ctrl, [VK_CONTROL, VK_LCONTROL]),
            (h.alt, [VK_MENU, VK_LMENU]),
            (h.shift, [VK_SHIFT, VK_LSHIFT]),
        ] {
            if on {
                for k in keys {
                    state[k as usize] |= 0x80;
                    changed.push(k);
                }
            }
        }
        if have {
            SetKeyboardState(state.as_ptr());
        }
        let out = deliver();
        // The target reads the state when it handles each message: give it time to.
        std::thread::sleep(std::time::Duration::from_millis(250));
        if have {
            put_back(&saved, &changed);
        }
        if attached {
            AttachThreadInput(me, thread, 0);
        }
        out
    }
}

/// Puts `keys` back down or up as they were in `saved`, and leaves every other
/// key of the (attached) thread's state as it is now: the whole saved state
/// written back would undo a key that changed in the meantime, the person's
/// own when the target is the window they use.
unsafe fn put_back(saved: &[u8; 256], keys: &[u16]) {
    let mut now = [0u8; 256];
    if unsafe { GetKeyboardState(now.as_mut_ptr()) } == 0 {
        return;
    }
    for &k in keys {
        let k = k as usize;
        now[k] = (now[k] & !0x80) | (saved[k] & 0x80);
    }
    unsafe { SetKeyboardState(now.as_ptr()) };
}

/// Waits until the app has handled the keys just posted to it: its message
/// queue is flushed and it paints a frame now. An app that draws with a frame
/// loop (egui, winit) reads its input only when it paints, and a window in the
/// background may not paint for a while: keys sent one after the other were
/// then handled in one frame and one was lost (F2 right after the Enter that
/// closed GridCraft's cell editor). Nothing is activated.
/// Waits, up to 3 s, until the app that owns `window` waits for input with
/// none pending (`WaitForInputIdle`): an app just opened takes no keys yet (a
/// ctrl+o right after GridCraft opened was lost).
fn ready(window: HWND) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE, WaitForInputIdle,
    };
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == 0 {
            return;
        }
        let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid);
        if !process.is_null() {
            WaitForInputIdle(process, 3_000);
            CloseHandle(process);
        }
    }
}

fn handled(top: HWND, target: HWND) {
    let mut answer = 0usize;
    unsafe {
        // The keys are handled (sent messages wait for the posted ones before
        // them only once the app reads its queue: at most 1 s).
        SendMessageTimeoutW(target, WM_NULL, 0, 0, SMTO_ABORTIFHUNG, 1_000, &mut answer);
        // A frame is asked for, and waited for without blocking: the window's
        // update region empties once it has painted (at most 500 ms).
        InvalidateRect(top, std::ptr::null(), 0);
        let end = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while GetUpdateRect(top, std::ptr::null_mut(), 0) != 0 && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Whether `top` is not the window the person is using: a window they use
/// has the focus already, and is never told anything here.
/// The class of the hidden window winit makes on every UI thread of its own.
pub(crate) const WINIT_EVENT_TARGET: &str = "Winit Thread Event Target";

/// Whether `top` is a window of a winit app (Rust: egui, WordCraft,
/// GridCraft), known by the event target window winit keeps on its thread.
/// winit's message loop makes no character from a posted key: it reads the
/// character message that follows a key down as that key's text, so a key
/// posted alone moves the caret or runs a shortcut but types nothing, and a
/// grid that starts editing on typed text never starts. It gets the key with
/// its character, as a real key press brings both; winit joins them into one
/// event, so the character is not typed twice.
pub(crate) fn is_winit(top: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetClassNameW};
    unsafe extern "system" fn check(window: HWND, found: isize) -> i32 {
        let mut name = [0u16; 64];
        let len = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
        if len > 0 && String::from_utf16_lossy(&name[..len as usize]) == WINIT_EVENT_TARGET {
            unsafe { *(found as *mut bool) = true };
            return 0;
        }
        1
    }
    let thread = unsafe { GetWindowThreadProcessId(top, std::ptr::null_mut()) };
    if thread == 0 {
        return false;
    }
    let mut found = false;
    unsafe { EnumThreadWindows(thread, Some(check), &mut found as *mut bool as isize) };
    found
}

fn is_behind(top: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let front = GetForegroundWindow();
        front.is_null() || GetAncestor(front, GA_ROOT) != top
    }
}

fn tell(window: HWND, message: u32, wparam: usize, lparam: isize) {
    let mut answer = 0usize;
    unsafe {
        SendMessageTimeoutW(
            window,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            1_000,
            &mut answer,
        );
    }
}

/// Tells a window behind it is active and focused, with the messages Windows
/// sends a window that comes to the front, in their order: active
/// (`WM_NCACTIVATE`, its title bar left unpainted with -1, so nothing changes
/// on the screen), activated, focused. Some frameworks (winit) take keys only
/// while the window is both active and focused; a classic or WinForms window
/// gives the focus back to the field that had it. Windows' own foreground and
/// focus never move. Returns whether it told it anything.
/// A wake that covers a whole command (focus a field, type, read, try again)
/// leaves the inner ones nothing to do: only the outermost one rests the window.
fn wake(top: HWND, target: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WA_ACTIVE, WM_ACTIVATE, WM_NCACTIVATE, WM_SETFOCUS,
    };
    if AWAKE.with(std::cell::Cell::get) || !is_behind(top) {
        return false;
    }
    tell(top, WM_NCACTIVATE, 1, -1);
    tell(top, WM_ACTIVATE, WA_ACTIVE as usize, 0);
    tell(target, WM_SETFOCUS, 0, 0);
    AWAKE.with(|awake| awake.set(true));
    true
}

/// Tells a window this thread keeps awake that it is active and focused once
/// more: an accessibility focus request may have brought it forward for a
/// moment, and the guard that gives the person their window back deactivated
/// it again (a WinUI field then dropped the keys).
fn wake_again(top: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WA_ACTIVE, WM_ACTIVATE, WM_NCACTIVATE, WM_SETFOCUS,
    };
    if AWAKE.with(std::cell::Cell::get) && is_behind(top) {
        tell(top, WM_NCACTIVATE, 1, -1);
        tell(top, WM_ACTIVATE, WA_ACTIVE as usize, 0);
        tell(top, WM_SETFOCUS, 0, 0);
    }
}

thread_local! {
    /// Whether a window is being kept awake by this thread now.
    static AWAKE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The control typed text goes into, while `type_text` aims at one by its
    /// own window (see `key_target`).
    static INTO: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Where typed keys go in `top`: the control `type_text` aims at, else the
/// window of `top` that holds its thread's keyboard focus.
fn key_target(top: HWND) -> HWND {
    let into = INTO.with(std::cell::Cell::get);
    if into != 0 {
        into as HWND
    } else {
        focused_in(top)
    }
}

/// Runs `deliver` with typed keys going into `into` (see `key_target`).
fn typing_into<T>(into: Option<HWND>, deliver: impl FnOnce() -> T) -> T {
    let Some(into) = into else {
        return deliver();
    };
    INTO.with(|cell| cell.set(into as isize));
    let out = deliver();
    INTO.with(|cell| cell.set(0));
    out
}

/// Tells a window [`wake`] woke that it is inactive again, as it was. Its
/// focus stays where it is within its own thread, as a window keeps it when
/// another one comes to the front: told it lost it, an app may forget its
/// focused field (WPF does), and the next key would find none.
fn rest(top: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WA_INACTIVE, WM_ACTIVATE, WM_NCACTIVATE};
    tell(top, WM_ACTIVATE, WA_INACTIVE as usize, 0);
    tell(top, WM_NCACTIVATE, 0, -1);
    AWAKE.with(|awake| awake.set(false));
}

/// Runs `deliver` while a window behind believes it is active and focused
/// ([`wake`]), then makes it inactive again ([`rest`]).
fn with_focus<T>(top: HWND, target: HWND, deliver: impl FnOnce() -> T) -> T {
    let woke = wake(top, target);
    let out = deliver();
    if woke {
        rest(top);
    }
    out
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        VK_DELETE | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN
    )
}

/// The window of `top`'s thread that holds the keyboard focus, when it is
/// `top` itself or inside it; else `top`.
fn focused_in(top: HWND) -> HWND {
    unsafe {
        let thread = GetWindowThreadProcessId(top, std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if thread != 0
            && GetGUIThreadInfo(thread, &mut info) != 0
            && !info.hwndFocus.is_null()
            && GetAncestor(info.hwndFocus, GA_ROOT) == top
        {
            return info.hwndFocus;
        }
    }
    // A frame of one process that hosts another's window (a packaged UWP app in
    // ApplicationFrameHost): the keys are the hosted app's, read on its thread
    // (with Shift in its key state), so they go to its window.
    hosted_window(top).unwrap_or(top)
}

/// The first visible child window of `top` that another process owns.
fn hosted_window(top: HWND) -> Option<HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumChildWindows, IsWindowVisible};
    unsafe extern "system" fn find(child: HWND, data: isize) -> i32 {
        let found = unsafe { &mut *(data as *mut (u32, HWND)) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(child, &mut pid) };
        if pid != 0 && pid != found.0 && unsafe { IsWindowVisible(child) } != 0 {
            found.1 = child;
            return 0;
        }
        1
    }
    let mut own = 0u32;
    unsafe { GetWindowThreadProcessId(top, &mut own) };
    let mut found: (u32, HWND) = (own, std::ptr::null_mut());
    unsafe { EnumChildWindows(top, Some(find), &mut found as *mut _ as isize) };
    if found.1.is_null() {
        return None;
    }
    // Its thread's focus, when it is inside the hosted window.
    unsafe {
        let thread = GetWindowThreadProcessId(found.1, std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(thread, &mut info) != 0
            && !info.hwndFocus.is_null()
            && GetAncestor(info.hwndFocus, GA_ROOT) == top
        {
            return Some(info.hwndFocus);
        }
    }
    Some(found.1)
}

fn window_handle(window: &WindowInfo) -> Option<HWND> {
    let raw: usize = window.id.strip_prefix("w-")?.parse().ok()?;
    let hwnd = raw as HWND;
    (raw != 0 && unsafe { IsWindow(hwnd) } != 0).then_some(hwnd)
}

/// Presses `combo` in `window` as posted messages.
pub(crate) fn press(
    window: &WindowInfo,
    combo: &KeyCombo,
    deadline: Deadline,
) -> Result<ActionResult, AdapterError> {
    // A browser told it is active for the keys may take the foreground: a
    // guard gives the person their window back (see
    // `focus_guard::during_input`).
    let input = crate::actions::chromium_input::is_chromium_top(window).map(|top| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(top, &mut pid) };
        crate::actions::focus_guard::during_input(
            Some(pid),
            crate::actions::focus_guard::PersonFront::now(),
            1200,
        )
    });
    let result = press_posted(window, combo, deadline);
    if let Some(input) = input
        && let Some(guard) = input.input_sent()
    {
        guard.finish();
    }
    result
}

fn press_posted(
    window: &WindowInfo,
    combo: &KeyCombo,
    deadline: Deadline,
) -> Result<ActionResult, AdapterError> {
    let Some(mut mods) = held(combo) else {
        return Err(AdapterError::new(
            ErrorCode::ActionNotSupported,
            "A Win shortcut belongs to the system and is not pressed in the background",
        )
        .with_suggestion(
            "Run its command instead: menu --window-id W --shortcut <keys>, or click its button",
        ));
    };
    if let Some((_, _, true)) = character_key(&combo.key) {
        mods.shift = true; // "A" or "?" is typed with Shift
    }
    let Some((vk, character)) = key(&combo.key) else {
        return Err(AdapterError::new(
            ErrorCode::ActionNotSupported,
            format!("'{}' cannot be pressed in the background", combo.key),
        )
        .with_suggestion(
            "Press a letter, digit or symbol, or one of return, escape, tab, space, backspace, forwarddelete, home, end, pageup, pagedown, the arrows or f1-f12 (with ctrl, alt or shift); write text with set-value or type",
        ));
    };
    // A page with DevTools is agent-browser's (`agent-record browser press`);
    // here the window gets the key as messages.
    let _ = deadline;
    let step = || {
        ActionStep::succeeded(LABEL)
            .with_mechanism(StepMechanism::SemanticApi)
            .with_verified(false)
    };
    // A Chromium window takes keys only while it believes it is active.
    if let Some(top) = crate::actions::chromium_input::is_chromium_top(window) {
        // Space on a focused radio button or checkbox must change it; read it
        // first, so a key that did nothing is not reported as done.
        let toggle = (vk == VK_SPACE)
            .then(|| crate::actions::chromium_input::page_focus(top))
            .flatten()
            .and_then(|element| {
                crate::actions::background::ControlState::of(&element).map(|state| (element, state))
            });
        let keys = combo_messages(vk, character, mods);
        let thread = unsafe { GetWindowThreadProcessId(top, std::ptr::null_mut()) };
        if !with_modifiers(thread, mods, || {
            crate::actions::chromium_input::send_keys(top, &keys)
        }) {
            return Err(AdapterError::new(
                ErrorCode::ActionFailed,
                "The browser window did not take the key",
            ));
        }
        let action = Action::PressKey(combo.clone());
        let Some((element, before)) = toggle else {
            return Ok(ActionResult::from_execution(&action, vec![step()]));
        };
        return space_on_toggle(&action, &element, before, step(), deadline);
    }
    let Some(top) = window_handle(window) else {
        return Err(AdapterError::new(
            ErrorCode::WindowNotFound,
            format!("Window {} is no longer open", window.id),
        ));
    };
    ready(top);
    // A window that lost the activation is told it is active first: it gives the
    // focus back to its field, and that field gets the key.
    let woke = wake(top, top);
    if woke {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let target = focused_in(top);
    let thread = unsafe { GetWindowThreadProcessId(target, std::ptr::null_mut()) };
    // The modifiers' releases go after the key state is back to the real one: an
    // app that reads the state with each key (winit: WordCraft) otherwise kept
    // Ctrl held after ctrl+b and took the next letters as shortcuts.
    // The key goes as a real one does: down and up, with no character of its
    // own. The app's message loop makes the character from the key
    // (`TranslateMessage`: winit, WPF, WinForms and classic apps all do), so a
    // character posted too was typed twice.
    // A symbol typed with Shift ("=" is Shift+0 on a Spanish keyboard) whose app's
    // key state cannot take Shift (an app in a sandbox, as a packaged UWP app,
    // refuses to share its input state) would arrive as the unshifted key ("0"):
    // it goes as its character instead.
    let implicit_shift = character_key(&combo.key).is_some_and(|(_, _, shift)| shift);
    if let Some(unit) = character
        && implicit_shift
        && !mods.ctrl
        && !mods.alt
        && !can_share_input(thread)
    {
        let sent = unsafe { PostMessageW(target, WM_CHAR, unit as usize, 1) != 0 };
        handled(top, target);
        if woke {
            rest(top);
        }
        if !sent {
            return Err(AdapterError::new(
                ErrorCode::ActionFailed,
                "The window did not accept the key (WM_CHAR)",
            ));
        }
        return Ok(ActionResult::from_execution(
            &Action::PressKey(combo.clone()),
            vec![step()],
        ));
    }
    // A winit window makes no character from a posted key: it takes the
    // character that follows the key as the key's text (see `is_winit`).
    let with_character = (is_winit(top) && !mods.ctrl && !mods.alt)
        .then_some(character)
        .flatten();
    let mut messages = combo_messages(vk, with_character, mods);
    let releases = messages.split_off(messages.len() - mods.count());
    let post = |list: &[(u32, usize, isize)]| {
        list.iter().all(|(message, wparam, lparam)| unsafe {
            PostMessageW(target, *message, *wparam, *lparam) != 0
        })
    };
    let posted = with_modifiers(thread, mods, || {
        let sent = post(&messages);
        if sent {
            handled(top, target);
        }
        sent
    }) && (releases.is_empty() || {
        let sent = post(&releases);
        handled(top, target);
        sent
    });
    if woke {
        rest(top);
    }
    if !posted {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The window did not accept the key (WM_KEYDOWN)",
        ));
    }
    // The page's answer to a key (a search, a closed popup) is the caller's to
    // check: read it with snapshot, get or wait.
    Ok(ActionResult::from_execution(
        &Action::PressKey(combo.clone()),
        vec![step()],
    ))
}

/// Types `text` into the window that holds `element`, as the messages real
/// keys send (Enter for a new line, Tab, every other character with its key),
/// for `type` in the background: a document or a canvas that has no value to
/// write (WordCraft's page, a slide) takes typed text only. They go to the
/// window that holds the keyboard focus inside the element's top window (a
/// Chromium window is told it is active for that moment). `None` when the
/// window cannot be found: the original code runs.
/// `None` when the window cannot be found (the original code runs), an error
/// when the field showed none of the text.
pub(crate) fn type_text(
    element: &crate::tree::element::UIAElement,
    text: &str,
) -> Option<Result<ActionResult, AdapterError>> {
    let client = crate::tree::automation::automation_client().ok()?;
    let top = crate::actions::chromium_input::top_window(&element.0, &client)?;
    let mut messages = Vec::new();
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => messages.extend(key_messages(VK_RETURN, Some(0x0D), false)),
            '\t' => messages.extend(key_messages(VK_TAB, Some(0x09), false)),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units).iter() {
                    let scanned = unsafe { VkKeyScanW(*unit) };
                    let vk = if scanned == -1 {
                        0
                    } else {
                        (scanned & 0xFF) as u16
                    };
                    messages.extend(key_messages(vk, Some(*unit), false));
                }
            }
        }
    }
    let delivered = if crate::actions::chromium_input::is_chromium_hwnd(top) {
        crate::actions::chromium_input::send_keys(top, &messages)
    } else {
        // The field's text, read back to measure how this app takes characters.
        let read = || {
            element
                .0
                .get_pattern::<uiautomation::patterns::UIValuePattern>()
                .ok()
                .and_then(|pattern| pattern.get_value().ok())
        };
        // A field without the keyboard focus gets it first, through accessibility,
        // with its window told it is active (an app may ignore a focus request
        // while it believes it is inactive): the keys go where its caret is.
        let woke = wake(top, top);
        if !element.0.has_keyboard_focus().unwrap_or(false) {
            let _ = element.0.set_focus();
            std::thread::sleep(std::time::Duration::from_millis(150));
            wake_again(top);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // A control with a window of its own (a classic or WinForms field) that still
        // does not hold its window's focus (a dialog that keeps it on another
        // control, as Character Map on its grid) takes the keys posted to it.
        let own = element
            .0
            .get_native_window_handle()
            .ok()
            .map(|handle| {
                let hwnd: windows::Win32::Foundation::HWND = handle.into();
                hwnd.0 as HWND
            })
            .filter(|own| {
                !own.is_null()
                    && unsafe { GetAncestor(*own, GA_ROOT) } == top
                    && focused_in(top) != *own
            });
        let typed = typing_into(own, || type_keys(top, text, Some(&read)));
        if woke {
            rest(top);
        }
        typed
    };
    if !delivered {
        return Some(Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The text did not reach the field: the app took none of its keys",
        )
        .with_suggestion(
            "Click the field first, or type with the window in front (--headed)",
        )));
    }
    // Typed text is the caller's to read back (the document's text, a snapshot).
    std::thread::sleep(std::time::Duration::from_millis(150));
    Some(Ok(ActionResult::from_execution(
        &Action::TypeText(text.to_owned()),
        vec![
            ActionStep::succeeded("PostMessage.Type")
                .with_mechanism(StepMechanism::SemanticApi)
                .with_verified(false),
        ],
    )))
}

/// Types `text` into a desktop app's window one key at a time, as a person's
/// keys arrive: a capital or a shifted symbol with Shift down in the app's key
/// state (an app that builds the text from the key and that state, as winit
/// does, takes neither the character alone nor a key without Shift), and a frame
/// painted after each key (an app that reads keys only when it paints lost some
/// when many came in one frame). The app's input is attached to ours once for
/// the whole text: detaching it after each capital left WordCraft without the
/// keyboard focus for the next keys. A character no key types (AltGr, an emoji)
/// goes as the character alone.
///
/// How a character goes depends on the app, which is measured, not guessed:
/// most apps make the character from the key in their message loop
/// (`TranslateMessage`), so a character posted with the key is typed twice;
/// others (an editor whose input method does not translate posted keys) take
/// the character only when it comes with the key. With a way to read the
/// field (`read`), the first character goes with both: when it shows twice,
/// one is erased with Backspace and the rest go as keys only; when once, the
/// rest go with their characters; when not at all, nothing more is typed and
/// it returns false. With no way to read it, keys go as real ones
/// do, down and up only.
fn type_keys(top: HWND, text: &str, read: Option<&dyn Fn() -> Option<String>>) -> bool {
    ready(top);
    // With nothing to read the text back from, a winit window gets each key
    // with its character (see `is_winit`), any other window the key alone.
    let unmeasured = is_winit(top);
    with_focus(top, key_target(top), || {
        let mut chars = text.chars();
        let (Some(first), Some(read)) = (chars.next(), read) else {
            return type_keys_now(top, text, unmeasured);
        };
        let Some(before) = read() else {
            return type_keys_now(top, text, unmeasured);
        };
        let rest: String = chars.collect();
        if !type_keys_now(top, &first.to_string(), true) {
            return false;
        }
        // A busy app (just opened, still loading) handles the key late: the reading
        // starts once it has taken its queue, or a late character would read as
        // none arrived and the text would be typed again.
        ready(top);
        let count = |text: &str| text.chars().count();
        let grown = |after: &Option<String>| {
            after
                .as_deref()
                .map_or(0, |after| count(after) as i64 - count(&before) as i64)
        };
        // Read until the field shows the character (up to 1 s), then once more
        // in case the app makes a second one a moment later.
        let end = std::time::Instant::now() + std::time::Duration::from_millis(1_000);
        let mut after = read();
        while grown(&after) == 0 && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(50));
            after = read();
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        match grown(&read()) {
            // Nothing arrived: nothing more is typed, and the caller is told.
            0 => false,
            1 => type_keys_now(top, &rest, true),
            _ => erase_one(top) && type_keys_now(top, &rest, false),
        }
    })
}

/// A click inside `element` (a document), as window messages: a move to the
/// point first (a frame that never saw the pointer there drops the press), then
/// the press and the release, near its lower right corner. The cursor does not
/// move and the window is not activated. Returns whether it was posted.
fn click_into(top: HWND, element: &uiautomation::UIElement) -> bool {
    let Ok(bounds) = element.get_bounding_rectangle() else {
        return false;
    };
    let (left, top_edge, right, bottom) = (
        bounds.get_left(),
        bounds.get_top(),
        bounds.get_right(),
        bounds.get_bottom(),
    );
    if right - left < 30 || bottom - top_edge < 30 {
        return false;
    }
    let screen = windows_sys::Win32::Foundation::POINT {
        x: right - 16,
        y: bottom - 16,
    };
    let (target, point) = crate::actions::point_click::innermost(top as isize, screen);
    let at = |x: i32, y: i32| (((y as u16) as usize) << 16 | (x as u16) as usize) as isize;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};
    let post = |message: u32, keys: usize, lparam: isize| unsafe {
        PostMessageW(target as HWND, message, keys, lparam) != 0
    };
    let sent = post(WM_MOUSEMOVE, 0, at(point.x - 1, point.y))
        && post(WM_MOUSEMOVE, 0, at(point.x, point.y))
        && post(WM_LBUTTONDOWN, 0x0001, at(point.x, point.y))
        && post(WM_LBUTTONUP, 0, at(point.x, point.y));
    handled(top, target as HWND);
    sent
}

/// One Backspace, as a real key: down and up.
fn erase_one(top: HWND) -> bool {
    let target = key_target(top);
    let sent = combo_messages(VK_BACK, None, Held::default()).iter().all(
        |(message, wparam, lparam)| unsafe {
            PostMessageW(target, *message, *wparam, *lparam) != 0
        },
    );
    handled(top, target);
    sent
}

fn type_keys_now(top: HWND, text: &str, with_characters: bool) -> bool {
    let target = key_target(top);
    let thread = unsafe { GetWindowThreadProcessId(target, std::ptr::null_mut()) };
    let post = |list: &[(u32, usize, isize)]| {
        list.iter().all(|(message, wparam, lparam)| unsafe {
            PostMessageW(target, *message, *wparam, *lparam) != 0
        })
    };
    let scan = |c: char| -> Option<i16> {
        let mut units = [0u16; 2];
        let units = c.encode_utf16(&mut units);
        (units.len() == 1)
            .then(|| unsafe { VkKeyScanW(units[0]) })
            .filter(|s| *s != -1)
    };
    let needs_shift = text
        .chars()
        .any(|c| scan(c).is_some_and(|s| (s >> 8) & 1 != 0));
    unsafe {
        let me = GetCurrentThreadId();
        let attached =
            needs_shift && thread != 0 && me != thread && AttachThreadInput(me, thread, 1) != 0;
        let mut saved = [0u8; 256];
        let have = attached && GetKeyboardState(saved.as_mut_ptr()) != 0;
        let mut ok = true;
        for c in text.chars() {
            let (vk, character, shift) = match c {
                '\r' => continue,
                '\n' => (VK_RETURN, Some(0x0D), false),
                '\t' => (VK_TAB, Some(0x09), false),
                _ => match scan(c) {
                    // A key that needs Shift goes as a key only when Shift could be put
                    // in the app's key state; else it would type another character
                    // ("=" is Shift+0 on a Spanish keyboard, and came out as "0"), and
                    // the character goes alone.
                    Some(scanned)
                        if (scanned >> 8) & 0b110 == 0 && ((scanned >> 8) & 1 == 0 || have) =>
                    {
                        let mut units = [0u16; 2];
                        let unit = c.encode_utf16(&mut units)[0];
                        ((scanned & 0xFF) as u16, Some(unit), (scanned >> 8) & 1 != 0)
                    }
                    _ => {
                        let mut units = [0u16; 2];
                        ok = c
                            .encode_utf16(&mut units)
                            .iter()
                            .all(|u| PostMessageW(target, WM_CHAR, *u as usize, 1) != 0);
                        handled(top, target);
                        if !ok {
                            break;
                        }
                        continue;
                    }
                },
            };
            if have {
                // Only Shift changes: the rest of the state stays as it is now.
                let mut state = [0u8; 256];
                if GetKeyboardState(state.as_mut_ptr()) == 0 {
                    state = saved;
                }
                for k in [VK_SHIFT, VK_LSHIFT] {
                    if shift {
                        state[k as usize] |= 0x80;
                    } else {
                        state[k as usize] &= !0x80;
                    }
                }
                SetKeyboardState(state.as_ptr());
            }
            let held = Held {
                shift: shift && have,
                ..Held::default()
            };
            // With its character, or down and up only, as measured for this app
            // (see `type_keys`).
            let character = if with_characters { character } else { None };
            ok = post(&combo_messages(vk, character, held));
            handled(top, target);
            if !ok {
                break;
            }
        }
        if have {
            put_back(&saved, &[VK_SHIFT, VK_LSHIFT]);
        }
        if attached {
            AttachThreadInput(me, thread, 0);
        }
        ok
    }
}

/// Types `text` into a window (\`type w-<id> TEXT\`): into the element that holds
/// its keyboard focus when that element takes text, else into its document
/// (a page or a slide that accessibility shows read-only, so it has no ref),
/// which gets the focus first through UI Automation. Windows may bring the
/// window forward for that focus: the whole typing is guarded and the person's
/// window is given back at once. Then the text goes as the messages real keys
/// send (see \`type_text\`).
pub(crate) fn type_in_window(window: &WindowInfo, text: &str) -> Result<(), AdapterError> {
    use uiautomation::types::{ControlType, Handle, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    let Some(top) = window_handle(window) else {
        return Err(AdapterError::new(
            ErrorCode::WindowNotFound,
            format!("Window {} is no longer open", window.id),
        ));
    };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(top, &mut pid) };
    // The window is told it is active for the whole command: an app that ignores a
    // focus request while it believes it is inactive (egui) takes the one given to
    // its document below, and the keys after it.
    let woke = wake(top, top);
    if woke {
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let result = crate::actions::focus_guard::guarded_program((pid != 0).then_some(pid), || {
        let client = crate::tree::automation::automation_client()?;
        let root = client
            .element_from_handle(Handle::from(top as isize))
            .map_err(|e| AdapterError::internal(format!("window {}: {e}", window.id)))?;
        let takes_text = |e: &uiautomation::UIElement| {
            matches!(
                e.get_control_type(),
                Ok(ControlType::Edit | ControlType::Document | ControlType::ComboBox)
            )
        };
        let focused = client
            .create_property_condition(UIProperty::HasKeyboardFocus, Variant::from(true), None)
            .ok()
            .and_then(|c| root.find_first(TreeScope::Descendants, &c).ok());
        // A window that is not active reports no keyboard focus through UI
        // Automation, but Windows keeps the control that had it (its thread's
        // focus), and that control gets the keys: a classic or WinForms field.
        // A window that lost the activation has no focused control at all until it is
        // told it is active again: then the app gives the focus back to its field.
        let kept = || {
            let mut held = focused_in(top);
            if held == top && wake(top, top) {
                std::thread::sleep(std::time::Duration::from_millis(150));
                held = focused_in(top);
                rest(top);
            }
            (held != top)
                .then(|| client.element_from_handle(Handle::from(held as isize)).ok())
                .flatten()
                .filter(takes_text)
        };
        // A field or a document that takes text and has the focus: typed where its
        // caret is. Focusing a focused document again through UI Automation made
        // WordCraft drop the keys that followed.
        let target = match focused.filter(takes_text).or_else(kept) {
            Some(element) => element,
            None => {
                let document = client
                    .create_property_condition(
                        UIProperty::ControlType,
                        Variant::from(ControlType::Document as i32),
                        None,
                    )
                    .ok()
                    .and_then(|c| root.find_first(TreeScope::Descendants, &c).ok());
                // No field and no document in the accessibility tree: an app that
                // shows none (Java Swing, a game, a canvas) still takes keys where its
                // own focus is, as from a keyboard. The keys go there, unread.
                let Some(document) = document else {
                    return if type_keys(top, text, None) {
                        Ok(())
                    } else {
                        Err(AdapterError::new(
                            ErrorCode::ActionFailed,
                            "The window did not take the keys",
                        ))
                    };
                };
                // Not focused (a button of the app had it, as after a click through
                // accessibility): the page gets the focus, so its editor takes the keys.
                let _ = document.set_focus();
                std::thread::sleep(std::time::Duration::from_millis(150));
                wake_again(top);
                std::thread::sleep(std::time::Duration::from_millis(100));
                document
            }
        };
        // Its value and, for a document, its text: a web page's document (Chromium,
        // Electron) gives its address as its value, and only its text shows what was
        // typed into it.
        let read = |e: &uiautomation::UIElement| {
            let value = e
                .get_pattern::<uiautomation::patterns::UIValuePattern>()
                .ok()
                .and_then(|p| p.get_value().ok());
            let text = matches!(e.get_control_type(), Ok(ControlType::Document))
                .then(|| {
                    e.get_pattern::<uiautomation::patterns::UITextPattern>()
                        .ok()
                })
                .flatten()
                .and_then(|p| p.get_document_range().ok())
                .and_then(|range| range.get_text(DOCUMENT_TEXT_READ).ok());
            match (value, text) {
                (None, None) => None,
                (value, text) => Some(format!(
                    "{}\u{0}{}",
                    value.unwrap_or_default(),
                    text.unwrap_or_default()
                )),
            }
        };
        let before = read(&target);
        // A document that reads its text back must show it changed: an app drawn on a
        // canvas may drop keys while its window is not active, and that is not success.
        // It is read once the app has handled the keys and painted, for up to 1.5 s.
        let typed = |into: &uiautomation::UIElement| -> Result<bool, AdapterError> {
            match type_text(&crate::tree::element::UIAElement::from(into.clone()), text) {
                None => {
                    return Err(AdapterError::new(
                        ErrorCode::ActionFailed,
                        "The window did not take the text",
                    ));
                }
                // Nothing arrived, so nothing is typed twice: the next way is tried.
                Some(Err(_)) => return Ok(false),
                Some(Ok(_)) => {}
            }
            handled(top, top);
            let end = std::time::Instant::now() + std::time::Duration::from_millis(1_500);
            let mut changed = before.is_none() || read(&target) != before;
            while !changed && std::time::Instant::now() < end {
                std::thread::sleep(std::time::Duration::from_millis(100));
                changed = read(&target) != before;
            }
            Ok(changed)
        };
        let mut changed = typed(&target)?;
        // A Chromium window (a browser, an Electron app) takes the keys through its
        // own route (see `chromium_input`), which delivers them; its page may just
        // not show its text to accessibility (VS Code's editor). Typing again, or a
        // click into the page, would type twice or land on whatever is there.
        if !changed && crate::actions::chromium_input::is_chromium_hwnd(top) {
            return Ok(());
        }
        // Nothing arrived (so nothing is typed twice): the page's own editor did not
        // have the keyboard although accessibility said its document did (WordCraft
        // after a click on one of its buttons). The document gets the focus through
        // UI Automation, and the text is typed again.
        if !changed && matches!(target.get_control_type(), Ok(ControlType::Document)) {
            let _ = target.set_focus();
            std::thread::sleep(std::time::Duration::from_millis(150));
            wake_again(top);
            std::thread::sleep(std::time::Duration::from_millis(100));
            changed = typed(&target)?;
            // Still nothing: its editor never had the focus (opened while the window was
            // behind) and takes none through accessibility (egui). A person clicks inside
            // the page: so does this, in the background, near its lower right corner,
            // which puts the caret at the end of the text.
            if !changed && click_into(top, &target) {
                std::thread::sleep(std::time::Duration::from_millis(200));
                changed = typed(&target)?;
            }
        }
        if !changed {
            return Err(AdapterError::new(
                ErrorCode::ActionFailed,
                "The text did not reach the document: this app takes typed text only while its window is active",
            )
            .with_suggestion("Type it with the window in front (--headed), or use a field that takes set-value")
            .with_disposition(agent_desktop_core::DeliverySemantics::delivered_unverified()));
        }
        Ok(())
    });
    if woke {
        rest(top);
    }
    result
}

/// The messages a real key sends: down, the character it types (if any), up.
pub(crate) fn key_messages(
    vk: u16,
    character: Option<u16>,
    extended: bool,
) -> Vec<(u32, usize, isize)> {
    let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as isize;
    let down = 1 | (scan << 16) | if extended { 1 << 24 } else { 0 };
    let up = down | (1 << 30) | (1 << 31);
    let mut messages = vec![(WM_KEYDOWN, vk as usize, down)];
    if let Some(character) = character {
        messages.push((WM_CHAR, character as usize, down));
    }
    messages.push((WM_KEYUP, vk as usize, up));
    messages
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn plain_keys_map_to_their_virtual_key_and_character() {
        assert_eq!(key("return"), Some((0x0D, Some(0x0D))));
        assert_eq!(key("enter"), key("return"));
        assert_eq!(key("escape").map(|k| k.0), Some(0x1B));
        assert_eq!(key("f5"), Some((0x74, None)));
        assert_eq!(key("f13"), None);
        assert_eq!(key("a").map(|k| k.1), Some(Some(u16::from(b'a'))));
        assert_eq!(key("ab"), None);
    }
}
