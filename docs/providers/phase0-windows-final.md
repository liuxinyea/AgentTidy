# Phase 0 Windows Sampling - Final Report

> Date: 2026-09-20
> Status: **Completed**

## Summary

Phase 0 Windows sampling has been completed successfully. All three P0 providers (WorkBuddy, Claude Code, Codex) have been verified on Windows 10. The sampling confirms that the assumed Windows paths are correct and provides detailed information about directory structures, file sizes, and storage patterns.

**Installation-model decision**: Claude Desktop is **not** a separate `ProviderId`. It is the **second `claude-code` installation** on this machine — the `claude-code:desktop` installation. It shares session identity with the CLI (mirrored transcripts) but writes to a different root (`%LOCALAPPDATA%\Claude-3p\`) and bundles a Linux VM (`vm_bundles/`, 9.6 GB). v0.1 detects it as `claude-code:desktop` with `ReadOnly` capability. Codex CLI and Codex Desktop, by contrast, share the **same** state root and are one `codex` installation with two `data_roots`. See `windows-path-matrix.md`, `claude-desktop.md`, and the new "Detected installations" section in `claude-code.md` / `codex.md`.

## Key findings

### 1. Provider verification

All three providers are installed and active on the development machine. `claude-code` has two installations:

| Provider | Installation id | Version | State root | Default workspace root | Total size |
|---|---|---|---|---|---|
| WorkBuddy | `workbuddy:default` | 5.5.6 | `%USERPROFILE%\.workbuddy\` | `%USERPROFILE%\WorkBuddy\` | ~960 MB |
| Claude Code | `claude-code:cli` | 2.1.218 | `%USERPROFILE%\.claude\` | None | ~137 MB |
| Claude Code | `claude-code:desktop` | 2.2553.1 (CLI 2.1.275) | `%LOCALAPPDATA%\Claude-3p\` | `coworkUserFilesPath` (e.g. `C:\Users\18712\Claude\`) | **~10 GB** (read-only in v0.1) |
| Codex | `codex:default` | 0.145.0 | `%USERPROFILE%\.codex\` | `%USERPROFILE%\Documents\Codex\` | ~870 MB |

### 2. Windows-specific path handling

1. **Path separator**: All providers use `\` (backslash) on Windows instead of `/` (forward slash) on macOS.
2. **Case sensitivity**: Windows is case-insensitive; path comparisons must be case-insensitive.
3. **Drive letters**: Paths include drive letters (e.g. `C:\`, `F:\`).
4. **Slug mapping**: All three providers use the same slug mapping scheme: `/` → `-`, `\` → `-`.
5. **Default workspace roots**: All providers create default workspace directories outside their state roots.

### 3. Additional Windows locations

Providers also store data in:
- `%APPDATA%\` (Roaming AppData)
- `%LOCALAPPDATA%\` (Local AppData)
- `%PROGRAMDATA%\` (ProgramData)
- `%USERPROFILE%\Documents\` (User documents)

### 4. Storage patterns

| Provider | Installation id | Session storage | Database | Logs | Caches |
|---|---|---|---|---|---|
| WorkBuddy | `workbuddy:default` | JSONL files in `projects/` | `workbuddy.db` (SQLite) | `logs/` directory | `app/`, `blobs/`, etc. |
| Claude Code | `claude-code:cli` | JSONL files in `projects/` | None (JSONL only) | `%LOCALAPPDATA%\Claude\` | `cache/`, `downloads/`, etc. |
| Claude Code | `claude-code:desktop` | Embedded `.claude/projects/` in `local-agent-mode-sessions\` + `claude-code-sessions\` | None (JSONL + Chromium LevelDB) | `logs/main.log`, `cli-diagnostics.jsonl`, … | `vm_bundles/` (9.6 GB), Chromium `Cache/`, `GPUCache/`, … |
| Codex | `codex:default` (CLI + Desktop) | JSONL files in `sessions/` | Multiple SQLite DBs | `logs_2.sqlite` | `cache/`, `plugins/`, etc. |

### 5. Cross-installation duplication risk (within `claude-code`)

`%LOCALAPPDATA%\Claude-3p\local-agent-mode-sessions\<account>\<profile>\<session>\.claude\projects\<slug>\<uuid>.jsonl` is a **mirror** of `%USERPROFILE%\.claude\projects\<slug>\<uuid>.jsonl`. The Application layer (not the Provider layer) must dedupe by `(cwd-slug, session-uuid)` pair, not by path, when merging the two `claude-code` installations into one Snapshot. The Desktop copy is the **authoritative** copy when it exists (it captured the session), but the CLI may continue writing the same file after the session ends.

## Files created

### Fixtures (sanitized samples)

1. `fixtures/workbuddy/config/directory-tree.json` - WorkBuddy directory structure
2. `fixtures/claude-code/config/directory-tree.json` - Claude Code directory structure
3. `fixtures/codex/config/directory-tree.json` - Codex directory structure
4. `fixtures/workbuddy/sessions/sample-session.jsonl` - Sample WorkBuddy session

### Documentation

1. `crates/core/src/provider.rs` - `ProviderId` constants annotated with the CLI/Desktop-installation relationship.
2. `docs/providers/workbuddy.md` - Updated with Windows paths.
3. `docs/providers/claude-code.md` - Updated with Windows paths + "Detected installations" section clarifying `claude-code:cli` / `claude-code:desktop`.
4. `docs/providers/codex.md` - Updated with Windows paths + "Detected installations" section clarifying `codex` covers both CLI and Desktop (shared root).
5. `docs/providers/claude-desktop.md` - Re-scoped from "out-of-scope Provider" to "second `claude-code` installation".
6. `docs/providers/windows-path-matrix.md` - Path matrix now lists installations (not providers) and Claude Desktop as `claude-code:desktop`.
7. `docs/providers/phase0-windows-summary.md` - Initial summary.
8. `docs/providers/phase0-windows-complete.md` - Complete summary.

## Recommendations for Phase 1

1. **Path handling**: Implement case-insensitive path comparison for Windows.
2. **Canonicalization**: Use `std::fs::canonicalize()` to resolve paths before comparison.
3. **Drive letters**: Normalize drive letters to uppercase.
4. **Long paths**: Handle `\\?\` prefix for paths longer than 260 characters.
5. **Reserved names**: Check for reserved filenames in cleanup operations.
6. **Additional locations**: Scan `%APPDATA%`, `%LOCALAPPDATA%`, `%PROGRAMDATA%`, and `%USERPROFILE%\Documents\` for provider data.
7. **Per-installation detection**: The `claude-code` adapter must emit **two** `AgentInstallation`s (`claude-code:cli`, `claude-code:desktop`) on Windows/macOS, not one. Application layer dedupes `(slug, session-uuid)` across them.
8. **Capability split**: `claude-code:cli` → supported; `claude-code:desktop` → `ReadOnly` for v0.1. UI must show both.
9. **Heavy paths**: Any path under `%LOCALAPPDATA%\Claude-3p\vm_bundles\` (9.6 GB) needs its own classification (regenerable VM image, not user data) before any size reporting — never auto-clean.

## Next steps

1. **Phase 1**: Implement read-only core models (AgentInstallation, Session, Resource, etc.) — the `data_roots: Vec<String>` and per-installation `installation_id` are already in place to absorb the two-installations case.
2. **Phase 2**: Implement infrastructure (Safe Filesystem, Read-only SQLite, etc.).
3. **Phase 3**: Implement three read-only providers. `claude-code` adapter must enumerate both installations; the dedup logic lives one layer up.
4. **Phase 4**: CLI validation (doctor, scan, sessions).

## Conclusion

Phase 0 Windows sampling is complete. All three providers have been verified on Windows 10, and the assumed paths are correct. The sampling produced detailed information about directory structures, file sizes, and storage patterns.

The most important architectural outcome of this sampling is the **installation-model decision**: Claude Code CLI and Claude Desktop are one `ProviderId` (`claude-code`) with two `AgentInstallation`s; Codex CLI and Codex Desktop are one `ProviderId` (`codex`) with one `AgentInstallation` whose `data_roots` cover both roots. This keeps `ProviderId` a closed set (Start.md §22 red line #10) and pushes storage dedup to the Application layer, where it belongs.

The next step is to proceed with Phase 1. The Windows-specific findings from this sampling will inform the implementation of path handling, canonicalization, and platform-specific code in the infrastructure layer.