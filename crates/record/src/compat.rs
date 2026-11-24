// Part of Agent Record Adapter (see NOTICE).
//! Small copies of `agent-desktop-core` helpers that are private there
//! (`pub(crate)`), for the commands ported from the old fork. Copied from
//! vercel-labs/agent-desktop under Apache-2.0; keep them in step with upstream
//! when it changes them (docs/updating-upstream.md lists them).

pub mod storage {
    pub mod state_root {
        use agent_desktop_core::AppError;
        use std::path::PathBuf;

        const AGENT_DESKTOP_HOME_ENV: &str = "AGENT_DESKTOP_HOME";
        const STATE_DIR_NAME: &str = ".agent-desktop";

        /// The agent-desktop state folder: `AGENT_DESKTOP_HOME`, else
        /// `~/.agent-desktop`. The engine uses the same one, so the agent
        /// inbox a recording reads is the one its actions write.
        pub fn resolve_configured_state_root() -> Result<PathBuf, AppError> {
            if let Some(path) = std::env::var_os(AGENT_DESKTOP_HOME_ENV).map(PathBuf::from) {
                if crate::refs::is_symlink(&path) {
                    return Err(AppError::invalid_input(format!(
                        "{AGENT_DESKTOP_HOME_ENV} must not be a symlink"
                    )));
                }
                return Ok(path);
            }
            let home = crate::refs::home_dir()
                .ok_or_else(|| AppError::Internal("HOME directory not found".into()))?;
            Ok(home.join(STATE_DIR_NAME))
        }
    }
}

pub mod refs {
    use agent_desktop_core::AppError;
    pub use agent_desktop_core::refs::*;
    use std::path::{Path, PathBuf};

    /// A write that fails at a path the caller named is an argument error.
    pub fn unwritable_output(what: &str, path: &Path, error: AppError) -> AppError {
        match error {
            AppError::Io(io) => AppError::invalid_input_with_suggestion(
                format!("Cannot write {what} to '{}': {io}", path.display()),
                "Pass an output path in a directory this user can write to; missing parent directories are created",
            ),
            other => other,
        }
    }

    pub fn is_symlink(path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
    }

    pub fn home_dir() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))?;
        let meta = std::fs::symlink_metadata(&home).ok()?;
        (meta.is_dir() && !meta.file_type().is_symlink()).then_some(home)
    }
}

pub mod tree {
    pub use agent_desktop_core::tree::*;
}
