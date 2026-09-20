# Phase 0 Windows Sampling Summary

> Date: 2026-09-20
> Status: **Completed**

## Overview

Phase 0 Windows sampling has been completed. All three P0 providers (WorkBuddy, Claude Code, Codex) have been verified on Windows 10. The sampling confirms that the assumed Windows paths are correct and provides detailed information about directory structures, file sizes, and storage patterns.

## Verified providers

### WorkBuddy

- **Version**: 5.5.6 (build 5f96929)
- **State root**: `%USERPROFILE%\.workbuddy\` (verified)
- **Default workspace root**: `%USERPROFILE%\WorkBuddy\` (verified)
- **Total size**: ~960 MB (state root)
- **Key directories**:
  - `projects/`: 45.73 MB (44 JSONL files)
  - `logs/`: 108.01 MB (143 log files)
  - `binaries/`: 712.68 MB (bundled runtimes)
  - `app/`: 63.09 MB (Electron app data)
- **Database**: `workbuddy.db` (0.18 MB)
- **Slug mapping**: Windows paths use `\` → `-` conversion

### Claude Code

- **Version**: 2.1.218 CLI
- **State root**: `%USERPROFILE%\.claude\` (verified)
- **Default workspace root**: None (sessions run in user cwd)
- **Total size**: ~137 MB (state root)
- **Key directories**:
  - `projects/`: 0.55 MB (2 JSONL files)
  - `downloads/`: 130.41 MB
  - `plugins/`: 5.29 MB
- **Database**: None (JSONL only)
- **Slug mapping**: Windows paths use `\` → `-` conversion

### Codex

- **Version**: 0.145.0 CLI
- **State root**: `%USERPROFILE%\.codex\` (verified)
- **Default workspace root**: `%USERPROFILE%\Documents\Codex\` (verified)
- **Total size**: ~870 MB (state root)
- **Key directories**:
  - `sessions/`: 208.64 MB (19 JSONL files)
  - `plugins/`: 419.84 MB
  - `cache/`: 26.95 MB
  - `generated_images/`: 39.80 MB
- **Databases**:
  - `state_5.sqlite`: 0.84 MB (thread index)
  - `thread_history_1.sqlite`: 135.18 MB (rollout projection)
  - `logs_2.sqlite`: 45.24 MB (runtime logs)
- **Slug mapping**: Windows paths use `\` → `-` conversion

## Windows-specific findings

1. **Path separator**: All providers use `\` (backslash) on Windows instead of `/` (forward slash) on macOS.
2. **Case sensitivity**: Windows is case-insensitive; path comparisons must be case-insensitive.
3. **Drive letters**: Paths include drive letters (e.g. `C:\`, `F:\`).
4. **Slug mapping**: All three providers use the same slug mapping scheme: `/` → `-`, `\` → `-`.
5. **Default workspace roots**: All providers create default workspace directories outside their state roots.
6. **Additional Windows locations**: Providers also store data in:
   - `%APPDATA%\WorkBuddy\` (Roaming AppData)
   - `%LOCALAPPDATA%\WorkBuddy\` (Local AppData, logs)
   - `%LOCALAPPDATA%\Claude\` (Local AppData, logs)
   - `%LOCALAPPDATA%\Claude-3p\` (Local AppData, blob storage, cache, sessions)
   - `%LOCALAPPDATA%\claude-cli-nodejs\` (Local AppData, cache)
   - `%LOCALAPPDATA%\Claude-Data\` (Local AppData)
   - `%PROGRAMDATA%\Claude\` (ProgramData, logs)

## Files created

1. **Fixtures**: `fixtures/<provider>/config/directory-tree.json` for each provider
2. **Documentation**: Updated `docs/providers/workbuddy.md`, `docs/providers/claude-code.md`, `docs/providers/codex.md`
3. **Path matrix**: `docs/providers/windows-path-matrix.md`

## Recommendations for Phase 1

1. **Path handling**: Implement case-insensitive path comparison for Windows.
2. **Canonicalization**: Use `std::fs::canonicalize()` to resolve paths before comparison.
3. **Drive letters**: Normalize drive letters to uppercase.
4. **Long paths**: Handle `\\?\` prefix for paths longer than 260 characters.
5. **Reserved names**: Check for reserved filenames in cleanup operations.

## Next steps

1. **Phase 1**: Implement read-only core models (AgentInstallation, Session, Resource, etc.)
2. **Phase 2**: Implement infrastructure (Safe Filesystem, Read-only SQLite, etc.)
3. **Phase 3**: Implement three read-only providers
4. **Phase 4**: CLI validation (doctor, scan, sessions)

## Conclusion

Phase 0 Windows sampling is complete. All three providers have been verified on Windows 10, and the assumed paths are correct. The sampling provides detailed information about directory structures, file sizes, and storage patterns, which will be used to implement the read-only core in Phase 1.