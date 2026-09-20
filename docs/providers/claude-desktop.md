# Claude Desktop Investigation (claude-code:desktop installation)

> Status: **In progress** — Windows sample collected 2026-09-20 (Claude Desktop 2.2553.1, bundled Claude Code CLI 2.1.275 inside Linux VM).
>
> macOS sample not yet collected. Filled from real samples, not guesses.
>
> **Modelling**: Claude Desktop is **not** its own `ProviderId`. It is the second `AgentInstallation` under `claude-code` (see `crates/core/src/provider.rs` and `claude-code.md`). Use this doc to understand its storage so the Claude Code Provider can detect it as `claude-code:desktop` and dedupe its embedded `.claude/projects/` against the CLI's `%USERPROFILE%\.claude\projects\`.

## Why this is the second `claude-code` installation, not its own provider

Claude Code CLI and Claude Desktop share the **same** `ProviderId` because they:

1. **Share session identity** — Desktop's `local-agent-mode-sessions/<account>/<profile>/<session>/.claude/projects/<slug>/<uuid>.jsonl` is a literal byte-for-byte mirror of the CLI's `%USERPROFILE%\.claude\projects\<slug>\<uuid>.jsonl` for any session Desktop launched. The `(slug, session-uuid)` pair identifies the same conversation across both storage locations.
2. **Invoke the same CLI binary** — Desktop spawns a Linux VM whose rootfs contains `claude-code-vm/<ver>/claude` (the bundled Claude Code CLI binary, Linux variant). It is the same product, just executed under Electron supervision with a sandboxed FS.
3. **Use the same on-disk schema** — Inside the VM, the `.claude/` tree follows the same conventions documented in `claude-code.md` (slug mapping, JSONL line types, `subagents/`, `file-history/`, `history.jsonl`, …).

What makes it a **separate installation** rather than the same one is:
- **Different data roots** — `%LOCALAPPDATA%\Claude-3p\` (Desktop) vs `%USERPROFILE%\.claude\` (CLI). Both can exist on one machine.
- **Different session bookkeeping** — Desktop maintains its own `<account>/<profile>/<session>/` envelope plus `claude-code-sessions/` and `spaces-present/` for Desktop-managed state, none of which the CLI writes.
- **Different capabilities** — Desktop bundles a Linux VM (`vm_bundles/`, ~9.6 GB), has account-level credentials (`host-creds-<account>.json`), audit logs (`audit.jsonl`), and a separate config (`claude_desktop_config.json`). For v0.1, Desktop is **read-only** (`CapabilityStatus::ReadOnly` or `Unsupported`); the CLI is the supported surface.

Adding a new `ProviderId` would require the Claude Code Provider to know about two ProviderIds' worth of session dedup, and would still need a closed-set key (`ProviderId` is closed per `crates/core/src/provider.rs`). Splitting into `claude-code-cli` + `claude-code-desktop` would also force `registry.rs`, fixture directories, capability matrices, and the UI to grow a new Provider just to model "same product, two storage locations" — exactly the kind of branching the current `data_roots: Vec<String>` plus per-installation `installation_id` was designed to absorb.

## Verified version matrix

| Platform | App version | Bundled CLI version | Sample date | Notes |
|---|---|---|---|---|
| Windows 10 Home China (64-bit) | 2.2553.1 (`config.json`: `updaterLastSeenVersion`, `version_first_launch.version`) | 2.1.275 (`claude-code-vm/.sdk-version`) | 2026-09-20 | `deploymentMode: "3p"` (third-party / cowork enabled); Linux VM runtime |

## Installation & data locations (Windows)

This is the data root of the `claude-code:desktop` installation. Root: `%LOCALAPPDATA%\Claude-3p\` (e.g. `C:\Users\18712\AppData\Local\Claude-3p\`). The `-3p` suffix marks the "cowork" / third-party build. **Total ~10 GB in sample — by far the largest single installation footprint on this machine.**

| Path | Role | Size (sample) | Cleanup class |
|---|---|---|---|
| `vm_bundles/claudevm.bundle/rootfs.vhdx` | **Linux VM root filesystem** (ext4 image) | **8.0 GB** | Program files — never clean (regenerable on next launch, but ~8 GB is significant) |
| `vm_bundles/claudevm.bundle/rootfs.vhdx.zst` | Compressed rootfs seed | 1.2 GB | Program files |
| `vm_bundles/claudevm.bundle/initrd`, `initrd.zst` | VM initrd | ~28 MB each | Program files |
| `vm_bundles/claudevm.bundle/vmlinuz`, `vmlinuz.zst` | VM kernel | ~15 MB each | Program files |
| `vm_bundles/claudevm.bundle/sessiondata.vhdx` | **VM session data** (changes since boot) | **484 MB** | Program files (wiped on shutdown? verify) |
| `vm_bundles/claudevm.bundle/smol-bin.vhdx` | Small binary overlay | 36 MB | Program files |
| `claude-code/2.1.275/claude.exe` | Bundled Claude Code CLI binary (Windows host copy, not used at runtime) | 224 MB | Program files |
| `claude-code-vm/2.1.275/claude` | Bundled Claude Code CLI binary (Linux VM copy, the one actually run) | 221 MB | Program files |
| `claude-code-sessions/<account>/<session>/` | Desktop-managed Claude Code session bookkeeping | ~0 MB in sample (mostly empty placeholders) | Per-session |
| `local-agent-mode-sessions/<account>/<profile>/<session>/` | **Cowork / local agent session workspaces** | 3.8 MB in sample | Per-session |
| `local-agent-mode-sessions/<account>/<profile>/<session>/.claude/projects/<slug>/<uuid>.jsonl` | Embedded Claude Code CLI session transcripts inside the VM workspace | — | Per-session (mirror of `%USERPROFILE%\.claude\projects\`) |
| `local-agent-mode-sessions/<account>/<profile>/<session>/.audit-key`, `audit.jsonl` | Per-session audit key + audit log | — | Never clean (compliance) |
| `local-agent-mode-sessions/<account>/<profile>/<session>/memory/`, `usage-ledger/`, `outputs/`, `uploads/` | Cowork session artifacts | — | Per-session / report-only |
| `local-agent-mode-sessions/skills-plugin/<plugin-id>/` | Installed skills plugin assets | — | Program files |
| `spaces-present/<account>/<profile>/` | "Cowork space" workspace presence records | — | Per-workspace |
| `blob_storage/<uuid>/` | Content-addressed blobs (Desktop-managed, NOT Claude Code CLI `file-history/`) | 0 MB in sample | Shared |
| `model-catalog/published.json`, `published-floor.json` | Cached model catalog | 0.16 MB | Derivable cache |
| `config.json` | App-level config (`updaterLastSeenVersion`, `first_launch_at`, window state, monitor info) | 2.7 KB | App state — never clean |
| `claude_desktop_config.json` | Cowork preferences (`deploymentMode`, `coworkUserFilesPath`, …) | 1.3 KB | App state — never clean |
| `Preferences`, `Local State`, `window-state.json` | Electron Chromium preferences | <1 KB | App state |
| `Local Storage/leveldb/`, `Session Storage/`, `IndexedDB/` | Electron Chromium storage (LevelDB) | 0.02 MB | App state |
| `Cache/Cache_Data/`, `Code Cache/`, `GPUCache/`, `DawnGraphiteCache/`, `DawnWebGPUCache/` | Chromium/Electron caches | ~5 MB combined | Derivable cache |
| `Network/Cookies`, `Network/TransportSecurity` | Chromium network state | 0.06 MB | App state |
| `Shared Dictionary/cache/`, `db` | Chromium SharedDictionary (HTTP compression dictionary cache) | 0.04 MB | Derivable cache |
| `WebStorage/QuotaManager` | Chromium quota tracking | 0.04 MB | App state |
| `Crashpad/`, `sentry/` | Crash reporter + Sentry breadcrumbs | 0.01 MB | Diagnostics — never clean (support data) |
| `logs/main.log`, `claude.ai-web.log`, `mcp.log`, `ssh.log`, `cowork_vm_node.log` | Per-subsystem logs | 0 KB (empty in sample) | Logs |
| `logs/cli-diagnostics.jsonl` | Claude Code CLI diagnostic stream (per-session metadata, no message bodies) | 12 KB | Logs |
| `blob_storage/` | Content-addressed Desktop blobs | 0 MB | Shared |
| `ant-did` | Anonymous device identifier (base64 UUID) | 48 B | Identity — never log |
| `host-creds-*.json` | Host credentials (per-account secrets — **never read, never log**) | 0.3 KB | Never clean |
| `mcp-user-tool-toggles.json`, `cowork-enabled-cli-ops.json`, `git-worktrees.json` | Per-user MCP / CLI toggle state | <0.2 KB each | App state |
| `lockfile`, `declarative_performance_observer.db`, `DIPS`, `DIPS-wal` | Telemetry / locking / declarative observer | 0 KB | App state — never clean |
| `Local State` | Chromium `Local State` JSON | 0.5 KB | App state |

## Why this matters for scanning

1. **The biggest disk consumer on a typical cowork user is the VM rootfs** (`vm_bundles/claudevm.bundle/rootfs.vhdx`, ~8 GB). When Claude Desktop launches cowork, it spawns a Linux VM whose rootfs is this image; on shutdown it may or may not wipe `sessiondata.vhdx` (the diff overlay). User expectation: "I closed the app, why is there 8 GB of data?" — but the data is regenerable on next launch.
2. **Claude Desktop duplicates Claude Code CLI's session storage** in two ways:
   - Inside each `local-agent-mode-sessions/<account>/<profile>/<session>/.claude/projects/<slug>/` there is a copy of the Claude Code project tree (created when the session was launched; may drift from `%USERPROFILE%\.claude\projects\` after the session ends).
   - `claude-code-sessions/<account>/<session>/` keeps Desktop-side scheduling metadata (scheduled-tasks.json, profile-origin.json) separate from the CLI's history.
3. **Account identity is a directory level**: `<account-uuid>/` (e.g. `0966ea55-328b-4e2a-bcfd-7be84dffc532`) appears throughout. This is the same UUID as in `config.json`'s `lastKnownAccountUuid` and in `coworkUserFilesPath` — it ties the Desktop data to the user's Claude account, not the local machine user.
4. **`coworkUserFilesPath` (`C:\Users\18712\Claude` in sample) is a separate per-account workspace root** that the Desktop app uses for the user's files when in cowork mode. It is currently absent on this machine (no `C:\Users\18712\Claude` directory), so the Desktop app may create it on first cowork launch, or it lives behind the VM.

## Version discovery mechanism

- `config.json` → `updaterLastSeenVersion` (e.g. `"2.2553.1"`), `version_first_launch.version` (first-launch version, e.g. `"2.2553.1"`), `first_launch_at` (epoch ms).
- `claude-code-vm\.sdk-version` → bundled CLI version (e.g. `"2.1.275"`).
- `claude_desktop_config.json` → `deploymentMode` (e.g. `"3p"`), `preferences` (cowork toggles, account UUIDs).
- `ant-did` → base64-encoded anonymous device UUID (no PII, but stable per device).

## Session identity & storage structure

Claude Desktop maintains **three parallel session layers**:

1. **Cowork / local-agent sessions** (`local-agent-mode-sessions/<account>/<profile>/<session-uuid>/`):
   - `<session-uuid>/` is a UUID; the profile is a local profile number (e.g. `00000000`); the account is the signed-in Claude account UUID.
   - Each session dir contains a full Claude Code CLI workspace:
     - `<session>/.claude/projects/<cwd-slug>/<uuid>.jsonl` — embedded CLI session transcript (a second copy of data also in `%USERPROFILE%\.claude\projects\`)
     - `<session>/.claude/sessions/`, `tasks/`, `backups/` — embedded CLI bookkeeping
     - `<session>/.audit-key`, `audit.jsonl` — Desktop-side audit (compliance)
     - `<session>/memory/`, `usage-ledger/` — Desktop session memory + usage records
     - `<session>/outputs/`, `uploads/`, `uploads-tmp/` — Desktop session artifacts (user files produced by the agent — **user data, Blocked**)
   - `local-agent-mode-sessions/skills-plugin/<plugin-uuid>/` — installed skills plugin assets (per-account, not per-session).

2. **Cowork spaces** (`spaces-present/<account>/<profile>/`):
   - Tracks which "cowork spaces" (multi-session workspaces) the user has open. Empty in sample.

3. **CLI bookkeeping** (`claude-code-sessions/<account>/<session-uuid>/`):
   - Per-session Desktop-side metadata (`scheduled-tasks.json`, `profile-origin.json`).
   - Empty placeholders in sample.

The **embedded `<session>/.claude/projects/<slug>/<uuid>.jsonl`** duplicates the Claude Code CLI's `%USERPROFILE%\.claude\projects\<slug>\<uuid>.jsonl`. AgentTidy scanning Claude Code should treat these as **shared/duplicate resources** with the CLI's transcripts — not as independent sessions.

## Archive / lifecycle semantics

- No archive semantics observed at the Claude Desktop layer. Session files remain on disk indefinitely.
- Cowork sessions are referenced from `claude_desktop_config.json.preferences.epitaxyPrefs` (e.g. `desktop-frame.paneStore.v1.state.lastPrimaryCodeSession`) and from `lastKnownAccountUuid`; deletion safety unknown.
- Inside the embedded `.claude/`, the usual Claude Code CLI semantics apply (see `claude-code.md`).

## Project mapping

- Cowork sessions are tied to the user's chosen `coworkUserFilesPath` (`C:\Users\18712\Claude` in config — currently absent on this machine).
- The embedded `.claude/projects/<cwd-slug>/` inside each session follows the same slug scheme as Claude Code CLI: `/` → `-`, all other characters preserved verbatim.
- The Desktop-side `<account-uuid>/<profile>/<session-uuid>/` path is purely Desktop bookkeeping; it does not map to a project.

## Resource dependency graph

```
%LOCALAPPDATA%\Claude-3p
├── vm_bundles/claudevm.bundle/
│   ├── rootfs.vhdx         (regenerable; the heavy one)
│   ├── rootfs.vhdx.zst     (seed)
│   ├── sessiondata.vhdx    (per-session overlay; verify cleanup on shutdown)
│   ├── vmlinuz, initrd     (kernel + initrd)
│   └── smol-bin.vhdx       (small binary overlay)
├── claude-code/<ver>/claude.exe          (Windows host copy; not used at runtime)
├── claude-code-vm/<ver>/claude           (Linux VM copy; the one that runs)
├── claude-code-sessions/<account>/<session>/
│   ├── scheduled-tasks.json
│   └── <session>.profile-origin.json
├── local-agent-mode-sessions/
│   ├── <account>/<profile>/<session-uuid>/
│   │   ├── .claude/projects/<slug>/<uuid>.jsonl   (embedded CLI transcript — duplicate of %USERPROFILE%\.claude\)
│   │   ├── .claude/sessions/, tasks/, backups/
│   │   ├── audit.jsonl, .audit-key               (compliance)
│   │   ├── memory/, usage-ledger/
│   │   └── outputs/, uploads/                     (user artifacts, Blocked)
│   └── skills-plugin/<plugin-uuid>/              (program files)
├── spaces-present/<account>/<profile>/           (workspace presence)
├── blob_storage/<uuid>/                          (Desktop content-addressed blobs)
├── model-catalog/published*.json                 (derivable cache)
├── Cache/, Code Cache/, GPUCache/, Dawn*Cache/    (Chromium caches)
├── Local Storage/leveldb/, Session Storage/, IndexedDB/  (Chromium storage)
├── Shared Dictionary/cache, db                    (HTTP compression dictionary cache)
├── WebStorage/                                  (Chromium quota)
├── Network/Cookies, TransportSecurity           (Chromium network state)
├── Crashpad/, sentry/                           (crash + diagnostics)
├── logs/main.log, cowork_vm_node.log, mcp.log, ssh.log, claude.ai-web.log
├── logs/cli-diagnostics.jsonl                   (CLI diagnostics, no bodies)
├── config.json                                  (app config — version discovery)
├── claude_desktop_config.json                   (cowork prefs — version discovery)
├── Preferences, Local State, window-state.json  (Chromium prefs)
├── ant-did                                      (anonymous device id)
├── host-creds-<account>.json                    (per-account secrets — never log)
├── mcp-user-tool-toggles.json                   (per-user toggles)
├── lockfile, DIPS, DIPS-wal, declarative_performance_observer.db
└── cowork-enabled-cli-ops.json, git-worktrees.json
```

Key edges:
- `<session>/.claude/...` is a **mirror** of `%USERPROFILE%\.claude\...` — same files, different path; counting either side double-counts.
- `vm_bundles/claudevm.bundle/rootfs.vhdx` is **regenerable** but 8 GB; deleting it forces re-download on next cowork launch. Risky to classify as Low Risk without user confirmation.
- `vm_bundles/claudevm.bundle/sessiondata.vhdx` (484 MB) is the per-session overlay — if Desktop wipes it on shutdown, the file in the sample is residual.

## Caches, logs, checkpoints

- **Huge**: `vm_bundles/` total ~9.65 GB. `rootfs.vhdx` alone is 8 GB and `sessiondata.vhdx` is 484 MB. Without confirmation that Desktop cleans these on shutdown, this is the biggest "potential reclaimable" target, but it's regenerable (not archive, not user data — it's a VM image).
- Chromium caches (`Cache/`, `Code Cache/`, `GPUCache/`, `DawnGraphiteCache/`, `DawnWebGPUCache/`, `Shared Dictionary/`, `WebStorage/`) total ~5 MB combined — small.
- `model-catalog/published*.json` — small, derivable cache.
- Logs are tiny in this sample (mostly 0 KB). `logs/cli-diagnostics.jsonl` (12 KB) is the only non-empty one.

## Databases & schema

- Only `declarative_performance_observer.db` (+journal) is a SQLite DB, and it's empty/small (telemetry, not user data).
- All other state is JSON / JSONL / LevelDB (Chromium).

## Runtime write behavior

- VM is spawned on cowork launch, runs Claude Code CLI 2.1.275, writes to `local-agent-mode-sessions/<account>/<profile>/<session>/.claude/...` and to `%USERPROFILE%\.claude\...` (shared with the standalone CLI).
- `sessiondata.vhdx` is the VM's overlay — actively written while running.
- `logs/cli-diagnostics.jsonl` appended per CLI invocation.
- Electron app itself writes `Preferences`, `Local State`, `Local Storage/leveldb/` while running.

## Atomic cleanup units

- ✅ **Safe pure-FS unit (regenerable on next launch)**: `vm_bundles/claudevm.bundle/sessiondata.vhdx` — VM overlay, regenerated each launch (needs verification; otherwise productized-unsafe). 484 MB.
- ❌ **Not a unit without further confirmation**: `vm_bunddy/claudevm.bundle/rootfs.vhdx` — 8 GB, regenerable, but re-download cost is significant. Mark as `review-required`, never auto-clean.
- ❌ **Not a unit**: `claude-code/`, `claude-code-vm/` (program files), `local-agent-mode-sessions/<session>/outputs/`, `uploads/` (user data).
- ⚠ **Mirror**: `<session>/.claude/projects/<slug>/<uuid>.jsonl` duplicates `%USERPROFILE%\.claude\projects\<slug>\<uuid>.jsonl` — must not be cleaned independently of the CLI copy.

## Known risks

- **`host-creds-<account>.json` contains credentials** — never read, never log, never include in fixtures. Fixture sanitization must remove it entirely.
- **`audit.jsonl` is append-only compliance** — never delete.
- **VM bundle size dominates everything else** — 8 GB rootfs makes Claude Desktop's "data footprint" misleading. Users may expect cleanup to release tens of GB; only 500 MB (sessiondata.vhdx) is safely reclaimable, and that requires confirming the VM lifecycle.
- **Duplicate CLI transcripts** between `%USERPROFILE%\.claude\projects\` and `%LOCALAPPDATA%\Claude-3p\local-agent-mode-sessions\...\.claude\projects\` — double-counting is a real risk for any unified Claude/Claude Desktop Provider.
- **Per-account UUIDs in directory paths** — `0966ea55-328b-4e2a-bcfd-7be84dffc532` is the user's Claude account, not their local machine user. Mixing local user identity with account identity in size reports needs careful handling.
- **Empty sample**: many subsystems (`spaces-present/`, `claude-code-sessions/`, `blob_storage/`, logs) are present-but-empty in this machine; the schemas may fill in on real cowork usage. The directory structure is the source of truth, not the sizes.

## Capability degradation rules

- Missing `config.json` ⇒ cannot determine Desktop version ⇒ mark Unknown → Blocked.
- `deploymentMode != "3p"` ⇒ cowork not enabled ⇒ this Provider is just a Chromium app wrapper with no agent data; degrade to read-only listing.
- `vm_bundles/claudevm.bundle/rootfs.vhdx` missing ⇒ cowork never launched ⇒ no VM data to consider.
- Missing `local-agent-mode-sessions/` ⇒ no cowork sessions ⇒ Claude Desktop storage is just app cache (Chromium caches).

## Open questions (Phase 0 continues)

- [ ] Confirm whether `vm_bundles/claudevm.bundle/sessiondata.vhdx` is wiped on graceful shutdown, or persists (explains 484 MB residual).
- [ ] Confirm whether `vm_bundles/claudevm.bundle/rootfs.vhdx` is wiped on Desktop uninstall or persists (regenerable?).
- [ ] macOS Claude Desktop equivalent path (likely `~/Library/Application Support/Claude-3p/` — verify).
- [ ] Does Claude Desktop have an in-app "clear cowork data" / "reset" that we can call into (would invalidate any FS-level cleanup)?
- [ ] Does the embedded `<session>/.claude/projects/...` ever get reconciled back into `%USERPROFILE%\.claude\projects\` on session end, or do they truly diverge?
- [ ] What populates `claude-code-sessions/<account>/<session>/`? Empty in this sample — Desktop-managed scheduled tasks?
- [ ] Application-layer dedup: confirm the join key `(cwd-slug, session-uuid)` is sufficient, or whether account UUID (`0966ea55-…`) must also be part of the key when sessions live on different installations.
- [ ] Capability matrix for `claude-code:desktop`: should it be `ReadOnly` (data visible, no cleanup), `PermissionRequired` (credentials/`host-creds-*.json` can't be read), or `Unsupported` for v0.1?