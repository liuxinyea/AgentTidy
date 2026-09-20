# Claude Code Provider Investigation

> Status: **In progress** — macOS sample collected 2026-09-19 (Claude Code 2.1.235 CLI; sample session written by 2.1.274).
>
> Windows paths not yet verified. Filled from real samples, not guesses.

## Verified version matrix

| Platform | CLI version | Sample date | Notes |
|---|---|---|---|
| macOS (arm64, Darwin 25.6.0) | 2.1.235 | 2026-09-19 | `claude --version`; sessions record `version` per line |
| Windows 10 Home China (64-bit) | 2.1.218 | 2026-09-20 | `claude --version`; only 2 transcripts in `%USERPROFILE%\.claude\projects\` |

## Installation & data locations (macOS)

Root: `~/.claude/` (all state under one directory; no `~/Library/Application Support` involvement observed). Unlike WorkBuddy/Codex Desktop, **no default workspace directory is created outside the state root** — sessions run directly in the user's chosen cwd; there is nothing additional to scan under `~/Documents` or similar. Install locations (for detection only, never clean): CLI at `~/.local/bin/claude` (symlink → `~/.local/share/claude/versions/<ver>`), desktop app at `/Applications/Claude.app`. Note: on macOS the Claude Desktop equivalent of `%LOCALAPPDATA%\Claude-3p\` (Windows) is unverified — Phase 0 macOS sample did not collect it.

| Path | Role | Size (sample) | Cleanup class |
|---|---|---|---|
| `projects/<cwd-slug>/<session-id>.jsonl` | Session transcripts (JSONL, append-only) | 39 MB / 61 files | Per-session |
| `projects/<cwd-slug>/<session-id>/` | Per-session side dir (`subagents/`, `auto-mode-classifier-error.txt`) | — | Per-session |
| `projects/<cwd-slug>/memory/` | Per-project agent memory | empty in sample | Shared per project |
| `file-history/<hash>@v<n>/` | File edit snapshots (content-addressed) | 832 KB | Shared / review |
| `shell-snapshots/` | Shell env snapshot per shell PID | 2.7 MB | Cache-like |
| `session-env/` | Per-session env records (often empty) | 0 B | Per-session |
| `plans/`, `tasks/` | Plan & task outputs | <200 KB | Per-session-ish |
| `history.jsonl` | Global prompt history (one line per prompt, has `project` field) | 22 KB | Shared |
| `stats-cache.json` | Cached daily usage stats (`dailyActivity[]` with `sessionCount`) | — | Derivable cache |
| `backups/`, `cache/`, `paste-cache/`, `debug/`, `downloads/`, `ide/`, `daemon/`, `jobs/`, `skills/`, `plugins/`, `agents/`, `todo*` | App-managed caches / state | — | Not yet classified |
| `.last-cleanup`, `.last-update-result.json`, `config.json`, `settings.json` | Markers / config | — | Never clean |

`<cwd-slug>` = absolute cwd with `/` → `-` (e.g. `/Users/lxy/Desktop/MyProjects/AgentTidy` → `-Users-lxy-Desktop-MyProjects-AgentTidy`). No hashing — path is recoverable by inverse mapping, and each transcript line also carries the original `cwd`.

## Version discovery mechanism

- `claude --version` → `2.1.235 (Claude Code)`.
- Every JSONL line records the writing version in `version` (e.g. `2.1.274`), so a session can span versions.

## Session identity & storage structure

- **Session ID**: UUIDv4 (`sessionId` on every line; matches filename `<uuid>.jsonl`).
- **Transcript**: one JSONL file per session, append-only; a session has exactly one file in exactly one project dir.
- **Line types observed** (sample file): `user` (155), `assistant` (211), `attachment` (91), `queue-operation`, `atis-latch`, `last-prompt`.
- **Line-level metadata** (present on user/assistant/attachment lines): `uuid`, `parentUuid`, `timestamp` (ISO 8601 UTC), `cwd`, `gitBranch`, `sessionId`, `version`, `permissionMode`, `isSidechain`, `entrypoint`, `userType`.
- **Messages**: `message.content` is either a string or an array of `{type: text|tool_result|…}` blocks (Anthropic message shape).
- **Subagents**: nested under `projects/<slug>/<session-id>/subagents/agent-*.jsonl` + `.meta.json`; the subagent transcript's `sessionId` still points at the **parent** session UUID. Meta records `agentType`, `toolUseId`, `spawnDepth`, `requestShape`.
- **Sidechains**: lines with `isSidechain: true` stay inside the main file.
- No SQLite anywhere in `~/.claude` — JSONL is the only session store.

## Archive / lifecycle semantics

- No archive directory or archive marker observed; old sessions simply remain in `projects/` forever (sample history back to 2025-09). `.last-cleanup` records an ISO timestamp of Claude Code's own internal cleanup — semantics unknown, treat as opaque marker, not an archive.
- **Archive ≠ deletable** still applies (red line #5).

## Project mapping

- Directory name = cwd slug; every line carries original `cwd` → two independent ways to map session → project.
- `history.jsonl` lines carry `project` (cwd) for prompt-level history.
- `projects/` contains only project dirs; a project with zero sessions may still hold `memory/`.

## Resource dependency graph

```
~/.claude
├── projects/<slug>/                     (project node)
│   ├── <uuid>.jsonl                     (session transcript — the primary resource)
│   ├── <uuid>/subagents/agent-*.jsonl   (owned by session <uuid>)
│   └── memory/                          (project-shared)
├── history.jsonl                        (global; references cwd only)
├── file-history/<hash>@v<n>/            (content-addressed; NOT keyed by session — shared)
├── shell-snapshots/<pid>-…              (keyed by shell PID, ephemeral)
└── session-env/<uuid>…                  (session-scoped)
```

Key edges: session → its side dir (delete together); project → memory (do not delete with one session); file-history is content-addressed with no verified session back-reference → treat as Shared/Review.

## Caches, logs, checkpoints

- `cache/`, `paste-cache/`, `shell-snapshots/`, `stats-cache.json`, `backups/`, `debug/` — cache/backup class; sizes small in sample but classification for cleanup needs per-dir write-behavior confirmation.
- `file-history/` is the checkpoint/restore system for edited files — needed for `/rewind`; deletion trades recoverability for space.

## Databases & schema

- None (JSONL + JSON files only).

## Runtime write behavior

- Transcripts are append-only; `claude` may be running with a live file handle (`daemon/`, `ide/`, per-PID ports under `session-env/` indicate background processes exist).
- Assume "session file mtime within N minutes / process running" ⇒ active ⇒ Blocked.

## Atomic cleanup units

- ✅ Unit: `projects/<slug>/<uuid>.jsonl` + `projects/<slug>/<uuid>/` (transcript + side dir).
- ❌ Not units: `projects/<slug>/memory/` (project-shared), `file-history/` entries (no verified back-ref), `history.jsonl` lines (append-only global log; no per-line deletion format defined).

## Known risks

- Subagent transcripts live *inside* the parent session's side dir → unit deletion must include the whole `<uuid>/` dir, and must not delete sibling `memory/`.
- Session file names are plain UUIDs; two projects can theoretically hold the same UUID filename (different parent dirs) — always key by full path.
- `history.jsonl` is a global append-only log; removing sessions does not remove their prompts from history (privacy review must call this out).
- Live processes append to transcripts concurrently (daemon/IDE) → revalidate mtime before execute.

## Capability degradation rules

- If a `projects/<slug>` dir fails to parse as a slug (e.g. hand-created), treat contained sessions as Unknown → Blocked.
- If `projects/<slug>/<uuid>.jsonl` exists but lines lack `sessionId` → Unknown → Blocked.
- Missing `history.jsonl` or `stats-cache.json` degrades stats only, not session listing.

## Installation & data locations (Windows)

**Verified**: Windows sample collected 2026-09-20 (Claude Code 2.1.218 CLI).

Root: `%USERPROFILE%\.claude\` (e.g. `C:\Users\18712\.claude\`). Unlike WorkBuddy/Codex Desktop, **no default workspace directory is created outside the state root** — sessions run directly in the user's chosen cwd; there is nothing additional to scan under `%USERPROFILE%\Documents` or similar. Install locations (for detection only, never clean): CLI at `%USERPROFILE%\.local\bin\claude` (symlink → `%USERPROFILE%\.local\share\claude\versions\<ver>`), desktop app elsewhere. **Claude Desktop is a second `claude-code` installation** — see `claude-desktop.md`; v0.1 detects it as `claude-code:desktop` but treats it as read-only.

| Path | Role | Size (sample) | Cleanup class |
|---|---|---|---|
| `projects/<cwd-slug>/<session-id>.jsonl` | Session transcripts (JSONL, append-only) | 0.36 MB / 2 files | Per-session |
| `projects/<cwd-slug>/<session-id>/` | Per-session side dir (`subagents/`, `auto-mode-classifier-error.txt`) | — | Per-session |
| `projects/<cwd-slug>/memory/` | Per-project agent memory | empty in sample | Shared per project |
| `file-history/<hash>@v<n>/` | File edit snapshots (content-addressed) | — | Shared / review |
| `shell-snapshots/` | Shell env snapshot per shell PID | 0 MB | Cache-like |
| `session-env/` | Per-session env records (often empty) | 0 MB | Per-session |
| `plans/`, `tasks/` | Plan & task outputs | — | Per-session-ish |
| `history.jsonl` | Global prompt history (one line per prompt, has `project` field) | — | Shared |
| `stats-cache.json` | Cached daily usage stats (`dailyActivity[]` with `sessionCount`) | — | Derivable cache |
| `backups/`, `cache/`, `paste-cache/`, `debug/`, `downloads/`, `ide/`, `daemon/`, `jobs/`, `skills/`, `plugins/`, `agents/`, `todo*` | App-managed caches / state | — | Not yet classified |
| `.last-cleanup`, `.last-update-result.json`, `config.json`, `settings.json` | Markers / config | — | Never clean |

**Windows-specific notes**:
- Path separator: `\` (backslash) instead of `/` (forward slash) on macOS.
- Slug mapping: `/` → `-` (same as macOS), but Windows paths use `\` as separator. Example: `F:\Work\AgentTidy` → `F--Work-AgentTidy`.
- Cross-drive cwds: possible (e.g. `D:\Projects\...`), but not observed in sample.
- No default workspace directory outside state root (confirmed).
- Case sensitivity: Windows is case-insensitive; compare paths case-insensitively.

## Detected installations on Windows

`claude-code` may install twice on the same machine — the **CLI** install and the **Desktop (cowork)** install. They share session identity and on-disk schema (the Desktop's `local-agent-mode-sessions/.../.claude/projects/` is a literal mirror of the CLI's `%USERPROFILE%\.claude\projects/`), but they write to different roots and need separate `AgentInstallation`s. Provider adapter must enumerate both and emit the same `ProviderId` for each.

| Installation id | Data roots | v0.1 capability | Notes |
|---|---|---|---|
| `claude-code:cli` | `%USERPROFILE%\.claude\`, `%LOCALAPPDATA%\Claude\`, `%LOCALAPPDATA%\claude-cli-nodejs\`, `%LOCALAPPDATA%\Claude-Data\`, `%PROGRAMDATA%\Claude\` | Supported | This doc |
| `claude-code:desktop` | `%LOCALAPPDATA%\Claude-3p\` | `ReadOnly` (v0.1) | See `claude-desktop.md`. Bundles Linux VM (~9.6 GB) running the same CLI 2.1.275; data overlaps with CLI |

**Scan locations for `claude-code:cli` on Windows** (deduplicated, case-insensitive):

| Location | Content | Role in app |
|---|---|---|
| `%USERPROFILE%\.claude\` | transcripts, caches (this doc) | State root — primary scan target |
| `%LOCALAPPDATA%\Claude\` | Local AppData (logs) | Logs |
| `%LOCALAPPDATA%\claude-cli-nodejs\` | Local AppData (cache) | Cache |
| `%LOCALAPPDATA%\Claude-Data\` | Local AppData | App state |
| `%PROGRAMDATA%\Claude\` | ProgramData (logs) | Logs |

**Scan locations for `claude-code:desktop` on Windows**: see `claude-desktop.md` (single root `%LOCALAPPDATA%\Claude-3p\`). The adapter reports both installations; **the Application layer deduplicates sessions** by `(cwd-slug, session-uuid)` so a session that Desktop launched and that the CLI kept writing after Desktop quit is counted once, with the union of resources.

## Open questions (Phase 0 continues)

- [x] Windows path for `~/.claude` (`%USERPROFILE%\.claude` verified 2026-09-20).
- [ ] Whether `backups/` is safe to clean (write cadence unknown).
- [ ] `file-history` retention semantics across Claude Code versions.
- [x] Claude Desktop installation model: same `ProviderId`, second `AgentInstallation` (`claude-code:desktop`) — confirmed.
- [ ] Application-layer session dedup: pick the join key (proposed: `(cwd-slug, session-uuid)`) and confirm the more recent copy wins when both installations hold the same transcript.
- [ ] macOS counterpart of `%LOCALAPPDATA%\Claude-3p\` (likely `~/Library/Application Support/Claude-3p/` — verify).
