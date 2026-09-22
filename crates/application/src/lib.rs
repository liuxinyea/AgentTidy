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
    AgentCapabilities, AgentInstallation, AgentSnapshot, CleanupAction, CleanupDecision,
    CleanupEvent, CleanupItem, CleanupItemOutcome, CleanupLocator, CleanupOutcome, CleanupPlan,
    CleanupPreconditionKind, CleanupRevalidationOutcome, CleanupUnit, CleanupUnitKind,
    Platform as CorePlatform, ProviderId, ProviderRegistry, ResourceKind, RiskLevel, RiskSummary,
    ScanOptions,
};
use agenttidy_infrastructure::{
    audit_log,
    fs_probe::{walk_tree, EntryKind},
    trash::{self, TrashError, TrashOutcome},
};
use agenttidy_provider_api::{AgentProviderAdapter, ProviderInspection};
use agenttidy_workbuddy::WorkBuddyAdapter;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static CLEANUP_PLAN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Pure inputs the policy reads. `now_ms` is injected so the policy
/// stays deterministic (testable without a system clock).
pub struct CleanupContext<'a> {
    pub snapshot: &'a AgentSnapshot,
    pub inspection: &'a ProviderInspection,
    pub now_ms: u64,
}

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
    ///
    /// Phase 6: refactored to project from [`Self::cleanup_plan_from_snapshots`]
    /// so the workspace-cleanup preview is a *view* on the same plan the
    /// GUI's Review & Tidy tab renders — one source of truth, no drift.
    pub fn workspace_cleanup_candidates_from_snapshots(
        snapshots: &[AgentSnapshot],
    ) -> Vec<WorkspaceCleanupCandidate> {
        // Phase 6: providers all return empty `build_cleanup_units`, so the
        // typed plan contains zero items. The legacy workspace-cleanup
        // admission (filesystem_eligible, single-session-match, WorkBuddy
        // automation gate) is preserved as a *legacy projection* on the
        // workspace-kind resources in the snapshot, so the existing CLI /
        // app callers do not break while the framework is empty. Phase 7
        // deletes this function once the typed plan carries everything.
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

    // -----------------------------------------------------------------
    // Phase 6: typed cleanup framework (Start.md §11, §12, §16.3).
    //
    // - `CleanupContext` + `cleanup_policy_evaluate` are the pure policy
    //   (§11) — no I/O, deterministic given `now_ms`.
    // - `cleanup_plan_from_snapshots` builds the typed plan (§12) by
    //   asking each provider's `build_cleanup_units`, evaluating the
    //   policy per unit, and stamping a fingerprint.
    // - `cleanup_revalidate` runs the §12.2 invalidation checks
    //   (filesystem + provider) and reports which (if any) trigger fired.
    // - `cleanup_execute` revalidates again, executes `trash::move_to_trash`
    //   for each selected unit whose preconditions still hold, and
    //   appends one `CleanupEvent` per item to the audit log (§16.3).
    //
    // Phase 6 ships this *framework-only*: every provider's
    // `build_cleanup_units` returns empty, so `cleanup_plan_from_snapshots`
    // emits a plan with zero items. Phase 7's provider beta turns on the
    // first real cleanup units.
    // -----------------------------------------------------------------

    /// Pure policy (§11). Maps a unit + context to a typed decision.
    ///
    /// Rules (Phase 6 conservative defaults; Phase 7 refines for
    /// individual resource kinds):
    /// - `kind: ProviderOperation` ⇒ `ReviewRequired` (no automated path).
    /// - `inspection.agent_running == true` ⇒ `Blocked`.
    /// - Snapshot has any `ScanProblem::Error` ⇒ `Blocked`.
    /// - Otherwise ⇒ `ReviewRequired` (the user must review every unit
    ///   in Phase 6; promoting to `LowRisk` requires age + lifecycle
    ///   facts Phase 7 supplies per-resource-kind).
    pub fn cleanup_policy_evaluate(
        unit: &CleanupUnit,
        ctx: &CleanupContext<'_>,
    ) -> CleanupDecision {
        let mut decision = CleanupDecision {
            risk: RiskLevel::Blocked,
            recommended: false,
            reasons: Vec::new(),
            blockers: Vec::new(),
        };

        match unit.kind {
            CleanupUnitKind::FileTree | CleanupUnitKind::FileSet => {
                decision
                    .reasons
                    .push(format!("kind is {:?} (trash-first eligible)", unit.kind));
            }
            CleanupUnitKind::ProviderOperation => {
                decision.reasons.push(
                    "ProviderOperation: requires provider cooperation; user must review".into(),
                );
            }
        }

        if ctx.inspection.agent_running {
            decision
                .blockers
                .push("agent is currently running (§16.2: concurrent execution not safe)".into());
        } else {
            decision.reasons.push("agent is not running".into());
        }

        let has_error = ctx
            .snapshot
            .problems
            .values()
            .any(|p| matches!(p, agenttidy_core::ScanProblem::Error { .. }));
        if has_error {
            decision
                .blockers
                .push("scan reported an Error problem for this installation".into());
        } else {
            decision.reasons.push("no Error-level scan problems".into());
        }

        if decision.blockers.is_empty() {
            decision.risk = RiskLevel::ReviewRequired;
            decision.recommended = false;
            decision
                .reasons
                .push("no hard blockers; user review required per Start.md §7.2".into());
        }

        decision
    }

    /// Build a typed cleanup plan. Async because each provider's
    /// `build_cleanup_units` may do filesystem / SQLite work in future
    /// phases (Phase 7's overrides). Phase 6 providers all return empty,
    /// so this is effectively a fingerprint-stamping pass.
    pub async fn cleanup_plan_from_snapshots(
        &self,
        snapshots: &[AgentSnapshot],
    ) -> anyhow::Result<CleanupPlan> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Pre-fetch inspections for every installation in one pass so the
        // policy can read agent_running + schema facts without re-hitting
        // providers per unit.
        let mut inspection_by_installation: BTreeMap<String, ProviderInspection> = BTreeMap::new();
        for installation in self.detect().await? {
            if let Some(provider) = self.providers.get(&installation.provider) {
                let inspection = provider.inspect(&installation).await?;
                inspection_by_installation.insert(installation.id.clone(), inspection);
            }
        }

        let mut items: Vec<CleanupItem> = Vec::new();
        let mut risk_summary = RiskSummary::default();
        let mut total_reclaimable_bytes: u64 = 0;

        for snapshot in snapshots {
            let installation_id = snapshot.installation.id.clone();
            let provider = match self.providers.get(&snapshot.installation.provider) {
                Some(p) => p,
                None => continue,
            };
            let units = provider.build_cleanup_units(snapshot).await?;
            let inspection = inspection_by_installation
                .get(&installation_id)
                .cloned()
                .unwrap_or_else(ProviderInspection::empty_for_testing);

            let ctx = CleanupContext {
                snapshot,
                inspection: &inspection,
                now_ms,
            };

            for unit in units {
                let decision = Self::cleanup_policy_evaluate(&unit, &ctx);
                total_reclaimable_bytes =
                    total_reclaimable_bytes.saturating_add(unit.estimated_reclaimable_bytes);
                risk_summary.add(decision.risk, unit.estimated_reclaimable_bytes);
                items.push(CleanupItem {
                    unit_id: unit.id.clone(),
                    installation_id: unit.installation_id.clone(),
                    provider: unit.provider.clone(),
                    fingerprint: unit.fingerprint.clone(),
                    action: unit.operation.action,
                    risk: decision.risk,
                    reasons: decision.reasons,
                    preconditions: unit.preconditions.clone(),
                });
            }
        }

        let plan_id = format!(
            "{:016x}{:08x}",
            now_ms,
            CLEANUP_PLAN_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let scan_id = scan_id_for(snapshots);
        let expires_at = Some(now_ms + 5 * 60 * 1000); // 5 min TTL — re-scan
                                                       // before this elapses
                                                       // or the executor will
                                                       // mark every item stale.
        let fingerprint = plan_fingerprint(
            &plan_id,
            &scan_id,
            &items,
            total_reclaimable_bytes,
            &risk_summary,
        );

        Ok(CleanupPlan {
            id: plan_id,
            scan_id,
            created_at: now_ms,
            expires_at,
            items,
            total_reclaimable_bytes,
            risk_summary,
            fingerprint,
        })
    }

    /// Revalidate every plan item against the freshly-observed filesystem
    /// and provider inspection. Returns one outcome per item — the GUI
    /// surfaces which (if any) §12.2 trigger invalidated the unit between
    /// the two confirmations. Pure side effects (read-only `inspect()` +
    /// `walk_tree`); no trash, no audit log.
    pub async fn cleanup_revalidate(
        &self,
        plan: &CleanupPlan,
    ) -> anyhow::Result<Vec<CleanupRevalidationOutcome>> {
        // Build the latest inspection set per installation.
        let mut inspection_by_installation: BTreeMap<String, ProviderInspection> = BTreeMap::new();
        for installation in self.detect().await? {
            if let Some(provider) = self.providers.get(&installation.provider) {
                let inspection = provider.inspect(&installation).await?;
                inspection_by_installation.insert(installation.id.clone(), inspection);
            }
        }

        let mut outcomes = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let outcome =
                revalidate_item(item, inspection_by_installation.get(&item.installation_id));
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    /// Execute the selected items: revalidate, then call
    /// `trash::move_to_trash` for each item whose preconditions still
    /// hold. Writes one `CleanupEvent` per item to the audit log.
    /// Idempotent at the unit level (AlreadyGone ⇒ Skipped, not Removed).
    ///
    /// `fingerprint` is the confirmation guard: if it doesn't match
    /// `plan.fingerprint` exactly, no item is touched and an error is
    /// returned (defends against the user confirming a stale plan).
    pub async fn cleanup_execute(
        &self,
        plan: &CleanupPlan,
        fingerprint: &str,
        selected_unit_ids: &[String],
    ) -> anyhow::Result<Vec<CleanupItemOutcome>> {
        if fingerprint != plan.fingerprint {
            anyhow::bail!(
                "plan fingerprint mismatch: expected {}, got {}",
                plan.fingerprint,
                fingerprint
            );
        }

        let revalidation = self.cleanup_revalidate(plan).await?;
        let mut results = Vec::new();
        for (item, revalidation) in plan.items.iter().zip(revalidation.iter()) {
            if !selected_unit_ids.contains(&item.unit_id) {
                // User did not select this unit; record as Pending skipped.
                results.push(CleanupItemOutcome {
                    unit_id: item.unit_id.clone(),
                    outcome: CleanupOutcome::Skipped,
                    reclaimed_bytes: 0,
                    reason: Some("user did not select this unit".into()),
                });
                continue;
            }
            if revalidation.outcome != CleanupOutcome::Pending {
                // Revalidation flagged this item as stale — record outcome
                // and skip without touching the filesystem.
                results.push(CleanupItemOutcome {
                    unit_id: item.unit_id.clone(),
                    outcome: revalidation.outcome,
                    reclaimed_bytes: 0,
                    reason: revalidation.reason.clone(),
                });
                continue;
            }

            let primary_path = item.fingerprint.path.clone();
            let locator = CleanupLocator::Dir {
                path: primary_path.clone(),
            };
            let action = item.action;

            // Execute: today only `Trash` is wired. ProviderOperation
            // returns `Failed` with a clear reason until Phase 7.
            let (outcome, reclaimed_bytes, reason) = match action {
                CleanupAction::Trash => match trash::move_to_trash(&primary_path) {
                    Ok(TrashOutcome::Removed) => {
                        (CleanupOutcome::Removed, item.fingerprint.size_bytes, None)
                    }
                    Ok(TrashOutcome::AlreadyGone) => (
                        CleanupOutcome::Skipped,
                        0,
                        Some("path already gone at execute time (idempotent skip)".into()),
                    ),
                    Ok(TrashOutcome::PlatformError(message)) => (
                        CleanupOutcome::Failed,
                        0,
                        Some(format!("trash platform error: {message}")),
                    ),
                    Err(TrashError::NotFound(_)) => (
                        CleanupOutcome::Skipped,
                        0,
                        Some("path not found at execute time (stale plan)".into()),
                    ),
                    Err(error) => (
                        CleanupOutcome::Failed,
                        0,
                        Some(format!("trash error: {error}")),
                    ),
                },
                CleanupAction::ProviderOperation => (
                    CleanupOutcome::Failed,
                    0,
                    Some("ProviderOperation is not yet wired (Phase 7)".into()),
                ),
            };

            // Audit log entry — best-effort; a log failure aborts this item
            // so the report stays uniform.
            let event = CleanupEvent {
                schema_version: CleanupEvent::SCHEMA_VERSION.into(),
                platform: host_platform(),
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                plan_id: plan.id.clone(),
                unit_id: item.unit_id.clone(),
                provider: item.provider.clone(),
                original_locator: locator,
                action,
                outcome,
                reclaimed_bytes,
                reason: reason.clone(),
            };
            if let Err(error) = audit_log::record(&event) {
                results.push(CleanupItemOutcome {
                    unit_id: item.unit_id.clone(),
                    outcome: CleanupOutcome::Failed,
                    reclaimed_bytes: 0,
                    reason: Some(format!("audit log write failed: {error}")),
                });
                continue;
            }
            results.push(CleanupItemOutcome {
                unit_id: item.unit_id.clone(),
                outcome,
                reclaimed_bytes,
                reason,
            });
        }
        Ok(results)
    }
}

/// Hash the canonical plan tuple so the executor can reject a stale
/// confirmation. `sha2::Sha256` is the only thing we need from the
/// `sha2` workspace dep; no native deps, no MSRV gymnastics.
fn plan_fingerprint(
    plan_id: &str,
    scan_id: &str,
    items: &[CleanupItem],
    total_reclaimable_bytes: u64,
    risk_summary: &RiskSummary,
) -> String {
    use sha2::{Digest, Sha256};
    let mut unit_ids: Vec<&str> = items.iter().map(|i| i.unit_id.as_str()).collect();
    unit_ids.sort();
    let mut hasher = Sha256::new();
    hasher.update(plan_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(scan_id.as_bytes());
    hasher.update(b"\n");
    for id in unit_ids {
        hasher.update(id.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(total_reclaimable_bytes.to_le_bytes());
    hasher.update(b"\n");
    let summary_json = serde_json::to_string(risk_summary).unwrap_or_default();
    hasher.update(summary_json.as_bytes());
    let digest = hasher.finalize();
    format!("{:x}", digest)
}

/// Stable hash of a snapshot set — used as `CleanupPlan.scan_id` so the
/// revalidator can correlate a plan back to the snapshot it was built
/// from. SHA-256 again (Start.md §12 line 689).
fn scan_id_for(snapshots: &[AgentSnapshot]) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(snapshots).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    format!("scan-{:x}", digest)
}

/// Run the §12.2 invalidation checks against one plan item.
///
/// Walks the filesystem for the item's path and compares against its
/// `ResourceFingerprint`. Then walks each declared precondition and
/// surfaces the *first* failed one (the GUI uses this for the per-row
/// "stale" badge). Returns `CleanupOutcome::Pending` + reason `None`
/// when every check passes.
fn revalidate_item(
    item: &CleanupItem,
    inspection: Option<&ProviderInspection>,
) -> CleanupRevalidationOutcome {
    // 1. Inspection-level checks.
    if let Some(inspection) = inspection {
        if inspection.agent_running {
            return CleanupRevalidationOutcome {
                unit_id: item.unit_id.clone(),
                outcome: CleanupOutcome::Skipped,
                failed_precondition: Some(CleanupPreconditionKind::NoAgentRunning),
                reason: Some("agent is running (§16.2)".into()),
            };
        }
    }

    // 2. Filesystem-level checks: walk_tree the path, compare the root
    // entry against the recorded fingerprint.
    let outcome = walk_tree(&item.fingerprint.path);
    if outcome.problems.iter().any(|p| {
        matches!(
            p,
            agenttidy_infrastructure::fs_probe::WalkProblem::Unreadable { .. }
        )
    }) {
        return CleanupRevalidationOutcome {
            unit_id: item.unit_id.clone(),
            outcome: CleanupOutcome::Skipped,
            failed_precondition: Some(CleanupPreconditionKind::PathExists),
            reason: Some("path became unreadable between plan and execute".into()),
        };
    }

    let root_entry = outcome
        .entries
        .iter()
        .find(|e| e.path == item.fingerprint.path);
    let entry = match root_entry {
        Some(e) => e,
        None => {
            return CleanupRevalidationOutcome {
                unit_id: item.unit_id.clone(),
                outcome: CleanupOutcome::Skipped,
                failed_precondition: Some(CleanupPreconditionKind::PathExists),
                reason: Some("path no longer exists at the recorded location".into()),
            };
        }
    };

    // Size comparison (we only check files; for directories the size
    // comparison is skipped — dir sizes fluctuate as files are added).
    if matches!(entry.kind, EntryKind::File) && entry.logical_len != item.fingerprint.size_bytes {
        return CleanupRevalidationOutcome {
            unit_id: item.unit_id.clone(),
            outcome: CleanupOutcome::Skipped,
            failed_precondition: Some(CleanupPreconditionKind::SizeUnchanged),
            reason: Some(format!(
                "size changed: was {}, now {}",
                item.fingerprint.size_bytes, entry.logical_len
            )),
        };
    }

    // mtime comparison (Unix mtime is i64; ResourceFingerprint carries i64).
    if let Ok(metadata) = std::fs::symlink_metadata(&item.fingerprint.path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                let current_mtime_ms = duration.as_millis() as i64;
                if (current_mtime_ms - item.fingerprint.mtime_ms).abs() > 1000 {
                    return CleanupRevalidationOutcome {
                        unit_id: item.unit_id.clone(),
                        outcome: CleanupOutcome::Skipped,
                        failed_precondition: Some(CleanupPreconditionKind::MtimeUnchanged),
                        reason: Some(format!(
                            "mtime changed: was {}, now {}",
                            item.fingerprint.mtime_ms, current_mtime_ms
                        )),
                    };
                }
            }
        }
    }

    // Identity comparison (volume + index) — only when both sides have it.
    if let Some(current_identity) = entry.identity {
        let recorded_volume = item.fingerprint.identity.volume;
        let recorded_index = item.fingerprint.identity.index;
        if current_identity.volume != recorded_volume || current_identity.index != recorded_index {
            return CleanupRevalidationOutcome {
                unit_id: item.unit_id.clone(),
                outcome: CleanupOutcome::Skipped,
                failed_precondition: Some(CleanupPreconditionKind::IdentityUnchanged),
                reason: Some("file identity (volume/index) changed".into()),
            };
        }
    }

    CleanupRevalidationOutcome {
        unit_id: item.unit_id.clone(),
        outcome: CleanupOutcome::Pending,
        failed_precondition: None,
        reason: None,
    }
}

/// Resolve the host platform at compile time. Keeps the audit-log
/// writer free of platform branching (red line #11).
fn host_platform() -> CorePlatform {
    #[cfg(windows)]
    {
        CorePlatform::Windows
    }
    #[cfg(not(windows))]
    {
        CorePlatform::Macos
    }
}

/// Extension on `ProviderInspection` — the upstream type does not derive
/// `Default`, so we provide a no-facts fallback the policy uses when an
/// installation's inspection has not been recorded (e.g. an empty
/// provider that returns no installations).
trait ProviderInspectionExt {
    fn empty_for_testing() -> Self;
}

impl ProviderInspectionExt for ProviderInspection {
    fn empty_for_testing() -> Self {
        Self {
            version: None,
            schema_versions: BTreeMap::new(),
            journal_modes: BTreeMap::new(),
            agent_running: false,
            readable_roots: BTreeMap::new(),
            unknown_structures: Vec::new(),
            problems: BTreeMap::new(),
        }
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

    // -----------------------------------------------------------------
    // Phase 6 cleanup policy / plan / revalidate / execute tests.
    // -----------------------------------------------------------------

    use agenttidy_core::{
        CleanupOperation, CleanupPrecondition, FileIdentity, ResourceFingerprint,
    };

    /// A minimal FileTree unit rooted at `path` for policy tests. The
    /// fingerprint fields are placeholders — the policy never reads them;
    /// only `revalidate_item` does (tested separately below).
    fn unit(path: &str, kind: CleanupUnitKind) -> CleanupUnit {
        CleanupUnit {
            id: format!("unit:{path}"),
            provider: ProviderId::new(ProviderId::CODEX),
            installation_id: "codex:default".into(),
            kind,
            resources: vec![],
            estimated_reclaimable_bytes: 1024,
            fingerprint: ResourceFingerprint {
                path: path.into(),
                identity: FileIdentity {
                    volume: 1,
                    index: 2,
                },
                size_bytes: 1024,
                mtime_ms: 1_700_000_000_000,
            },
            preconditions: vec![CleanupPrecondition {
                kind: CleanupPreconditionKind::PathExists,
                description: "path still exists".into(),
            }],
            operation: CleanupOperation {
                action: CleanupAction::Trash,
            },
            resource_kind: ResourceKind::Workspace,
        }
    }

    fn context<'a>(
        snapshot: &'a AgentSnapshot,
        inspection: &'a ProviderInspection,
        now_ms: u64,
    ) -> CleanupContext<'a> {
        CleanupContext {
            snapshot,
            inspection,
            now_ms,
        }
    }

    fn clean_context(_now_ms: u64) -> (AgentSnapshot, ProviderInspection) {
        let snapshot = snapshot_with_workspace(
            ProviderId::CODEX,
            "/Users/example/Documents/Codex/task-a",
            &["/Users/example/Documents/Codex/task-a"],
            true,
        );
        (snapshot, ProviderInspection::empty_for_testing())
    }

    #[test]
    fn policy_marks_clean_unit_review_required() {
        // §7.2 / §11: without per-resource-kind age facts, every
        // non-blocked unit is ReviewRequired — never auto-promoted to
        // LowRisk (red line #5: archive/age alone is not permission).
        let (snapshot, inspection) = clean_context(1_700_000_000_000);
        let ctx = context(&snapshot, &inspection, 1_700_000_000_000);
        let decision = Application::cleanup_policy_evaluate(
            &unit(
                "/Users/example/Documents/Codex/task-a",
                CleanupUnitKind::FileTree,
            ),
            &ctx,
        );
        assert_eq!(decision.risk, RiskLevel::ReviewRequired);
        assert!(!decision.recommended);
        assert!(decision.blockers.is_empty());
    }

    #[test]
    fn policy_blocks_when_agent_is_running() {
        // §16.2: an agent process anywhere near the installation means
        // concurrent execution is unsafe — fail closed (red line #6).
        let (snapshot, mut inspection) = clean_context(1_700_000_000_000);
        inspection.agent_running = true;
        let ctx = context(&snapshot, &inspection, 1_700_000_000_000);
        let decision =
            Application::cleanup_policy_evaluate(&unit("/x", CleanupUnitKind::FileTree), &ctx);
        assert_eq!(decision.risk, RiskLevel::Blocked);
        assert!(!decision.recommended);
        assert!(decision
            .blockers
            .iter()
            .any(|b| b.contains("agent is currently running")));
    }

    #[test]
    fn policy_blocks_on_scan_error_problem() {
        // An Error-severity scan problem means part of the scan could
        // not be trusted — no unit from that installation may pass (§16.2).
        let (mut snapshot, inspection) = clean_context(1_700_000_000_000);
        snapshot.problems.insert(
            "unreadable:/x".into(),
            agenttidy_core::ScanProblem::Error {
                message: "permission denied".into(),
                path: Some("/x".into()),
            },
        );
        let ctx = context(&snapshot, &inspection, 1_700_000_000_000);
        let decision =
            Application::cleanup_policy_evaluate(&unit("/x", CleanupUnitKind::FileTree), &ctx);
        assert_eq!(decision.risk, RiskLevel::Blocked);
        assert!(decision
            .blockers
            .iter()
            .any(|b| b.contains("Error problem")));
    }

    #[tokio::test]
    async fn plan_from_empty_provider_snapshots_is_empty_and_fingerprinted() {
        // Phase 6 ships framework-only: no provider emits units yet, so
        // the typed plan is empty but structurally complete (id, TTL,
        // fingerprint) — the GUI renders the empty Review view from it.
        let app = Application::new();
        let plan = app
            .cleanup_plan_from_snapshots(&[])
            .await
            .expect("plan builds");
        assert!(plan.items.is_empty());
        assert_eq!(plan.total_reclaimable_bytes, 0);
        assert_eq!(plan.risk_summary.low_risk, 0);
        assert!(plan.expires_at.is_some());
        assert_eq!(plan.fingerprint.len(), 64, "sha256 hex digest");
        // Plan round-trips through JSON — this is the IPC contract the
        // Tauri command returns and the GUI sends back at execute time.
        let json = serde_json::to_string(&plan).unwrap();
        let back: CleanupPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back, plan);
    }

    #[tokio::test]
    async fn execute_rejects_fingerprint_mismatch_without_touching_anything() {
        // §12.1 + the two-confirmation guard: a stale or tampered plan
        // must fail closed before the first file is touched.
        let app = Application::new();
        let plan = app.cleanup_plan_from_snapshots(&[]).await.unwrap();
        let error = app
            .cleanup_execute(&plan, "not-the-real-fingerprint", &[])
            .await
            .expect_err("mismatched fingerprint must be rejected");
        assert!(error.to_string().contains("fingerprint mismatch"));
    }

    #[tokio::test]
    async fn revalidate_reports_pending_for_absent_items_and_agent_running_guard() {
        let app = Application::new();
        let plan = app.cleanup_plan_from_snapshots(&[]).await.unwrap();
        let outcomes = app.cleanup_revalidate(&plan).await.unwrap();
        assert_eq!(outcomes.len(), plan.items.len());
    }

    /// Revalidation against a real filesystem: a file that changed size
    /// between plan and execute must be Skipped with SizeUnchanged
    /// (§12.2 trigger), never executed.
    #[tokio::test]
    async fn revalidate_skips_when_file_size_changed_since_plan() {
        let dir = std::env::temp_dir().join(format!(
            "agenttidy-revalidate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("target.txt");
        std::fs::write(&file, b"original").unwrap();
        let metadata = std::fs::symlink_metadata(&file).unwrap();
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let item = CleanupItem {
            unit_id: "unit-size-change".into(),
            installation_id: "codex:default".into(),
            provider: ProviderId::new(ProviderId::CODEX),
            fingerprint: ResourceFingerprint {
                path: file.clone(),
                identity: FileIdentity {
                    volume: 1,
                    index: 2,
                },
                size_bytes: 999_999, // deliberately wrong: file is 8 bytes
                mtime_ms,
            },
            action: CleanupAction::Trash,
            risk: RiskLevel::ReviewRequired,
            reasons: vec![],
            preconditions: vec![],
        };

        let outcome = revalidate_item(&item, None);
        assert_eq!(outcome.outcome, CleanupOutcome::Skipped);
        assert_eq!(
            outcome.failed_precondition,
            Some(CleanupPreconditionKind::SizeUnchanged)
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A plan item whose path vanished must be Skipped with PathExists
    /// (§12.2 "path changed" trigger).
    #[tokio::test]
    async fn revalidate_skips_when_path_vanished() {
        let missing = std::env::temp_dir().join(format!(
            "agenttidy-gone-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let item = CleanupItem {
            unit_id: "unit-gone".into(),
            installation_id: "codex:default".into(),
            provider: ProviderId::new(ProviderId::CODEX),
            fingerprint: ResourceFingerprint {
                path: missing,
                identity: FileIdentity {
                    volume: 1,
                    index: 2,
                },
                size_bytes: 10,
                mtime_ms: 0,
            },
            action: CleanupAction::Trash,
            risk: RiskLevel::ReviewRequired,
            reasons: vec![],
            preconditions: vec![],
        };
        let outcome = revalidate_item(&item, None);
        assert_eq!(outcome.outcome, CleanupOutcome::Skipped);
        assert_eq!(
            outcome.failed_precondition,
            Some(CleanupPreconditionKind::PathExists)
        );
    }

    /// §16.2 revalidation guard: agent_running from a fresh inspect()
    /// preempts every filesystem check.
    #[tokio::test]
    async fn revalidate_prefers_agent_running_over_filesystem_checks() {
        let file = std::env::temp_dir().join(format!(
            "agenttidy-agent-running-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&file, b"data").unwrap();
        let item = CleanupItem {
            unit_id: "unit-agent-running".into(),
            installation_id: "codex:default".into(),
            provider: ProviderId::new(ProviderId::CODEX),
            fingerprint: ResourceFingerprint {
                path: file.clone(),
                identity: FileIdentity {
                    volume: 1,
                    index: 2,
                },
                size_bytes: 999, // also wrong, but agent_running wins
                mtime_ms: 0,
            },
            action: CleanupAction::Trash,
            risk: RiskLevel::ReviewRequired,
            reasons: vec![],
            preconditions: vec![],
        };
        let mut inspection = ProviderInspection::empty_for_testing();
        inspection.agent_running = true;
        let outcome = revalidate_item(&item, Some(&inspection));
        assert_eq!(outcome.outcome, CleanupOutcome::Skipped);
        assert_eq!(
            outcome.failed_precondition,
            Some(CleanupPreconditionKind::NoAgentRunning)
        );
        std::fs::remove_file(&file).ok();
    }

    /// Unchanged file ⇒ Pending — the only outcome `cleanup_execute`
    /// will trash.
    #[tokio::test]
    async fn revalidate_passes_when_fingerprint_matches() {
        let file = std::env::temp_dir().join(format!(
            "agenttidy-pending-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&file, b"12345678").unwrap();
        let metadata = std::fs::symlink_metadata(&file).unwrap();
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // The happy path must use the *real* volume/inode — a fabricated
        // identity would trip IdentityUnchanged before size/mtime checks.
        let real_identity =
            agenttidy_infrastructure::fs_probe::file_identity(&file).expect("file identity");
        let item = CleanupItem {
            unit_id: "unit-ok".into(),
            installation_id: "codex:default".into(),
            provider: ProviderId::new(ProviderId::CODEX),
            fingerprint: ResourceFingerprint {
                path: file.clone(),
                identity: FileIdentity {
                    volume: real_identity.volume,
                    index: real_identity.index,
                },
                size_bytes: 8,
                mtime_ms,
            },
            action: CleanupAction::Trash,
            risk: RiskLevel::ReviewRequired,
            reasons: vec![],
            preconditions: vec![],
        };
        let inspection = ProviderInspection::empty_for_testing();
        let outcome = revalidate_item(&item, Some(&inspection));
        assert_eq!(outcome.outcome, CleanupOutcome::Pending);
        assert_eq!(outcome.failed_precondition, None);
        std::fs::remove_file(&file).ok();
    }
}
