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

    /// Confidential predicate: assert the subject account's (shielded) balance
    /// is >= threshold. A valid proof existing == the predicate held, without
    /// revealing the balance.
    #[instruction]
    pub fn check_balance_over_threshold(
        subject: AccountWithMetadata,
        threshold: u128,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            predicate_program::check::check_balance_over_threshold(subject, threshold),
            vec![],
        ))
    }
}
