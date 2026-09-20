//! Static provider registry (`Start.md` §13, red line #10).
//!
//! The registry maps the closed set of `ProviderId`s to their adapter
//! implementations. It is static — no dynamic plugin discovery — and lives
//! in core-neutral form here so that both the application layer and the
//! CLI can resolve "given a ProviderId, which adapter implements it?"
//! without depending on concrete provider crates.
//!
//! The type is intentionally generic over the adapter type: core defines
//! the *shape* of registration; the concrete registry instantiating it
//! with the three v0.1 providers lives in the application layer
//! (see `crates/application`, wired in Phase 3).

use crate::provider::ProviderId;
use std::collections::BTreeMap;

/// A registry of provider adapters, keyed by `ProviderId`.
///
/// `A` is the adapter type — typically the trait object
/// `Box<dyn AgentProviderAdapter>` from `crates/provider-api` (dyn-compatible
/// via `async_trait`). Registering a duplicate provider id returns
/// `Err(DuplicateProvider)`: that is a programmer error surfaced loudly at
/// registry construction time, not a runtime condition to handle — callers
/// should treat the `Result` as "must never be `Err`" (fail fast at startup).
#[derive(Debug, Clone)]
pub struct ProviderRegistry<A> {
    adapters: BTreeMap<ProviderId, A>,
}

impl<A> ProviderRegistry<A> {
    /// Create an empty registry.
    pub fn empty() -> Self {
        Self {
            adapters: BTreeMap::new(),
        }
    }

    /// Register an adapter; returns `Err` on duplicate provider id so the
    /// wiring layer fails loudly at startup instead of silently shadowing
    /// a provider.
    pub fn register(&mut self, id: ProviderId, adapter: A) -> Result<(), DuplicateProvider> {
        if self.adapters.contains_key(&id) {
            return Err(DuplicateProvider(id));
        }
        self.adapters.insert(id, adapter);
        Ok(())
    }

    /// Look up the adapter for a provider id.
    pub fn get(&self, id: &ProviderId) -> Option<&A> {
        self.adapters.get(id)
    }

    /// Iterate registered adapters in stable (id) order.
    pub fn iter(&self) -> impl Iterator<Item = (&ProviderId, &A)> {
        self.adapters.iter()
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.adapters.len()
    }

    /// Whether no providers are registered.
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }
}

/// Error returned when two adapters claim the same provider id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateProvider(pub ProviderId);

impl std::fmt::Display for DuplicateProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "duplicate provider registration: {}", self.0)
    }
}

impl std::error::Error for DuplicateProvider {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut reg: ProviderRegistry<&'static str> = ProviderRegistry::empty();
        reg.register(ProviderId::new(ProviderId::CLAUDE_CODE), "cc")
            .unwrap();
        reg.register(ProviderId::new(ProviderId::CODEX), "cx")
            .unwrap();
        reg.register(ProviderId::new(ProviderId::WORKBUDDY), "wb")
            .unwrap();
        assert_eq!(reg.len(), 3);
        assert_eq!(reg.get(&ProviderId::new(ProviderId::CODEX)), Some(&"cx"));
    }

    #[test]
    fn duplicate_registration_fails() {
        let mut reg: ProviderRegistry<()> = ProviderRegistry::empty();
        reg.register(ProviderId::new(ProviderId::CODEX), ())
            .unwrap();
        let err = reg
            .register(ProviderId::new(ProviderId::CODEX), ())
            .unwrap_err();
        assert_eq!(err.to_string(), "duplicate provider registration: codex");
    }
}
