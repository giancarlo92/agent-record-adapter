// Part of Agent Record Adapter (see NOTICE).
pub(crate) mod mouse;
pub(crate) mod record;
pub(crate) mod secret_prompt;
pub(crate) mod stroke;

pub(crate) use mouse::drag_state;

pub(crate) mod clipboard {
    use agent_desktop_core::{AdapterError, ClipboardContent, ClipboardFormat, Deadline, InputOps};

    /// The clipboard, read through the unmodified upstream adapter.
    pub(crate) fn get_clipboard_content(
        format: ClipboardFormat,
        deadline: Deadline,
    ) -> Result<Option<ClipboardContent>, AdapterError> {
        upstream_windows::WindowsAdapter::new().get_clipboard_content(format, deadline)
    }

    /// Writes the clipboard through the unmodified upstream adapter (tests).
    #[cfg(test)]
    pub(crate) fn set_content(
        content: &ClipboardContent,
        deadline: Deadline,
    ) -> Result<(), AdapterError> {
        use agent_desktop_core::SystemOps;
        let upstream = upstream_windows::WindowsAdapter::new();
        let lease = upstream.acquire_interaction_lease(deadline)?;
        upstream.set_clipboard_content(content, &lease).map(|_| ())
    }
}
