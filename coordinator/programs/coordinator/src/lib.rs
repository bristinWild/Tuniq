// Tuniq Coordinator — on-chain Solana program (M2).
//
// Verifies a shielded confidential-predicate proof and forwards the result
// to a consumer program via CPI.
//
// Key facts confirmed from source:
//   - journal_digest = sha256(journal bytes)
//   - pi_a must be pre-negated (BN254 G1) by the caller
//   - image_id = PRIVACY_PRESERVING_CIRCUIT_ID words as LE bytes
//   - Replay guard: nullifier PDA (init = one-shot, existence = spent)
//   - Consumer: flexible — passed as account, coordinator CPIs into it

use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hashv;
use verifier_router::cpi::accounts::Verify;
use verifier_router::program::VerifierRouter as VerifierRouterProgram;
use verifier_router::state::{VerifierEntry, VerifierRouter};
use verifier_router::Seal;

declare_id!("39jHP7Hs6zvCWsG3gJHVPfZfdFwAjhGfnFiyGDcPN7bY");

#[program]
pub mod coordinator {
    use super::*;

    /// Store PRIVACY_PRESERVING_CIRCUIT_ID. image_id = [u32;8] as LE bytes.
    pub fn initialize(ctx: Context<Initialize>, image_id: [u8; 32]) -> Result<()> {
        ctx.accounts.config.image_id = image_id;
        ctx.accounts.config.authority = ctx.accounts.authority.key();
        ctx.accounts.config.verified_count = 0;
        Ok(())
    }

    /// Verify a shielded confidential-predicate proof, prevent replay, and
    /// forward "predicate held" to the consumer program via CPI.
    pub fn verify_predicate(
        ctx: Context<VerifyPredicate>,
        seal: Seal,
        journal: Vec<u8>,
        nullifier: [u8; 32],
    ) -> Result<()> {
        // 1. journal digest = sha256(journal bytes).
        let journal_digest = hashv(&[journal.as_slice()]).to_bytes();

        // 2. CPI into Verifier Router. Returns Err if proof is invalid.
        let image_id = ctx.accounts.config.image_id;
        let cpi_accounts = Verify {
            router: ctx.accounts.router_account.to_account_info(),
            verifier_entry: ctx.accounts.verifier_entry.to_account_info(),
            verifier_program: ctx.accounts.verifier_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.router.to_account_info(), cpi_accounts);
        verifier_router::cpi::verify(cpi_ctx, seal, image_id, journal_digest)?;

        // 3. Proof verified. Nullifier PDA was init-ed (replay guard passed).
        ctx.accounts.config.verified_count = ctx.accounts.config.verified_count.saturating_add(1);

        // 4. Forward to consumer via CPI.
        //    The consumer program is passed as an account — any compliant
        //    consumer can be wired in without redeploying the coordinator.
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
        space = 8 + 32 + 32 + 8
    )]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(seal: Seal, journal: Vec<u8>, nullifier: [u8; 32])]
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
    /// CHECK: the consumer program — any program implementing record_verification.
    pub consumer_program: UncheckedAccount<'info>,
    #[account(mut)]
    /// CHECK: the consumer's registry PDA — validated by the consumer program.
    pub consumer_registry: UncheckedAccount<'info>,

    #[account(mut)]
    pub prover: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum CoordinatorError {
    #[msg("Provided nullifier is not present in the verified journal")]
    NullifierNotInJournal,
}

#[event]
pub struct PredicateVerified {
    pub by: Pubkey,
    pub nullifier: [u8; 32],
}
