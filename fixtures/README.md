# Test Fixtures

Sanitized, versioned samples of real provider data used by fixture,
contract, safety and golden tests (design doc §18).

Rules:

- **Sanitized**: no session content, credentials, tokens or user-identifying
  paths. Only structure and metadata survive.
- **Versioned**: each fixture records the provider version and schema it
  was captured from.
- **Real**: fixtures must come from real samples on real machines — never
  hand-crafted guesses.
- Layout: `fixtures/<provider>/<topic>/` (e.g. `claude-code/sessions/`).

Empty until Phase 0 samples are collected.
