//! Read-only Claude Code CLI provider adapter (`Start.md` §10, Phase 3).
//!
//! This adapter recognizes only the verified CLI transcript layout under
//! `~/.claude/projects/`. It reads compact line metadata while deliberately
//! ignoring message bodies, and treats unknown structures as diagnostics. The
//! separate Windows Claude Desktop VM installation is discovered later; its
//! credentials and VM images are outside this adapter's scan scope.

use agenttidy_core::{
    AgentCapabilities, AgentInstallation, AgentSnapshot, CapabilityStatus, CapabilityTopic,
    CleanupPrecondition, CleanupUnit, InstallationStatus, ManagedBy, Ownership, Platform,
    ProjectRef, ProviderId, Resource, ResourceId, ResourceKind, ResourceLocator, ResourceRef,
    ScanOptions, ScanProblem, Session, SessionId, SessionLifecycle, SizeConfidence, SizeInfo,
};
use agenttidy_infrastructure::disk_usage::UsageAccumulator;
use agenttidy_infrastructure::fs_probe::{walk_tree, EntryKind, WalkProblem};
use agenttidy_infrastructure::processes::{any_process_running, ProcessSignature};
use agenttidy_provider_api::{AgentProviderAdapter, ProviderInspection};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const INSTALLATION_ID: &str = "claude-code:cli";

/// Read-only adapter for the standalone Claude Code CLI installation.
#[derive(Debug, Default)]
pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    fn home_dir() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("USERPROFILE").map(PathBuf::from)
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("HOME").map(PathBuf::from)
        }
    }

    fn platform() -> Platform {
        #[cfg(windows)]
        {
            Platform::Windows
        }
        #[cfg(not(windows))]
        {
            Platform::Macos
        }
    }

    fn root(home: &Path) -> PathBuf {
        home.join(".claude")
    }

    fn running() -> bool {
        // Presence is only a conservative active-data signal (§16.2), never
        // permission to mutate a transcript.
        any_process_running(&[ProcessSignature::new(&["claude", "claude-code"], &[])])
    }

    fn is_readable_dir(path: &Path) -> bool {
        fs::read_dir(path).is_ok()
    }

    fn scan_projects(
        root: &Path,
        installation: &AgentInstallation,
        active: bool,
        options: &ScanOptions,
    ) -> (Vec<Session>, Vec<Resource>, BTreeMap<String, ScanProblem>) {
        let mut sessions = Vec::new();
        let mut resources = Vec::new();
        let mut problems = BTreeMap::new();
        let mut seen_paths = HashSet::new();
        let projects = root.join("projects");
        if !projects.is_dir() {
            problems.insert(
                "projects-missing".into(),
                ScanProblem::Warning {
                    message: "projects directory missing; no CLI sessions found".into(),
                },
            );
            return (sessions, resources, problems);
        }

        let project_dirs = match fs::read_dir(&projects) {
            Ok(entries) => entries,
            Err(error) => {
                problems.insert(
                    "projects-unreadable".into(),
                    ScanProblem::Error {
                        message: error.to_string(),
                        path: Some(projects.display().to_string()),
                    },
                );
                return (sessions, resources, problems);
            }
        };
        for project_dir in project_dirs.flatten() {
            let path = project_dir.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                problems.insert(
                    format!("project-unreadable:{}", path.display()),
                    ScanProblem::Error {
                        message: "cannot stat project directory".into(),
                        path: Some(path.display().to_string()),
                    },
                );
                continue;
            };
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            let outcome = walk_tree(&path);
            for problem in outcome.problems {
                let WalkProblem::Unreadable { path, error } = problem;
                problems.insert(
                    format!("unreadable:{}", path.display()),
                    ScanProblem::Error {
                        message: error,
                        path: Some(path.display().to_string()),
                    },
                );
            }
            for entry in outcome.entries {
                let is_direct_transcript = entry.kind == EntryKind::File
                    && entry.path.extension().and_then(|part| part.to_str()) == Some("jsonl")
                    && entry
                        .path
                        .strip_prefix(&path)
                        .ok()
                        .is_some_and(|relative| relative.components().count() == 1);
                if !is_direct_transcript || !seen_paths.insert(entry.path.clone()) {
                    continue;
                }
                Self::record_session(
                    &entry.path,
                    &path,
                    installation,
                    active,
                    options,
                    &mut sessions,
                    &mut resources,
                    &mut problems,
                );
            }
        }
        (sessions, resources, problems)
    }

    /// Desktop mirrors CLI transcripts below per-session `.claude` roots.
    /// Only those verified transcript trees are scanned; the surrounding VM,
    /// credentials, audit logs and user outputs remain entirely out of scope.
    fn scan_desktop_projects(
        root: &Path,
        installation: &AgentInstallation,
        options: &ScanOptions,
    ) -> (Vec<Session>, Vec<Resource>, BTreeMap<String, ScanProblem>) {
        let mut sessions = Vec::new();
        let mut resources = Vec::new();
        let mut problems = BTreeMap::new();
        let sessions_root = root.join("local-agent-mode-sessions");
        if !sessions_root.is_dir() {
            problems.insert(
                "desktop-sessions-missing".into(),
                ScanProblem::Warning {
                    message: "Claude Desktop has no local-agent-mode sessions".into(),
                },
            );
            return (sessions, resources, problems);
        }
        for entry in walk_tree(&sessions_root).entries {
            if entry.kind != EntryKind::Dir
                || entry.path.file_name().and_then(|name| name.to_str()) != Some(".claude")
            {
                continue;
            }
            let (found_sessions, found_resources, found_problems) =
                Self::scan_projects(&entry.path, installation, Self::running(), options);
            sessions.extend(found_sessions);
            resources.extend(found_resources);
            problems.extend(found_problems);
        }
        (sessions, resources, problems)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_session(
        transcript: &Path,
        project_dir: &Path,
        installation: &AgentInstallation,
        active: bool,
        options: &ScanOptions,
        sessions: &mut Vec<Session>,
        resources: &mut Vec<Resource>,
        problems: &mut BTreeMap<String, ScanProblem>,
    ) {
        let meta = match Self::first_line_metadata(transcript) {
            Ok(meta) => meta,
            Err(error) => {
                Self::record_unknown(
                    transcript,
                    installation,
                    options,
                    resources,
                    problems,
                    error,
                );
                return;
            }
        };
        let Some(id) = meta.session_id else {
            Self::record_unknown(
                transcript,
                installation,
                options,
                resources,
                problems,
                "transcript metadata has no sessionId".into(),
            );
            return;
        };
        let file_id = transcript.file_stem().and_then(|value| value.to_str());
        if file_id != Some(id.as_str()) {
            Self::record_unknown(
                transcript,
                installation,
                options,
                resources,
                problems,
                "sessionId does not match transcript filename".into(),
            );
            return;
        }

        let side_dir = project_dir.join(&id);
        let mut paths = vec![transcript.to_path_buf()];
        if side_dir.is_dir() {
            paths.push(side_dir);
        }
        let (size, measurement_problems) = Self::measure_unit(&paths);
        for (key, problem) in measurement_problems {
            problems.insert(key, problem);
        }
        let session_id = SessionId::new(id.clone());
        let resource_id = ResourceId::new(format!(
            "{INSTALLATION_ID}:session:{}:{}",
            project_dir.display(),
            id
        ));
        let project = meta.cwd.map(ProjectRef::from_cwd);
        let lifecycle = if active {
            // A global Claude process cannot identify the writer's session;
            // Unknown prevents later policy from treating it as inactive.
            SessionLifecycle::Unknown
        } else {
            SessionLifecycle::Inactive
        };
        let mut metadata = serde_json::Map::new();
        if let Some(version) = meta.version {
            metadata.insert("version".into(), serde_json::Value::String(version));
        }
        resources.push(Resource {
            id: resource_id.clone(),
            provider: ProviderId::new(ProviderId::CLAUDE_CODE),
            installation_id: installation.id.clone(),
            kind: ResourceKind::Session,
            locator: ResourceLocator::FileSet {
                paths: paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            },
            ownership: Ownership::Exclusive,
            managed_by: ManagedBy::Agent,
            size,
            session_id: Some(session_id.clone()),
            project: project.clone(),
            created_at: None,
            updated_at: None,
            dependencies: vec![],
            metadata: serde_json::Map::new(),
        });
        sessions.push(Session {
            id: session_id,
            provider: ProviderId::new(ProviderId::CLAUDE_CODE),
            installation_id: installation.id.clone(),
            title: None,
            project,
            created_at: None,
            updated_at: None,
            lifecycle,
            size,
            resource_refs: vec![ResourceRef {
                id: resource_id,
                kind: ResourceKind::Session,
            }],
            metadata,
        });
    }

    fn first_line_metadata(path: &Path) -> Result<ClaudeLine, String> {
        let file = fs::File::open(path).map_err(|error| error.to_string())?;
        let line = BufReader::new(file)
            .lines()
            .next()
            .ok_or_else(|| "empty transcript".to_string())?
            .map_err(|error| error.to_string())?;
        // `ClaudeLine` deliberately declares no message/content field: serde
        // skips those bodies while extracting only schema metadata.
        serde_json::from_str(&line).map_err(|error| error.to_string())
    }

    fn measure_unit(paths: &[PathBuf]) -> (SizeInfo, BTreeMap<String, ScanProblem>) {
        let mut usage = UsageAccumulator::new();
        let mut problems = BTreeMap::new();
        for path in paths {
            let outcome = walk_tree(path);
            for problem in outcome.problems {
                let WalkProblem::Unreadable { path, error } = problem;
                problems.insert(
                    format!("unreadable:{}", path.display()),
                    ScanProblem::Error {
                        message: error,
                        path: Some(path.display().to_string()),
                    },
                );
            }
            for entry in outcome
                .entries
                .iter()
                .filter(|entry| entry.kind == EntryKind::File)
            {
                usage.add_file(entry);
            }
        }
        let usage = usage.finish();
        let confidence = if usage.unidentifiable_files == 0 {
            SizeConfidence::Exact
        } else {
            SizeConfidence::Estimated
        };
        (
            SizeInfo {
                logical_bytes: usage.logical_bytes,
                allocated_bytes: usage.allocated_bytes,
                exclusive_bytes: Some(usage.logical_bytes),
                confidence,
                ..SizeInfo::default()
            },
            problems,
        )
    }

    fn record_unknown(
        transcript: &Path,
        installation: &AgentInstallation,
        options: &ScanOptions,
        resources: &mut Vec<Resource>,
        problems: &mut BTreeMap<String, ScanProblem>,
        reason: String,
    ) {
        let path = transcript.display().to_string();
        problems.insert(
            format!("unrecognized-transcript:{path}"),
            ScanProblem::Warning { message: reason },
        );
        if options.include_unknown {
            resources.push(Resource {
                id: ResourceId::new(format!("{INSTALLATION_ID}:unknown:{path}")),
                provider: ProviderId::new(ProviderId::CLAUDE_CODE),
                installation_id: installation.id.clone(),
                kind: ResourceKind::Unknown,
                locator: ResourceLocator::File { path },
                ownership: Ownership::Unknown,
                managed_by: ManagedBy::Agent,
                size: SizeInfo::unknown(),
                session_id: None,
                project: None,
                created_at: None,
                updated_at: None,
                dependencies: vec![],
                metadata: serde_json::Map::new(),
            });
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeLine {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    version: Option<String>,
}

#[async_trait::async_trait]
impl AgentProviderAdapter for ClaudeCodeAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CLAUDE_CODE)
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    async fn detect(&self) -> anyhow::Result<Vec<AgentInstallation>> {
        let Some(home) = Self::home_dir() else {
            return Ok(vec![]);
        };
        let root = Self::root(&home);
        let mut installations = Vec::new();
        if root.exists() {
            installations.push(AgentInstallation {
                id: INSTALLATION_ID.into(),
                provider: self.id(),
                platform: Self::platform(),
                version: None,
                data_roots: vec![root.display().to_string()],
                status: if Self::is_readable_dir(&root) {
                    InstallationStatus::Available
                } else {
                    InstallationStatus::PermissionRequired
                },
            });
        }
        #[cfg(windows)]
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let desktop_root = PathBuf::from(local_app_data).join("Claude-3p");
            if desktop_root.exists() {
                installations.push(AgentInstallation {
                    id: "claude-code:desktop".into(),
                    provider: self.id(),
                    platform: Platform::Windows,
                    version: None,
                    data_roots: vec![desktop_root.display().to_string()],
                    status: if Self::is_readable_dir(&desktop_root) {
                        InstallationStatus::Available
                    } else {
                        InstallationStatus::PermissionRequired
                    },
                });
            }
        }
        Ok(installations)
    }

    async fn inspect(
        &self,
        installation: &AgentInstallation,
    ) -> anyhow::Result<ProviderInspection> {
        let readable_roots = installation
            .data_roots
            .iter()
            .map(|root| (root.clone(), Self::is_readable_dir(Path::new(root))))
            .collect();
        Ok(ProviderInspection {
            version: installation.version.clone(),
            schema_versions: BTreeMap::new(),
            journal_modes: BTreeMap::new(),
            agent_running: Self::running(),
            readable_roots,
            unknown_structures: vec![],
            problems: BTreeMap::new(),
        })
    }

    async fn capabilities(
        &self,
        inspection: &ProviderInspection,
    ) -> anyhow::Result<AgentCapabilities> {
        let readable = !inspection.readable_roots.is_empty()
            && inspection.readable_roots.values().all(|readable| *readable);
        let status = if readable {
            CapabilityStatus::ReadOnly
        } else {
            CapabilityStatus::PermissionRequired
        };
        Ok(AgentCapabilities::build([
            (CapabilityTopic::Sessions, status),
            (CapabilityTopic::Projects, status),
        ]))
    }

    async fn scan(
        &self,
        installation: &AgentInstallation,
        options: &ScanOptions,
    ) -> anyhow::Result<AgentSnapshot> {
        let root = installation
            .data_roots
            .first()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Claude Code installation has no state root"))?;
        let (sessions, resources, problems) = if installation.id == "claude-code:desktop" {
            Self::scan_desktop_projects(&root, installation, options)
        } else {
            Self::scan_projects(&root, installation, Self::running(), options)
        };
        Ok(AgentSnapshot {
            installation: installation.clone(),
            sessions,
            resources,
            problems,
            completed_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64,
        })
    }

    /// Phase 6: no cleanup units yet. Claude Code transcripts + sidecar
    /// dirs stay report-only until the Phase 7 provider beta wires the
    /// `FileSet` unit shape (`*.jsonl` + same-named side directory as one
    /// atomic unit) with its safety doc. The Windows Desktop VM-mirror
    /// installation stays out of scope entirely (`Start.md` §2.3).
    async fn build_cleanup_units(
        &self,
        snapshot: &AgentSnapshot,
    ) -> anyhow::Result<Vec<CleanupUnit>> {
        let _ = snapshot;
        Ok(Vec::new())
    }

    /// Phase 6: no units, no provider-specific preconditions.
    async fn validate_cleanup_unit(
        &self,
        unit: &CleanupUnit,
    ) -> anyhow::Result<Vec<CleanupPrecondition>> {
        let _ = unit;
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "agenttidy-claude-code-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            TEMP_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join("projects/example-project")).unwrap();
        root
    }

    fn installation(root: &Path) -> AgentInstallation {
        AgentInstallation {
            id: INSTALLATION_ID.into(),
            provider: ProviderId::new(ProviderId::CLAUDE_CODE),
            platform: ClaudeCodeAdapter::platform(),
            version: None,
            data_roots: vec![root.display().to_string()],
            status: InstallationStatus::Available,
        }
    }

    #[test]
    fn scan_groups_transcript_and_side_directory_without_parsing_message_content() {
        let root = temp_root();
        let project = root.join("projects/example-project");
        let id = "9521178d-ae3a-4959-b9ac-7591e7eaf5d6";
        fs::write(
            project.join(format!("{id}.jsonl")),
            format!("{{\"type\":\"user\",\"sessionId\":\"{id}\",\"cwd\":\"/tmp/project\",\"version\":\"2.1.218\",\"message\":{{\"content\":\"not retained\"}}}}\n"),
        )
        .unwrap();
        fs::create_dir_all(project.join(id).join("subagents")).unwrap();
        fs::write(
            project.join(id).join("subagents/agent-1.jsonl"),
            "subagent body",
        )
        .unwrap();
        let installation = installation(&root);
        let (sessions, resources, problems) =
            ClaudeCodeAdapter::scan_projects(&root, &installation, false, &ScanOptions::default());

        assert!(problems.is_empty());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id.as_str(), id);
        assert_eq!(
            sessions[0].project.as_ref().unwrap().cwd.as_deref(),
            Some("/tmp/project")
        );
        assert_eq!(resources.len(), 1);
        assert!(matches!(
            resources[0].locator,
            ResourceLocator::FileSet { .. }
        ));
        assert!(resources[0].size.logical_bytes > 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mismatched_filename_is_blocked_unless_unknowns_are_requested() {
        let root = temp_root();
        fs::write(
            root.join("projects/example-project/not-the-session-id.jsonl"),
            "{\"type\":\"user\",\"sessionId\":\"actual-id\"}\n",
        )
        .unwrap();
        let installation = installation(&root);
        let (sessions, resources, problems) =
            ClaudeCodeAdapter::scan_projects(&root, &installation, false, &ScanOptions::default());
        assert!(sessions.is_empty());
        assert!(resources.is_empty());
        assert_eq!(problems.len(), 1);

        let (_, resources, _) = ClaudeCodeAdapter::scan_projects(
            &root,
            &installation,
            false,
            &ScanOptions {
                include_unknown: true,
            },
        );
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, ResourceKind::Unknown);
        fs::remove_dir_all(root).unwrap();
    }
}
