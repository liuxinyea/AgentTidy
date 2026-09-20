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
