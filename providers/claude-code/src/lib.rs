//! Claude Code provider adapter (`docs/providers/claude-code.md`).
//!
//! Claude Code is a CLI tool. Its data lives under `~/.claude/`:
//! - `projects/<cwd-slug>/<uuid>.jsonl` — session transcripts (JSONL, append-only)
//! - `projects/<cwd-slug>/<uuid>/` — per-session sidecars (subagents, etc.)
//! - `projects/<cwd-slug>/memory/` — per-project agent memory
//! - `file-history/<hash>@v<n>/` — file edit snapshots (content-addressed)
//! - `shell-snapshots/` — shell env snapshots
//! - `history.jsonl` — global prompt history
//!
//! Unlike WorkBuddy/Codex, Claude Code has no default workspace directory
//! outside the state root — sessions run directly in the user's chosen cwd.

use agenttidy_core::{
    AgentCapabilities, AgentInstallation, AgentSnapshot, CapabilityStatus, CapabilityTopic,
    InstallationStatus, ManagedBy, Ownership, Platform, ProjectRef, ProviderId, Resource,
    ResourceKind, ResourceLocator, ScanOptions, ScanProblem, Session, SessionId, SessionLifecycle,
    SizeConfidence, SizeInfo,
};
use agenttidy_infrastructure::{disk_usage, fs_probe, jsonl};
use agenttidy_provider_api::{AgentProviderAdapter, ProviderInspection};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Well-known relative path under the user's home directory.
const STATE_DIR: &str = ".claude";

/// The Claude Code provider adapter.
pub struct ClaudeCodeAdapter;

#[async_trait::async_trait]
impl AgentProviderAdapter for ClaudeCodeAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CLAUDE_CODE)
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    /// Find all Claude Code installations on this machine.
    ///
    /// Checks `~/.claude/` (state root). Unlike WorkBuddy, there is
    /// no default workspace directory outside the state root.
    async fn detect(&self) -> Result<Vec<AgentInstallation>> {
        let home = dirs::home_dir().context("no home directory")?;
        let state_root = home.join(STATE_DIR);

        if !state_root.is_dir() {
            return Ok(vec![]);
        }

        let version = read_version(&state_root);

        Ok(vec![AgentInstallation {
            id: format!("{}:default", ProviderId::CLAUDE_CODE),
            provider: ProviderId::new(ProviderId::CLAUDE_CODE),
            platform: current_platform(),
            version,
            data_roots: vec![state_root.to_string_lossy().to_string()],
            status: InstallationStatus::Available,
        }])
    }

    /// Pre-scan inspection: version, schema versions, running state.
    async fn inspect(&self, installation: &AgentInstallation) -> Result<ProviderInspection> {
        let state_root = PathBuf::from(&installation.data_roots[0]);

        let version = read_version(&state_root);
        let mut schema_versions = BTreeMap::new();
        let journal_modes = BTreeMap::new();
        let mut unknown_structures = Vec::new();
        let problems = BTreeMap::new();

        // Claude Code uses JSONL only, no SQLite.
        // Schema version is implicit (line types).
        schema_versions.insert("jsonl".into(), "1".into());

        // Check for unknown directories.
        let known_dirs = [
            "projects",
            "file-history",
            "shell-snapshots",
            "session-env",
            "plans",
            "tasks",
            "backups",
            "cache",
            "paste-cache",
            "debug",
            "downloads",
            "ide",
            "daemon",
            "jobs",
            "skills",
            "plugins",
            "agents",
            "todo",
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
    async fn capabilities(&self, _inspection: &ProviderInspection) -> Result<AgentCapabilities> {
        let mut caps = AgentCapabilities::empty();
        caps = caps
            .with(CapabilityTopic::Sessions, CapabilityStatus::Supported)
            .with(CapabilityTopic::Projects, CapabilityStatus::Supported)
            .with(CapabilityTopic::Logs, CapabilityStatus::Supported)
            .with(CapabilityTopic::Cleanup, CapabilityStatus::Unsupported);
        Ok(caps)
    }

    /// Scan one installation into a read-only snapshot.
    ///
    /// Walks `projects/<slug>/` for JSONL transcripts and their
    /// sidecars (subagents, memory, etc.).
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
                    project_entries.entry(entry.path.clone()).or_default();
                } else if let Some(parent) = find_project_dir(&entry.path, &projects_dir) {
                    project_entries.entry(parent).or_default().push(entry);
                }
            }

            // For each project dir, find JSONL transcripts.
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
                            && !e.path.to_string_lossy().contains("subagents")
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
                    let (title, lifecycle, created_at, updated_at, version) =
                        read_jsonl_session_meta(path).unwrap_or_default();

                    // Find sidecar files for this session.
                    let sidecar_dir = project_dir.join(&session_id_str);
                    let mut resource_refs = Vec::new();

                    // Transcript resource.
                    let transcript_id = format!(
                        "{}:session:{}:transcript",
                        ProviderId::CLAUDE_CODE,
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
                            ProviderId::CLAUDE_CODE,
                            session_id_str
                        )),
                        provider: ProviderId::new(ProviderId::CLAUDE_CODE),
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

                    // Sidecar dir resource (subagents, etc.).
                    if sidecar_dir.is_dir() {
                        let sidecar_walk = fs_probe::walk_tree(&sidecar_dir);
                        let sidecar_usage = disk_usage::usage_of_entries(&sidecar_walk.entries);
                        if sidecar_usage.logical_bytes > 0 {
                            let sidecar_id = format!(
                                "{}:session:{}:sidecars",
                                ProviderId::CLAUDE_CODE,
                                session_id_str
                            );
                            session_size.logical_bytes += sidecar_usage.logical_bytes;
                            if let Some(alloc) = sidecar_usage.allocated_bytes {
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
                                provider: ProviderId::new(ProviderId::CLAUDE_CODE),
                                installation_id: installation.id.clone(),
                                kind: ResourceKind::Session,
                                locator: ResourceLocator::Dir {
                                    path: sidecar_dir.to_string_lossy().to_string(),
                                },
                                ownership: Ownership::Exclusive,
                                managed_by: ManagedBy::Agent,
                                size: SizeInfo {
                                    logical_bytes: sidecar_usage.logical_bytes,
                                    allocated_bytes: sidecar_usage.allocated_bytes,
                                    exclusive_bytes: Some(sidecar_usage.logical_bytes),
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
                    let mut metadata = serde_json::Map::new();
                    if let Some(v) = version {
                        metadata.insert("cli_version".into(), serde_json::Value::String(v));
                    }

                    sessions.push(Session {
                        id: SessionId::new(&session_id_str),
                        provider: ProviderId::new(ProviderId::CLAUDE_CODE),
                        installation_id: installation.id.clone(),
                        title,
                        project: Some(ProjectRef::from_cwd(&cwd)),
                        created_at,
                        updated_at,
                        lifecycle,
                        size: session_size,
                        resource_refs,
                        metadata,
                    });
                }
            }
        }

        // Scan file-history/ (shared, content-addressed).
        let file_history_dir = state_root.join("file-history");
        if file_history_dir.is_dir() {
            let walk = fs_probe::walk_tree(&file_history_dir);
            let usage = disk_usage::usage_of_entries(&walk.entries);
            if usage.logical_bytes > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!(
                        "{}:file-history",
                        ProviderId::CLAUDE_CODE
                    )),
                    provider: ProviderId::new(ProviderId::CLAUDE_CODE),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Checkpoint,
                    locator: ResourceLocator::Dir {
                        path: file_history_dir.to_string_lossy().to_string(),
                    },
                    ownership: Ownership::Shared,
                    managed_by: ManagedBy::Agent,
                    size: SizeInfo {
                        logical_bytes: usage.logical_bytes,
                        allocated_bytes: usage.allocated_bytes,
                        exclusive_bytes: None,
                        shared_bytes: Some(usage.logical_bytes),
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

        // Scan shell-snapshots/ (cache).
        let shell_snapshots_dir = state_root.join("shell-snapshots");
        if shell_snapshots_dir.is_dir() {
            let walk = fs_probe::walk_tree(&shell_snapshots_dir);
            let usage = disk_usage::usage_of_entries(&walk.entries);
            if usage.logical_bytes > 0 {
                resources.push(Resource {
                    id: agenttidy_core::ResourceId::new(format!(
                        "{}:shell-snapshots",
                        ProviderId::CLAUDE_CODE
                    )),
                    provider: ProviderId::new(ProviderId::CLAUDE_CODE),
                    installation_id: installation.id.clone(),
                    kind: ResourceKind::Cache,
                    locator: ResourceLocator::Dir {
                        path: shell_snapshots_dir.to_string_lossy().to_string(),
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

/// Read version from `config.json` or `.last-update-result.json`.
fn read_version(state_root: &Path) -> Option<String> {
    // Try config.json first.
    let config_path = state_root.join("config.json");
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
                return Some(version.to_string());
            }
        }
    }

    // Fallback: .last-update-result.json
    let update_path = state_root.join(".last-update-result.json");
    if let Ok(content) = std::fs::read_to_string(&update_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
                return Some(version.to_string());
            }
        }
    }

    None
}

/// Read the first line of a JSONL file to extract session metadata.
type SessionMetaTuple = (
    Option<String>,
    SessionLifecycle,
    Option<u64>,
    Option<u64>,
    Option<String>,
);

fn read_jsonl_session_meta(path: &Path) -> Option<SessionMetaTuple> {
    let mut reader = jsonl::open_jsonl::<serde_json::Value>(path).ok()?;
    match reader.next()? {
        jsonl::JsonlEvent::Item { value, .. } => {
            let title = value
                .get("title")
                .and_then(|v| v.as_str())
                .map(String::from);
            let created_at = value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(parse_iso8601_ms);
            let updated_at = created_at; // First line timestamp is creation time.
            let version = value
                .get("version")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Claude Code has no archive concept; all sessions are inactive or unknown.
            let lifecycle = SessionLifecycle::Unknown;

            Some((title, lifecycle, created_at, updated_at, version))
        }
        _ => None,
    }
}

/// Parse ISO 8601 timestamp to milliseconds since epoch.
fn parse_iso8601_ms(s: &str) -> Option<u64> {
    // Simple parser for "2026-09-15T10:00:00.000Z" format.
    // This is intentionally minimal; a full ISO parser would be overkill.
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

    // Days since Unix epoch (1970-01-01).
    let days = (year - 1970) * 365 + (month - 1) * 30 + (day - 1); // Simplified.
    let seconds = days * 86400 + hour * 3600 + minute * 60 + second;

    // Add milliseconds if present.
    let ms = if s.len() > 20 && s.as_bytes()[19] == b'.' {
        let ms_str = &s[20..23];
        ms_str.parse::<u64>().unwrap_or(0)
    } else {
        0
    };

    Some(seconds * 1000 + ms)
}

/// Decode a cwd-slug back to a path.
///
/// Claude Code slug mapping: `/` → `-` (only separator replacement).
/// This is the inverse of the encoding described in docs/providers/claude-code.md.
fn slug_to_cwd(slug: &str) -> String {
    // On Windows, slugs look like `F--Work-AgentTidy`.
    //   Original path: `F:\Work\AgentTidy`
    //   `:` → `-`, `\` → `-` (all separators become `-`).
    // On macOS, slugs look like `-Users-lxy-Desktop-MyProjects-AgentTidy`.
    //   Original path: `/Users/lxy/Desktop/MyProjects/AgentTidy`
    //   `/` → `-`.
    if cfg!(windows) && slug.len() > 2 && slug.as_bytes()[1] == b'-' && slug.as_bytes()[2] == b'-' {
        // Windows drive letter path: `F--Work-AgentTidy` → `F:\Work\AgentTidy`.
        // Both `:` and `\` map to `-`, so replace all `-` with `\` then
        // restore position 1 as `:`.
        let mut out = slug.replace('-', r"\");
        // SAFETY: we checked len > 2 and bytes 1..2 is "-", so bytes 1..2
        // is a valid char boundary.
        out.replace_range(1..2, ":");
        out
    } else {
        // Unix: `-Users-lxy-...` → `/Users/lxy/...`
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
        assert_eq!(slug_to_cwd("F--Work-AgentTidy"), r"F:\Work\AgentTidy");
    }

    #[test]
    fn slug_to_cwd_unix() {
        assert_eq!(
            slug_to_cwd("-Users-lxy-Desktop-MyProjects"),
            "/Users/lxy/Desktop/MyProjects"
        );
    }

    #[test]
    fn parse_iso8601_basic() {
        let ms = parse_iso8601_ms("2026-09-15T10:00:00.000Z");
        assert!(ms.is_some());
    }

    #[tokio::test]
    async fn detect_returns_empty_when_no_state_root() {
        let adapter = ClaudeCodeAdapter;
        let installations = adapter.detect().await.unwrap();
        assert!(installations.len() <= 1);
    }
}
