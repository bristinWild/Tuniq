//! Tuniq confidential predicate — program logic.
//!
//! Execution lives in `check.rs`; the guest binary wraps it via SPEL.
//! Ported from the proven Experiment 2 `confidential_predicate`.

pub mod check;

pub use predicate_core::Instruction;
