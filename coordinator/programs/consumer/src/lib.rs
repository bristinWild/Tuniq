// Tuniq Consumer — on-chain Solana program (M2).
//
// Receives a CPI from the coordinator after a shielded proof verifies.
// Maintains a global Registry that counts verified predicates.
//
// Deliberately minimal: prove the coordinator → consumer CPI pattern works.
// Real consumers gate access, mint tokens, record eligibility, etc.
// The counter makes it unambiguous that the consumer was actually invoked.

use anchor_lang::prelude::*;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod consumer {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.registry.verified_count = 0;
        ctx.accounts.registry.authority = ctx.accounts.authority.key();
        Ok(())
    }

    /// Called by the coordinator via CPI when a shielded predicate proof verifies.
    /// `nullifier` identifies which proof triggered this — useful for auditing.
    pub fn record_verification(
        ctx: Context<RecordVerification>,
        nullifier: [u8; 32],
    ) -> Result<()> {
        ctx.accounts.registry.verified_count =
            ctx.accounts.registry.verified_count.saturating_add(1);

        emit!(VerificationRecorded {
            nullifier,
            count: ctx.accounts.registry.verified_count,
        });

        msg!(
            "Verification recorded. total={}",
            ctx.accounts.registry.verified_count
        );
        Ok(())
    }
}

#[account]
pub struct Registry {
    pub verified_count: u64,
    pub authority: Pubkey,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        seeds = [b"registry"],
        bump,
        space = 8 + 8 + 32
    )]
    pub registry: Account<'info, Registry>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RecordVerification<'info> {
    #[account(mut, seeds = [b"registry"], bump)]
    pub registry: Account<'info, Registry>,
    /// The coordinator signs this CPI. In production constrain:
    /// #[account(constraint = caller.key() == COORDINATOR_ID)]
    pub caller: Signer<'info>,
}

#[event]
pub struct VerificationRecorded {
    pub nullifier: [u8; 32],
    pub count: u64,
}
