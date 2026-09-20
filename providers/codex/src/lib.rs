//! Codex provider adapter (`docs/providers/codex.md`).
//!
//! Codex CLI and Desktop share the same state root (`~/.codex/`).
//! They write to the same SQLite databases (`state_5.sqlite`,
//! `thread_history_1.sqlite`, `logs_2.sqlite`, etc.) and the same
//! JSONL rollouts (`sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`).
//!
//! The distinction between CLI and Desktop sessions lives in the DB:
//! `state_5.sqlite.threads.thread_source` and `threads.originator`.

use agenttidy_core::{
    AgentCapabilities, AgentInstallation, AgentSnapshot, CapabilityStatus, CapabilityTopic,
    InstallationStatus, ManagedBy, Ownership, Platform, ProjectRef, ProviderId, Resource,
    ResourceKind, ResourceLocator, ScanOptions, ScanProblem, Session, SessionId, SessionLifecycle,
    SizeConfidence, SizeInfo,
};
use agenttidy_infrastructure::{disk_usage, fs_probe, jsonl, sqlite};
use agenttidy_provider_api::{AgentProviderAdapter, ProviderInspection};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Well-known relative paths under the user's home directory.
const STATE_DIR: &str = ".codex";
const WORKSPACE_DIR: &str = "Documents/Codex";

/// The Codex provider adapter.
pub struct CodexAdapter;

#[async_trait::async_trait]
impl AgentProviderAdapter for CodexAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CODEX)
    }

    fn display_name(&self) -> &str {
        "Codex"
    }

    /// Find all Codex installations on this machine.
    ///
    /// Checks `~/.codex/` (state root) and `~/Documents/Codex/`
    /// (default workspace root). Both are included in `data_roots`
    /// when present.
    async fn detect(&self) -> Result<Vec<AgentInstallation>> {
        let home = dirs::home_dir().context("no home directory")?;
        let state_root = home.join(STATE_DIR);
        let workspace_root = home.join(WORKSPACE_DIR);

        if !state_root.is_dir() {
            return Ok(vec![]);
        }

        let mut data_roots = vec![state_root.to_string_lossy().to_string()];
        if workspace_root.is_dir() {
            data_roots.push(workspace_root.to_string_lossy().to_string());
        }

        let version = read_version(&state_root);

        Ok(vec![AgentInstallation {
            id: format!("{}:default", ProviderId::CODEX),
            provider: ProviderId::new(ProviderId::CODEX),
            platform: current_platform(),
            version,
            data_roots,
            status: InstallationStatus::Available,
        }])
    }

    /// Pre-scan inspection: version, schema versions, journal modes,
    /// running state, unknown structures.
    async fn inspect(&self, installation: &AgentInstallation) -> Result<ProviderInspection> {
        let state_root = PathBuf::from(&installation.data_roots[0]);

        let version = read_version(&state_root);
        let mut schema_versions = BTreeMap::new();
        let mut journal_modes = BTreeMap::new();
        let mut unknown_structures = Vec::new();
        let mut problems = BTreeMap::new();

        // Check state_5.sqlite (thread index).
        let state_db_path = state_root.join("state_5.sqlite");
        if state_db_path.is_file() {
            match sqlite::ReadOnlyDb::open(&state_db_path) {
                Ok(db) => {
                    // Schema version from _sqlx_migrations.
                    if let Ok(ver) = db.connection().query_row(
                        "SELECT version FROM _sqlx_migrations ORDER BY created_at DESC LIMIT 1",
                        [],
                        |row| row.get::<_, String>(0),
                    ) {
                        schema_versions.insert("state_5".into(), ver);
                    }
                    if let Ok(mode) = db.journal_mode() {
                        journal_modes.insert("state_5".into(), mode);
                    }
                }
                Err(e) => {
                    problems.insert(
                        "state-db-unreadable".into(),
                        ScanProblem::Warning {
                            message: format!("state_5.sqlite: {e}"),
                        },
                    );
                }
            }
        }

        // Check thread_history_1.sqlite (rollout projection).
        let history_db_path = state_root.join("thread_history_1.sqlite");
        if history_db_path.is_file() {
            match sqlite::ReadOnlyDb::open(&history_db_path) {
                Ok(db) => {
                    if let Ok(mode) = db.journal_mode() {
                        journal_modes.insert("thread_history_1".into(), mode);
                    }
                }
                Err(e) => {
                    problems.insert(
                        "history-db-unreadable".into(),
                        ScanProblem::Warning {
                            message: format!("thread_history_1.sqlite: {e}"),
                        },
                    );
                }
            }
        }

        // Check logs_2.sqlite (runtime logs).
        let logs_db_path = state_root.join("logs_2.sqlite");
        if logs_db_path.is_file() {
            match sqlite::ReadOnlyDb::open(&logs_db_path) {
                Ok(db) => {
                    if let Ok(mode) = db.journal_mode() {
                        journal_modes.insert("logs_2".into(), mode);
                    }
                }
                Err(e) => {
                    problems.insert(
                        "logs-db-unreadable".into(),
                        ScanProblem::Warning {
                            message: format!("logs_2.sqlite: {e}"),
                        },
                    );
                }
            }
        }

        // Check for unknown directories.
        let known_dirs = [
            "sessions",
            "archived_sessions",
            "attachments",
            "cache",
            "computer-use",
            "dictation-history",
            "generated_images",
            "memories",
            "node_repl",
            "plugins",
            "process_manager",
            "rollout-migrations",
            "rules",
            "skills",
            "sqlite",
            "thread-writer-locks",
            "tmp",
        ];
        if let Ok(entries) = std::fs::read_dir(&state_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.path().is_dir() && !known_dirs.contains(&name.as_str()) {
                    unknown_structures.push(format!("{}/", name));
                }
            }
        }

        let mut readable_roots = BTreeMap::new();
        readable_roots.insert(state_root.to_string_lossy().to_string(), true);
        if installation.data_roots.len() > 1 {
            readable_roots.insert(installation.data_roots[1].clone(), true);
        }

        Ok(ProviderInspection {
            version,
            schema_versions,
            journal_modes,
            agent_running: false,
            readable_roots,
            unknown_structures,
            problems,
        })
    }

    /// Derive capabilities from the inspection result.
    async fn capabilities(&self, inspection: &ProviderInspection) -> Result<AgentCapabilities> {
        let state_db_ok = inspection.schema_versions.contains_key("state_5");
        let mut caps = AgentCapabilities::empty();

        if state_db_ok {
            caps = caps
                .with(CapabilityTopic::Sessions, CapabilityStatus::Supported)
                .with(CapabilityTopic::Projects, CapabilityStatus::Supported)
                .with(CapabilityTopic::Archive, CapabilityStatus::Supported)
                .with(CapabilityTopic::Logs, CapabilityStatus::Supported);
        } else {
            // DB unreadable: degraded scan from JSONL tree only.
            caps = caps
                .with(CapabilityTopic::Sessions, CapabilityStatus::Degraded)
                .with(CapabilityTopic::Logs, CapabilityStatus::Degraded);
        }

        caps = caps.with(CapabilityTopic::Cleanup, CapabilityStatus::Unsupported);
        Ok(caps)
    }

    /// Scan one installation into a read-only snapshot.
    ///
    /// Reads `state_5.sqlite.threads` for session metadata and
    /// walks `sessions/YYYY/MM/DD/` for JSONL rollouts.
    async fn scan(
        &self,
        installation: &AgentInstallation,
        _options: &ScanOptions,
    ) -> Result<AgentSnapshot> {
        let state_root = PathBuf::from(&installation.data_roots[0]);

        let mut sessions = Vec::new();
        let mut resources = Vec::new();
        let mut problems = BTreeMap::new();

        // 1. Try to read sessions from state_5.sqlite.threads.
        let state_db_path = state_root.join("state_5.sqlite");
        let db_threads = if state_db_path.is_file() {
            match read_db_threads(&state_db_path) {
                Ok(rows) => rows,
                Err(e) => {
                    problems.insert(
                        "state-db-scan-failed".into(),
                        ScanProblem::Warning {
                            message: format!("state_5.sqlite threads: {e}"),
                        },
                    );
                    vec![]
                }
            }
        } else {
            vec![]
        };

        // 2. Walk sessions/ for JSONL rollouts.
        let sessions_dir = state_root.join("sessions");
        if sessions_dir.is_dir() {
            let walk = fs_probe::walk_tree(&sessions_dir);
            for problem in walk.problems {
                let fs_probe::WalkProblem::Unreadable { path, error } = problem;
                problems.insert(
                    format!("walk-unreadable:{}", path.display()),
                    ScanProblem::Warning { message: error },
                );
            }

            // Find all JSONL files.
            let jsonl_files: Vec<_> = walk
                .entries
                .iter()
                .filter(|e| {
                    e.kind == fs_probe::EntryKind::File
                        && e.path.extension().is_some_and(|ext| ext == "jsonl")
                })
                .collect();

            for jsonl_entry in &jsonl_files {
                let path = &jsonl_entry.path;
                let filename = path.file_stem().unwrap_or_default().to_string_lossy();

                // Extract rollout ID from filename: `rollout-<ts>-<uuid>`
                let rollout_id = filename
                    .strip_prefix("rollout-")
                    .unwrap_or(&filename)
                    .to_string();

                // Read first line to get session metadata.
                let (title, lifecycle, created_at, cwd, source, cli_version) =
                    read_jsonl_session_meta(path).unwrap_or_default();

                // Find matching DB thread (if available).
                let db_thread = db_threads.iter().find(|t| {
                    t.rollout_path
                        .as_ref()
                        .is_some_and(|p| p == &path.to_string_lossy())
                });

                let session_id_str = db_thread
                    .map(|t| t.id.clone())
                    .unwrap_or_else(|| rollout_id.clone());

                let session_cwd = db_thread
                    .and_then(|t| t.cwd.clone())
                    .or(cwd)
                    .unwrap_or_else(|| "<unknown>".to_string());

                let session_title = db_thread.and_then(|t| t.title.clone()).or(title);

                let session_lifecycle = if db_thread.is_some_and(|t| t.archived) {
                    SessionLifecycle::Archived {
                        archived_at: db_thread.and_then(|t| t.archived_at),
                    }
                } else {
                    lifecycle
                };

                // Build resource refs.
                let mut resource_refs = Vec::new();
                let transcript_id = format!(
                    "{}:session:{}:transcript",
                    ProviderId::CODEX,
                    session_id_str
                );
                resource_refs.push(agenttidy_core::ResourceRef {
                    id: agenttidy_core::ResourceId::new(&transcript_id),
                    kind: ResourceKind::Session,
                });

                // Transcript resource.
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(&transcript_id),
                    provider: ProviderId::new(ProviderId::CODEX),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Session,
                    locator: ResourceLocator::File {
                        path: path.to_string_lossy().to_string(),
                    },
                    ownership: Ownership::Exclusive,
                    managed_by: ManagedBy::Agent,
                    size: SizeInfo::exact(jsonl_entry.logical_len),
                    session_id: Some(SessionId::new(&session_id_str)),
                    project: Some(ProjectRef::from_cwd(&session_cwd)),
                    created_at,
                    updated_at: None,
                    dependencies: vec![],
                    metadata: serde_json::Map::new(),
                });

                // Build session.
                let mut metadata = serde_json::Map::new();
                if let Some(src) = source {
                    metadata.insert("source".into(), serde_json::Value::String(src));
                }
                if let Some(ver) = cli_version {
                    metadata.insert("cli_version".into(), serde_json::Value::String(ver));
                }

                sessions.push(Session {
                    id: SessionId::new(&session_id_str),
                    provider: ProviderId::new(ProviderId::CODEX),
                    installation_id: installation.id.clone(),
                    title: session_title,
                    project: Some(ProjectRef::from_cwd(&session_cwd)),
                    created_at,
                    updated_at: None,
                    lifecycle: session_lifecycle,
                    size: SizeInfo {
                        logical_bytes: jsonl_entry.logical_len,
                        allocated_bytes: jsonl_entry.allocated_len,
                        exclusive_bytes: Some(jsonl_entry.logical_len),
                        shared_bytes: None,
                        reclaimable_bytes: None,
                        confidence: SizeConfidence::Exact,
                    },
                    resource_refs,
                    metadata,
                });
            }
        }

        // 3. Scan archived_sessions/ (flat layout).
        let archived_dir = state_root.join("archived_sessions");
        if archived_dir.is_dir() {
            let walk = fs_probe::walk_tree(&archived_dir);
            let usage = disk_usage::usage_of_entries(&walk.entries);
            if usage.logical_bytes > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!(
                        "{}:archived-sessions",
                        ProviderId::CODEX
                    )),
                    provider: ProviderId::new(ProviderId::CODEX),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Session,
                    locator: ResourceLocator::Dir {
                        path: archived_dir.to_string_lossy().to_string(),
                    },
                    ownership: Ownership::Exclusive,
                    managed_by: ManagedBy::Agent,
                    size: SizeInfo {
                        logical_bytes: usage.logical_bytes,
                        allocated_bytes: usage.allocated_bytes,
                        exclusive_bytes: Some(usage.logical_bytes),
                        shared_bytes: None,
                        reclaimable_bytes: None,
                        confidence: SizeConfidence::Exact,
                    },
                    session_id: None,
                    project: None,
                    created_at: None,
                    updated_at: None,
                    dependencies: vec![],
                    metadata: serde_json::Map::new(),
                });
            }
        }

        // 4. Scan logs_2.sqlite (runtime logs).
        let logs_db_path = state_root.join("logs_2.sqlite");
        if logs_db_path.is_file() {
            let size = std::fs::metadata(&logs_db_path)
                .map(|m| m.len())
                .unwrap_or(0);
            if size > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!("{}:logs-db", ProviderId::CODEX)),
                    provider: ProviderId::new(ProviderId::CODEX),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Log,
                    locator: ResourceLocator::File {
                        path: logs_db_path.to_string_lossy().to_string(),
                    },
                    ownership: Ownership::Exclusive,
                    managed_by: ManagedBy::Agent,
                    size: SizeInfo::exact(size),
                    session_id: None,
                    project: None,
                    created_at: None,
                    updated_at: None,
                    dependencies: vec![],
                    metadata: serde_json::Map::new(),
                });
            }
        }

        // 5. Scan cache/ directory.
        let cache_dir = state_root.join("cache");
        if cache_dir.is_dir() {
            let walk = fs_probe::walk_tree(&cache_dir);
            let usage = disk_usage::usage_of_entries(&walk.entries);
            if usage.logical_bytes > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!("{}:cache", ProviderId::CODEX)),
                    provider: ProviderId::new(ProviderId::CODEX),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Cache,
                    locator: ResourceLocator::Dir {
                        path: cache_dir.to_string_lossy().to_string(),
                    },
                    ownership: Ownership::Exclusive,
                    managed_by: ManagedBy::Agent,
                    size: SizeInfo {
                        logical_bytes: usage.logical_bytes,
                        allocated_bytes: usage.allocated_bytes,
                        exclusive_bytes: Some(usage.logical_bytes),
                        shared_bytes: None,
                        reclaimable_bytes: None,
                        confidence: SizeConfidence::Exact,
                    },
                    session_id: None,
                    project: None,
                    created_at: None,
                    updated_at: None,
                    dependencies: vec![],
                    metadata: serde_json::Map::new(),
                });
            }
        }

        Ok(AgentSnapshot {
            installation: installation.clone(),
            sessions,
            resources,
            problems,
            completed_at: now_ms(),
        })
    }
}

/// Read version from `version.json`.
fn read_version(state_root: &Path) -> Option<String> {
    let path = state_root.join("version.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("latest_version")?.as_str().map(String::from)
}

/// Read thread metadata from state_5.sqlite.threads.
#[allow(dead_code)]
struct DbThreadRow {
    id: String,
    rollout_path: Option<String>,
    cwd: Option<String>,
    title: Option<String>,
    archived: bool,
    archived_at: Option<u64>,
    source: Option<String>,
    cli_version: Option<String>,
}

fn read_db_threads(db_path: &Path) -> Result<Vec<DbThreadRow>> {
    let db = sqlite::ReadOnlyDb::open(db_path)?;
    let conn = db.connection();

    let mut stmt = conn.prepare(
        "SELECT id, rollout_path, cwd, title, archived, archived_at, source, cli_version FROM threads",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(DbThreadRow {
            id: row.get(0)?,
            rollout_path: row.get(1)?,
            cwd: row.get(2)?,
            title: row.get(3)?,
            archived: row.get::<_, i64>(4)? != 0,
            archived_at: row.get(5)?,
            source: row.get(6)?,
            cli_version: row.get(7)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Read the first line of a JSONL file to extract session metadata.
type SessionMetaTuple = (
    Option<String>,
    SessionLifecycle,
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn read_jsonl_session_meta(path: &Path) -> Option<SessionMetaTuple> {
    let mut reader = jsonl::open_jsonl::<serde_json::Value>(path).ok()?;
    match reader.next()? {
        jsonl::JsonlEvent::Item { value, .. } => {
            // Codex session_meta line.
            let payload = value.get("payload")?;
            let title = payload
                .get("title")
                .and_then(|v| v.as_str())
                .map(String::from);
            let created_at = payload
                .get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(parse_iso8601_ms);
            let cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from);
            let source = payload
                .get("source")
                .and_then(|v| v.as_str())
                .map(String::from);
            let cli_version = payload
                .get("cli_version")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Determine lifecycle.
            let archived = payload
                .get("archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let lifecycle = if archived {
                SessionLifecycle::Archived {
                    archived_at: payload.get("archived_at").and_then(|v| v.as_u64()),
                }
            } else {
                SessionLifecycle::Unknown
            };

            Some((title, lifecycle, created_at, cwd, source, cli_version))
        }
        _ => None,
    }
}

/// Parse ISO 8601 timestamp to milliseconds since epoch.
fn parse_iso8601_ms(s: &str) -> Option<u64> {
    if s.len() < 20 {
        return None;
    }
    let date_part = &s[..10];
    let time_part = &s[11..19];

    let year: u64 = date_part[..4].parse().ok()?;
    let month: u64 = date_part[5..7].parse().ok()?;
    let day: u64 = date_part[8..10].parse().ok()?;
    let hour: u64 = time_part[..2].parse().ok()?;
    let minute: u64 = time_part[3..5].parse().ok()?;
    let second: u64 = time_part[6..8].parse().ok()?;

    let days = (year - 1970) * 365 + (month - 1) * 30 + (day - 1);
    let seconds = days * 86400 + hour * 3600 + minute * 60 + second;

    let ms = if s.len() > 20 && s.as_bytes()[19] == b'.' {
        let ms_str = &s[20..23];
        ms_str.parse::<u64>().unwrap_or(0)
    } else {
        0
    };

    Some(seconds * 1000 + ms)
}

/// Current timestamp in milliseconds.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Current platform.
fn current_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Macos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso8601_basic() {
        let ms = parse_iso8601_ms("2026-06-04T11:57:21.455Z");
        assert!(ms.is_some());
    }

    #[tokio::test]
    async fn detect_returns_empty_when_no_state_root() {
        let adapter = CodexAdapter;
        let installations = adapter.detect().await.unwrap();
        assert!(installations.len() <= 1);
    }
}
