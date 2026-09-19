//! Application API — the single stable entry point shared by the desktop GUI
//! and the CLI (design doc `Start.md` §13):
//!
//! - Detect / Scan / List
//! - Plan / Validate / Execute
//!
//! To be implemented in Phase 1+. Neither the GUI nor the CLI may bypass
//! this layer and touch providers or the filesystem directly.
