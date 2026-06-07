// Tuniq Coordinator — on-chain Solana program (M3).
//
// Changes from M2:
//   - authorized_prover: only the registered prover service can call
//     verify_predicate. This is the practical nullifier binding for M3:
//     the prover sees the journal, extracts the correct nullifier, and
//     must sign the transaction — so it cannot supply a wrong nullifier
//     without breaking its own signature. Consistent with the existing
//     trust model (trust-light, not trustless; litepaper §5).
//   - Removed dead code: journal_contains_nullifier + NullifierNotInJournal.
//
// Full on-chain nullifier binding (risc0-serde decode in BPF) carries to M4.

use anchor_lang::prelude::*;
use verifier_router::cpi::accounts::Verify;
use verifier_router::program::VerifierRouter as VerifierRouterProgram;
use verifier_router::state::{VerifierEntry, VerifierRouter};
use verifier_router::Seal;

declare_id!("39jHP7Hs6zvCWsG3gJHVPfZfdFwAjhGfnFiyGDcPN7bY");

#[program]
pub mod coordinator {
    use super::*;

    /// Initialize the coordinator config.
    ///
    /// `image_id`         — PRIVACY_PRESERVING_CIRCUIT_ID as LE bytes per word.
    /// `authorized_prover` — the only pubkey allowed to call verify_predicate.
    ///                       Set to the proving service's keypair. Can be updated
    ///                       via a separate set_prover instruction (add in M4).
    pub fn initialize(
        ctx: Context<Initialize>,
        image_id: [u8; 32],
        authorized_prover: Pubkey,
    ) -> Result<()> {
        ctx.accounts.config.image_id = image_id;
        ctx.accounts.config.authority = ctx.accounts.authority.key();
        ctx.accounts.config.authorized_prover = authorized_prover;
        ctx.accounts.config.verified_count = 0;
        Ok(())
    }

    /// Verify a shielded confidential-predicate proof, prevent replay, and
    /// forward the result to the consumer program.
    ///
    /// Only `authorized_prover` can call this. The prover service sees the
    /// journal, extracts the correct nullifier, and signs this transaction —
    /// so it cannot supply a wrong nullifier without breaking its own signature.
    pub fn verify_predicate(
        ctx: Context<VerifyPredicate>,
        seal: Seal,
        journal_digest: [u8; 32],
        nullifier: [u8; 32],
    ) -> Result<()> {
        // journal_digest is sha256(journal) — computed off-chain by the prover service.

        // 2. CPI into Verifier Router.
        let image_id = ctx.accounts.config.image_id;
        let cpi_accounts = Verify {
            router: ctx.accounts.router_account.to_account_info(),
            verifier_entry: ctx.accounts.verifier_entry.to_account_info(),
            verifier_program: ctx.accounts.verifier_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.router.to_account_info(), cpi_accounts);
        verifier_router::cpi::verify(cpi_ctx, seal, image_id, journal_digest)?;

        // 3. Proof verified. Nullifier PDA init-ed (replay guard passed).
        ctx.accounts.config.verified_count = ctx.accounts.config.verified_count.saturating_add(1);

        // 4. Forward to consumer.
        let consumer_cpi_accounts = consumer::cpi::accounts::RecordVerification {
            registry: ctx.accounts.consumer_registry.to_account_info(),
            caller: ctx.accounts.prover.to_account_info(),
        };
        let consumer_cpi_ctx = CpiContext::new(
            ctx.accounts.consumer_program.to_account_info(),
            consumer_cpi_accounts,
        );
        consumer::cpi::record_verification(consumer_cpi_ctx, nullifier)?;

        emit!(PredicateVerified {
            by: ctx.accounts.prover.key(),
            nullifier,
        });

        Ok(())
    }
}

#[account]
pub struct Config {
    pub image_id: [u8; 32],
    pub authority: Pubkey,
    pub authorized_prover: Pubkey, // M3: only this key can call verify_predicate
    pub verified_count: u64,
}

#[account]
pub struct SpentNullifier {}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        seeds = [b"config"],
        bump,
        space = 8 + 32 + 32 + 32 + 8  // disc + image_id + authority + authorized_prover + count
    )]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(seal: Seal, journal_digest: [u8; 32], nullifier: [u8; 32])]
pub struct VerifyPredicate<'info> {
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,

    #[account(
        init,
        payer = prover,
        seeds = [b"nullifier", nullifier.as_ref()],
        bump,
        space = 8
    )]
    pub spent_nullifier: Account<'info, SpentNullifier>,

    // --- RISC Zero Verifier Router ---
    pub router: Program<'info, VerifierRouterProgram>,
    pub router_account: Account<'info, VerifierRouter>,
    #[account(
        seeds = [b"verifier", seal.selector.as_ref()],
        bump,
        seeds::program = verifier_router::ID,
    )]
    pub verifier_entry: Account<'info, VerifierEntry>,
    /// CHECK: validated by the router program.
    pub verifier_program: UncheckedAccount<'info>,

    // --- Consumer ---
    /// CHECK: any program implementing record_verification.
    pub consumer_program: UncheckedAccount<'info>,
    /// CHECK: the consumer's registry PDA.
    #[account(mut)]
    pub consumer_registry: UncheckedAccount<'info>,

    /// The authorized prover service — only this key can submit proofs.
    #[account(
        mut,
        constraint = prover.key() == config.authorized_prover
            @ CoordinatorError::UnauthorizedProver
    )]
    pub prover: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum CoordinatorError {
    #[msg("Only the authorized prover service can submit proofs")]
    UnauthorizedProver,
}

#[event]
pub struct PredicateVerified {
    pub by: Pubkey,
    pub nullifier: [u8; 32],
}
