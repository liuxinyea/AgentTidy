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
