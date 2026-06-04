//! Tuniq shared interface types — the one contract every layer agrees on.
//!
//! Two distinct "result" notions exist in Tuniq; do not conflate them:
//!
//! 1. On the **fully-shielded LEZ path** (Experiment 2 / the moat), the predicate
//!    result is surfaced the idiomatic LEZ way: the guest *asserts* and panics on
//!    failure, so a valid proof *existing* is itself the statement that the
//!    predicate held. There is no `EligibilityResult` committed in that journal —
//!    the journal is the privacy circuit's output (commitments / nullifiers),
//!    which the Solana coordinator parses (Half B).
//!
//! 2. `EligibilityResult` below is the **clean public result contract**: what the
//!    coordinator forwards to a consumer program, and what the standalone /
//!    fast-lane guest commits directly. Its byte layout is pinned by a test so the
//!    prover side and the Solana side can never silently drift.
#![cfg_attr(not(feature = "std"), no_std)]

use borsh::{BorshDeserialize, BorshSerialize};

/// Bump whenever the journal/result layout changes. Both sides assert on it.
pub const JOURNAL_SCHEMA_VERSION: u8 = 1;

/// The public result of a confidential threshold predicate.
/// This is the ONLY value revealed — never the secret balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct EligibilityResult {
    pub eligible: bool,
    pub threshold: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let r = EligibilityResult { eligible: true, threshold: 1000 };
        let bytes = borsh::to_vec(&r).expect("serialize");
        let back = EligibilityResult::try_from_slice(&bytes).expect("deserialize");
        assert_eq!(r, back);
    }

    /// Pins the exact on-the-wire shape observed in Experiment 3:
    /// journal.bin = [1, 232,3,0,0,0,0,0,0] = (eligible: true, threshold: 1000).
    /// If this test breaks, the Solana coordinator's parser must change in lockstep.
    #[test]
    fn byte_layout_matches_experiment_3() {
        let r = EligibilityResult { eligible: true, threshold: 1000 };
        let bytes = borsh::to_vec(&r).expect("serialize");
        assert_eq!(bytes, vec![1, 232, 3, 0, 0, 0, 0, 0, 0]);
    }
}
