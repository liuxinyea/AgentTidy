# ADR-0001: Initial Architecture & Tech Stack

- Status: Accepted
- Date: 2026-09-19
- Deciders: AgentTidy maintainers

## Context

AgentTidy v0.1 is a local-first desktop tool for inspecting and safely
tidying AI-agent data on macOS and Windows. We need a stack and repo layout
that keeps a single source of truth for domain logic, keeps the GUI and CLI
from diverging, and confines platform differences to the infrastructure
layer. Full requirements: `Start.md` (esp. §13, §22).

## Decision

1. **Rust workspace** with layered crates mirroring the architecture:
   `crates/core` (domain models), `crates/application` (Application API —
   the single shared entry point), `crates/infrastructure` (platform
   services), `crates/provider-api` (adapter contracts),
   `crates/test-support`; per-agent crates under `providers/`.
2. **Tauri 2 + React + TypeScript + Tailwind CSS** for the desktop GUI;
   the Tauri crate is a thin shell that forwards to `agenttidy-application`.
   shadcn/ui will be introduced when real pages exist (v0.1).
3. **Rust CLI** (`apps/cli`, clap) calling the same Application API —
   never its own business logic.
4. **pnpm workspaces** for JS packages.
5. **Static provider registry** — no dynamic plugin system (red line #10).
6. CI on GitHub Actions: macOS + Windows runners for Rust; Linux for
   frontend build. Native Windows testing is mandatory (design doc §18.4).
7. MIT license.

## Consequences

- Domain types are intentionally not defined yet; they will be frozen only
  after Phase 0 provider investigation (avoids premature modeling).
- All cleanup-safety logic lives in core/policy; providers only describe
  facts and operations (red lines #2, #3).
- Platform-specific code may exist only in `crates/infrastructure` and
  provider path discovery (red line #11).
