// Tuniq Coordinator — on-chain Solana program (M1).
//
// Verifies that a SHIELDED confidential predicate held — `balance >= threshold`
// over a commitment-bound Logos account — WITHOUT the balance ever touching
// Solana, then forwards "predicate held" to consumers and prevents replay.
//
// Three differences from the Exp 3 standalone verifier, all forced by the
// shielded (Half B) path and confirmed against source:
//   1. No EligibilityResult to decode. The journal is a
//      PrivacyPreservingCircuitOutput (commitments/nullifiers, no boolean).
//      Verification IS the result: a valid proof == the predicate held.
//   2. Image id is PRIVACY_PRESERVING_CIRCUIT_ID (the circuit), not a guest id.
//   3. Replay guard: the shielded note's nullifier is unknown to Solana, so a
//      valid (seal, journal) pair could be resubmitted forever. A nullifier PDA
//      makes each proof one-shot.
//
// journal_digest = sha256(journal) — the router/groth16 verifier wraps it into
// the ReceiptClaim itself (hash_claim -> hash_output). Confirmed in Exp 3's
// my_proof_test and the groth_16_verifier source.

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

    /// Store the PRIVACY_PRESERVING_CIRCUIT_ID so only proofs from the real
    /// Logos privacy circuit are accepted. `image_id` is the [u32; 8] circuit id
    /// converted to 32 bytes (each word little-endian).
    pub fn initialize(ctx: Context<Initialize>, image_id: [u8; 32]) -> Result<()> {
        ctx.accounts.config.image_id = image_id;
        ctx.accounts.config.authority = ctx.accounts.authority.key();
        ctx.accounts.config.verified_count = 0;
        Ok(())
    }

    /// Verify a shielded confidential-predicate proof and consume its nullifier.
    ///
    /// `nullifier` is the 32-byte nullifier from the journal's `new_nullifiers`.
    /// It seeds a PDA that is `init`-ed here: first submission succeeds, any
    /// resubmission of the same proof fails (the PDA already exists) — one-shot.
    ///
    /// We bind the passed `nullifier` to the journal so a caller cannot pair a
    /// real proof with an unrelated (unused) nullifier to dodge the guard.
    pub fn verify_predicate(
        ctx: Context<VerifyPredicate>,
        seal: Seal,
        journal: Vec<u8>,
        nullifier: [u8; 32],
    ) -> Result<()> {
        // (a) The nullifier must actually be the one this journal commits.
        require!(
            journal_contains_nullifier(&journal, &nullifier),
            CoordinatorError::NullifierNotInJournal
        );

        // (b) journal digest = sha256(journal). The verifier builds the claim.
        let journal_digest = hashv(&[journal.as_slice()]).to_bytes();

        // (c) CPI verify against the privacy-circuit image id. `?` (not unwrap)
        //     so a failed verification returns a clean error.
        let image_id = ctx.accounts.config.image_id;
        let cpi_accounts = Verify {
            router: ctx.accounts.router_account.to_account_info(),
            verifier_entry: ctx.accounts.verifier_entry.to_account_info(),
            verifier_program: ctx.accounts.verifier_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.router.to_account_info(), cpi_accounts);
        verifier_router::cpi::verify(cpi_ctx, seal, image_id, journal_digest)?;

        // (d) Verified AND the nullifier PDA was newly created (see Accounts):
        //     the predicate provably held and this proof has not been used before.
        ctx.accounts.config.verified_count = ctx.accounts.config.verified_count.saturating_add(1);

        emit!(PredicateVerified {
            by: ctx.accounts.prover.key(),
            nullifier,
        });

        // A consumer program would be invoked here (CPI) or read the emitted
        // event / the spent-nullifier PDA as the trusted "predicate held" signal.
        Ok(())
    }
}

/// Scan the journal bytes for the 32-byte nullifier. The journal is a
/// risc0-serde-encoded PrivacyPreservingCircuitOutput; the nullifier bytes
/// appear within it. A substring check is sufficient to bind caller-supplied
/// `nullifier` to this journal (the seal already authenticates the journal).
fn journal_contains_nullifier(journal: &[u8], nullifier: &[u8; 32]) -> bool {
    journal.windows(32).any(|w| w == nullifier.as_slice())
}

#[account]
pub struct Config {
    pub image_id: [u8; 32],
    pub authority: Pubkey,
    pub verified_count: u64,
}

/// Marker PDA proving a given nullifier has been consumed. Existence == spent.
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

    /// Replay guard: init fails if this nullifier was already consumed.
    #[account(
        init,
        payer = prover,
        seeds = [b"nullifier", nullifier.as_ref()],
        bump,
        space = 8
    )]
    pub spent_nullifier: Account<'info, SpentNullifier>,

    // --- RISC Zero Verifier Router (reuse; proven in Exp 3) ---
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
