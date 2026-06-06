//! Core types for the Tuniq confidential predicate program.
//!
//! Minimal standalone LEZ program: reads the native balance of a (possibly
//! SHIELDED) account and evaluates a predicate over it, emitting ONLY a public
//! boolean statement via assert — never the balance itself.
//!
//! Ported from the proven Experiment 2 `confidential_predicate` (the moat).

use serde::{Deserialize, Serialize};

/// Instruction type for the confidential predicate program.
#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// Evaluate `account.balance >= threshold` over the single input account.
    /// Panics (=> proof fails) if the predicate does not hold. The balance is
    /// never revealed.
    ///
    /// Required accounts: `[subject]` — read-only.
    CheckBalanceOverThreshold { threshold: u128 },
}
