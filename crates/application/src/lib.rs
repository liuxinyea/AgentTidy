//! Application API — the single stable entry point shared by the desktop GUI
//! and the CLI (design doc `Start.md` §13):
//!
//! - Detect / Scan / List
//! - Plan / Validate / Execute
//!
//! This crate wires the three provider adapters with the core domain model
//! and infrastructure services. Neither the GUI nor the CLI may bypass
//! this layer and touch providers or the filesystem directly.

use agenttidy_core::{AgentInstallation, AgentSnapshot, ProviderId, ProviderRegistry};
use agenttidy_provider_api::AgentProviderAdapter;
use anyhow::Result;
use std::sync::Arc;

/// Create a provider registry with all three v0.1 providers.
///
/// This is the single place where concrete provider adapters are
/// registered. The registry is static (red line #10 — no dynamic
/// plugin system).
pub fn create_registry() -> ProviderRegistry<Arc<dyn AgentProviderAdapter>> {
    let mut reg = ProviderRegistry::empty();
    reg.register(
        ProviderId::new(ProviderId::WORKBUDDY),
        Arc::new(agenttidy_workbuddy::WorkBuddyAdapter) as Arc<dyn AgentProviderAdapter>,
    )
    .expect("duplicate provider registration");
    reg.register(
        ProviderId::new(ProviderId::CLAUDE_CODE),
        Arc::new(agenttidy_claude_code::ClaudeCodeAdapter) as Arc<dyn AgentProviderAdapter>,
    )
    .expect("duplicate provider registration");
    reg.register(
        ProviderId::new(ProviderId::CODEX),
        Arc::new(agenttidy_codex::CodexAdapter) as Arc<dyn AgentProviderAdapter>,
    )
    .expect("duplicate provider registration");
    reg
}

/// Detect all installations across all registered providers.
pub async fn detect_all(
    registry: &ProviderRegistry<Arc<dyn AgentProviderAdapter>>,
) -> Result<Vec<AgentInstallation>> {
    let mut installations = Vec::new();
    for (_id, adapter) in registry.iter() {
        match adapter.detect().await {
            Ok(mut found) => installations.append(&mut found),
            Err(e) => {
                // Detection failures are non-fatal — log and continue.
                eprintln!(
                    "warning: detection failed for {}: {e}",
                    adapter.display_name()
                );
            }
        }
    }
    Ok(installations)
}

/// Scan one installation into a read-only snapshot.
pub async fn scan_installation(
    registry: &ProviderRegistry<Arc<dyn AgentProviderAdapter>>,
    installation: &AgentInstallation,
    options: &agenttidy_core::ScanOptions,
) -> Result<AgentSnapshot> {
    let adapter = registry
        .get(&installation.provider)
        .ok_or_else(|| anyhow::anyhow!("no adapter for provider {}", installation.provider))?;
    adapter.scan(installation, options).await
}

/// Inspect one installation (pre-scan facts).
pub async fn inspect_installation(
    registry: &ProviderRegistry<Arc<dyn AgentProviderAdapter>>,
    installation: &AgentInstallation,
) -> Result<agenttidy_provider_api::ProviderInspection> {
    let adapter = registry
        .get(&installation.provider)
        .ok_or_else(|| anyhow::anyhow!("no adapter for provider {}", installation.provider))?;
    adapter.inspect(installation).await
}

/// Derive capabilities from an inspection result.
pub async fn capabilities_for(
    registry: &ProviderRegistry<Arc<dyn AgentProviderAdapter>>,
    installation: &AgentInstallation,
    inspection: &agenttidy_provider_api::ProviderInspection,
) -> Result<agenttidy_core::AgentCapabilities> {
    let adapter = registry
        .get(&installation.provider)
        .ok_or_else(|| anyhow::anyhow!("no adapter for provider {}", installation.provider))?;
    adapter.capabilities(inspection).await
}
