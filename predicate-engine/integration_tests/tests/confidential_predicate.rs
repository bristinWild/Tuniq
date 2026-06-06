//! Integration test: confidential predicate over a SHIELDED account.
//!
//! The Layer-2 moat test, ported from Experiment 2. Proves `balance >= threshold`
//! over a private account whose balance is bound to its on-chain commitment —
//! a verifiable proof that the predicate held WITHOUT revealing the balance, and
//! WITHOUT the prover being able to forge it. Seeds the commitment at genesis via
//! `new_with_genesis_accounts` (no separate shielding transfer needed).

use nssa::{
    execute_and_prove,
    privacy_preserving_transaction::{Message, WitnessSet},
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    PrivacyPreservingTransaction, SharedSecretKey, V03State,
};
use nssa_core::{
    account::{Account, AccountWithMetadata, Data, Nonce},
    encryption::{EphemeralPublicKey, ViewingPublicKey},
    Commitment, Nullifier, NullifierPublicKey, NullifierSecretKey,
};

// ---- harness: keys for the subject private account ----
struct Pk;
impl Pk {
    fn subject_nsk() -> NullifierSecretKey {
        [55; 32]
    }
    fn subject_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::subject_nsk())
    }
    fn subject_vsk() -> [u8; 32] {
        [66; 32]
    }
    fn subject_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_scalar(Self::subject_vsk())
    }
}

fn confidential_predicate_program() -> Program {
    Program::new(predicate_methods::CONFIDENTIAL_PREDICATE_ELF.to_vec())
        .expect("valid confidential_predicate ELF")
}

fn deploy_program(state: &mut V03State) {
    let message = program_deployment_transaction::Message::new(
        predicate_methods::CONFIDENTIAL_PREDICATE_ELF.to_vec(),
    );
    let tx = ProgramDeploymentTransaction::new(message);
    state
        .transition_from_program_deployment_transaction(&tx)
        .expect("program deployment must succeed");
}

/// Build the private subject account holding `secret_balance`, with state seeded
/// with its commitment at genesis.
fn setup(secret_balance: u128) -> (V03State, Account) {
    let subject_npk = Pk::subject_npk();

    let subject_account = Account {
        program_owner: confidential_predicate_program().id(),
        balance: secret_balance,
        data: Data::default(),
        nonce: Nonce::private_account_nonce_init(&subject_npk),
    };

    // Commitment seeded into the genesis CommitmentSet so the membership proof
    // resolves (the anti-substitution binding). The genesis nullifier is the
    // account-initialization nullifier, NOT the spend-nullifier, so the note
    // stays readable.
    let commitment = Commitment::new(&subject_npk, &subject_account);
    let nullifier = Nullifier::for_account_initialization(&subject_npk);

    let mut state = V03State::new_with_genesis_accounts(&[], vec![(commitment, nullifier)], 0);
    deploy_program(&mut state);

    (state, subject_account)
}

#[test]
fn confidential_predicate_passes_over_shielded_balance() {
    let secret_balance = 5_000_u128;
    let threshold = 1_000_u128;

    let (mut state, subject_account) = setup(secret_balance);
    let subject_npk = Pk::subject_npk();
    let subject_nsk = Pk::subject_nsk();
    let subject_vpk = Pk::subject_vpk();
    let subject_commitment = Commitment::new(&subject_npk, &subject_account);

    let esk = [7u8; 32];
    let shared_secret = SharedSecretKey::new(&esk, &subject_vpk);
    let epk = EphemeralPublicKey::from_scalar(esk);

    let subject_pre = AccountWithMetadata::new(subject_account.clone(), true, &subject_npk);

    let instruction = predicate_core::Instruction::CheckBalanceOverThreshold { threshold };

    let (output, proof) = execute_and_prove(
        vec![subject_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![1],
        vec![(subject_npk, shared_secret)],
        vec![subject_nsk],
        vec![state.get_proof_for_commitment(&subject_commitment)],
        &confidential_predicate_program().into(),
    )
    .expect("predicate should prove: 5000 >= 1000");

    let message = Message::try_from_circuit_output(
        vec![],
        vec![],
        vec![(subject_npk, subject_vpk, epk)],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .expect("transaction applies because the predicate held");

    // Read-only: a commitment to the same account (balance unchanged, nonce
    // incremented) should now exist. The balance never appeared publicly.
    let subject_after = Account {
        nonce: Nonce::private_account_nonce_init(&subject_npk)
            .private_account_nonce_increment(&subject_nsk),
        ..subject_account
    };
    assert!(
        state
            .get_proof_for_commitment(&Commitment::new(&subject_npk, &subject_after))
            .is_some(),
        "post-state commitment for the unchanged-balance account should exist"
    );
}

#[test]
#[should_panic]
fn confidential_predicate_fails_when_below_threshold() {
    let secret_balance = 500_u128; // below threshold
    let threshold = 1_000_u128;

    let (state, subject_account) = setup(secret_balance);
    let subject_npk = Pk::subject_npk();
    let subject_nsk = Pk::subject_nsk();
    let subject_vpk = Pk::subject_vpk();
    let subject_commitment = Commitment::new(&subject_npk, &subject_account);

    let esk = [7u8; 32];
    let shared_secret = SharedSecretKey::new(&esk, &subject_vpk);

    let subject_pre = AccountWithMetadata::new(subject_account.clone(), true, &subject_npk);

    let instruction = predicate_core::Instruction::CheckBalanceOverThreshold { threshold };

    // Guest panics (500 < 1000) => execute_and_prove returns Err => unwrap panics.
    let _ = execute_and_prove(
        vec![subject_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![1],
        vec![(subject_npk, shared_secret)],
        vec![subject_nsk],
        vec![state.get_proof_for_commitment(&subject_commitment)],
        &confidential_predicate_program().into(),
    )
    .unwrap();
}
