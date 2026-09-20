//! Stable agent provider identity (`Start.md` §9.1).
//!
//! A `ProviderId` names one supported agent product (workbuddy, claude-code,
//! codex). It is the join key across installations, sessions and resources,
//! so it is a newtype over `String` with explicit construction — never a
//! free-form string flowing through the system.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identifier of a supported agent provider.
///
/// Well-known ids are provided as constants; providers are registered
/// statically (red line #10 — no dynamic plugin system), so the set of
/// valid ids is closed at compile time in practice.
///
/// Both `parse` and `Deserialize` validate against that closed set — the
/// invariant must hold on the wire/IPC path too, not just in-process
/// construction (fail closed, §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ProviderId(pub String);

impl ProviderId {
    /// Anthropic Claude Code (CLI + desktop).
    pub const CLAUDE_CODE: &str = "claude-code";
    /// OpenAI Codex (CLI + desktop).
    pub const CODEX: &str = "codex";
    /// WorkBuddy desktop app.
    pub const WORKBUDDY: &str = "workbuddy";

    /// Construct from a known-good literal (e.g. a constant above).
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Construct and validate against the statically known provider set.
    ///
    /// Unknown ids are rejected because they indicate a mismatch between
    /// registry and caller — fail closed (§3.5).
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            Self::CLAUDE_CODE | Self::CODEX | Self::WORKBUDDY => Some(Self(id.to_owned())),
            _ => None,
        }
    }

    /// Raw string form (serialization stays newtype-transparent;
    /// deserialization validates, see the manual impl below).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// Manual Deserialize (instead of a derive) so the closed-set invariant holds
// on the wire too: a derived impl would happily round-trip any string.
impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        ProviderId::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown provider id: {raw:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_known_providers() {
        assert_eq!(
            ProviderId::parse(ProviderId::CLAUDE_CODE),
            Some(ProviderId::new("claude-code"))
        );
        assert_eq!(
            ProviderId::parse(ProviderId::CODEX),
            Some(ProviderId::new("codex"))
        );
        assert_eq!(
            ProviderId::parse(ProviderId::WORKBUDDY),
            Some(ProviderId::new("workbuddy"))
        );
    }

    #[test]
    fn parse_rejects_unknown_providers() {
        assert!(ProviderId::parse("chatgpt").is_none());
        assert!(ProviderId::parse("").is_none());
    }

    #[test]
    fn deserialize_rejects_unknown_providers() {
        // The closed-set invariant must hold on the wire path too —
        // deserializing an id that parse() rejects is an error, not a
        // round-tripped ProviderId (fail closed, §3.5).
        assert!(serde_json::from_str::<ProviderId>("\"chatgpt\"").is_err());
        assert!(serde_json::from_str::<ProviderId>("\"\"").is_err());
        let ok: ProviderId = serde_json::from_str("\"codex\"").unwrap();
        assert_eq!(ok.as_str(), "codex");
    }
}
