//! Core types for the Tuniq confidential predicate program.
//!
//! Mirrors the three-crate LEZ pattern (core / program / methods) used by the
//! lez-programs token program. `#[account_type]` is a marker; the borsh/serde
//! derives and the `Data` <-> type bridge are written explicitly, exactly as the
//! token program does for `TokenHolding`.

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::account::Data;
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// Confidential predicate instruction.
#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// Evaluate `balance >= threshold` over a single shielded account.
    ///
    /// Required accounts:
    /// - The confidential balance account (shielded; its balance is bound to its
    ///   on-chain commitment by the privacy-preserving transaction the host
    ///   constructs via `execute_and_prove`).
    ///
    /// The result is surfaced by assert-and-panic: a valid proof existing IS the
    /// statement that `balance >= threshold` held. A false statement is unprovable.
    CheckEligibility { threshold: u128 },
}

/// The shielded account payload the predicate reads.
#[account_type]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum ConfidentialBalance {
    /// A confidential balance.
    Balance { amount: u128 },
}

impl TryFrom<&Data> for ConfidentialBalance {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        ConfidentialBalance::try_from_slice(data.as_ref())
    }
}

impl From<&ConfidentialBalance> for Data {
    fn from(value: &ConfidentialBalance) -> Self {
        // size_of_val as a Vec allocation hint, mirroring the token program.
        let mut data = Vec::with_capacity(std::mem::size_of_val(value));
        BorshSerialize::serialize(value, &mut data).expect("Serialization to Vec should not fail");
        Data::try_from(data).expect("Confidential balance encoded data should fit into Data")
    }
}
