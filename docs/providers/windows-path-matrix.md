# Windows Provider Path Matrix

> Status: **Verified** — Windows sample collected 2026-09-20.

## Verified Windows version matrix

| Platform | OS version | Architecture | Sample date | Notes |
|---|---|---|---|---|
| Windows 10 Home China | 2009 | 64-bit | 2026-09-20 | Current development machine |

## Provider path matrix

| Provider | Installation id | State root | Default workspace root | Version discovery | Notes |
|---|---|---|---|---|---|
| WorkBuddy | `workbuddy:default` | `%USERPROFILE%\.workbuddy\` | `%USERPROFILE%\WorkBuddy\` | `last-launch.json` → `version`, `build`, `timestamp` | Verified on Windows 10 |
| Claude Code | `claude-code:cli` | `%USERPROFILE%\.claude\` | None (sessions run in user cwd) | `claude --version` | Verified on Windows 10 |
| Claude Code | `claude-code:desktop` | `%LOCALAPPDATA%\Claude-3p\` | `coworkUserFilesPath` (`%USERPROFILE%\Claude\` when set) | `claude_desktop_config.json` → `deploymentMode`; `claude-code-vm\.sdk-version` for bundled CLI | Second installation of `claude-code`, v0.1 capability = `ReadOnly`. Bundles Linux VM (`vm_bundles/`, ~9.6 GB) running CLI 2.1.275. See `claude-desktop.md`. |
| Codex | `codex:default` | `%USERPROFILE%\.codex\` | `%USERPROFILE%\Documents\Codex\` | `codex --version`, `version.json` → `latest_version` | One installation serves both CLI and Desktop (shared state root). Verified on Windows 10. |

## Additional app-related paths discovered on Windows 10

These are peripheral caches / support files owned by the providers above; include in `data_roots` for completeness:

| Path | Owner | Notes |
|---|---|---|
| `%APPDATA%\WorkBuddy\` | WorkBuddy | Roaming AppData (settings) |
| `%LOCALAPPDATA%\WorkBuddy\` | WorkBuddy | Local AppData (logs) |
| `%LOCALAPPDATA%\@genieworkbuddy-desktop-updater\` | WorkBuddy updater | Install-time cache |
| `%LOCALAPPDATA%\Claude\` | Claude Code CLI | Local AppData (logs) |
| `%LOCALAPPDATA%\claude-cli-nodejs\` | Claude Code CLI | Local AppData (cache) |
| `%LOCALAPPDATA%\Claude-Data\` | Claude Code CLI | Local AppData (app state) |
| `%PROGRAMDATA%\Claude\` | Claude Code CLI | ProgramData (logs) |

## Path handling notes

### Windows-specific considerations

1. **Path separator**: `\` (backslash) instead of `/` (forward slash) on macOS.
2. **Case sensitivity**: Windows is case-insensitive; compare paths case-insensitively.
3. **Drive letters**: Paths may include drive letters (e.g. `C:\`, `D:\`).
4. **UNC paths**: Possible but not observed in sample.
5. **Long paths**: Windows has a 260-character path limit by default; `\\?\` prefix can extend this.
6. **Reserved names**: `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9` are reserved.

### Slug mapping

All three providers use the same slug mapping scheme on Windows:
- `/` → `-` (forward slash to hyphen)
- `\` → `-` (backslash to hyphen)
- All other characters preserved verbatim (spaces, CJK, dots survive)

Examples:
- `C:\Users\18712\WorkBuddy\2026-06-25-22-33-03` → `c-Users-18712-WorkBuddy-2026-06-25-22-33-03`
- `F:\Work\AgentTidy` → `F--Work-AgentTidy`
- `D:\Projects\My App` → `D--Projects-My App`

### Cross-drive considerations

- Users may have data on multiple drives (e.g. `C:\`, `D:\`).
- Provider state roots are always on the system drive (`%USERPROFILE%`).
- Default workspace roots may be on different drives if user configured them.
- Always canonicalize paths before comparison.

## Minimum supported versions

Based on Phase 0 investigation:

| Provider | Minimum version | Verified version | Notes |
|---|---|---|---|
| WorkBuddy | 5.5.6 | 5.5.6 | Only version tested |
| Claude Code | 2.1.218 | 2.1.218 | Only version tested |
| Codex | 0.145.0 | 0.145.0 | Only version tested |

**Note**: These are the only versions tested. Older versions may work but are not verified.

## Recommendations for Phase 1

1. **Path comparison**: Always use case-insensitive comparison on Windows.
2. **Canonicalization**: Use `std::fs::canonicalize()` to resolve paths before comparison.
3. **Drive letters**: Normalize drive letters to uppercase.
4. **UNC paths**: Handle `\\?\` and `\\.\` prefixes.
5. **Long paths**: Use `\\?\` prefix for paths longer than 260 characters.
6. **Reserved names**: Check for reserved filenames in cleanup operations.