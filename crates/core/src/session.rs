//! Session model (`Start.md` §9.3).
//!
//! A `Session` is one conversation/run of an agent, as discovered by a
//! provider scan. Sessions are the primary user-facing concept: the UI
//! groups space by session, and cleanup plans ultimately operate on
//! session-scoped resources.

use crate::capability::CapabilityTopic;
use crate::provider::ProviderId;
use crate::resource::ResourceRef;
use crate::size::SizeInfo;
use serde::{Deserialize, Serialize};

/// Stable identifier of one session within a provider.
///
/// Newtype (not bare `String`) so provider-native id formats (Claude Code
/// UUIDv4, Codex UUIDv7, WorkBuddy UUIDv4) never leak as free strings.
/// Uniqueness is only meaningful *within* one provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reference to the project/workspace a session ran in.
///
/// Providers map projects differently (Claude Code: cwd slug dirs; Codex:
/// `projects` table; WorkBuddy: raw cwd only) — this neutral form records
/// what we know without claiming a project entity exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRef {
    /// Absolute working directory of the session, when known.
    ///
    /// This is the strongest cross-provider project signal (present in every
    /// provider's session metadata). Stored verbatim — never normalized,
    /// because user-chosen cwds may contain arbitrary Unicode and macOS
    /// paths are case-preserving (see docs/providers/workbuddy.md).
    pub cwd: Option<String>,
    /// Provider-side project name/title, if the provider has one.
    pub display_name: Option<String>,
}

impl ProjectRef {
    /// A project reference that only knows the cwd.
    pub fn from_cwd(cwd: impl Into<String>) -> Self {
        Self {
            cwd: Some(cwd.into()),
            display_name: None,
        }
    }
}

/// Lifecycle state of a session (`Start.md` §9.3).
///
/// The design doc rule: agents without an archive concept must not use
/// `Archived`. `Unknown` is fail-closed — cleanup treats it as Blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SessionLifecycle {
    /// Currently being written to (running process or very recent mtime).
    Active,
    /// Agent-side archived (WorkBuddy `status='archived'`, Codex
    /// `archived=1`). Archive is a *signal*, never cleanup permission
    /// (red line #5).
    Archived {
        /// When the agent archived it, if recorded (epoch ms, same
        /// convention as `created_at`/`updated_at` — §9.3 `archivedAt?`,
        /// needed by the §11.1 "archived + inactive > 30d" rule).
        archived_at: Option<u64>,
    },
    /// Finished, not archived, no recent activity.
    Inactive,
    /// State could not be determined — fail closed.
    Unknown,
}

/// A discovered agent session (read-only analysis view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Provider-native session id (see `SessionId`).
    pub id: SessionId,
    /// Owning provider.
    pub provider: ProviderId,
    /// Installation this session was scanned from.
    pub installation_id: String,
    /// Human-readable title, if the provider stores one.
    pub title: Option<String>,
    /// Project/workspace association.
    pub project: Option<ProjectRef>,
    /// Creation time (epoch ms, provider-reported or file-derived).
    pub created_at: Option<u64>,
    /// Last update time (epoch ms).
    pub updated_at: Option<u64>,
    /// Lifecycle classification.
    pub lifecycle: SessionLifecycle,
    /// Space accounting for this session's exclusive + shared resources.
    pub size: SizeInfo,
    /// Resources attributed to this session (transcript, sidecars, DB refs).
    pub resource_refs: Vec<ResourceRef>,
    /// Provider-specific facts (e.g. `{"source_mode": "craft"}` for
    /// WorkBuddy, `{"originator": "codex_vscode"}` for Codex). Facts only —
    /// never policy conclusions.
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl Session {
    /// Shorthand for the `CapabilityTopic::Sessions` topic constant —
    /// capability checks use this when deciding how to present a session.
    ///
    /// Whether an agent *has* an archive concept at all is likewise a
    /// provider-reported fact: providers that support archives report
    /// `CapabilityTopic::Archive`, and only they may emit
    /// `SessionLifecycle::Archived` (doc §9.3 rule). Core deliberately
    /// keeps no hardcoded provider list here — that would be a second
    /// source of truth that can drift from the capability reports.
    pub const CAPABILITY_TOPIC: CapabilityTopic = CapabilityTopic::Sessions;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(lifecycle: SessionLifecycle) -> Session {
        Session {
            id: SessionId::new("02e8fad4-0531-4a27-b5bb-6fa12019561c"),
            provider: ProviderId::new(ProviderId::CLAUDE_CODE),
            installation_id: "claude-code:default".into(),
            title: None,
            project: Some(ProjectRef::from_cwd("/Users/x/proj")),
            created_at: Some(1789000000000),
            updated_at: Some(1789000000000),
            lifecycle,
            size: SizeInfo::unknown(),
            resource_refs: vec![],
            metadata: serde_json::Map::new(),
        }
    }

    #[test]
    fn archive_support_is_provider_reported_not_hardcoded() {
        // §9.3 rule: whether an agent has an archive concept is a provider
        // capability (`CapabilityTopic::Archive`), not a core-side
        // hardcoded provider list — so no `supports_archive` function
        // exists here. This test pins the capability topic's existence
        // so the vocabulary cannot silently disappear.
        assert_ne!(CapabilityTopic::Archive, CapabilityTopic::Sessions);
    }

    #[test]
    fn lifecycle_serde_is_tagged() {
        let s = session(SessionLifecycle::Archived { archived_at: None });
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""state":"archived""#));
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.lifecycle,
            SessionLifecycle::Archived { archived_at: None }
        );
    }
}
