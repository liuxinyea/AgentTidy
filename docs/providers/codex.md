# Codex Provider Investigation

> Status: **In progress** — macOS sample collected 2026-09-19 (codex-cli 0.152.0; sample sessions written by 0.133.0 – 0.152.0 line).
>
> Windows paths not yet verified. Filled from real samples, not guesses.

## Verified version matrix

| Platform | CLI version | Sample date | Notes |
|---|---|---|---|
| macOS (arm64, Darwin 25.6.0) | 0.152.0 | 2026-09-19 | `codex --version`; `version.json` records `latest_version` 0.153.4 |
| macOS | 0.133.0 – 0.145.0-alpha | via `threads.cli_version` / `session_meta.cli_version` | schema stable across the sampled range |

## Installation & data locations (macOS)

Root: `~/.codex/`. Unlike Claude Code, Codex keeps **both** JSONL rollouts and several SQLite databases.

| Path | Role | Size (sample) | Cleanup class |
|---|---|---|---|
| `sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` | Session rollouts (append-only JSONL) | 394 MB / 91 files | Per-session |
| `archived_sessions/rollout-*.jsonl` | User-archived rollouts (flat) | 6.5 MB / 5 files | Per-session (archived) |
| `state_5.sqlite` (+ `-wal`/`-shm`) | **Thread index**: `threads`, `projects`, `thread_attachments`, … | 8.1 MB | Index — never delete rows directly |
| `thread_history_1.sqlite` (+wal) | Projection of rollout content (`thread_items`, `thread_turns`) | 128 MB | Index |
| `logs_2.sqlite` (+wal) | Append-only runtime logs (`logs` table) | 145 MB | Log cache class |
| `queue_1.sqlite`, `memories_1.sqlite`, `goals_1.sqlite` | Queue / memory / goals features | ≤1 MB each | Feature state |
| `session_index.jsonl` | UI thread list: `{id, thread_name, updated_at}` | 34 lines | Derivable index |
| `attachments/<uuid>/` | Per-thread attachment blobs | — | Per-thread |
| `cache/`, `.tmp/`, `shell_snapshots/`, `thread-writer-locks/`, `ipc/` | Caches / locks | — | Cache class |
| `config.toml`, `auth.json`, `installation_id`, `version.json`, `hooks.json`, `rules/` | Config / credentials | — | Never clean |
| `..codex-global-state.json.tmp-*` (14 files) | **Orphaned temp write leftovers** in `~/.codex/` root | ~1 MB | Garbage (see risks) |
| `models_cache.json`, `cc-switch-model-catalog.json` | Model catalog caches | — | Cache |
| `archived_sessions/` sibling dirs (`agents/`, `automations/`, `browser/`, `computer-use/`, `generated_images/`, `goals_1.sqlite`…) | Feature data | — | Not yet classified |

## Version discovery mechanism

- `codex --version` → `codex-cli 0.152.0`.
- `version.json`: `{"latest_version":"0.153.4","last_checked_at":…}` — self-update checker, not the installed version.
- Per session: `session_meta.payload.cli_version` (JSONL) and `threads.cli_version` (DB).

## Session identity & storage structure

- **Thread ID**: UUIDv7 (`019f…`). **Session (rollout) ID**: also UUIDv7; a rollout may have `parent_thread_id` ≠ its own `id` (subagent rollouts, `thread_source: "subagent"`, e.g. a `guardian` judge spawned from VS Code originator).
- **Rollout file**: `sessions/YYYY/MM/DD/rollout-<ISO-ts>-<uuid>.jsonl`, one per session, append-only. The **DB (`state_5.sqlite.threads`) is authoritative**: `threads.rollout_path` stores the absolute rollout path.
- **JSONL line types** (sample): `session_meta` (first line: `session_id`, `parent_thread_id`, `cwd`, `originator`, `cli_version`, `source`, `model_provider`), `turn_context` (per turn: `turn_id`, `cwd`, `workspace_roots`, `approval_policy`, `sandbox_policy`), `event_msg` (`task_started`, `token_count`, `task_complete`, `thread_settings_applied`, …), `response_item` (`message`, `reasoning`, …), `world_state`.
- `state_5.sqlite.threads` columns (key ones): `id`, `rollout_path`, `created_at`, `updated_at`, `source`, `model_provider`, `cwd`, `title`, `archived`, `archived_at`, `git_sha`, `git_branch`, `git_origin_url`, `cli_version`, `first_user_message`, `thread_source`, `originator`, `is_pinned`, `project_id` → `projects`.
- `thread_history_1.sqlite` = searchable projection: `thread_items(thread_id, turn_id, item_id, rollout_ordinal, item_json, …)`, `thread_turns(thread_id, turn_id, rollout_ordinal, status, duration_ms, …)`.
- `thread_spawn_edges(parent_thread_id, child_thread_id, status)` records parent/child relations.

## Archive / lifecycle semantics

- **Real archive exists**: `threads.archived` flag (5 of 96 in sample) + `archived_at` timestamp; archived rollouts are *moved* to `~/.codex/archived_sessions/` (flat layout, filename unchanged).
- `state_5.sqlite.rollout_migration_state` / `rollout_migration_skipped_rollouts` track a rollout→DB migration — schema versioning lives in `_sqlx_migrations` per DB.

## Project mapping

- Two levels: `threads.cwd` (absolute path) and optional `threads.project_id` → `projects` table (user-defined named projects, 6 in sample; `position`, metadata JSON).
- Rollout line-level: `session_meta.payload.cwd` and `turn_context.payload.workspace_roots[]`.
- No directory-slug filesystem mapping — mapping is DB-only. ⇒ Project views come from `state_5.sqlite`, not from the tree.

## Resource dependency graph

```
~/.codex
├── sessions/YYYY/MM/DD/rollout-*.jsonl      (per-session; path recorded in state_5.threads)
├── archived_sessions/rollout-*.jsonl       (same file, archived state)
├── state_5.sqlite
│   ├── threads (id → rollout_path, cwd, project_id, archived)
│   ├── projects
│   ├── thread_attachments (thread_id → attachments/<id>)
│   └── thread_spawn_edges (parent ↔ child)
├── thread_history_1.sqlite (projection of rollout content; thread_id keyed)
├── logs_2.sqlite (runtime logs; thread_id column)
├── session_index.jsonl (UI index: thread id + name)
└── attachments/<uuid>/                     (per-thread blobs)
```

Deleting a rollout file **without** DB consistency breaks: `threads.rollout_path` dangling, `thread_items` orphaned, `session_index` stale. ⇒ Cleanup must treat (rollout file + DB rows + projection rows + index lines) as one consistency set (red line #7/#8: no direct row deletion; executor protocol needed).

## Caches, logs, checkpoints

- `logs_2.sqlite` is the single largest safe-ish win (145 MB) but is append-only logging with `thread_id` references — row deletion is a DB mutation, so it must be its own reviewed cleanup unit, not free space.
- `cache/`, `models_cache.json`, `shell_snapshots/`, `.tmp/` — cache class.
- `thread-writer-locks/` — active writer locks; **never** clean while present.

## Databases & schema

- 5 SQLite DBs (`state_5`, `thread_history_1`, `logs_2`, `queue_1`, `memories_1`, `goals_1`), all WAL-mode with `-shm`/`-wal` sidecars, all with `_sqlx_migrations` (sqlx). Numbered suffixes (`state_5`) indicate **generation-bumped schemas** — old generations may coexist.
- Read access must be read-only (`mode=ro&immutable=1` URI or copy) and must tolerate WAL sidecars.

## Runtime write behavior

- WAL files actively written (`logs_2.sqlite-wal` mtime = now). Codex (CLI + VS Code extension + app server) writes concurrently.
- JSONL rollouts append with `thread-writer-locks` held during writes.

## Atomic cleanup units

- ✅ Candidate unit: archived thread = `archived_sessions/rollout-*.jsonl` + `state_5.threads` row + `thread_history_1` rows + `session_index` line — **but** DB row deletion violates red line #8 as "trash"; requires Codex-native archive/delete capability or a validated DB-transaction plan. v0.1 should stay read-only here.
- ✅ Safe pure-FS unit: nothing yet confirmed. `..codex-global-state.json.tmp-*` orphans are filesystem-only garbage (no DB refs) — small but trivially safe.
- ❌ Not units: any single SQLite file (contains cross-thread state), `cache/` only after write-behavior confirmation.

## Known risks

- **DB consistency**: naive file deletion dangles `threads.rollout_path`. Any cleanup plan must revalidate DB state.
- **Live WAL writers**: opening a DB read-write or deleting during writes corrupts state.
- **Orphaned tmp files** (`..codex-global-state.json.tmp-*`) show the atomic-write pattern fails sometimes — cleanup logic must not treat "tmp suffix" as unknown garbage blindly; verify no `.json` counterpart is newer.
- Subagent rollouts have `parent_thread_id`; deleting a parent must consider children (spawn edges).

## Capability degradation rules

- Missing `state_5.sqlite` ⇒ no thread index ⇒ read-only listing from JSONL tree only (degraded, no archive info).
- `threads.cli_version` older than min-supported ⇒ mark Unknown → Blocked.
- Rollout file present but first line not `session_meta` ⇒ Unknown → Blocked.
- `archived=1` but file not in `archived_sessions/` (or vice versa) ⇒ inconsistent ⇒ Blocked until reconciled.

## Open questions (Phase 0 continues)

- [ ] Windows path (`%USERPROFILE%\.codex` assumed — verify).
- [ ] Does Codex expose a native "delete thread" that also fixes DB? (protocol investigation)
- [ ] `logs_2.sqlite` rotation/retention policy (does Codex ever prune it?).
- [ ] Behavior of `attachments/` when its thread is archived (moved or left?).
