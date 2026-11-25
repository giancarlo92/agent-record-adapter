// Part of Agent Record Adapter (see NOTICE).
//! System helpers for our layer. Window and display lists come from the
//! unmodified upstream adapter through its public traits; the rest are small
//! copies of upstream's private helpers (Apache-2.0).

pub(crate) mod desktop_lease;
pub(crate) mod separate_launch;
pub(crate) mod work_desktop;

pub(crate) mod window {
    pub(crate) mod window_ops {
        use agent_desktop_core::{
            AdapterError, Deadline, ErrorCode, ObservationOps, WindowFilter, WindowInfo,
        };

        /// `w-<hwnd>` to the window handle (upstream's id format).
        pub(crate) fn parse_window_id(id: &str) -> Result<isize, AdapterError> {
            id.strip_prefix("w-")
                .and_then(|number| number.parse::<isize>().ok())
                .filter(|handle| *handle > 0)
                .ok_or_else(|| {
                    AdapterError::new(ErrorCode::InvalidArgs, "Malformed window identifier")
                })
        }

        /// Every window, as upstream's `list-windows` reads them.
        pub(crate) fn list_windows_live(
            filter: &WindowFilter,
            deadline: Deadline,
        ) -> Result<Vec<WindowInfo>, AdapterError> {
            upstream_windows::WindowsAdapter::new().list_windows(filter, deadline)
        }
    }
}

pub(crate) mod display {
    use agent_desktop_core::{AdapterError, Deadline, DisplayInfo, SystemOps};

    pub(crate) fn list_displays_live(deadline: Deadline) -> Result<Vec<DisplayInfo>, AdapterError> {
        upstream_windows::WindowsAdapter::new().list_displays(deadline)
    }
}

pub(crate) mod process_identity {
    use agent_desktop_core::ProcessId;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    /// The executable file name of a running process (`explorer.exe`).
    pub(crate) fn process_image_name(pid: ProcessId) -> Option<String> {
        // SAFETY: a process snapshot handle, checked and closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        // SAFETY: `entry.dwSize` is set as the API requires.
        let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
        while ok != 0 {
            if entry.th32ProcessID == u32::from(pid) {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                found = Some(String::from_utf16_lossy(&entry.szExeFile[..end]));
                break;
            }
            // SAFETY: as above.
            ok = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        // SAFETY: the snapshot handle is valid and closed once.
        unsafe { CloseHandle(snapshot) };
        found
    }
}
