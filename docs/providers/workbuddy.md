# WorkBuddy Provider Investigation

> Status: **In progress** — macOS sample collected 2026-09-19 (WorkBuddy 5.5.6, build 5f96929).
>
> Windows paths not yet verified. Filled from real samples, not guesses.

## Verified version matrix

| Platform | App version | Sample date | Notes |
|---|---|---|---|
| macOS (arm64, Electron app) | 5.5.6 (`last-launch.json`: version/build/timestamp) | 2026-09-19 | sessions DB covers 2026-06 → 2026-09 |

## Installation & data locations (macOS)

Root: `~/.workbuddy/` (4.4 GB total in sample). Also writes workspace scratch dirs under `~/WorkBuddy/<timestamp or name>/` (outside the root!).

| Path | Role | Size (sample) | Cleanup class |
|---|---|---|---|
| `workbuddy.db` (+wal/shm, Drizzle migrations) | **Session index DB** | 2 MB | Index — never delete rows |
| `.workbuddy-sqlite-migrations/*.sql` | Applied migration SQL files | — | Never clean |
| `projects/<cwd-slug>/<uuid>.jsonl` (+ `<uuid>/tool-results/`) | Session transcripts (JSONL) + per-session tool results | 270 MB | Per-session |
| `logs/YYYY-MM-DD/*.log`, `logs/<hash>.log` | Per-day app logs | 657 MB | Log class |
| `traces/<id>/` | Per-run traces | 481 MB | Log/trace class |
| `binaries/node`, `binaries/python` | Bundled runtimes (1.9 GB!) | 1.9 GB | Program files — never clean |
| `app/` | Electron app data (cache, session, crashpad, `sessions.json`) | 420 MB | App state |
| `plugins/`, `connectors-marketplace/`, `appearance-resources/`, `buddy-skill-store/` | Installed extensions | ~300 MB | Program-ish files |
| `blobs/` (sharded `00`..`ff`) | Content-addressed blobs | 102 MB | Shared |
| `shell-snapshots/` (812 files) | Shell env snapshots | 111 MB | Cache class |
| `clipboard-images/` (115) | Pasted images | 18 MB | Per-use |
| `file-history/<uuid>/` | File snapshots (`<hash>@v<n>`) | 20 MB | Shared / review |
| `audit-log/YYYY-MM-DD.jsonl` | Append-only audit trail | — | Never clean (compliance) |
| `changes-index/`, `changes-detail/`, `file-tree-manifests/`, `artifact-index/` | Derived indices | ~50 MB | Derivable cache |
| `edge-sync-mapping-v{1..4}.db` (+wal/shm) | Edge sync mappings — **four generations coexist**, v1–v3 stale | ~3 MB | Old generations = stale |
| `db-backups/`, `automation-backups/` | Backups | — | Backup class |
| `workspace/`, `storage/`, `security/`, `credentials/`, `memory/` | Identity / user data | — | Never clean |
| `MEMORY.md`, `SOUL.md`, `IDENTITY.md`, `USER.md`, `BOOTSTRAP.md` | Persona/identity files | — | Never clean |
| `models.json` | User model configs — **contains API keys in plaintext** | — | Never clean / never log |
| `usage-log.json`, `user-state.json`, `settings.json`, `last-launch.json` | App state | — | Never clean |

## Version discovery mechanism

- `~/.workbuddy/last-launch.json` → `{"version":"5.5.6","build":"5f96929…","timestamp":…}` (authoritative, updated each launch).
- DB migrations: `__workbuddy_drizzle_migrations` + `migration_meta` tables.

## Session identity & storage structure

- **Session ID**: UUIDv4. Two-layer storage:
  1. `workbuddy.db.sessions` row (id, cwd, user_id, title, status, created_at, updated_at, deleted_at, source_mode (`working`/`craft`/`design`), model, project_id, buddy_snapshot_id, …) — 305 rows in sample.
  2. Transcript on disk: `projects/<cwd-slug>/<uuid>.jsonl` (same slug scheme as Claude Code: `/` → `-`), plus optional side dir `<uuid>/tool-results/`.
- Transcript line types (sample): `message` (role user/assistant), `function_call`, `function_call_result`, `reasoning`, `file-history-snapshot`. Lines carry `id`, `timestamp` (epoch ms), `sessionId`, `cwd`, `providerData`.
- `session_usage(session_id, used, size, updated_at, credit_json)` — per-session size accounting already exists in-app.
- `workspaces(path PRIMARY KEY, last_opened_at)` — known working directories (14 in sample).
- `buddy_snapshots` — agent persona snapshots referenced by `sessions.buddy_snapshot_id`.
- `automations`, `automation_runs`, `automation_delivery_outbox`, `automation_runtime_state` — scheduled automations (14 in sample), referencing `cwds` (JSON array of paths).

## Archive / lifecycle semantics

- **Soft delete**: `sessions.deleted_at` (268 of 305 rows deleted in sample — deleted rows may keep transcripts on disk → orphaned transcript files are a real cleanup target).
- `status` values: `completed` (243), `archived` (59), `error` (2), `terminated` (1) — archive is an in-DB status, transcripts stay in place.
- **Archive ≠ permission** (red line #5) — archived status alone must not lower risk class.

## Project mapping

- `sessions.cwd` (absolute path) + filesystem slug dirs; `projects/<cwd-slug>/` mirrors cwd one-to-one.
- `workspaces` table = "known dirs" registry; `workspace-display-names.json` gives display names.
- No separate projects table (unlike Codex); project = cwd.

## Resource dependency graph

```
~/.workbuddy
├── workbuddy.db
│   ├── sessions (id, cwd, deleted_at, buddy_snapshot_id …)
│   ├── workspaces (path)
│   ├── session_usage (session size)
│   └── automations → cwds[] (workspace paths)
├── projects/<slug>/<uuid>.jsonl          (transcript; slug↔cwd)
│   └── <uuid>/tool-results/             (owned by session <uuid>)
├── blobs/<shard>/                        (content-addressed, shared)
├── file-history/<uuid>/<hash>@v<n>/      (file snapshots)
├── clipboard-images/                    (pasted images)
├── traces/<run-id>/                      (per-run traces)
└── logs/YYYY-MM-DD/*.log
```

Deleting a transcript while leaving `sessions` row (or vice versa) creates dangling references; `session_usage` rows orphan too. Automations may reference a cwd whose project dir is being considered for cleanup — check `automations.cwds` before touching a project dir.

## Caches, logs, checkpoints

- `logs/` (657 MB) and `traces/` (481 MB) are the big recoverable space, keyed by date/id — but both may be needed for support/audit; treat as Review, not auto-clean.
- `shell-snapshots/` (812 files, 111 MB) — cache class, safe-ish.
- `changes-*`, `artifact-index/`, `file-tree-manifests/` — derivable indices; regenerable in principle (verify before enabling cleanup).
- Old `edge-sync-mapping-v1..v3.db` generations — superseded by v4; stale-generation cleanup is a clear candidate unit.

## Databases & schema

- `workbuddy.db` (Drizzle ORM, WAL). Tables: `sessions`, `workspaces`, `session_usage`, `buddy_snapshots`, `automations`, `automation_runs`, `automation_delivery_outbox`, `automation_runtime_state`, migration tables.
- `edge-sync-mapping-v{1..4}.db` — separate sync DBs, generation-numbered, all present simultaneously with their own WAL files.

## Runtime write behavior

- WorkBuddy is an Electron desktop app that stays resident (SingletonLock in `app/`); `workbuddy.db-wal` actively written.
- Transcripts appended per turn; `automation` runs may write at scheduled times (14 automations in sample) — "app not running" is not guaranteed safe; check lock/process.

## Atomic cleanup units

- ✅ Candidate: transcript `projects/<slug>/<uuid>.jsonl` + `<uuid>/` side dir, **only for sessions with `deleted_at IS NOT NULL`** and consistent `session_usage` — still requires DB row handling per red line #7/#8 (v0.1 read-only: report only).
- ✅ Candidate: `edge-sync-mapping-v1..v3.db(+wal/shm)` superseded generations (needs app-not-running + v4 healthy check).
- ❌ Not units: `binaries/`, `app/`, `plugins/` (program files), `blobs/` (shared, content-addressed, no verified back-refs), `audit-log/`, `models.json`, persona files.

## Known risks

- **Plaintext API keys in `models.json`** — never display, log, or include in fixtures; fixture sanitization must scrub this file entirely.
- Deleted sessions (88% of sample) leave transcripts on disk — user-visible "deleted" in-app does not mean disk-freed; AgentTidy must not assume DB state == disk state.
- Session dirs live in the **same tree** as nothing else session-scoped; but `projects/<slug>/` interleaves many sessions — never delete a whole slug dir on one session's behalf.
- Automations reference cwds; a "stale" project dir may still be an automation target.
- `~/WorkBuddy/` scratch workspaces (user data!) live outside `~/.workbuddy` — out of scope, never touch.

## Capability degradation rules

- `workbuddy.db` missing/corrupt ⇒ fall back to slug-dir transcript scan (degraded: no deleted_at/status info ⇒ all sessions Unknown → Blocked for cleanup, list-only).
- Transcript first line not a `message` with `sessionId` ⇒ Unknown → Blocked.
- `last-launch.json` version older than min-supported ⇒ warn, degrade to list-only.

## Open questions (Phase 0 continues)

- [ ] Windows path (`%USERPROFILE%\.workbuddy` assumed — verify).
- [ ] Does WorkBuddy have a native "empty trash" that also removes transcripts? (worth investigating before we clean orphans)
- [ ] `traces/` and `logs/` retention knobs in-app?
- [ ] Blob GC: are `blobs/` entries ever unreferenced? Needs write-path study.
