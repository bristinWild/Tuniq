use nssa_core::{account::AccountWithMetadata, program::AccountPostState};

/// Confidential predicate handler: assert `subject.balance >= threshold`.
///
/// `subject.account.balance` is the (shielded) secret. For a private account it
/// is bound to its on-chain commitment by the privacy-preserving circuit, so the
/// prover cannot substitute a fake balance. The balance is never returned in any
/// post-state field beyond the (unchanged) account itself.
///
/// Read-only: the subject's post-state equals its pre-state.
pub fn check_balance_over_threshold(
    subject: AccountWithMetadata,
    threshold: u128,
) -> Vec<AccountPostState> {
    assert!(
        subject.account.balance >= threshold,
        "Predicate failed: balance is below threshold"
    );

    // Emit the account unchanged (read-only). No mutation, no balance leak.
    vec![AccountPostState::new(subject.account)]
}
