//! Tuniq confidential predicate — program logic.
//!
//! Mirrors the lez-programs token handlers: take `AccountWithMetadata`, assert
//! authorization, parse account data via the `#[account_type]` bridge, run the
//! predicate. Failure is expressed by panic (the LEZ soundness mechanic): if the
//! predicate does not hold, no valid proof can be produced.

pub mod eligibility;
