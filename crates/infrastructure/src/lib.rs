//! Platform infrastructure (design doc `Start.md` §13.1).
//!
//! Exposes platform capabilities behind unified interfaces
//! (`PlatformServices`: paths, trash, processes, filesystem), implemented
//! separately for macOS and Windows. Platform differences must never leak
//! out of this crate (and provider path-discovery logic).
//!
//! To be implemented in Phase 2.
