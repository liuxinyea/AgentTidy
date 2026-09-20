//! `agenttidy` — CLI for AgentTidy (v0.1, design doc §15).
//!
//! The CLI is the core validation and diagnostics surface. It shares the
//! same Application API as the GUI — it never implements its own business
//! logic.

use agenttidy_application::{create_registry, detect_all, inspect_installation, scan_installation};
use agenttidy_core::ScanOptions;
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "agenttidy",
    version,
    about = "Keep your AI agents tidy.",
    long_about = "AgentTidy — local-first AI agent session & storage manager.\nDefault: read-only. All cleanup is trash-first and fail-closed."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show providers, paths, versions, permissions, schema and capability status
    Doctor,
    /// Scan and show a space-usage summary
    Scan,
    /// List recognizable sessions
    Sessions,
    /// Generate (dry-run) or execute a cleanup plan
    Clean {
        /// Generate a plan without executing it
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => cmd_doctor().await,
        Command::Scan => cmd_scan().await,
        Command::Sessions => cmd_sessions().await,
        Command::Clean { dry_run } => cmd_clean(dry_run).await,
    }
}

/// `agenttidy doctor` — show detected installations and their status.
async fn cmd_doctor() -> Result<()> {
    let registry = create_registry();
    let installations = detect_all(&registry).await?;

    if installations.is_empty() {
        println!("No agent installations detected.");
        return Ok(());
    }

    println!("Detected installations:\n");
    for inst in &installations {
        println!("  {} [{}]", inst.id, inst.provider);
        println!("    Platform: {:?}", inst.platform);
        if let Some(ref v) = inst.version {
            println!("    Version:  {v}");
        }
        println!("    Status:   {:?}", inst.status);
        println!("    Roots:");
        for root in &inst.data_roots {
            println!("      {root}");
        }
        println!();
    }

    // Inspect each installation.
    println!("Inspection:\n");
    for inst in &installations {
        match inspect_installation(&registry, inst).await {
            Ok(inspection) => {
                println!("  {} [{}]", inst.id, inst.provider);
                if let Some(ref v) = inspection.version {
                    println!("    Version:        {v}");
                }
                if !inspection.schema_versions.is_empty() {
                    println!(
                        "    Schema versions: {}",
                        inspection
                            .schema_versions
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !inspection.journal_modes.is_empty() {
                    println!(
                        "    Journal modes:  {}",
                        inspection
                            .journal_modes
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                println!("    Agent running:  {}", inspection.agent_running);
                if !inspection.unknown_structures.is_empty() {
                    println!(
                        "    Unknown:        {}",
                        inspection.unknown_structures.join(", ")
                    );
                }
                if !inspection.problems.is_empty() {
                    println!("    Problems:");
                    for (code, problem) in &inspection.problems {
                        println!("      {code}: {problem:?}");
                    }
                }
                println!();
            }
            Err(e) => {
                println!("  {} [{}]", inst.id, inst.provider);
                println!("    Inspection failed: {e}");
                println!();
            }
        }
    }

    Ok(())
}

/// `agenttidy scan` — scan all installations and show space summary.
async fn cmd_scan() -> Result<()> {
    let registry = create_registry();
    let installations = detect_all(&registry).await?;

    if installations.is_empty() {
        println!("No agent installations detected.");
        return Ok(());
    }

    let options = ScanOptions::default();
    let mut total_logical: u64 = 0;
    let mut total_allocated: Option<u64> = Some(0);
    let mut total_sessions: usize = 0;
    let mut total_resources: usize = 0;

    for inst in &installations {
        match scan_installation(&registry, inst, &options).await {
            Ok(snapshot) => {
                let size = snapshot.total_size();
                println!("{} [{}]", inst.id, inst.provider);
                println!("  Sessions:   {}", snapshot.sessions.len());
                println!("  Resources:  {}", snapshot.resources.len());
                println!("  Logical:    {}", format_bytes(size.logical_bytes));
                if let Some(alloc) = size.allocated_bytes {
                    println!("  Allocated:  {}", format_bytes(alloc));
                }
                if !snapshot.problems.is_empty() {
                    println!("  Problems:   {}", snapshot.problems.len());
                }
                println!();

                total_logical += size.logical_bytes;
                total_allocated = merge_opt_u64(total_allocated, size.allocated_bytes);
                total_sessions += snapshot.sessions.len();
                total_resources += snapshot.resources.len();
            }
            Err(e) => {
                println!("{} [{}]", inst.id, inst.provider);
                println!("  Scan failed: {e}");
                println!();
            }
        }
    }

    println!("Total:");
    println!("  Sessions:   {total_sessions}");
    println!("  Resources:  {total_resources}");
    println!("  Logical:    {}", format_bytes(total_logical));
    if let Some(alloc) = total_allocated {
        println!("  Allocated:  {}", format_bytes(alloc));
    }

    Ok(())
}

/// `agenttidy sessions` — list all recognizable sessions.
async fn cmd_sessions() -> Result<()> {
    let registry = create_registry();
    let installations = detect_all(&registry).await?;

    if installations.is_empty() {
        println!("No agent installations detected.");
        return Ok(());
    }

    let options = ScanOptions::default();
    let mut all_sessions: Vec<(String, String, agenttidy_core::Session)> = Vec::new();

    for inst in &installations {
        match scan_installation(&registry, inst, &options).await {
            Ok(snapshot) => {
                for session in snapshot.sessions {
                    all_sessions.push((inst.provider.to_string(), inst.id.clone(), session));
                }
            }
            Err(e) => {
                eprintln!("warning: scan failed for {}: {e}", inst.id);
            }
        }
    }

    if all_sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    // Sort by provider, then by updated_at (newest first).
    all_sessions.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            b.2.updated_at
                .unwrap_or(0)
                .cmp(&a.2.updated_at.unwrap_or(0))
        })
    });

    println!(
        "{:<15} {:<40} {:<10} {:<20}",
        "PROVIDER", "SESSION", "SIZE", "PROJECT"
    );
    println!("{}", "-".repeat(90));

    for (provider, _inst_id, session) in &all_sessions {
        let project = session
            .project
            .as_ref()
            .and_then(|p| p.cwd.as_deref())
            .unwrap_or("-");
        let size = format_bytes(session.size.logical_bytes);
        let title = session.title.as_deref().unwrap_or(session.id.as_str());
        println!("{provider:<15} {title:<40} {size:<10} {project:<20}");
    }

    Ok(())
}

/// `agenttidy clean --dry-run` — generate a plan without executing.
async fn cmd_clean(dry_run: bool) -> Result<()> {
    if !dry_run {
        println!("Cleanup execution is not yet implemented (Phase 6).");
        println!("Use --dry-run to generate a plan without executing.");
        return Ok(());
    }

    let registry = create_registry();
    let installations = detect_all(&registry).await?;

    if installations.is_empty() {
        println!("No agent installations detected.");
        return Ok(());
    }

    let options = ScanOptions::default();
    println!("Cleanup plan (dry-run — no files will be modified):\n");

    for inst in &installations {
        match scan_installation(&registry, inst, &options).await {
            Ok(snapshot) => {
                let mut candidates = 0;
                let mut reclaimable: u64 = 0;

                // In Phase 3, we report sessions with deleted_at (soft-deleted)
                // as cleanup candidates. Real cleanup-unit logic lands in Phase 6.
                for session in &snapshot.sessions {
                    if session.lifecycle == agenttidy_core::SessionLifecycle::Inactive {
                        candidates += 1;
                        reclaimable += session.size.logical_bytes;
                    }
                }

                // Logs and caches are always candidates.
                for resource in &snapshot.resources {
                    match resource.kind {
                        agenttidy_core::ResourceKind::Log | agenttidy_core::ResourceKind::Cache => {
                            candidates += 1;
                            reclaimable += resource.size.logical_bytes;
                        }
                        _ => {}
                    }
                }

                if candidates > 0 {
                    println!(
                        "  {} [{}]: {} candidates, {} reclaimable",
                        inst.id,
                        inst.provider,
                        candidates,
                        format_bytes(reclaimable)
                    );
                }
            }
            Err(e) => {
                eprintln!("  {} [{}]: scan failed: {e}", inst.id, inst.provider);
            }
        }
    }

    println!("\n(CleanupPlan generation and execution is Phase 6 work.)");
    Ok(())
}

/// Format bytes into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Merge two optional u64 values (None if either is None).
fn merge_opt_u64(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        _ => None,
    }
}
