// Tuniq Coordinator — on-chain Solana program (M5).
//
// Changes from M3/M4:
//   - Full on-chain nullifier binding. The `authorized_prover` trust shortcut
//     is removed. A new `store_journal` instruction stores the raw journal
//     bytes in a PDA keyed by sha256(journal) (= journal_digest, the same
//     digest the Verifier Router checks against the proof's claim).
//
//     `verify_predicate` now takes `claimed_nullifier: [u8; 32]` as an
//     instruction argument (used to derive the `spent_nullifier` replay-guard
//     PDA, since Anchor resolves `init` PDA seeds before the handler body
//     runs). Inside the handler, the journal account is loaded, its hash is
//     re-checked against `journal_digest`, and `new_nullifiers[nullifier_index]`
//     is decoded directly from the journal (risc0-serde word-aligned layout —
//     see `journal_decode.rs`). If the decoded value doesn't equal
//     `claimed_nullifier`, the instruction fails — so even though the caller
//     picks the PDA seed up front, they cannot get an incorrect nullifier
//     recorded: either it matches what the proof actually committed to, or
//     the transaction reverts.
//
//     `authorized_prover` and the `prover` signer constraint are removed.
//     Anyone can call `store_journal` and `verify_predicate` — the proof
//     itself (verified via the Router CPI) is the only authorization needed.
//
// `journal_decode` known limitation: only supports the
// `check_balance_over_threshold` shape (empty public_pre_states /
// public_post_states). See module docs.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hash;
use verifier_router::cpi::accounts::Verify;
use verifier_router::program::VerifierRouter as VerifierRouterProgram;
use verifier_router::state::{VerifierEntry, VerifierRouter};
use verifier_router::Seal;

pub mod journal_decode;
use journal_decode::decode_nullifier_from_journal;

declare_id!("39jHP7Hs6zvCWsG3gJHVPfZfdFwAjhGfnFiyGDcPN7bY");

/// Maximum journal size we'll store. The real artifact is 696 bytes; this
/// gives headroom for predicates with a few more ciphertexts/commitments.
pub const MAX_JOURNAL_LEN: usize = 2048;

#[program]
pub mod coordinator {
    use super::*;

    /// Initialize the coordinator config.
    ///
    /// `image_id` — PRIVACY_PRESERVING_CIRCUIT_ID as LE bytes per word.
    pub fn initialize(ctx: Context<Initialize>, image_id: [u8; 32]) -> Result<()> {
        ctx.accounts.config.image_id = image_id;
        ctx.accounts.config.authority = ctx.accounts.authority.key();
        ctx.accounts.config.verified_count = 0;
        Ok(())
    }

    /// Store the raw journal bytes for a proof, keyed by sha256(journal).
    ///
    /// Anyone may call this — it's pure storage, keyed by the journal's own
    /// hash, so the content is self-certifying. `verify_predicate` re-derives
    /// this same digest and checks it against the proof's claim via the
    /// Verifier Router CPI, so a mismatched or tampered journal cannot be
    /// substituted.
    pub fn store_journal(ctx: Context<StoreJournal>, journal: Vec<u8>) -> Result<()> {
        require!(
            journal.len() <= MAX_JOURNAL_LEN,
            CoordinatorError::JournalTooLarge
        );
        ctx.accounts.journal_account.data = journal;
        Ok(())
    }

    /// Verify a shielded confidential-predicate proof, confirm its nullifier
    /// on-chain, prevent replay, and forward the result to the consumer
    /// program.
    ///
    /// `claimed_nullifier` is supplied by the caller to derive the
    /// `spent_nullifier` PDA address (Anchor needs this before the handler
    /// runs). It is NOT trusted on its own: the handler decodes
    /// `new_nullifiers[nullifier_index]` directly from the journal
    /// (proof-committed via `journal_digest`) and requires it to equal
    /// `claimed_nullifier`. A caller cannot get a fabricated nullifier
    /// recorded — either `claimed_nullifier` matches what the proof actually
    /// committed to, or this instruction fails.
    ///
    /// `journal_digest` must equal sha256(journal_account.data) (checked
    /// on-chain below) AND must match the digest embedded in the proof's
    /// claim (checked by the Verifier Router CPI).
    ///
    /// No signer-based trust: the proof + journal together are the only
    /// authorization. Anyone can submit.
    pub fn verify_predicate(
        ctx: Context<VerifyPredicate>,
        seal: Seal,
        journal_digest: [u8; 32],
        nullifier_index: u8,
        claimed_nullifier: [u8; 32],
    ) -> Result<()> {
        // 1. Re-derive the journal digest on-chain and check it matches the
        //    claimed digest. The PDA seed already constrains
        //    journal_account.key() == pda(journal_digest), but that only
        //    proves *which* digest was claimed at store_journal time — this
        //    proves the *stored bytes* still hash to it.
        let computed_digest = hash(&ctx.accounts.journal_account.data).to_bytes();
        require!(
            computed_digest == journal_digest,
            CoordinatorError::JournalDigestMismatch
        );

        // 2. Decode the nullifier directly from the proof-committed journal
        //    and confirm it matches what the caller claimed (and therefore
        //    what spent_nullifier's PDA was seeded with).
        let nullifier =
            decode_nullifier_from_journal(&ctx.accounts.journal_account.data, nullifier_index)?;
        require!(
            nullifier == claimed_nullifier,
            CoordinatorError::NullifierMismatch
        );

        // 3. CPI into Verifier Router. journal_digest must match the digest
        //    embedded in the proof's claim, or this CPI fails.
        let image_id = ctx.accounts.config.image_id;
        let cpi_accounts = Verify {
            router: ctx.accounts.router_account.to_account_info(),
            verifier_entry: ctx.accounts.verifier_entry.to_account_info(),
            verifier_program: ctx.accounts.verifier_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.router.to_account_info(), cpi_accounts);
        verifier_router::cpi::verify(cpi_ctx, seal, image_id, journal_digest)?;

        // 4. Proof verified. Nullifier PDA init-ed (replay guard passed).
        ctx.accounts.config.verified_count = ctx.accounts.config.verified_count.saturating_add(1);

        // 5. Forward to consumer.
        let consumer_cpi_accounts = consumer::cpi::accounts::RecordVerification {
            registry: ctx.accounts.consumer_registry.to_account_info(),
            caller: ctx.accounts.caller.to_account_info(),
        };
        let consumer_cpi_ctx = CpiContext::new(
            ctx.accounts.consumer_program.to_account_info(),
            consumer_cpi_accounts,
        );
        consumer::cpi::record_verification(consumer_cpi_ctx, nullifier)?;

        emit!(PredicateVerified {
            by: ctx.accounts.caller.key(),
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

/// Stores the raw journal bytes for a proof, PDA-keyed by sha256(journal).
#[account]
pub struct JournalAccount {
    pub data: Vec<u8>,
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
        space = 8 + 32 + 32 + 8 // disc + image_id + authority + count
    )]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(journal: Vec<u8>)]
pub struct StoreJournal<'info> {
    #[account(
        init,
        payer = payer,
        seeds = [b"journal", hash(&journal).to_bytes().as_ref()],
        bump,
        space = 8 + 4 + journal.len(), // disc + Vec len prefix + bytes
    )]
    pub journal_account: Account<'info, JournalAccount>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(seal: Seal, journal_digest: [u8; 32], nullifier_index: u8, claimed_nullifier: [u8; 32])]
pub struct VerifyPredicate<'info> {
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,

    /// The journal bytes stored by `store_journal`, PDA-keyed by their own hash.
    #[account(seeds = [b"journal", journal_digest.as_ref()], bump)]
    pub journal_account: Account<'info, JournalAccount>,

    /// Replay guard, seeded by `claimed_nullifier`. The handler verifies
    /// `claimed_nullifier` against the journal-decoded value before this
    /// account is used for anything — see module docs.
    #[account(
        init,
        payer = caller,
        seeds = [b"nullifier", claimed_nullifier.as_ref()],
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

    /// Anyone may call — the proof + journal are the authorization.
    #[account(mut)]
    pub caller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum CoordinatorError {
    #[msg("journal exceeds MAX_JOURNAL_LEN")]
    JournalTooLarge,
    #[msg("sha256(journal) does not match the claimed journal_digest")]
    JournalDigestMismatch,
    #[msg("decoded nullifier does not match claimed_nullifier")]
    NullifierMismatch,
}

#[event]
pub struct PredicateVerified {
    pub by: Pubkey,
    pub nullifier: [u8; 32],
}
