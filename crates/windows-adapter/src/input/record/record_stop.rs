// Part of Agent Record Adapter (see NOTICE).
//! Decides when a `record` session ends: the console's Ctrl+C or Ctrl+Break,
//! the stop file appearing, or the maximum duration elapsing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{FALSE, TRUE};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Ends the session as soon as the stop watch looks: the indicator's Stop.
pub(super) fn request_stop() {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

const STOP_FILE_POLL: Duration = Duration::from_millis(200);

unsafe extern "system" fn on_console_control(_control: u32) -> i32 {
    INTERRUPTED.store(true, Ordering::SeqCst);
    TRUE
}

/// Holds the console control handler for the session and removes it when
/// dropped, so a later command in the same process gets the default
/// Ctrl+C behavior back.
pub(super) struct InterruptGuard {
    installed: bool,
}

impl InterruptGuard {
    pub(super) fn install() -> Self {
        INTERRUPTED.store(false, Ordering::SeqCst);
        STOP_REQUESTED.store(false, Ordering::SeqCst);
        let installed = unsafe { SetConsoleCtrlHandler(Some(on_console_control), TRUE) } != 0;
        Self { installed }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.installed {
            unsafe { SetConsoleCtrlHandler(Some(on_console_control), FALSE) };
        }
    }
}

/// How long input keeps being processed after a stop signal. Windows hands
/// transitions to low-level hooks asynchronously, so an action taken just
/// before the signal can reach the hook after it.
const STOP_GRACE: Duration = Duration::from_millis(300);

pub(super) struct StopWatch {
    started: Instant,
    max: Duration,
    stop_file: Option<PathBuf>,
    next_file_check: Instant,
    stopping: Option<(&'static str, Instant)>,
}

impl StopWatch {
    pub(super) fn new(started: Instant, max_duration_ms: u64, stop_file: Option<PathBuf>) -> Self {
        Self {
            started,
            max: Duration::from_millis(max_duration_ms),
            stop_file,
            next_file_check: started,
            stopping: None,
        }
    }

    /// The reason the session ends, once the grace period after the first
    /// stop signal has passed.
    pub(super) fn finished(&mut self, now: Instant) -> Option<&'static str> {
        match self.stopping {
            Some((reason, until)) if now >= until => Some(reason),
            Some(_) => None,
            None => {
                self.stopping = self.reason(now).map(|reason| (reason, now + STOP_GRACE));
                None
            }
        }
    }

    /// The reason to stop now, if there is one. The stop file is polled at
    /// most every 200 ms so an idle session does not hammer the disk.
    pub(super) fn reason(&mut self, now: Instant) -> Option<&'static str> {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Some("interrupted");
        }
        if STOP_REQUESTED.load(Ordering::SeqCst) {
            return Some("indicator");
        }
        if now >= self.next_file_check {
            self.next_file_check = now + STOP_FILE_POLL;
            if self.stop_file.as_ref().is_some_and(|path| path.exists()) {
                return Some("stop_file");
            }
        }
        if now.saturating_duration_since(self.started) >= self.max {
            return Some("max_duration");
        }
        None
    }
}

#[cfg(test)]
#[path = "record_stop_tests.rs"]
mod tests;
