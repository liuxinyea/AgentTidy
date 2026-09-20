//! Platform-native move-to-trash (`Start.md` §3.4, §16.4).
//!
//! "Trash first" is the default disposal path for file-shaped cleanup:
//! recovery is delegated to the OS (Windows Recycle Bin / macOS
//! Finder Trash) and AgentTidy never claims an in-app undo it cannot
//! honor (§16.4). The `trash` crate wraps the platform APIs
//! (`SHFileOperation`/`IFileOperation` on Windows, `NSWorkspace` on
//! macOS).
//!
//! §16.2 note: on Windows a file held open by another process cannot
//! be moved — this surfaces as an `Err` here; the Phase 6 executor
//! maps it to `skipped`, never to a force-close or a delayed delete.

use std::path::{Path, PathBuf};

/// Error moving something to the trash.
#[derive(Debug, thiserror::Error)]
pub enum TrashError {
    #[error("path not found: {0}")]
    NotFound(PathBuf),
    /// The platform refused the operation (file in use, headless
    /// session without a trash service, …). The message is the
    /// platform error verbatim — surfaced to diagnostics, not guessed
    /// around.
    #[error("platform trash operation failed: {0}")]
    Platform(String),
}

/// Move `path` to the OS trash. Works for files and directories.
///
/// Fail-closed semantics: any doubt (missing path, locked file,
/// platform error) is an `Err` — the caller keeps the item untouched.
pub fn move_to_trash(path: &Path) -> Result<(), TrashError> {
    if path.symlink_metadata().is_err() {
        // symlink_metadata: a broken symlink still counts as present
        // (its link entry can be trashed); the target is irrelevant.
        return Err(TrashError::NotFound(path.to_path_buf()));
    }
    trash::delete(path).map_err(|e| TrashError::Platform(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_file_disappears_from_its_location() {
        let dir = std::env::temp_dir().join(format!(
            "agenttidy-trash-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("to-trash.txt");
        std::fs::write(&file, b"temporary").unwrap();

        move_to_trash(&file).expect("trash works in dev/CI sessions");

        // The whole point of trash-first: the file is no longer at its
        // old location (its new home — the Recycle Bin / Trash — is
        // platform-owned and intentionally out of scope).
        assert!(!file.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_path_fails_closed() {
        let missing = std::env::temp_dir().join("agenttidy-never-existed.txt");
        assert!(matches!(
            move_to_trash(&missing),
            Err(TrashError::NotFound(_))
        ));
    }
}
