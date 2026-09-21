# AgentTidy

> Keep your AI agents tidy.

**Local-first AI Agent Session & Storage Manager** — an open-source tool that
discovers, understands, browses and — safely — tidies up the local data your
AI agents leave behind on your machine.

AgentTidy is not a disk cleaner. It first answers:

1. Which agents stored data on this machine?
2. Where, and how much space does it take?
3. Which sessions, projects and resources does it belong to?
4. What can be handled safely, what needs review, and what is off-limits?

Core principles: **Local-first · Read-first · Trash-first · Fail-closed.**
See [`Start.md`](./Start.md) for the full v0.1 design document (Chinese).

## Status

Pre-alpha. See the [development phases](./Start.md#19-开发顺序) and the
[project progress changelog](./docs/CHANGELOG.md). First release targets
read-only analysis of **WorkBuddy**, **Claude Code** and **Codex** on macOS and
Windows.

## Architecture

```text
┌─────────────────────────────┐
│ Presentation                │
│ Desktop GUI / CLI           │
└──────────────┬──────────────┘
               │
┌──────────────▼──────────────┐
│ Application API             │
│ Detect / Scan / List        │
│ Plan / Validate / Execute   │
└──────────────┬──────────────┘
               │
┌──────────────▼──────────────┐
│ Core                        │
│ Models / Policy / Planning  │
└──────────────┬──────────────┘
     ┌─────────┴─────────┐
┌────▼─────────┐  ┌──────▼────────┐
│ Providers    │  │ Infrastructure│
│ WB/Claude/   │  │ FS/SQLite/     │
│ Codex        │  │ Trash/Process  │
└──────────────┘  └───────────────┘
```

## Repository layout

```text
apps/desktop/    Tauri 2 + React desktop GUI
apps/cli/        agenttidy CLI
crates/          Rust core, application API, infrastructure, provider contracts
providers/       Per-agent adapters (workbuddy, claude-code, codex)
fixtures/        Sanitized test fixtures
docs/            Provider investigation docs, safety notes, ADRs
```

## Development

Requirements: Rust (stable), Node.js ≥ 20, pnpm.

```bash
pnpm install                     # frontend dependencies
cargo build --workspace          # build all Rust crates
cargo test --workspace           # run tests
cargo run -p agenttidy-cli       # run the CLI
pnpm tauri dev                   # run the desktop app
```

## CLI v0.1 — release usage

The first CLI release is diagnostic-only: it discovers supported local agent
data and reports session and storage facts. It never changes agent data or a
workspace. Cleanup, candidate selection, time-range filtering and confirmations
belong to the future desktop GUI.

### Build a release binary

Rust stable is required. Build the binary for the current platform:

```bash
cargo build --release -p agenttidy-cli
./target/release/agenttidy --version
./target/release/agenttidy --help
```

The resulting executable is `target/release/agenttidy` on macOS/Linux and
`target\\release\\agenttidy.exe` on Windows. Distribute that binary together
with its matching platform build; do not copy a macOS binary to Windows or the
reverse.

### Commands

```bash
# Inspect discovered installations, paths, permissions and capabilities.
agenttidy doctor

# Summarize known sessions, resources and default workspace footprint.
agenttidy scan

# Print recognizable sessions, one per line.
agenttidy sessions

```

When running from a source checkout, replace `agenttidy` with
`cargo run -p agenttidy-cli --`.

Without `--json`, commands render adaptive Unicode tables with readable byte
units and coloured status cells. Table width follows `COLUMNS` when it is set,
with a safe 120-column fallback for redirected output.

### JSON for automation

Append `--json` to any diagnostic command:

```bash
agenttidy doctor --json
agenttidy scan --json
agenttidy sessions --json
```

Every JSON response uses the versioned `agenttidy.cli.v1` envelope:

```json
{
  "schema_version": "agenttidy.cli.v1",
  "command": "scan",
  "mode": "read-only",
  "data": []
}
```

Automation should check `schema_version` and `command`, and treat fields under
`data` as the command-specific contract. Human-readable output is intended for
interactive use and is not a scripting API.

### Release verification

Run these from the repository root before publishing a build:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p agenttidy-cli
./target/release/agenttidy --help
./target/release/agenttidy doctor --json
./target/release/agenttidy scan --json
```

On Windows, use `target\\release\\agenttidy.exe` in the last three commands.
The `doctor` and `scan` smoke tests should complete even when no supported
agent is installed; an empty `data` array is a valid result. A non-zero exit
means the diagnostic command could not complete.

For a machine-readable smoke assertion when `jq` is available:

```bash
./target/release/agenttidy scan --json \
  | jq -e '.schema_version == "agenttidy.cli.v1" and .command == "scan" and .mode == "read-only"'
```

## License

[MIT](./LICENSE) — see [CHANGELOG.md](./CHANGELOG.md) for release history.
