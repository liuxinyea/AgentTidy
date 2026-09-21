//! Read-only safety facts for default-workspace cleanup candidates (§16).
//!
//! This module intentionally does not decide ownership or perform cleanup.
//! It reports filesystem facts that the Phase 6 planner must combine with
//! provider session/automation references before a workspace is eligible.

use std::path::Path;

use crate::fs_probe::{walk_tree, EntryKind};

/// Filesystem facts required before a workspace can be proposed for cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceSafetyFacts {
    /// A repository marker anywhere in the tree blocks cleanup (§2.3).
    pub has_git_marker: bool,
    /// Links and unclassified entries prevent proving the boundary.
    pub has_unsafe_entry: bool,
    /// Read errors prevent a complete ownership/safety assessment.
    pub has_unreadable_entry: bool,
}

impl WorkspaceSafetyFacts {
    /// True only for a fully readable, link-free, non-repository tree.
    pub fn is_filesystem_eligible(&self) -> bool {
        !self.has_git_marker && !self.has_unsafe_entry && !self.has_unreadable_entry
    }
}

/// Inspect a workspace without following links or reading file contents.
pub fn inspect_workspace_safety(path: &Path) -> WorkspaceSafetyFacts {
    let outcome = walk_tree(path);
    WorkspaceSafetyFacts {
        has_git_marker: outcome
            .entries
            .iter()
            .any(|entry| entry.path.file_name().and_then(|name| name.to_str()) == Some(".git")),
        has_unsafe_entry: outcome
            .entries
            .iter()
            .any(|entry| matches!(entry.kind, EntryKind::Link { .. } | EntryKind::Unknown)),
        has_unreadable_entry: !outcome.problems.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_git_marker_blocks_workspace() {
        let root = std::env::temp_dir().join(format!(
            "agenttidy-workspace-safety-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("nested/.git")).unwrap();
        let facts = inspect_workspace_safety(&root);
        assert!(facts.has_git_marker);
        assert!(!facts.is_filesystem_eligible());
        std::fs::remove_dir_all(root).unwrap();
    }
}
