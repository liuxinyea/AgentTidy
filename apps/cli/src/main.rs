//! `agenttidy` — CLI for AgentTidy (v0.1, design doc §15).
//!
//! The CLI is the core validation and diagnostics surface. It shares the
//! same Application API as the GUI — it never implements its own business
//! logic. Subcommands are placeholders until the corresponding phases land.

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
    Clean,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor | Command::Scan | Command::Sessions | Command::Clean => {
            println!("not implemented yet");
            Ok(())
        }
    }
}
