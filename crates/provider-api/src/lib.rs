//! Provider adapter contracts.
//!
//! This crate defines the `AgentProviderAdapter` interface (design doc
//! `Start.md` §10): the boundary between AgentTidy core and per-agent
//! providers. Providers describe facts, resource relationships, operations
//! and preconditions — they never decide cleanup policy.
//!
//! Implemented in Phase 3; only the `ProviderId` placeholder exists today.

use serde::{Deserialize, Serialize};

/// Stable identifier of a supported agent provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub String);
