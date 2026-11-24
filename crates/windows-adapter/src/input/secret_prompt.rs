// Part of Agent Record Adapter (see NOTICE).
//! The window that asks a person for a secret the first time a script or an
//! agent needs it: a short message, a password box (its characters hidden)
//! and Save and continue / Cancel. What is typed goes back to the caller,
//! which stores it in the system's secret store; it is never shown, logged or
//! returned to an agent.
//!
//! The window appears on top without taking the focus, and its taskbar button
//! flashes: whatever the person is typing elsewhere keeps going there, and
//! never lands in the password box by accident. It takes the keyboard when the
//! person clicks it, and gives it back when it closes.

use std::cell::RefCell;

use agent_desktop_core::AdapterError;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD, FLASHW_ALL, FLASHW_TIMERNOFG, FLASHWINFO,
    FlashWindowEx, GetMessageW, GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW,
    IsDialogMessageW, KillTimer, MSG, PostQuitMessage, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN,
    SW_SHOWNOACTIVATE, SendMessageW, SetForegroundWindow, SetTimer, SetWindowTextW, ShowWindow,
    TranslateMessage, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT,
    WM_TIMER, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_EX_TOPMOST,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

const SAVE: usize = 1;
const CANCEL: usize = 2;
const TIMEOUT: usize = 3;
/// A person who walked away does not leave the caller waiting forever.
const TIMEOUT_MS: u32 = 5 * 60 * 1000;

thread_local! {
    static EDIT: RefCell<HWND> = const { RefCell::new(std::ptr::null_mut()) };
    static ANSWER: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The window that had the keyboard when the person clicked the prompt.
    static RETURN_TO: RefCell<HWND> = const { RefCell::new(std::ptr::null_mut()) };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn scaled(value: i32) -> i32 {
    value * unsafe { GetDpiForSystem() }.max(96) as i32 / 96
}

/// Asks for a secret; `None` when the person cancels or does not answer.
pub(crate) fn ask(title: &str, message: &str) -> Result<Option<String>, AdapterError> {
    ANSWER.with(|answer| *answer.borrow_mut() = None);
    RETURN_TO.with(|slot| *slot.borrow_mut() = std::ptr::null_mut());
    let class = wide("AgentDesktopSecretPrompt");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let mut definition: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    definition.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    definition.lpfnWndProc = Some(procedure);
    definition.hInstance = instance;
    definition.hbrBackground = 16 as _;
    definition.lpszClassName = class.as_ptr();
    unsafe { RegisterClassExW(&definition) };

    let (width, height) = (scaled(440), scaled(230));
    let x = (unsafe { GetSystemMetrics(SM_CXSCREEN) } - width) / 2;
    let y = (unsafe { GetSystemMetrics(SM_CYSCREEN) } - height) / 2;
    let caption = wide(title);
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            caption.as_ptr(),
            WS_CAPTION | WS_SYSMENU,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if window.is_null() {
        return Err(AdapterError::internal(
            "Could not open the secret prompt window",
        ));
    }
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    let child = |class: &str, text: &str, style: u32, rect: (i32, i32, i32, i32), id: usize| {
        let (class, text) = (wide(class), wide(text));
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                scaled(rect.0),
                scaled(rect.1),
                scaled(rect.2),
                scaled(rect.3),
                window,
                id as _,
                instance,
                std::ptr::null(),
            )
        };
        unsafe { SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1) };
        hwnd
    };
    child("STATIC", message, 0, (16, 12, 400, 80), 0);
    child("STATIC", "Password:", 0, (16, 100, 70, 20), 0);
    let edit = child(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
        (90, 97, 326, 24),
        0,
    );
    child(
        "BUTTON",
        "Save and continue",
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        (186, 140, 130, 30),
        SAVE,
    );
    child(
        "BUTTON",
        "Cancel",
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        (326, 140, 90, 30),
        CANCEL,
    );
    EDIT.with(|slot| *slot.borrow_mut() = edit);
    unsafe {
        SetTimer(window, TIMEOUT, TIMEOUT_MS, None);
        ShowWindow(window, SW_SHOWNOACTIVATE);
        let flash = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: window,
            dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
            uCount: 0,
            dwTimeout: 0,
        };
        FlashWindowEx(&flash);
    }

    let mut message_slot: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message_slot, std::ptr::null_mut(), 0, 0) } > 0 {
        if unsafe { IsDialogMessageW(window, &message_slot) } == 0 {
            unsafe {
                TranslateMessage(&message_slot);
                DispatchMessageW(&message_slot);
            }
        }
    }
    let return_to = RETURN_TO.with(|slot| *slot.borrow());
    if !return_to.is_null() {
        unsafe { SetForegroundWindow(return_to) };
    }
    Ok(ANSWER.with(|answer| answer.borrow_mut().take()))
}

fn read_edit(edit: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let read = unsafe { GetWindowTextW(edit, buffer.as_mut_ptr(), buffer.len() as i32) }.max(0);
    let text = String::from_utf16_lossy(&buffer[..read as usize]);
    buffer.iter_mut().for_each(|unit| *unit = 0);
    let empty = wide("");
    unsafe { SetWindowTextW(edit, empty.as_ptr()) };
    text
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND if (wparam & 0xFFFF) == SAVE => {
            let edit = EDIT.with(|slot| *slot.borrow());
            let text = read_edit(edit);
            if !text.is_empty() {
                ANSWER.with(|answer| *answer.borrow_mut() = Some(text));
                unsafe { DestroyWindow(hwnd) };
            }
            0
        }
        WM_COMMAND if (wparam & 0xFFFF) == CANCEL => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_ACTIVATE if (wparam & 0xFFFF) as u32 != WA_INACTIVE => {
            let deactivated = lparam as HWND;
            RETURN_TO.with(|slot| {
                if slot.borrow().is_null() && !deactivated.is_null() {
                    *slot.borrow_mut() = deactivated;
                }
            });
            let edit = EDIT.with(|slot| *slot.borrow());
            unsafe { SetFocus(edit) };
            0
        }
        WM_TIMER if wparam == TIMEOUT => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            unsafe {
                KillTimer(hwnd, TIMEOUT);
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
