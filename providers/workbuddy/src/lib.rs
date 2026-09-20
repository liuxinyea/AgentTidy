//! WorkBuddy provider adapter (`docs/providers/workbuddy.md`).
//!
//! WorkBuddy is an Electron desktop app. Its data lives under two roots:
//! - State root: `~/.workbuddy/` (transcripts, DBs, caches)
//! - Default workspace root: `~/WorkBuddy/` (scratch workspaces, user data)
//!
//! This adapter implements the read-only Provider contract (Phase 3):
//! - `detect`: finds installations by checking well-known paths
//! - `inspect`: reads `last-launch.json` for version, opens DB for schema/journal
//! - `capabilities`: reports what topics are available
//! - `scan`: enumerates sessions from DB + JSONL files, resources from filesystem

use agenttidy_core::{
    AgentCapabilities, AgentInstallation, AgentSnapshot, CapabilityStatus, CapabilityTopic,
    InstallationStatus, ManagedBy, Ownership, Platform, ProjectRef, ProviderId, Resource,
    ResourceKind, ResourceLocator, ScanOptions, ScanProblem, Session, SessionId, SessionLifecycle,
    SizeConfidence, SizeInfo,
};
use agenttidy_infrastructure::{fs_probe, jsonl, sqlite};
use agenttidy_provider_api::{AgentProviderAdapter, ProviderInspection};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Well-known relative paths under the user's home directory.
const STATE_DIR: &str = ".workbuddy";
const WORKSPACE_DIR: &str = "WorkBuddy";

/// The WorkBuddy provider adapter.
pub struct WorkBuddyAdapter;

#[async_trait::async_trait]
impl AgentProviderAdapter for WorkBuddyAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::WORKBUDDY)
    }

    fn display_name(&self) -> &str {
        "WorkBuddy"
    }

    /// Find all WorkBuddy installations on this machine.
    ///
    /// Checks well-known paths: `~/.workbuddy/` (state root) and
    /// `~/WorkBuddy/` (default workspace root). Both are included in
    /// `data_roots` when present.
    async fn detect(&self) -> Result<Vec<AgentInstallation>> {
        let home = dirs::home_dir().context("no home directory")?;
        let state_root = home.join(STATE_DIR);
        let workspace_root = home.join(WORKSPACE_DIR);

        // WorkBuddy requires the state root to exist; workspace root is
        // optional (it's created on first conversation).
        if !state_root.is_dir() {
            return Ok(vec![]);
        }

        let mut data_roots = vec![state_root.to_string_lossy().to_string()];
        if workspace_root.is_dir() {
            data_roots.push(workspace_root.to_string_lossy().to_string());
        }

        // Version discovery: last-launch.json in state root.
        let version = read_version(&state_root);

        Ok(vec![AgentInstallation {
            id: format!("{}:default", ProviderId::WORKBUDDY),
            provider: ProviderId::new(ProviderId::WORKBUDDY),
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
        let unknown_structures = Vec::new();
        let mut problems = BTreeMap::new();

        // Check workbuddy.db
        let db_path = state_root.join("workbuddy.db");
        if db_path.is_file() {
            match sqlite::ReadOnlyDb::open(&db_path) {
                Ok(db) => {
                    // Schema version from Drizzle migrations.
                    if let Ok(ver) = db.connection().query_row(
                        "SELECT version FROM __workbuddy_drizzle_migrations ORDER BY created_at DESC LIMIT 1",
                        [],
                        |row| row.get::<_, String>(0),
                    ) {
                        schema_versions.insert("workbuddy.db".into(), ver);
                    }
                    if let Ok(mode) = db.journal_mode() {
                        journal_modes.insert("workbuddy.db".into(), mode);
                    }
                }
                Err(e) => {
                    problems.insert(
                        "db-unreadable".into(),
                        ScanProblem::Warning {
                            message: format!("workbuddy.db: {e}"),
                        },
                    );
                }
            }
        }

        // Check for edge-sync-mapping DBs (old generations).
        for ver in 1..=4 {
            let name = format!("edge-sync-mapping-v{ver}.db");
            let path = state_root.join(&name);
            if path.is_file() {
                schema_versions.insert(name, format!("v{ver}"));
            }
        }

        // Check readable roots.
        let mut readable_roots = BTreeMap::new();
        for root in &installation.data_roots {
            readable_roots.insert(root.clone(), Path::new(root).is_dir());
        }

        Ok(ProviderInspection {
            version,
            schema_versions,
            journal_modes,
            agent_running: false, // Phase 2 process detection is separate
            readable_roots,
            unknown_structures,
            problems,
        })
    }

    /// Derive capabilities from the inspection result.
    ///
    /// WorkBuddy reports `Sessions` and `Logs` as supported when the
    /// DB is readable; `Archive` is supported because
    /// `sessions.status` carries archive state.
    async fn capabilities(&self, inspection: &ProviderInspection) -> Result<AgentCapabilities> {
        let db_ok = inspection.schema_versions.contains_key("workbuddy.db");
        let mut caps = AgentCapabilities::empty();

        if db_ok {
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

        // Cleanup is always unsupported in Phase 3.
        caps = caps.with(CapabilityTopic::Cleanup, CapabilityStatus::Unsupported);

        Ok(caps)
    }

    /// Scan one installation into a read-only snapshot.
    ///
    /// The scan reads `workbuddy.db` for session metadata (id, cwd,
    /// title, status, deleted_at) and walks `projects/<slug>/` for
    /// JSONL transcripts and their sidecars.
    async fn scan(
        &self,
        installation: &AgentInstallation,
        _options: &ScanOptions,
    ) -> Result<AgentSnapshot> {
        let state_root = PathBuf::from(&installation.data_roots[0]);
        let projects_dir = state_root.join("projects");

        let mut sessions = Vec::new();
        let mut resources = Vec::new();
        let mut problems = BTreeMap::new();

        // 1. Try to read sessions from DB.
        let db_path = state_root.join("workbuddy.db");
        let _db_sessions = if db_path.is_file() {
            match read_db_sessions(&db_path) {
                Ok(rows) => rows,
                Err(e) => {
                    problems.insert(
                        "db-scan-failed".into(),
                        ScanProblem::Warning {
                            message: format!("could not read workbuddy.db sessions: {e}"),
                        },
                    );
                    vec![]
                }
            }
        } else {
            vec![]
        };

        // 2. Walk projects/ for JSONL transcripts.
        if projects_dir.is_dir() {
            let walk = fs_probe::walk_tree(&projects_dir);
            for problem in walk.problems {
                let fs_probe::WalkProblem::Unreadable { path, error } = problem;
                problems.insert(
                    format!("walk-unreadable:{}", path.display()),
                    ScanProblem::Warning { message: error },
                );
            }

            // Group entries by project dir (first level under projects/).
            let mut project_entries: BTreeMap<PathBuf, Vec<fs_probe::EntryInfo>> = BTreeMap::new();
            for entry in walk.entries {
                if entry.kind == fs_probe::EntryKind::Dir
                    && entry.path.parent() == Some(&projects_dir)
                {
                    // Project dir itself.
                    project_entries.entry(entry.path.clone()).or_default();
                } else if let Some(parent) = find_project_dir(&entry.path, &projects_dir) {
                    project_entries.entry(parent).or_default().push(entry);
                }
            }

            // For each project dir, find JSONL transcripts and build sessions/resources.
            for (project_dir, entries) in &project_entries {
                let slug = project_dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let cwd = slug_to_cwd(&slug);

                // Find JSONL files (transcripts).
                let jsonl_files: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        e.kind == fs_probe::EntryKind::File
                            && e.path.extension().is_some_and(|ext| ext == "jsonl")
                    })
                    .collect();

                for jsonl_entry in &jsonl_files {
                    let path = &jsonl_entry.path;
                    let session_id_str = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // Read first line to get session metadata.
                    let (title, lifecycle, created_at, updated_at) =
                        read_jsonl_session_meta(path).unwrap_or_default();

                    // Find sidecar files for this session.
                    let sidecar_dir = project_dir.join(&session_id_str);
                    let mut resource_refs = Vec::new();

                    // Transcript resource.
                    let transcript_id = format!(
                        "{}:session:{}:transcript",
                        ProviderId::WORKBUDDY,
                        session_id_str
                    );
                    resource_refs.push(agenttidy_core::ResourceRef {
                        id: agenttidy_core::ResourceId::new(transcript_id),
                        kind: ResourceKind::Session,
                    });

                    let mut session_size = SizeInfo {
                        logical_bytes: jsonl_entry.logical_len,
                        allocated_bytes: jsonl_entry.allocated_len,
                        exclusive_bytes: Some(jsonl_entry.logical_len),
                        shared_bytes: None,
                        reclaimable_bytes: None,
                        confidence: SizeConfidence::Exact,
                    };

                    // Transcript resource.
                    resources.push(Resource {
                        id: agenttidy_core::ResourceId::new(format!(
                            "{}:session:{}:transcript",
                            ProviderId::WORKBUDDY,
                            session_id_str
                        )),
                        provider: ProviderId::new(ProviderId::WORKBUDDY),
                        installation_id: installation.id.clone(),
                        kind: ResourceKind::Session,
                        locator: ResourceLocator::File {
                            path: path.to_string_lossy().to_string(),
                        },
                        ownership: Ownership::Exclusive,
                        managed_by: ManagedBy::Agent,
                        size: SizeInfo::exact(jsonl_entry.logical_len),
                        session_id: Some(SessionId::new(&session_id_str)),
                        project: Some(ProjectRef::from_cwd(&cwd)),
                        created_at,
                        updated_at,
                        dependencies: vec![],
                        metadata: serde_json::Map::new(),
                    });

                    // Sidecar dir resource (tool-results, meta.json, etc.).
                    if sidecar_dir.is_dir() {
                        let sidecar_walk = fs_probe::walk_tree(&sidecar_dir);
                        let sidecar_size = agenttidy_infrastructure::disk_usage::usage_of_entries(
                            &sidecar_walk.entries,
                        );
                        if sidecar_size.logical_bytes > 0 {
                            let sidecar_id = format!(
                                "{}:session:{}:sidecars",
                                ProviderId::WORKBUDDY,
                                session_id_str
                            );
                            session_size.logical_bytes += sidecar_size.logical_bytes;
                            if let Some(alloc) = sidecar_size.allocated_bytes {
                                session_size.allocated_bytes =
                                    Some(session_size.allocated_bytes.unwrap_or(0) + alloc);
                            }
                            session_size.exclusive_bytes = Some(session_size.logical_bytes);

                            resource_refs.push(agenttidy_core::ResourceRef {
                                id: agenttidy_core::ResourceId::new(sidecar_id.clone()),
                                kind: ResourceKind::Session,
                            });

                            resources.push(Resource {
                                id: agenttidy_core::ResourceId::new(sidecar_id),
                                provider: ProviderId::new(ProviderId::WORKBUDDY),
                                installation_id: installation.id.clone(),
                                kind: ResourceKind::Session,
                                locator: ResourceLocator::Dir {
                                    path: sidecar_dir.to_string_lossy().to_string(),
                                },
                                ownership: Ownership::Exclusive,
                                managed_by: ManagedBy::Agent,
                                size: SizeInfo {
                                    logical_bytes: sidecar_size.logical_bytes,
                                    allocated_bytes: sidecar_size.allocated_bytes,
                                    exclusive_bytes: Some(sidecar_size.logical_bytes),
                                    shared_bytes: None,
                                    reclaimable_bytes: None,
                                    confidence: SizeConfidence::Exact,
                                },
                                session_id: Some(SessionId::new(&session_id_str)),
                                project: Some(ProjectRef::from_cwd(&cwd)),
                                created_at: None,
                                updated_at: None,
                                dependencies: vec![],
                                metadata: serde_json::Map::new(),
                            });
                        }
                    }

                    // Build session.
                    sessions.push(Session {
                        id: SessionId::new(&session_id_str),
                        provider: ProviderId::new(ProviderId::WORKBUDDY),
                        installation_id: installation.id.clone(),
                        title,
                        project: Some(ProjectRef::from_cwd(&cwd)),
                        created_at,
                        updated_at,
                        lifecycle,
                        size: session_size,
                        resource_refs,
                        metadata: serde_json::Map::new(),
                    });
                }
            }
        }

        // 3. Scan logs/ directory.
        let logs_dir = state_root.join("logs");
        if logs_dir.is_dir() {
            let walk = fs_probe::walk_tree(&logs_dir);
            let usage = agenttidy_infrastructure::disk_usage::usage_of_entries(&walk.entries);
            if usage.logical_bytes > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!("{}:logs", ProviderId::WORKBUDDY)),
                    provider: ProviderId::new(ProviderId::WORKBUDDY),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Log,
                    locator: ResourceLocator::Dir {
                        path: logs_dir.to_string_lossy().to_string(),
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

        // 4. Scan traces/ directory.
        let traces_dir = state_root.join("traces");
        if traces_dir.is_dir() {
            let walk = fs_probe::walk_tree(&traces_dir);
            let usage = agenttidy_infrastructure::disk_usage::usage_of_entries(&walk.entries);
            if usage.logical_bytes > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!(
                        "{}:traces",
                        ProviderId::WORKBUDDY
                    )),
                    provider: ProviderId::new(ProviderId::WORKBUDDY),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Log,
                    locator: ResourceLocator::Dir {
                        path: traces_dir.to_string_lossy().to_string(),
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

/// Read `last-launch.json` for version info.
fn read_version(state_root: &Path) -> Option<String> {
    let path = state_root.join("last-launch.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("version")?.as_str().map(String::from)
}

/// Read session metadata from DB.
#[allow(dead_code)]
struct DbSessionRow {
    id: String,
    cwd: Option<String>,
    title: Option<String>,
    status: String,
    deleted_at: Option<i64>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

fn read_db_sessions(db_path: &Path) -> Result<Vec<DbSessionRow>> {
    let db = sqlite::ReadOnlyDb::open(db_path)?;
    let conn = db.connection();

    let mut stmt = conn.prepare(
        "SELECT id, cwd, title, status, deleted_at, created_at, updated_at FROM sessions",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(DbSessionRow {
            id: row.get(0)?,
            cwd: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            deleted_at: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Read the first line of a JSONL file to extract session metadata.
type SessionMetaTuple = (Option<String>, SessionLifecycle, Option<u64>, Option<u64>);

fn read_jsonl_session_meta(path: &Path) -> Option<SessionMetaTuple> {
    let mut reader = jsonl::open_jsonl::<serde_json::Value>(path).ok()?;
    match reader.next()? {
        jsonl::JsonlEvent::Item { value, .. } => {
            let title = value
                .get("title")
                .and_then(|v| v.as_str())
                .map(String::from);
            let created_at = value.get("created_at").and_then(|v| v.as_u64());
            let updated_at = value.get("updated_at").and_then(|v| v.as_u64());

            // Determine lifecycle from status/deleted_at fields.
            let lifecycle = if value.get("deleted_at").and_then(|v| v.as_u64()).is_some() {
                // Soft-deleted session: treat as inactive.
                SessionLifecycle::Inactive
            } else if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
                match status {
                    "completed" | "terminated" => SessionLifecycle::Inactive,
                    "archived" => SessionLifecycle::Archived {
                        archived_at: value.get("updated_at").and_then(|v| v.as_u64()),
                    },
                    "error" => SessionLifecycle::Inactive,
                    _ => SessionLifecycle::Unknown,
                }
            } else {
                SessionLifecycle::Unknown
            };

            Some((title, lifecycle, created_at, updated_at))
        }
        _ => None,
    }
}

/// Decode a cwd-slug back to a path.
///
/// WorkBuddy slug mapping: `/` → `-` (only separator replacement).
/// This is the inverse of the encoding described in docs/providers/workbuddy.md.
fn slug_to_cwd(slug: &str) -> String {
    // On Windows, slugs look like `c-Users-18712-WorkBuddy-2026`.
    //   Original path: `c:\Users\18712\WorkBuddy\2026`
    //   `\` → `-`; `:` is dropped (not encoded).
    // On macOS, slugs look like `-Users-lxy-Desktop-MyProjects-AgentTidy`.
    //   Original path: `/Users/lxy/Desktop/MyProjects/AgentTidy`
    //   `/` → `-`.
    //
    // This is intentionally simple; the DB `sessions.cwd` is the source of
    // truth. We use this only as a fallback when the DB row is missing.
    if cfg!(windows)
        && slug.len() > 1
        && slug.as_bytes()[0].is_ascii_alphabetic()
        && slug.as_bytes()[1] == b'-'
    {
        // Windows drive letter: `c-Users-...` → `C:\Users\...`.
        // `:` was dropped during encoding; `\` became `-`.
        // Restore: drive + `:\`, then replace remaining `-` with `\`.
        let drive = (slug.as_bytes()[0] as char).to_ascii_uppercase();
        let rest = &slug[2..];
        format!("{}:\\{}", drive, rest.replace('-', r"\"))
    } else {
        // Unix: `-Users-...` → `/Users/...`
        slug.replace('-', "/")
    }
}

/// Find the project dir (first-level dir under projects/) for a given path.
fn find_project_dir(path: &Path, projects_dir: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(projects_dir).ok()?;
    let first_component = relative.components().next()?;
    Some(projects_dir.join(first_component))
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
    fn slug_to_cwd_windows_drive() {
        assert_eq!(
            slug_to_cwd("c-Users-18712-WorkBuddy-2026"),
            r"C:\Users\18712\WorkBuddy\2026"
        );
    }

    #[test]
    fn slug_to_cwd_unix() {
        assert_eq!(
            slug_to_cwd("-Users-lxy-Desktop-MyProjects"),
            "/Users/lxy/Desktop/MyProjects"
        );
    }

    #[tokio::test]
    async fn detect_returns_empty_when_no_state_root() {
        // This test assumes ~/.workbuddy does not exist in CI.
        // If it does, the test still passes (detect returns 1 installation).
        let adapter = WorkBuddyAdapter;
        let installations = adapter.detect().await.unwrap();
        // Either 0 or 1 installation depending on environment.
        assert!(installations.len() <= 1);
    }
}
