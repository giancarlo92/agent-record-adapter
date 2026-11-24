// Part of Agent Record Adapter (see NOTICE).
//! Keeps the person's window in front while a headless action or a launch runs.
//!
//! A semantic UI Automation call never sends input, but a target can still
//! bring its own window to the front: Chromium (Chrome, Edge, Electron) does
//! it about 30 ms after a click or a value arrives, and Windows lets a newly
//! started program take the foreground when the person has just typed or
//! clicked. Keys the person presses at that moment would land in the target.
//!
//! While a guard runs, a thread of its own:
//!
//! - looks at the foreground every millisecond and, when a window of the
//!   target has taken it, gives it back to the window the person had in front
//!   (the latest one: the person can move between their own windows freely);
//! - observes key presses with a low-level keyboard hook, only to run that
//!   same check *before* each key is delivered: Windows decides where a key
//!   goes after its hooks return, so a key pressed while the target is in
//!   front still reaches the person's window. No key is read, kept or changed.
//!
//! The person stays in charge: a click on the target's window or on the
//! taskbar, or Alt/Windows held, means they brought it forward themselves, and
//! the guard lets go.
//!
//! Two other ways were measured and left out: dropping Chromium's injected
//! "foreground" key (it then waits 2 s and comes forward anyway), and
//! `LockSetForegroundWindow` (Chromium still came forward).

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, POINT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LMENU, VK_LWIN, VK_MBUTTON, VK_RBUTTON, VK_RWIN,
    VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, EnumChildWindows, EnumWindows, GA_ROOT, GUITHREADINFO,
    GetAncestor, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    HC_ACTION, IsWindowVisible, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP, MSG,
    MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_NULL, WindowFromPoint,
};

/// How long after an action the target's activation is still watched for:
/// Chromium can activate its window a while after the call returns.
const ACTION_SETTLE: Duration = Duration::from_millis(1_500);
/// How long after a launched program shows its window: it activates the
/// window right after showing it.
const LAUNCH_SETTLE: Duration = Duration::from_millis(1_500);
/// A click or Alt/Windows this recent means the person moved the focus.
const PERSON_RECENT: Duration = Duration::from_millis(600);

/// The windows a guard keeps behind: those of one process, and of any process
/// running the same executable (a program that restarts itself).
pub(crate) struct Target {
    pub(crate) pid: u32,
    pub(crate) image: Option<String>,
    /// Whether its new windows also go to the bottom of the stack, and if so
    /// whether Chromium windows do too (see `behind`).
    pub(crate) to_back: Option<bool>,
    /// For a launch: the top-level windows open before it started. A window
    /// that was not among them is the launched program's too, whatever process
    /// shows it: a packaged app is started by the system's broker, not by the
    /// program launched, and a single-instance app hands over to the instance
    /// already running. It counts only while the person has not clicked or
    /// pressed a key (then a new window may be one they opened).
    pub(crate) new_windows: Option<Vec<isize>>,
}

struct Watch {
    target: Target,
    /// The window the person has in front (0 when none is known).
    person: isize,
    /// Whether each process seen in front belongs to the target.
    known: Vec<(u32, bool)>,
    released: bool,
    /// What `front` last returned, for the debug trace.
    seen: isize,
    button_down: bool,
    person_click_on_target: Option<Instant>,
    /// A taskbar click or Alt/Windows not yet followed by a change of window.
    person_switch_key: Option<Instant>,
    behind: Option<crate::actions::behind::Behind>,
    debug: bool,
    /// While set, input is still being sent to the target: the foreground is
    /// given back only before a key of the person's (so it reaches their
    /// window), not as soon as the target takes it, which would deactivate it
    /// in the middle of the input.
    hold: Option<Arc<AtomicBool>>,
    /// Whether the person clicked or pressed a key since the guard started.
    person_acted: bool,
    /// Whether each top-level window seen in front hosts a window of the target.
    hosts: Vec<(isize, bool)>,
}

thread_local! {
    static WATCH: RefCell<Option<Watch>> = const { RefCell::new(None) };
}

fn tick() -> u32 {
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount() }
}

/// The window keys would go to now. While a window is being activated the
/// system reports no foreground window, although the keys already go to the
/// thread that is taking over: then it is that thread's active window.
fn front() -> HWND {
    let window = unsafe { GetForegroundWindow() };
    if !window.is_null() {
        return window;
    }
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if unsafe { GetGUIThreadInfo(0, &mut info) } == 0 {
        return std::ptr::null_mut();
    }
    if !info.hwndActive.is_null() {
        info.hwndActive
    } else {
        info.hwndFocus
    }
}

fn process_of(window: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid
}

fn image_of(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0_u16; 1_024];
    let mut size = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    unsafe { CloseHandle(handle) };
    (ok != 0 && size != 0)
        .then(|| String::from_utf16_lossy(&buffer[..size as usize]).to_lowercase())
}

fn key_down(key: u16) -> bool {
    (unsafe { GetAsyncKeyState(i32::from(key)) } as u16 & 0x8000) != 0
}

fn recent(at: Option<Instant>) -> bool {
    at.is_some_and(|at| at.elapsed() <= PERSON_RECENT)
}

impl Watch {
    fn is_target(&mut self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        if pid == self.target.pid {
            return true;
        }
        if let Some((_, known)) = self.known.iter().find(|(seen, _)| *seen == pid) {
            return *known;
        }
        let same_image = self
            .target
            .image
            .as_ref()
            .is_some_and(|image| image_of(pid).as_ref() == Some(image));
        self.known.push((pid, same_image));
        same_image
    }

    /// Whether `window` is the target's: a window of its program, or, for a
    /// launch, a window that was not open before it (see `Target::new_windows`).
    fn is_target_window(&mut self, window: HWND) -> bool {
        if self.is_target(process_of(window)) || self.hosts_target(window) {
            return true;
        }
        !self.person_acted
            && self
                .target
                .new_windows
                .as_ref()
                .is_some_and(|before| !before.contains(&(window as isize)))
    }

    /// Whether `window` holds a child window of the target: a packaged (UWP)
    /// app's window is a frame of another process (ApplicationFrameHost) that
    /// hosts the app's own window.
    fn hosts_target(&mut self, window: HWND) -> bool {
        if let Some((_, hosts)) = self.hosts.iter().find(|(seen, _)| *seen == window as isize) {
            return *hosts;
        }
        unsafe extern "system" fn collect(child: HWND, data: isize) -> i32 {
            let found = unsafe { &mut *(data as *mut Vec<u32>) };
            let pid = process_of(child);
            if !found.contains(&pid) {
                found.push(pid);
            }
            1
        }
        let mut pids: Vec<u32> = Vec::new();
        unsafe { EnumChildWindows(window, Some(collect), &mut pids as *mut _ as isize) };
        let own = process_of(window);
        let hosts = pids
            .into_iter()
            .filter(|pid| *pid != own)
            .any(|pid| self.is_target(pid));
        self.hosts.push((window as isize, hosts));
        hosts
    }

    /// Notes what the person is doing with the mouse and the switching keys.
    fn observe_person(&mut self) {
        let down = key_down(VK_LBUTTON) || key_down(VK_RBUTTON) || key_down(VK_MBUTTON);
        if down {
            self.person_acted = true;
        }
        if down && !self.button_down {
            let mut point = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut point) };
            let root = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
            let in_front = front();
            // A click on the target counts only when it brings the target forward.
            let target_behind = in_front.is_null() || !self.is_target(process_of(in_front));
            if !root.is_null() && target_behind && self.is_target(process_of(root)) {
                self.person_click_on_target = Some(Instant::now());
            }
            if !root.is_null() && crate::actions::behind::is_taskbar(root) {
                self.person_switch_key = Some(Instant::now());
            }
        }
        self.button_down = down;
        // Alt+Tab or the Windows key, never Alt alone: AltGr (Ctrl+Alt) types
        // characters on many keyboard layouts.
        let alt_tab = key_down(VK_TAB) && key_down(VK_LMENU) && !key_down(VK_CONTROL);
        if alt_tab || key_down(VK_LWIN) || key_down(VK_RWIN) {
            self.person_switch_key = Some(Instant::now());
        }
    }

    /// Gives the foreground back when a window of the target holds it and the
    /// person did not bring it there.
    fn keep_person_in_front(&mut self) {
        let now = front();
        if self.debug && now as isize != self.seen {
            self.seen = now as isize;
            let pid = process_of(now);
            let target = self.is_target_window(now);
            eprintln!("guard: front pid={pid} target={target} tick={}", tick());
        }
        if now.is_null() || now as isize == self.person {
            return;
        }
        if !self.is_target_window(now) {
            // The person moved to another window: their taskbar click was for
            // it. The taskbar itself comes forward for an instant on the way.
            if !crate::actions::behind::is_taskbar(now) {
                self.person = now as isize;
                self.person_switch_key = None;
            }
            return;
        }
        if self.released || self.person == 0 {
            return;
        }
        // A window that is only just appearing was not chosen by anyone: a
        // guard over a launch never lets go.
        let chosen = recent(self.person_click_on_target) || recent(self.person_switch_key);
        if chosen && self.target.to_back.is_none() {
            self.released = true;
            if self.debug {
                eprintln!(
                    "guard: the person brought the target forward tick={}",
                    tick()
                );
            }
            return;
        }
        crate::actions::behind::give_back(self.person as HWND, now);
        if self.debug {
            let back = front() as isize == self.person;
            eprintln!("guard: taken tick={}, given back: {back}", tick());
        }
    }
}

/// A guard that runs until `finish`, then for its settle time.
pub(crate) struct Guard {
    done: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Guard {
    /// Starts guarding, unless the target is the window the person is using.
    pub(crate) fn start(target: Target, settle: Duration) -> Option<Self> {
        Self::start_from(target, settle, front(), None)
    }

    /// Starts guarding with the window the person had in front given: taken
    /// before an action that may already have brought the target forward.
    fn start_from(
        target: Target,
        settle: Duration,
        person: HWND,
        hold: Option<Arc<AtomicBool>>,
    ) -> Option<Self> {
        if target.pid == 0 || (!person.is_null() && process_of(person) == target.pid) {
            return None;
        }
        let done = Arc::new(AtomicBool::new(false));
        let thread = {
            let done = done.clone();
            let person = person as isize;
            std::thread::spawn(move || watch(target, person, settle, &done, hold))
        };
        Some(Self {
            done,
            thread: Some(thread),
        })
    }

    /// A guard for a program that was just started: Windows lets it take the
    /// foreground when the person has just typed or clicked. `start` is what
    /// was on screen before the launch.
    fn for_launch(pid: u32, executable: &std::path::Path, start: &LaunchStart) -> Option<Self> {
        let image = Some(executable.to_string_lossy().to_lowercase());
        Self::start_from(
            Target {
                pid,
                image,
                to_back: Some(false),
                new_windows: Some(start.windows.clone()),
            },
            LAUNCH_SETTLE,
            start.person as HWND,
            None,
        )
    }

    /// A guard for a running program that is about to show a new window.
    pub(crate) fn for_new_window(pid: u32) -> Option<Self> {
        Self::start(
            Target {
                pid,
                image: None,
                to_back: Some(true),
                new_windows: None,
            },
            LAUNCH_SETTLE,
        )
    }

    /// Stops after the settle time has passed.
    pub(crate) fn finish(mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Runs `action` and keeps the foreground where the person has it, when the
/// action's target belongs to `target_pid` and is not the window in use. It
/// matches every process of the target's program, not only the target's own:
/// a browser opens a page's file picker in another of its processes, and that
/// dialog came forward.
pub(crate) fn guarded_program<T>(target_pid: Option<u32>, action: impl FnOnce() -> T) -> T {
    let guard = target_pid.and_then(|pid| {
        Guard::start(
            Target {
                pid,
                image: image_of(pid),
                to_back: None,
                new_windows: None,
            },
            ACTION_SETTLE,
        )
    });
    let result = action();
    if let Some(guard) = guard {
        guard.finish();
    }
    result
}

/// The window the person has in front, taken before an action that may bring
/// its target forward, for `during_input`.
#[derive(Clone, Copy)]
pub(crate) struct PersonFront(isize);

impl PersonFront {
    pub(crate) fn now() -> Self {
        Self(front() as isize)
    }
}

/// A guard for an action that sends input to a browser told it is active
/// (keys or a click as window messages, the address bar). It runs from before
/// the input: while the input is sent, a key the person presses gives them
/// their window back first, so the key reaches it, but the browser is not
/// deactivated otherwise, which would lose the input halfway (a typed-only date
/// lost its value). Once the input is sent and handled (`input_sent`), the
/// foreground is given back at once, for the rest of the action and
/// `settle_ms` after it.
pub(crate) fn during_input(
    target_pid: Option<u32>,
    person: PersonFront,
    settle_ms: u64,
) -> InputGuard {
    let hold = Arc::new(AtomicBool::new(true));
    let pid = target_pid.filter(|pid| *pid != 0).unwrap_or(0);
    let guard = (pid != 0)
        .then(|| {
            Guard::start_from(
                Target {
                    pid,
                    image: image_of(pid),
                    to_back: None,
                    new_windows: None,
                },
                Duration::from_millis(settle_ms),
                person.0 as HWND,
                Some(hold.clone()),
            )
        })
        .flatten();
    InputGuard { guard, hold, pid }
}

/// See `during_input`.
pub(crate) struct InputGuard {
    guard: Option<Guard>,
    hold: Arc<AtomicBool>,
    pid: u32,
}

impl InputGuard {
    /// The input was sent: once the program has handled it (its windows'
    /// message queues flushed, and its page a moment to take what they passed
    /// on), the foreground is given back whenever the program holds it. The
    /// guard returned goes on until it is dropped or finished, then its settle.
    pub(crate) fn input_sent(mut self) -> Option<Guard> {
        if self.guard.is_some() {
            flush_windows_of(self.pid);
            std::thread::sleep(INPUT_SETTLE);
        }
        self.hold.store(false, Ordering::SeqCst);
        self.guard.take()
    }
}

impl Drop for InputGuard {
    fn drop(&mut self) {
        self.hold.store(false, Ordering::SeqCst);
    }
}

/// How long a page gets, after its browser's windows handled the input sent
/// to them, before the foreground is given back.
const INPUT_SETTLE: Duration = Duration::from_millis(30);

/// Waits until every visible top-level window of `pid` has handled the
/// messages already sent or posted to it (at most 400 ms each).
fn flush_windows_of(pid: u32) {
    unsafe extern "system" fn collect(window: HWND, data: isize) -> i32 {
        let found = unsafe { &mut *(data as *mut (u32, Vec<isize>)) };
        if unsafe { IsWindowVisible(window) } != 0 && process_of(window) == found.0 {
            found.1.push(window as isize);
        }
        1
    }
    let mut found: (u32, Vec<isize>) = (pid, Vec::new());
    unsafe { EnumWindows(Some(collect), &mut found as *mut _ as isize) };
    for window in found.1 {
        let mut answer = 0usize;
        unsafe {
            SendMessageTimeoutW(
                window as HWND,
                WM_NULL,
                0,
                0,
                SMTO_ABORTIFHUNG,
                400,
                &mut answer,
            )
        };
    }
}

fn watch(
    target: Target,
    person: isize,
    settle: Duration,
    done: &AtomicBool,
    hold: Option<Arc<AtomicBool>>,
) {
    let debug = std::env::var_os("AGENT_DESKTOP_GUARD_DEBUG").is_some();
    if debug {
        eprintln!("guard: start tick={} target={}", tick(), target.pid);
    }
    let to_back = target.to_back;
    // A launch made on the work desktop is watched on the person's: what it
    // shows there (a packaged app opens on the desktop on screen) is what
    // could take their foreground.
    let desktop = (target.new_windows.is_some() && crate::system::work_desktop::active())
        .then(input_desktop::enter)
        .flatten();
    WATCH.with(|slot| {
        *slot.borrow_mut() = Some(Watch {
            target,
            person,
            known: Vec::new(),
            released: false,
            seen: 0,
            button_down: false,
            person_click_on_target: None,
            person_switch_key: None,
            behind: to_back.map(crate::actions::behind::Behind::new),
            debug,
            hold,
            person_acted: false,
            hosts: Vec::new(),
        });
    });
    let mut message: MSG = unsafe { std::mem::zeroed() };
    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(before_key), std::ptr::null_mut(), 0) };
    let mut finished: Option<Instant> = None;
    loop {
        unsafe { MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 1, QS_ALLINPUT) };
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        WATCH.with(|slot| {
            if let Some(watch) = slot.borrow_mut().as_mut() {
                watch.observe_person();
                let held = watch
                    .hold
                    .as_ref()
                    .is_some_and(|hold| hold.load(Ordering::SeqCst));
                if !held {
                    watch.keep_person_in_front();
                }
                if !watch.released
                    && let Some(mut behind) = watch.behind.take()
                {
                    behind.sweep(|pid| watch.is_target(pid));
                    watch.behind = Some(behind);
                }
            }
        });
        match finished {
            Some(at) if at.elapsed() >= settle => break,
            None if done.load(Ordering::SeqCst) => finished = Some(Instant::now()),
            _ => {}
        }
    }
    if !hook.is_null() {
        unsafe { UnhookWindowsHookEx(hook) };
    }
    WATCH.with(|slot| *slot.borrow_mut() = None);
    drop(desktop);
    if debug {
        eprintln!("guard: end tick={}", tick());
    }
}

/// Runs before a key the person pressed is delivered: if the target took the
/// foreground a moment ago, the person's window gets it back first, so the
/// key goes there. The key itself is not looked at.
unsafe extern "system" fn before_key(code: i32, wparam: usize, lparam: isize) -> isize {
    if code == HC_ACTION as i32 && lparam != 0 {
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if event.flags & (LLKHF_INJECTED | LLKHF_UP) == 0 {
            WATCH.with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    if let Some(watch) = slot.as_mut() {
                        watch.person_acted = true;
                        watch.observe_person();
                        watch.keep_person_in_front();
                    }
                }
            });
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// The window the person has in front now, as a plain handle.
pub(crate) fn person_front() -> isize {
    front() as isize
}

/// What was on the person's screen before a launch: the window in front and
/// every top-level window open, for `after_launch`.
pub(crate) struct LaunchStart {
    person: isize,
    windows: Vec<isize>,
}

/// Takes what is on the person's screen before a launch (on the desktop on
/// screen, also when this command runs on the work desktop).
pub(crate) fn launch_start() -> LaunchStart {
    input_desktop::run(|| LaunchStart {
        person: front() as isize,
        windows: top_windows(),
    })
}

/// How long a launch that showed no window yet is watched for one: a packaged
/// app is started by the system and shows its window seconds later.
const LAUNCH_WAIT: Duration = Duration::from_secs(10);

/// After a launch: when a window of the launched program, or one that was not
/// open before it, took the foreground from the person's window, it is given
/// back, and a guard keeps it so while the app settles (upstream starts the
/// app activated; the old fork started it with SW_SHOWNOACTIVATE instead).
/// `shown`: whether the launch already found the program's window; when it did
/// not (the program handed the start over to another process), the guard waits
/// for a new window to show, up to `LAUNCH_WAIT`.
pub(crate) fn after_launch(
    start: LaunchStart,
    pid: u32,
    executable: &std::path::Path,
    shown: bool,
) {
    input_desktop::run(|| {
        let person = start.person as HWND;
        let now = front();
        if !person.is_null()
            && now != person
            && (process_of(now) == pid || !start.windows.contains(&(now as isize)))
        {
            crate::actions::behind::give_back(person, now);
        }
    });
    let Some(guard) = Guard::for_launch(pid, executable, &start) else {
        return;
    };
    if !shown {
        let end = Instant::now() + LAUNCH_WAIT;
        while Instant::now() < end
            && input_desktop::run(|| {
                top_windows()
                    .iter()
                    .all(|window| start.windows.contains(window))
            })
        {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    guard.finish();
}

/// The visible top-level windows of the calling thread's desktop.
fn top_windows() -> Vec<isize> {
    unsafe extern "system" fn collect(window: HWND, data: isize) -> i32 {
        let found = unsafe { &mut *(data as *mut Vec<isize>) };
        if unsafe { IsWindowVisible(window) } != 0 {
            found.push(window as isize);
        }
        1
    }
    let mut found: Vec<isize> = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut found as *mut _ as isize) };
    found
}

/// The desktop on screen, for a command running on the work desktop.
mod input_desktop {
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, DESKTOP_ENUMERATE, DESKTOP_HOOKCONTROL, DESKTOP_READOBJECTS,
        DESKTOP_WRITEOBJECTS, GetThreadDesktop, HDESK, OpenInputDesktop, SetThreadDesktop,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    /// Puts the calling thread on the desktop on screen until dropped. The
    /// thread must have no window and no hook yet.
    pub(super) struct Entered {
        input: HDESK,
        previous: HDESK,
    }

    pub(super) fn enter() -> Option<Entered> {
        let access =
            DESKTOP_READOBJECTS | DESKTOP_ENUMERATE | DESKTOP_HOOKCONTROL | DESKTOP_WRITEOBJECTS;
        let input = unsafe { OpenInputDesktop(0, 0, access) };
        if input.is_null() {
            return None;
        }
        let previous = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if unsafe { SetThreadDesktop(input) } == 0 {
            unsafe { CloseDesktop(input) };
            return None;
        }
        Some(Entered { input, previous })
    }

    impl Drop for Entered {
        fn drop(&mut self) {
            unsafe {
                SetThreadDesktop(self.previous);
                CloseDesktop(self.input);
            }
        }
    }

    /// Runs `f` on the desktop on screen: here when this command runs there,
    /// else on a thread of its own put there.
    pub(super) fn run<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        if !crate::system::work_desktop::active() {
            return f();
        }
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _entered = enter();
                    f()
                })
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
}

/// When `window` (a `w-<hwnd>` id) took the foreground from the person's
/// window, it is given back.
pub(crate) fn give_back_from(person: isize, window: &str) {
    let person = person as HWND;
    let Ok(taken) = crate::system::window::window_ops::parse_window_id(window) else {
        return;
    };
    let now = front();
    if !person.is_null() && now as isize == taken && now != person {
        crate::actions::behind::give_back(person, now);
    }
}
