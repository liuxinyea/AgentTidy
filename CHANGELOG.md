# Changelog

All notable changes to AgentTidy will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial project skeleton: Rust workspace (core, application, infrastructure,
  provider-api, test-support crates; workbuddy / claude-code / codex provider
  crates; `agenttidy` CLI with placeholder subcommands), Tauri 2 + React
  desktop app shell, pnpm workspace, CI (macOS / Windows), docs and provider
  investigation placeholders.
- Provider investigation (Phase 0): filled `docs/providers/{claude-code,codex,workbuddy}.md`
  from real macOS samples — session storage structures, SQLite schemas,
  resource dependency graphs, archive semantics, atomic cleanup unit candidates
  and known risks. AGENTS.md + CLAUDE.md instruction files.
- Provider investigation update: default agent workspace directories outside
  state roots (`~/Documents/Codex/`, `~/WorkBuddy/` incl. nested `.workbuddy/`
  agent state) — detection must scan multiple common locations (Documents,
  home, app install), workspaces classify as Blocked/report-only; WorkBuddy
  user-chosen cwds with spaces/CJK verified; per-session sidecar files
  (`<uuid>.meta.json`, `<uuid>.file-rollback.ndjson`) documented.
- Provider investigation update (Windows 10, 2026-09-20): verified state
  roots for all three in-scope providers. New
  `docs/providers/claude-desktop.md` documents Claude Desktop
  (`%LOCALAPPDATA%\Claude-3p\`, ~10 GB dominated by a Linux VM rootfs) as
  the **second `claude-code` installation** (not a separate `ProviderId`).
  `docs/providers/windows-path-matrix.md` records the per-installation
  path matrix. Codex CLI + Codex Desktop share one state root and are
  modelled as a single `codex` installation. `docs/providers/phase0-windows-{summary,complete,final}.md`
  archive the full investigation.
- Sanitized Phase 0 fixtures (`fixtures/<provider>/config/directory-tree.json`
  + `fixtures/<provider>/sessions/sample-session.jsonl` for each of
  `workbuddy`, `claude-code`, `codex`). Directory skeletons are depth-1
  with zero paths/PII; session JSONLs preserve schema fields and line
  types but drop all message content, paths, identity files, trace IDs
  and usage payloads. UUIDs are kept because they are schema-relevant
  for session-link tests.

### Fixed (Phase 1 review, installation-model alignment)

- `SessionId` doc corrected: uniqueness is per *installation*, not per
  provider — the same id may legitimately appear in both `claude-code`
  installations (Desktop mirrors CLI transcripts); dedup by
  `(provider, session id)` is the application layer's job.
- `AgentInstallation.id` now documents the `<provider>:<role>` naming
  convention; test fixtures renamed `claude-code:default` →
  `claude-code:cli` to match it (5 occurrences).
- Removed unused `Session::CAPABILITY_TOPIC` const and the
  keep-import-alive `unused_topic_import_is_for_docs` test.

### Added (Phase 1: read-only core)

- `agenttidy-core` domain model: `ProviderId` (closed set, fail-closed parse),
  `AgentInstallation`/`Platform`/`InstallationStatus`, `CapabilityStatus`/
  `CapabilityTopic`/`AgentCapabilities` (missing topic ⇒ Unsupported),
  `Session`/`SessionId`/`ProjectRef`/`SessionLifecycle` (archived-as-signal),
  `Resource`/`ResourceKind`/`Ownership`/`ResourceLocator`/`ManagedBy`,
  `SizeInfo`/`SizeConfidence` (conservative merge, no fake precision),
  `AgentSnapshot`/`ScanOptions`/`ScanProblem`, static `ProviderRegistry`
  (duplicate registration is a hard error).
- `agenttidy-provider-api`: `AgentProviderAdapter` trait
  (detect/inspect/capabilities/scan, read-only) and `ProviderInspection`
  (§10.1 pre-scan facts: schema versions, journal modes, agent-running flag,
  unknown structures). Cleanup-unit methods intentionally deferred to Phase 6.

### Fixed (Phase 1 code review)

- `ProviderId` now validates on `Deserialize` as well as on `parse` — the
  closed-set invariant holds on the wire/IPC path, not just in-process
  construction (previously any string round-tripped through serde).
- `AgentProviderAdapter` is now dyn-compatible (via `async-trait`: boxed
  `Send` futures, `Send + Sync` supertraits) — the documented
  `ProviderRegistry<Box<dyn AgentProviderAdapter>>` wiring now compiles.
- `SizeInfo::merge` confidence now follows the weakest contributor: any
  unmeasured (`Unknown`) side makes the aggregate `Unknown` instead of
  `Estimated`, matching `opt_add`'s no-partial-sums stance.
- `AgentSnapshot::total_logical_bytes` replaced by `total_size()` returning
  `SizeInfo` — totals carry confidence and no longer silently count
  unmeasured resources as trustworthy zeros.
- `SessionLifecycle::Archived.archived_at` is `Option<u64>` (epoch ms)
  instead of `Option<serde_json::Value>`, matching `created_at`/`updated_at`
  and enabling the §11.1 "archived + inactive > 30d" rule.
- Removed `Session::supports_archive` (core-side hardcoded provider list) —
  archive support is a provider-reported fact via `CapabilityTopic::Archive`.
- `CapabilityStatus` and `InstallationStatus` now derive `Ord` with
  worst-to-best declaration order, making their documented ordering claims
  real (`max()` picks the strongest).
- Registry doc corrected: duplicate registration returns
  `Err(DuplicateProvider)`, it does not panic.
- `docs/providers/codex.md`: ChatGPT-dir recognition deferred — v0.1 only
  scans known provider roots, and the closed `ProviderId` set has no
  "known non-target app" representation yet.

### Added (Phase 2: infrastructure)

`crates/infrastructure` is now live (was a 7-line placeholder). The
crate exposes provider-neutral platform services with platform code
confined to `#[cfg]` branches inside its modules (red line #11).
Everything in Start.md §13.1 is implemented:

- **Path safety** (`paths`): `safe_canonicalize` (verbatim-stripped
  Windows canonical form), `strip_verbatim` (`\\?\C:\x` ↔ `C:\x`,
  `\\?\UNC\…` ↔ `\\…`), `normalize_drive_letter` (`c:` → `C:`),
  `is_within` (component-based containment, strict-descendant,
  Windows case-insensitive / macOS case-sensitive — the asymmetry is
  deliberately the conservative direction per §16.1), and
  `has_reserved_component` (`CON`/`PRN`/… — Windows-only).
- **Read-only filesystem probing** (`fs_probe`): `walk_tree` over
  `walkdir` with `follow_links(false)` — symlinks/junctions are
  returned as `EntryKind::Link` (never followed), Windows cloud
  reparse-point dirs and Unix special files are `EntryKind::Unknown`
  (caller fails closed). Each file carries `FileIdentity` (Windows
  `BY_HANDLE_FILE_INFORMATION` via `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`;
  Unix `dev`/`ino`) and `allocated_len` (Windows `GetCompressedFileSizeW`,
  Unix `st_blocks * 512`).
- **Size aggregation** (`disk_usage`): `UsageAccumulator` dedupes
  hard links by identity (§8), reports
  `hardlink_duplicates_skipped` and `unidentifiable_files`, and drops
  `allocated_bytes` to `None` on any unmeasured contribution (no
  fake precision).
- **Streaming JSONL** (`jsonl`): `open_jsonl`/`JsonlReader` — per-line
  events (`Item`/`Malformed`/`Fatal`); empty lines and parse errors
  are surfaced, never abort the stream.
- **Read-only SQLite** (`sqlite`): `ReadOnlyDb::open` does `mode=ro`
  URI open and falls back to a **private temp copy** of `db` + `-wal`
  + `-shm` opened rw on *our* copy when WAL recovery or a busy
  lock blocks the direct read. `journal_mode` and `quick_check_ok`
  surface §10.1 inspection facts. URI encoder percent-encodes
  provider-realistic paths (spaces, CJK).
- **Process signals** (`processes`): `ProcessSignature`
  (`exe_names` + `cmdline_substrings`) and `any_process_running`,
  driven by `sysinfo` 0.30 — deliberately narrow, never a decision
  (§16.2: one signal among several).
- **Trash** (`trash`): `move_to_trash` wrapping `trash` crate (Windows
  Recycle Bin / macOS NSWorkspace). Missing path = `TrashError::NotFound`
  (fail closed); locked files surface as `TrashError::Platform` for the
  Phase 6 executor to mark `skipped`.

Tests: 31 in infrastructure (31 pass on Windows). Workspace total:
55 tests pass. `cargo fmt` and `cargo clippy --workspace --all-targets
-- -D warnings` clean.

New workspace deps: `walkdir`, `rusqlite` (bundled), `trash`, `sysinfo`,
`windows-sys` (Win32_Storage_FileSystem, Win32_Foundation, Win32_Security —
last one gates `CreateFileW`'s `SECURITY_ATTRIBUTES` signature).

### Added (Phase 3: three read-only providers)

- **WorkBuddy provider** (`providers/workbuddy`): `WorkBuddyAdapter`
  implementing `AgentProviderAdapter`. `detect` checks
  `~/.workbuddy/` (state root) and `~/WorkBuddy/` (default workspace
  root); `inspect` reads `last-launch.json` for version and opens
  `workbuddy.db` for Drizzle migration schema + journal mode;
  `capabilities` reports `Sessions`/`Projects`/`Archive`/`Logs` when
  the DB is readable; `scan` reads `workbuddy.db.sessions` for
  session metadata and walks `projects/<slug>/` for JSONL transcripts
  and their sidecar dirs, plus `logs/` and `traces/` for shared
  resources. `slug_to_cwd` implements the WorkBuddy slug→cwd inverse
  (Windows: `:` dropped, `\` → `-`; Unix: `/` → `-`).
- **Claude Code provider** (`providers/claude-code`): `ClaudeCodeAdapter`.
  `detect` checks `~/.claude/`; `inspect` reads `config.json` for
  version; `capabilities` reports `Sessions`/`Projects`/`Logs`;
  `scan` walks `projects/<slug>/` for JSONL transcripts and sidecars
  (subagents, etc.), plus `file-history/` (shared, content-addressed
  checkpoints) and `shell-snapshots/` (cache). `slug_to_cwd`
  implements the Claude Code slug→cwd inverse (Windows: both `:` and
  `\` → `-`; Unix: `/` → `-`).
- **Codex provider** (`providers/codex`): `CodexAdapter`. `detect`
  checks `~/.codex/` (state root) and `~/Documents/Codex/` (default
  workspace root); `inspect` opens `state_5.sqlite`,
  `thread_history_1.sqlite`, `logs_2.sqlite` for schema + journal
  mode; `capabilities` reports `Sessions`/`Projects`/`Archive`/`Logs`
  when the state DB is readable; `scan` reads `state_5.sqlite.threads`
  for session metadata and walks `sessions/YYYY/MM/DD/` for JSONL
  rollouts, plus `archived_sessions/`, `logs_2.sqlite`, `cache/`.
  Codex sessions carry `source` and `cli_version` metadata from the
  rollout's `session_meta` payload.

Provider workspace deps added: `async-trait`, `dirs` 5.
Phase 1: `SessionLifecycle` now derives `Default` (→ `Unknown`).

Tests: 3 new in workbuddy (slug + detect), 4 in claude-code (slug +
ISO 8601 parse + detect), 3 in codex (ISO 8601 + detect). Workspace
total: 64 tests. `cargo fmt` and `cargo clippy --workspace
--all-targets -- -D warnings` clean.
