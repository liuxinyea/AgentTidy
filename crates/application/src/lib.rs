//! Application API — the single stable entry point shared by the desktop GUI
//! and the CLI (design doc `Start.md` §13):
//!
//! - Detect / Scan / List
//! - Plan / Validate / Execute
//!
//! Phase 4 wires the fixed v0.1 provider set here. Neither the GUI nor the
//! CLI may bypass this layer and touch providers or the filesystem directly.

use agenttidy_claude_code::ClaudeCodeAdapter;
use agenttidy_codex::CodexAdapter;
use agenttidy_core::{
    AgentCapabilities, AgentInstallation, AgentSnapshot, ProviderId, ProviderRegistry,
    ResourceKind, ScanOptions,
};
use agenttidy_provider_api::{AgentProviderAdapter, ProviderInspection};
use agenttidy_workbuddy::WorkBuddyAdapter;
use serde::Serialize;

/// Inspection facts and capabilities for one detected installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub installation: AgentInstallation,
    pub inspection: ProviderInspection,
    pub capabilities: AgentCapabilities,
}

/// Read-only result of applying the workspace-cleanup admission gates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceCleanupCandidate {
    pub installation_id: String,
    pub path: String,
    pub session_id: Option<String>,
    pub logical_bytes: u64,
    pub eligible: bool,
    pub blockers: Vec<String>,
}

/// The single shared application API for AgentTidy's current read-only flow.
pub struct Application {
    providers: ProviderRegistry<Box<dyn AgentProviderAdapter>>,
}

impl Application {
    /// Construct the static v0.1 registry. Duplicate ids are a programmer
    /// error caught at startup rather than a dynamically recoverable state.
    pub fn new() -> Self {
        let mut providers: ProviderRegistry<Box<dyn AgentProviderAdapter>> =
            ProviderRegistry::empty();
        providers
            .register(ProviderId::new(ProviderId::CODEX), Box::new(CodexAdapter))
            .expect("Codex registers once");
        providers
            .register(
                ProviderId::new(ProviderId::CLAUDE_CODE),
                Box::new(ClaudeCodeAdapter),
            )
            .expect("Claude Code registers once");
        providers
            .register(
                ProviderId::new(ProviderId::WORKBUDDY),
                Box::new(WorkBuddyAdapter),
            )
            .expect("WorkBuddy registers once");
        Self { providers }
    }

    /// Detect every known installation in stable provider order.
    pub async fn detect(&self) -> anyhow::Result<Vec<AgentInstallation>> {
        let mut installations = Vec::new();
        for (_, provider) in self.providers.iter() {
            installations.extend(provider.detect().await?);
        }
        Ok(installations)
    }

    /// Run the pre-scan diagnostic flow for every detected installation.
    pub async fn doctor(&self) -> anyhow::Result<Vec<DoctorReport>> {
        let mut reports = Vec::new();
        for installation in self.detect().await? {
            let provider = self
                .providers
                .get(&installation.provider)
                .expect("detected installation has a registered provider");
            let inspection = provider.inspect(&installation).await?;
            let capabilities = provider.capabilities(&inspection).await?;
            reports.push(DoctorReport {
                installation,
                inspection,
                capabilities,
            });
        }
        Ok(reports)
    }

    /// Scan every available installation with common read-only options.
    pub async fn scan(&self, options: &ScanOptions) -> anyhow::Result<Vec<AgentSnapshot>> {
        let mut snapshots = Vec::new();
        for installation in self.detect().await? {
            if !installation.is_available() {
                continue;
            }
            let provider = self
                .providers
                .get(&installation.provider)
                .expect("detected installation has a registered provider");
            snapshots.push(provider.scan(&installation, options).await?);
        }
        Ok(snapshots)
    }

    /// Produce a non-executable workspace preview. This is intentionally
    /// conservative: it does not create a CleanupPlan or mutate any path.
    pub async fn workspace_cleanup_candidates(
        &self,
    ) -> anyhow::Result<Vec<WorkspaceCleanupCandidate>> {
        let snapshots = self.scan(&ScanOptions::default()).await?;
        Ok(Self::workspace_cleanup_candidates_from_snapshots(
            &snapshots,
        ))
    }

    /// Apply workspace admission rules to already-scanned facts. Keeping this
    /// pure makes the conservative policy independently testable and lets
    /// future UI surfaces render the exact same preview as the CLI.
    pub fn workspace_cleanup_candidates_from_snapshots(
        snapshots: &[AgentSnapshot],
    ) -> Vec<WorkspaceCleanupCandidate> {
        let mut candidates = Vec::new();
        for snapshot in snapshots {
            for workspace in snapshot
                .resources
                .iter()
                .filter(|resource| resource.kind == ResourceKind::Workspace)
            {
                let path = match &workspace.locator {
                    agenttidy_core::ResourceLocator::Dir { path } => path.clone(),
                    _ => continue,
                };
                let mut blockers = Vec::new();
                if workspace
                    .metadata
                    .get("filesystem_eligible")
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
                {
                    blockers.push(
                        "workspace contains Git, links, unknown entries, or unreadable paths"
                            .into(),
                    );
                }
                let matches: Vec<_> = snapshot
                    .sessions
                    .iter()
                    .filter(|session| {
                        session
                            .project
                            .as_ref()
                            .and_then(|project| project.cwd.as_ref())
                            == Some(&path)
                    })
                    .collect();
                if matches.len() != 1 {
                    blockers.push(format!(
                        "requires exactly one session cwd match; found {}",
                        matches.len()
                    ));
                }
                // WorkBuddy has automations that can reference arbitrary cwd.
                // Its adapter does not yet expose that table's contract, so
                // absence cannot be proven and must block the candidate.
                if workspace.provider.as_str() == ProviderId::WORKBUDDY {
                    blockers.push("WorkBuddy automation references are not yet verified".into());
                }
                candidates.push(WorkspaceCleanupCandidate {
                    installation_id: snapshot.installation.id.clone(),
                    path,
                    session_id: matches
                        .first()
                        .map(|session| session.id.as_str().to_owned()),
                    logical_bytes: workspace.size.logical_bytes,
                    eligible: blockers.is_empty(),
                    blockers,
                });
            }
        }
        candidates
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenttidy_core::{
        AgentInstallation, ManagedBy, Ownership, Platform, ProjectRef, Resource, ResourceId,
        ResourceLocator, Session, SessionId, SessionLifecycle, SizeInfo,
    };
    use std::collections::BTreeMap;

    #[test]
    fn static_registry_contains_the_three_v01_providers() {
        let app = Application::new();
        assert_eq!(app.providers.len(), 3);
    }

    fn snapshot_with_workspace(
        provider: &str,
        workspace_path: &str,
        session_cwds: &[&str],
        filesystem_eligible: bool,
    ) -> AgentSnapshot {
        let provider_id = ProviderId::new(provider);
        let installation = AgentInstallation {
            id: format!("{provider}:default"),
            provider: provider_id.clone(),
            platform: Platform::Macos,
            version: None,
            data_roots: vec![workspace_path.into()],
            status: agenttidy_core::InstallationStatus::Available,
        };
        let sessions = session_cwds
            .iter()
            .enumerate()
            .map(|(index, cwd)| Session {
                id: SessionId::new(format!("session-{index}")),
                provider: provider_id.clone(),
                installation_id: installation.id.clone(),
                title: None,
                project: Some(ProjectRef::from_cwd(*cwd)),
                created_at: None,
                updated_at: None,
                lifecycle: SessionLifecycle::Inactive,
                size: SizeInfo::exact(0),
                resource_refs: vec![],
                metadata: serde_json::Map::new(),
            })
            .collect();
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            "filesystem_eligible".into(),
            serde_json::Value::Bool(filesystem_eligible),
        );
        AgentSnapshot {
            installation: installation.clone(),
            sessions,
            resources: vec![Resource {
                id: ResourceId::new(format!("{provider}:workspace")),
                provider: provider_id,
                installation_id: installation.id,
                kind: ResourceKind::Workspace,
                locator: ResourceLocator::Dir {
                    path: workspace_path.into(),
                },
                ownership: Ownership::Exclusive,
                managed_by: ManagedBy::User,
                size: SizeInfo::exact(42),
                session_id: None,
                project: None,
                created_at: None,
                updated_at: None,
                dependencies: vec![],
                metadata,
            }],
            problems: BTreeMap::new(),
            completed_at: 0,
        }
    }

    #[test]
    fn exact_safe_codex_workspace_is_preview_eligible() {
        let snapshot = snapshot_with_workspace(
            ProviderId::CODEX,
            "/Users/example/Documents/Codex/task-a",
            &["/Users/example/Documents/Codex/task-a"],
            true,
        );
        let candidates = Application::workspace_cleanup_candidates_from_snapshots(&[snapshot]);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].eligible);
        assert_eq!(candidates[0].session_id.as_deref(), Some("session-0"));
    }

    #[test]
    fn workbuddy_workspace_stays_blocked_until_automation_references_are_known() {
        let snapshot = snapshot_with_workspace(
            ProviderId::WORKBUDDY,
            "/Users/example/WorkBuddy/task-a",
            &["/Users/example/WorkBuddy/task-a"],
            true,
        );
        let candidates = Application::workspace_cleanup_candidates_from_snapshots(&[snapshot]);
        assert!(!candidates[0].eligible);
        assert!(candidates[0]
            .blockers
            .iter()
            .any(|blocker| blocker.contains("automation references")));
    }

    #[test]
    fn multiple_session_matches_are_blocked() {
        let snapshot = snapshot_with_workspace(
            ProviderId::CODEX,
            "/Users/example/Documents/Codex/task-a",
            &[
                "/Users/example/Documents/Codex/task-a",
                "/Users/example/Documents/Codex/task-a",
            ],
            true,
        );
        let candidates = Application::workspace_cleanup_candidates_from_snapshots(&[snapshot]);
        assert!(!candidates[0].eligible);
        assert!(candidates[0]
            .blockers
            .iter()
            .any(|blocker| blocker.contains("exactly one session")));
    }
}
