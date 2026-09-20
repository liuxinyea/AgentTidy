//! Capability model (`Start.md` §9.2).
//!
//! Capabilities answer "what can AgentTidy *reliably* do with this
//! installation" per topic. They are never a single boolean: an installation
//! can be readable but not archivable, degraded by a running process, etc.
//! Capability is provider-reported *fact*; policy consumes it and decides.

use crate::provider::ProviderId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-topic capability state.
///
/// Variants are declared worst-to-best, and `Ord` follows declaration
/// order — so comparisons and `max()` favor the stronger capability
/// (`Supported > ReadOnly > … > Unsupported`).
///
/// - `unsupported`   — this topic does not exist or is not understood
/// - `permission_required` — blocked on permissions, not on understanding
/// - `degraded`      — partially working (e.g. DB index missing; list-only)
/// - `read_only`     — structure understood, but AgentTidy must not mutate
/// - `supported`     — full read (and later write) support, verified
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityStatus {
    Unsupported,
    PermissionRequired,
    Degraded,
    ReadOnly,
    Supported,
}

/// The analysis topics a capability is reported for.
///
/// Phase 1 covers read-only topics; cleanup/restore exist in the enum
/// because the design doc (§9.2) fixes the vocabulary early — providers
/// report `Unsupported` until later phases implement them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityTopic {
    /// Listing and reading sessions.
    Sessions,
    /// Mapping sessions to projects/workspaces.
    Projects,
    /// Recognizing archive state (not available on all agents).
    Archive,
    /// Restoring archived sessions (post-v0.1).
    RestoreArchive,
    /// Cache classification (what is safe-to-drop cache data).
    Cache,
    /// Log classification.
    Logs,
    /// Cleanup planning/execution (Phase 6+).
    Cleanup,
}

/// Capability report for one installation.
///
/// Keys are `CapabilityTopic`s; a missing key means `Unsupported` — use
/// [`AgentCapabilities::get`] instead of map access to apply that default
/// (fail closed, §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentCapabilities {
    topics: BTreeMap<CapabilityTopic, CapabilityStatus>,
}

impl AgentCapabilities {
    /// Empty report — everything defaults to `Unsupported`.
    pub fn empty() -> Self {
        Self {
            topics: BTreeMap::new(),
        }
    }

    /// Build from an iterator of (topic, status) pairs.
    pub fn build<I: IntoIterator<Item = (CapabilityTopic, CapabilityStatus)>>(pairs: I) -> Self {
        Self {
            topics: pairs.into_iter().collect(),
        }
    }

    /// Capability for a topic; missing entries are `Unsupported` (fail closed).
    pub fn get(&self, topic: CapabilityTopic) -> CapabilityStatus {
        self.topics
            .get(&topic)
            .copied()
            .unwrap_or(CapabilityStatus::Unsupported)
    }

    /// Set one topic (builder-style for provider adapters).
    pub fn with(mut self, topic: CapabilityTopic, status: CapabilityStatus) -> Self {
        self.topics.insert(topic, status);
        self
    }
}

/// Convenience alias used by inspection results (§10.1).
pub type ProviderCapabilityReport = (ProviderId, AgentCapabilities);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_topic_fails_closed() {
        let caps =
            AgentCapabilities::empty().with(CapabilityTopic::Sessions, CapabilityStatus::Supported);
        assert_eq!(
            caps.get(CapabilityTopic::Sessions),
            CapabilityStatus::Supported
        );
        assert_eq!(
            caps.get(CapabilityTopic::Cleanup),
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn empty_report_is_all_unsupported() {
        let caps = AgentCapabilities::empty();
        for topic in [
            CapabilityTopic::Sessions,
            CapabilityTopic::Archive,
            CapabilityTopic::Logs,
        ] {
            assert_eq!(caps.get(topic), CapabilityStatus::Unsupported);
        }
    }

    #[test]
    fn status_ordering_favors_stronger_capabilities() {
        // Ord follows declaration (worst-to-best), so max() picks the best
        // capability — the direction the doc comment promises.
        use CapabilityStatus::*;
        assert!(Supported > ReadOnly);
        assert!(ReadOnly > Degraded);
        assert!(Degraded > PermissionRequired);
        assert!(PermissionRequired > Unsupported);
        assert_eq!(std::cmp::max(Degraded, ReadOnly), ReadOnly);
    }
}
