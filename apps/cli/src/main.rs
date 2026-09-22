//! `agenttidy` — CLI for AgentTidy (v0.1, design doc §15).
//!
//! The CLI is the core validation and diagnostics surface. It shares the
//! Application API as the GUI and only renders its read-only results.

use agenttidy_application::Application;
use agenttidy_core::{CapabilityTopic, ScanOptions};
use clap::{Parser, Subcommand};
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, ContentArrangement, Table,
};
use serde::Serialize;
use std::io::Write;

/// Versioned machine-output contract. Human-readable output may evolve, but
/// scripts can pin to this identifier and reject newer incompatible formats.
const JSON_SCHEMA_VERSION: &str = "agenttidy.cli.v1";

/// Common envelope for every JSON command, so consumers never need to infer
/// a command or schema from the shape of a bare array.
#[derive(Serialize)]
struct JsonEnvelope<T> {
    schema_version: &'static str,
    command: &'static str,
    mode: &'static str,
    data: T,
}

fn json_output<T: Serialize>(command: &'static str, data: T) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(&JsonEnvelope {
        schema_version: JSON_SCHEMA_VERSION,
        command,
        mode: "read-only",
        data,
    })?)
}

/// Write one complete logical output line without panicking when a consumer
/// such as `head` exits early. Broken pipes are successful CLI termination.
fn write_stdout_line(args: std::fmt::Arguments<'_>) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    match writeln!(output, "{args}") {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Use the broken-pipe-safe output primitive for all user-facing lines.
macro_rules! outputln {
    ($($arg:tt)*) => {
        write_stdout_line(format_args!($($arg)*))?
    };
}

/// Build a consistent adaptive table for people using an interactive terminal.
/// The JSON contract deliberately bypasses this renderer for automation.
fn user_table(headers: &[&str]) -> Table {
    // CI and redirected output have no terminal size. A bounded fallback keeps
    // wide paths and diagnostics readable instead of producing one huge row.
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|width| *width >= 72)
        .unwrap_or(120);
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_width(width)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(
            headers
                .iter()
                .map(|header| Cell::new(*header).fg(Color::Cyan)),
        );
    table
}

/// Keep byte totals readable while preserving exact values in the JSON API.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn status_cell(value: impl ToString, healthy: bool) -> Cell {
    Cell::new(value).fg(if healthy { Color::Green } else { Color::Yellow })
}

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
    Doctor {
        /// Emit stable diagnostic data as JSON
        #[arg(long)]
        json: bool,
    },
    /// Scan and show a space-usage summary
    Scan {
        /// Emit the stable read-only snapshot contract as JSON
        #[arg(long)]
        json: bool,
    },
    /// List recognizable sessions
    Sessions {
        /// Emit recognized sessions as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let application = Application::new();
    match cli.command {
        Command::Doctor { json } => {
            let reports = application.doctor().await?;
            if json {
                outputln!("{}", json_output("doctor", reports)?);
                return Ok(());
            }
            if reports.is_empty() {
                outputln!("No supported agent installations detected.");
            }
            let mut table = user_table(&[
                "Installation",
                "Provider",
                "Status",
                "Running",
                "Sessions",
                "Projects",
                "Archive",
                "Diagnostics",
            ]);
            for report in reports {
                let diagnostics = report.inspection.problems.len();
                table.add_row(vec![
                    Cell::new(&report.installation.id),
                    Cell::new(report.installation.provider.as_str()),
                    status_cell(
                        format!("{:?}", report.installation.status),
                        report.installation.is_available(),
                    ),
                    status_cell(
                        if report.inspection.agent_running {
                            "yes"
                        } else {
                            "no"
                        },
                        !report.inspection.agent_running,
                    ),
                    Cell::new(format!(
                        "{:?}",
                        report.capabilities.get(CapabilityTopic::Sessions)
                    )),
                    Cell::new(format!(
                        "{:?}",
                        report.capabilities.get(CapabilityTopic::Projects)
                    )),
                    Cell::new(format!(
                        "{:?}",
                        report.capabilities.get(CapabilityTopic::Archive)
                    )),
                    status_cell(diagnostics, diagnostics == 0),
                ]);
            }
            if !table.is_empty() {
                outputln!("{table}");
            }
            Ok(())
        }
        Command::Scan { json } => {
            let snapshots = application.scan(&ScanOptions::default()).await?;
            if json {
                outputln!("{}", json_output("scan", snapshots)?);
                return Ok(());
            }
            if snapshots.is_empty() {
                outputln!("No available agent installations detected.");
            }
            let mut table = user_table(&[
                "Installation",
                "Sessions",
                "Resources",
                "Total size",
                "Workspace footprint",
                "Diagnostics",
            ]);
            for snapshot in snapshots {
                let size = snapshot.total_size();
                let workspaces: Vec<_> = snapshot
                    .resources
                    .iter()
                    .filter(|resource| resource.kind == agenttidy_core::ResourceKind::Workspace)
                    .collect();
                let workspace_bytes = workspaces
                    .iter()
                    .map(|resource| resource.size.logical_bytes)
                    .sum::<u64>();
                let diagnostics = snapshot.problems.len();
                table.add_row(vec![
                    Cell::new(snapshot.installation.id),
                    Cell::new(snapshot.sessions.len()),
                    Cell::new(snapshot.resources.len()),
                    Cell::new(format!(
                        "{} ({:?})",
                        human_bytes(size.logical_bytes),
                        size.confidence
                    )),
                    Cell::new(format!(
                        "{} / {}",
                        workspaces.len(),
                        human_bytes(workspace_bytes)
                    )),
                    status_cell(diagnostics, diagnostics == 0),
                ]);
            }
            if !table.is_empty() {
                outputln!("{table}");
            }
            Ok(())
        }
        Command::Sessions { json } => {
            let snapshots = application.scan(&ScanOptions::default()).await?;
            if json {
                let sessions: Vec<_> = snapshots
                    .into_iter()
                    .flat_map(|snapshot| snapshot.sessions)
                    .collect();
                outputln!("{}", json_output("sessions", sessions)?);
                return Ok(());
            }
            let mut table =
                user_table(&["Installation", "Session", "Lifecycle", "Size", "Workspace"]);
            for snapshot in snapshots {
                for session in snapshot.sessions {
                    let cwd = session
                        .project
                        .and_then(|project| project.cwd)
                        .unwrap_or_else(|| "<unknown cwd>".into());
                    table.add_row(vec![
                        Cell::new(&snapshot.installation.id),
                        Cell::new(session.id.as_str()),
                        Cell::new(format!("{:?}", session.lifecycle)),
                        Cell::new(human_bytes(session.size.logical_bytes)),
                        Cell::new(cwd),
                    ]);
                }
            }
            if table.is_empty() {
                outputln!("No recognizable sessions found.");
            } else {
                outputln!("{table}");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_envelope_has_a_versioned_read_only_contract() {
        let value: serde_json::Value =
            serde_json::from_str(&json_output("scan", vec!["snapshot"]).unwrap()).unwrap();
        assert_eq!(value["schema_version"], JSON_SCHEMA_VERSION);
        assert_eq!(value["command"], "scan");
        assert_eq!(value["mode"], "read-only");
        assert_eq!(value["data"][0], "snapshot");
    }
}
