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

Pre-alpha. Project skeleton only — see the
[development phases](./Start.md#19-开发顺序). First release targets read-only
analysis of **WorkBuddy**, **Claude Code** and **Codex** on macOS and Windows.

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

## License

[MIT](./LICENSE) — see [CHANGELOG.md](./CHANGELOG.md) for release history.
