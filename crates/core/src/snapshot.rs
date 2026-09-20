//! Scan snapshot model (`Start.md` §9, Phase 1 vocabulary).
//!
//! An `AgentSnapshot` is the full read-only result of scanning one
//! installation: sessions, resources and scan diagnostics. Snapshots are
//! immutable facts; policy/planning consume them. The doc's golden tests
//! (§18.5) serialize snapshots for diffing, hence serde support.

use crate::installation::AgentInstallation;
use crate::resource::Resource;
use crate::session::Session;
use crate::size::SizeInfo;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Options controlling a provider scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScanOptions {
    /// Include resources the provider cannot fully attribute
    /// (unknown structures, shared stores). Default false keeps snapshots
    /// small; enabling it supports diagnostics views.
    pub include_unknown: bool,
}

/// Non-fatal problems discovered while scanning (diagnostics surface, §15).
///
/// Problems never abort a scan — they degrade it (fail closed applies to
/// cleanup, not to reporting).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "severity", rename_all = "lowercase")]
pub enum ScanProblem {
    /// Degraded accuracy; results still trustworthy for the affected scope.
    Warning { message: String },
    /// Part of the scan could not be completed or trusted.
    Error {
        message: String,
        path: Option<String>,
    },
}

/// Immutable read-only view of one installation at scan time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    /// The installation this snapshot was taken from.
    pub installation: AgentInstallation,
    /// Discovered sessions.
    pub sessions: Vec<Session>,
    /// Discovered resources (session-scoped and shared).
    pub resources: Vec<Resource>,
    /// Scan problems, keyed by a short stable code (e.g. `db-unreadable`).
    pub problems: BTreeMap<String, ScanProblem>,
    /// When the scan finished (epoch ms).
    pub completed_at: u64,
}

impl AgentSnapshot {
    /// Aggregate space accounting across all resources (fold of
    /// [`SizeInfo::merge`], so confidence follows the weakest contributor).
    ///
    /// Dedup across resources (hard links by inode, symlinks not followed,
    /// nested dirs counted once) is the scanner's responsibility (§8) —
    /// this is a plain fold over what the scanner reported. Shared bytes
    /// may overlap with other snapshots; reclaimability math must use
    /// `exclusive_bytes`, never `logical_bytes` (§8).
    ///
    /// An empty snapshot is exactly zero bytes — `SizeInfo::exact(0)`, not
    /// `unknown()`.
    pub fn total_size(&self) -> SizeInfo {
        self.resources
            .iter()
            .map(|r| r.size)
            .reduce(SizeInfo::merge)
            .unwrap_or_else(|| SizeInfo::exact(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityTopic;
    use crate::installation::{InstallationStatus, Platform};
    use crate::provider::ProviderId;
    use crate::resource::{ManagedBy, Ownership, Resource, ResourceKind, ResourceLocator};
    use crate::session::SessionLifecycle;
    use crate::size::{SizeConfidence, SizeInfo};

    fn fixture_snapshot() -> AgentSnapshot {
        let provider = ProviderId::new(ProviderId::CLAUDE_CODE);
        let installation = AgentInstallation {
            id: "claude-code:default".into(),
            provider: provider.clone(),
            platform: Platform::Macos,
            version: Some("2.1.235".into()),
            data_roots: vec!["/Users/x/.claude".into()],
            status: InstallationStatus::Available,
        };
        let session = Session {
            id: crate::session::SessionId::new("02e8fad4"),
            provider: provider.clone(),
            installation_id: installation.id.clone(),
            title: Some("AgentTidy skeleton".into()),
            project: Some(crate::session::ProjectRef::from_cwd("/Users/x/AgentTidy")),
            created_at: Some(1789000000000),
            updated_at: Some(1789000000000),
            lifecycle: SessionLifecycle::Inactive,
            size: SizeInfo::exact(4096),
            resource_refs: vec![],
            metadata: serde_json::Map::new(),
        };
        let resource = Resource {
            id: crate::resource::ResourceId::new("claude-code:default:session:02e8fad4"),
            provider,
            installation_id: installation.id.clone(),
            kind: ResourceKind::Session,
            locator: ResourceLocator::File {
                path: "/Users/x/.claude/projects/-Users-x-AgentTidy/02e8fad4.jsonl".into(),
            },
            ownership: Ownership::Exclusive,
            managed_by: ManagedBy::Agent,
            size: SizeInfo::exact(4096),
            session_id: Some(session.id.clone()),
            project: None,
            created_at: None,
            updated_at: None,
            dependencies: vec![],
            metadata: serde_json::Map::new(),
        };
        AgentSnapshot {
            installation,
            sessions: vec![session],
            resources: vec![resource],
            problems: BTreeMap::new(),
            completed_at: 1789000001000,
        }
    }

    #[test]
    fn snapshot_totals_resource_sizes() {
        let snap = fixture_snapshot();
        let total = snap.total_size();
        assert_eq!(total.logical_bytes, 4096);
        assert_eq!(total.confidence, SizeConfidence::Exact);
    }

    #[test]
    fn snapshot_total_propagates_unknown_confidence() {
        // One unmeasured resource must not produce a trustworthy-looking
        // total (§8 no-fake-precision; merge follows weakest contributor).
        let mut snap = fixture_snapshot();
        snap.resources.push(Resource {
            id: crate::resource::ResourceId::new("claude-code:default:session:unmeasured"),
            size: SizeInfo::unknown(),
            ..snap.resources[0].clone()
        });
        assert_eq!(snap.total_size().confidence, SizeConfidence::Unknown);
    }

    #[test]
    fn empty_snapshot_is_exactly_zero() {
        let snap = fixture_snapshot();
        let empty = AgentSnapshot {
            resources: vec![],
            ..snap
        };
        let total = empty.total_size();
        assert_eq!(total.logical_bytes, 0);
        assert_eq!(total.confidence, SizeConfidence::Exact);
    }

    #[test]
    fn snapshot_serde_roundtrip_supports_golden_tests() {
        let snap = fixture_snapshot();
        let json = serde_json::to_string_pretty(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn unused_topic_import_is_for_docs() {
        // CapabilityTopic is referenced by docs elsewhere; keep import used.
        let _ = CapabilityTopic::Sessions;
    }
}
