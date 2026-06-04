#![cfg_attr(not(test), no_main)]

use spel_framework::prelude::*;
use nssa_core::account::AccountWithMetadata;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "predicate_core::Instruction")]
mod confidential_predicate {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    /// Evaluate `balance >= threshold` over a single shielded account.
    /// Panics if the predicate does not hold — a false statement is unprovable.
    #[instruction]
    pub fn check_eligibility(
        account: AccountWithMetadata,
        threshold: u128,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            predicate_program::eligibility::check_eligibility(account, threshold),
            vec![],
        ))
    }
}
