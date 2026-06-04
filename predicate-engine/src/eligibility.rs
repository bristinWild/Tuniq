use nssa_core::{account::AccountWithMetadata, program::AccountPostState};
use predicate_core::ConfidentialBalance;

/// Evaluate `balance >= threshold` over a shielded account.
///
/// The predicate does not mutate state — the *existence* of a valid proof is the
/// statement that it held. On failure this panics (the LEZ soundness mechanic):
/// a false statement has no proof (Experiment 2).
pub fn check_eligibility(account: AccountWithMetadata, threshold: u128) -> Vec<AccountPostState> {
    assert!(account.is_authorized, "Account authorization is missing");

    let balance = match ConfidentialBalance::try_from(&account.account.data)
        .expect("Invalid confidential balance account data")
    {
        ConfidentialBalance::Balance { amount } => amount,
    };

    assert!(
        balance >= threshold,
        "Predicate failed: balance is below threshold"
    );

    // Predicate held. Read-only: the post-state is the account unchanged.
    vec![AccountPostState::new(account.account)]
}
