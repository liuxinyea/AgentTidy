//! Space accounting model (`Start.md` §8).
//!
//! "Occupied" and "reclaimable" are different numbers. Every size figure in
//! the system flows through `SizeInfo`, which separates logical/allocated
//! and exclusive/shared bytes and always carries a confidence — we never
//! present an estimate as an exact value (§8 rule: no fake precision).
//!
//! Dedup rules (hard links by inode, symlinks not followed, nested dirs
//! counted once) are enforced by the scanning layer (infrastructure,
//! Phase 2), not by this type — the type just records the outcome.

use serde::{Deserialize, Serialize};

/// How trustworthy a size figure is.
///
/// `Default` is `Unknown` — unmeasured, fail closed (§3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeConfidence {
    /// Measured directly (logical size of enumerated files).
    Exact,
    /// Extrapolated or partially measured.
    Estimated,
    /// Not measured at all.
    #[default]
    Unknown,
}

/// Space accounting for a resource, session or cleanup unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SizeInfo {
    /// Sum of file logical lengths.
    pub logical_bytes: u64,
    /// On-disk allocated bytes, when available.
    pub allocated_bytes: Option<u64>,
    /// Bytes belonging exclusively to this item (never double-counted).
    pub exclusive_bytes: Option<u64>,
    /// Bytes shared with other items or not attributable.
    pub shared_bytes: Option<u64>,
    /// Expected bytes actually freed if this item is cleaned.
    ///
    /// Absent until a cleanup plan exists — occupied ≠ reclaimable (§8).
    pub reclaimable_bytes: Option<u64>,
    /// Confidence of `logical_bytes` (and friends).
    pub confidence: SizeConfidence,
}

impl SizeInfo {
    /// All-zero size with `Unknown` confidence — safe default for
    /// unscanned items (fail closed: no fake numbers).
    pub fn unknown() -> Self {
        Self {
            confidence: SizeConfidence::Unknown,
            ..Self::default()
        }
    }

    /// Exact logical size for a single measured file set.
    pub fn exact(logical_bytes: u64) -> Self {
        Self {
            logical_bytes,
            confidence: SizeConfidence::Exact,
            ..Self::default()
        }
    }

    /// Sum two infos conservatively (used when aggregating sessions into a
    /// project or provider total).
    ///
    /// Shared bytes and exclusive bytes are summed independently; the sum
    /// may therefore overlap — consumers must use `exclusive_bytes` for
    /// reclaimability math, never `logical_bytes` (§8: shared size never
    /// counts toward a single session's reclaimable space).
    ///
    /// Confidence follows the weakest contributor, matching `opt_add`: if
    /// either side is unmeasured, the aggregate is unmeasured too — never
    /// silently present a partial sum as merely "estimated".
    pub fn merge(self, other: Self) -> Self {
        fn opt_add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            match (a, b) {
                (Some(x), Some(y)) => Some(x + y),
                // If either side is unknown the aggregate is unknown too —
                // never silently pretend a partial sum is complete.
                _ => None,
            }
        }
        Self {
            logical_bytes: self.logical_bytes + other.logical_bytes,
            allocated_bytes: opt_add(self.allocated_bytes, other.allocated_bytes),
            exclusive_bytes: opt_add(self.exclusive_bytes, other.exclusive_bytes),
            shared_bytes: opt_add(self.shared_bytes, other.shared_bytes),
            reclaimable_bytes: opt_add(self.reclaimable_bytes, other.reclaimable_bytes),
            confidence: match (self.confidence, other.confidence) {
                (SizeConfidence::Exact, SizeConfidence::Exact) => SizeConfidence::Exact,
                // An unmeasured contributor makes the whole aggregate
                // unmeasured — its (zero) bytes were never counted.
                (SizeConfidence::Unknown, _) | (_, SizeConfidence::Unknown) => {
                    SizeConfidence::Unknown
                }
                _ => SizeConfidence::Estimated,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_follows_weakest_confidence() {
        let exact = SizeInfo::exact(100);
        let est = SizeInfo {
            logical_bytes: 50,
            confidence: SizeConfidence::Estimated,
            ..SizeInfo::default()
        };
        assert_eq!(exact.merge(exact).confidence, SizeConfidence::Exact);
        assert_eq!(exact.merge(est).confidence, SizeConfidence::Estimated);
        // Any unmeasured contributor makes the aggregate unmeasured — its
        // (zero) logical bytes were never counted, so the total must not
        // look trustworthy at any level.
        assert_eq!(
            est.merge(SizeInfo::unknown()).confidence,
            SizeConfidence::Unknown
        );
        assert_eq!(
            exact.merge(SizeInfo::unknown()).confidence,
            SizeConfidence::Unknown
        );
    }

    #[test]
    fn merge_unknown_bytes_stay_unknown() {
        // One side lacking allocated_bytes must not produce a partial sum.
        let a = SizeInfo {
            logical_bytes: 10,
            allocated_bytes: Some(12),
            ..SizeInfo::default()
        };
        let b = SizeInfo {
            logical_bytes: 10,
            ..SizeInfo::default()
        };
        let m = a.merge(b);
        assert_eq!(m.logical_bytes, 20);
        assert_eq!(m.allocated_bytes, None);
    }

    #[test]
    fn unknown_default_is_zeroed() {
        let u = SizeInfo::unknown();
        assert_eq!(u.logical_bytes, 0);
        assert_eq!(u.confidence, SizeConfidence::Unknown);
    }
}
