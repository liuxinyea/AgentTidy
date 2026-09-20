//! Agent installation model (`Start.md` §9.1).
//!
//! An `AgentInstallation` is one *detected instance* of an agent product on
//! this machine: where its data lives and whether we can work with it.
//! Detection is a provider concern; this type is the neutral result.

use crate::provider::ProviderId;
use serde::{Deserialize, Serialize};

/// Operating system AgentTidy itself runs on (v0.1 supports only these two).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
}

/// Whether an installation is usable for analysis.
///
/// Variants are declared worst-to-best and `Ord` follows declaration order,
/// matching how far the installation got through inspection:
/// `available > permission_required > unsupported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallationStatus {
    /// Version or structure is outside the supported matrix — analysis off.
    Unsupported,
    /// Data exists but AgentTidy lacks read permission for at least one root.
    PermissionRequired,
    /// All data roots readable; full read-only analysis possible.
    Available,
}

/// A detected agent installation on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInstallation {
    /// Unique id for this installation (provider-scoped, stable per scan).
    pub id: String,
    /// Which agent product this is.
    pub provider: ProviderId,
    /// Host platform (red line #11: platform facts only, no platform code).
    pub platform: Platform,
    /// Agent version if discovered during detection, else absent.
    ///
    /// E.g. Claude Code `2.1.235`, Codex `0.152.0`, WorkBuddy `5.5.6`.
    pub version: Option<String>,
    /// All data roots this installation writes to — state roots *and*
    /// default workspace roots (e.g. Codex `~/.codex` + `~/Documents/Codex`),
    /// since providers split state across multiple locations.
    pub data_roots: Vec<String>,
    /// Overall usability of this installation.
    pub status: InstallationStatus,
}

impl AgentInstallation {
    /// True when the installation can be scanned for sessions and resources.
    pub fn is_available(&self) -> bool {
        self.status == InstallationStatus::Available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(status: InstallationStatus) -> AgentInstallation {
        AgentInstallation {
            id: "claude-code:default".into(),
            provider: ProviderId::new(ProviderId::CLAUDE_CODE),
            platform: Platform::Macos,
            version: Some("2.1.235".into()),
            data_roots: vec!["/Users/x/.claude".into()],
            status,
        }
    }

    #[test]
    fn availability_reflects_status() {
        assert!(install(InstallationStatus::Available).is_available());
        assert!(!install(InstallationStatus::PermissionRequired).is_available());
        assert!(!install(InstallationStatus::Unsupported).is_available());
    }

    #[test]
    fn status_ordering_matches_inspection_progress() {
        // available > permission_required > unsupported, as documented —
        // max() picks the installation that got furthest.
        assert!(InstallationStatus::Available > InstallationStatus::PermissionRequired);
        assert!(InstallationStatus::PermissionRequired > InstallationStatus::Unsupported);
    }

    #[test]
    fn multiple_data_roots_survive_roundtrip() {
        // Providers like Codex split state across roots; serde must keep all.
        let i = AgentInstallation {
            data_roots: vec!["~/.codex".into(), "~/Documents/Codex".into()],
            ..install(InstallationStatus::Available)
        };
        let json = serde_json::to_string(&i).unwrap();
        assert!(json.contains("Documents/Codex"));
        let back: AgentInstallation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.data_roots.len(), 2);
    }
}
