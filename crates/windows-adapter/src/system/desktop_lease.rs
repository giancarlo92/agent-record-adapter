// Part of Agent Record Adapter (see NOTICE).
//! The interaction lease of a work desktop.
//!
//! Upstream serializes every command that acts on the desktop with one lock
//! for the whole user (`%ProgramData%\agent-desktop\<SID>\interaction.lock`):
//! two commands acting at once would fight over the one mouse, keyboard and
//! foreground. A work desktop (`AGENT_DESKTOP_WORK_DESKTOP`) has none of the
//! person's: what runs there acts through window messages on its own
//! invisible desktop. So commands on a work desktop take a lock of that
//! desktop instead: they still wait for one another, but a bot on its own
//! desktop no longer waits for the agents on the person's desktop (a
//! recording, an MCP click), nor they for it. On the person's desktop the
//! lock is upstream's, unchanged.
//!
//! The lock is the same kind as upstream's: an exclusive open of a file, held
//! for the lease's life and released by the system if the process dies.

use std::path::{Path, PathBuf};

use agent_desktop_core::{AdapterError, Deadline, ErrorCode, InteractionLease, ProcessLeaseGuard};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_ALWAYS};

const PURPOSE: &str = "work desktop interaction lease";

/// What a lease holds: the in-process guard upstream's lease also takes, and
/// the desktop's lock file.
struct Held {
    _file: LockFile,
    _process: ProcessLeaseGuard,
}

/// The exclusive open of the lock file, kept as an integer so the lease can
/// be shared between threads; closed (released) when dropped.
struct LockFile(isize);

impl Drop for LockFile {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `open_exclusive` and is closed once.
        unsafe { CloseHandle(self.0 as _) };
    }
}

/// The lease of the work desktop this process runs on; `None` off a work
/// desktop (the lease is then upstream's).
pub(crate) fn acquire(deadline: Deadline) -> Option<Result<InteractionLease, AdapterError>> {
    if !super::work_desktop::active() {
        return None;
    }
    let name = super::work_desktop::name()?;
    Some(acquire_at(&lock_path(&name)?, deadline))
}

fn lock_path(desktop: &str) -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(base)
            .join("DesktopRecorder")
            .join("locks")
            .join(format!(
                "work-desktop-{}.lock",
                desktop.to_ascii_lowercase()
            )),
    )
}

fn acquire_at(path: &Path, deadline: Deadline) -> Result<InteractionLease, AdapterError> {
    let process = ProcessLeaseGuard::acquire(deadline)?;
    let (file, contention) = open_exclusive(path, deadline)?;
    let contention = contention.saturating_add(process.contention_count());
    let held = Held {
        _file: file,
        _process: process,
    };
    Ok(InteractionLease::guarded_with_contention(
        deadline, held, contention,
    ))
}

/// Opens the lock file for this process alone, waiting while another holds it.
fn open_exclusive(path: &Path, deadline: Deadline) -> Result<(LockFile, u64), AdapterError> {
    use std::os::windows::ffi::OsStrExt;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| {
            AdapterError::internal(format!("cannot create {}: {error}", folder.display()))
        })?;
    }
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut contention = 0_u64;
    loop {
        // SAFETY: a null-terminated path and no security attributes or template.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return Ok((LockFile(handle as isize), contention));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error().map(|code| code as u32) != Some(ERROR_SHARING_VIOLATION) {
            return Err(AdapterError::new(
                ErrorCode::Internal,
                format!("cannot open the {PURPOSE} {}: {error}", path.display()),
            ));
        }
        contention = contention.saturating_add(1);
        let remaining = deadline.remaining();
        if remaining.is_zero() {
            return Err(deadline.timeout_error().with_details(serde_json::json!({
                "kind": "lock_timeout",
                "purpose": PURPOSE,
                "path": path.display().to_string(),
                "contention_count": contention,
            })));
        }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(10)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_per_desktop_lock_and_another_desktop_is_free() {
        let folder =
            std::env::temp_dir().join(format!("desktop-lease-test-{}", std::process::id()));
        let first = folder.join("a.lock");
        let other = folder.join("b.lock");
        let held = open_exclusive(&first, Deadline::after(2_000).unwrap()).unwrap();
        // A second holder of the same desktop's lock waits, then times out.
        let waiting = open_exclusive(&first, Deadline::after(150).unwrap());
        assert!(waiting.is_err());
        // Another desktop's lock is free meanwhile.
        assert!(open_exclusive(&other, Deadline::after(150).unwrap()).is_ok());
        drop(held);
        assert!(open_exclusive(&first, Deadline::after(150).unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(folder);
    }
}
