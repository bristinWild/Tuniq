//! Capture a real confidential-predicate proof to disk — the M0 input artifacts.
//!
//! Reproduces the passing pass-case proof (shielded balance 5000 >= 1000) and
//! writes the two artifacts the Groth16 wrap step consumes on x86:
//!
//!   artifacts/proof.bin     = proof.into_inner()        (borsh InnerReceipt bytes)
//!   artifacts/journal.bin   = output.to_bytes()         (PrivacyPreservingCircuitOutput)
//!   artifacts/image_id.txt  = PRIVACY_PRESERVING_CIRCUIT_ID (hex; the verification key)
//!
//! Run (macOS):
//!   RISC0_DEV_MODE=0 \
//!     CC_aarch64_apple_darwin="$(xcrun --find clang)" HOST_CC="$(xcrun --find clang)" \
//!     cargo run -p integration_tests --example prove --release
//! Run (Linux):
//!   RISC0_DEV_MODE=0 cargo run -p integration_tests --example prove --release

use std::fs;
use std::path::Path;

use nssa::{
    execute_and_prove,
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    SharedSecretKey, V03State, PRIVACY_PRESERVING_CIRCUIT_ID,
};
use nssa_core::{
    account::{Account, AccountWithMetadata, Data, Nonce},
    encryption::ViewingPublicKey,
    Commitment, Nullifier, NullifierPublicKey, NullifierSecretKey,
};

fn subject_nsk() -> NullifierSecretKey {
    [55; 32]
}
fn subject_npk() -> NullifierPublicKey {
    NullifierPublicKey::from(&subject_nsk())
}
fn subject_vpk() -> ViewingPublicKey {
    ViewingPublicKey::from_scalar([66; 32])
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

fn main() {
    let secret_balance = 5_000_u128;
    let threshold = 1_000_u128;

    let npk = subject_npk();
    let nsk = subject_nsk();
    let vpk = subject_vpk();

    let subject_account = Account {
        program_owner: confidential_predicate_program().id(),
        balance: secret_balance,
        data: Data::default(),
        nonce: Nonce::private_account_nonce_init(&npk),
    };
    let commitment = Commitment::new(&npk, &subject_account);
    let nullifier = Nullifier::for_account_initialization(&npk);

    let mut state =
        V03State::new_with_genesis_accounts(&[], vec![(commitment.clone(), nullifier)], 0);
    deploy_program(&mut state);

    let esk = [7u8; 32];
    let shared_secret = SharedSecretKey::new(&esk, &vpk);
    let subject_pre = AccountWithMetadata::new(subject_account.clone(), true, &npk);

    let instruction = predicate_core::Instruction::CheckBalanceOverThreshold { threshold };

    println!("Proving confidential predicate (5000 >= 1000) — real proof, ~90s...");
    let (output, proof) = execute_and_prove(
        vec![subject_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![1],
        vec![(npk, shared_secret)],
        vec![nsk],
        vec![state.get_proof_for_commitment(&commitment)],
        &confidential_predicate_program().into(),
    )
    .expect("predicate should prove: 5000 >= 1000");

    // --- write the M0 artifacts ---
    let dir = Path::new("artifacts");
    fs::create_dir_all(dir).expect("create artifacts/");

    let proof_bytes = proof.into_inner();
    fs::write(dir.join("proof.bin"), &proof_bytes).expect("write proof.bin");

    let journal_bytes = output.to_bytes();
    fs::write(dir.join("journal.bin"), &journal_bytes).expect("write journal.bin");

    // The proof verifies against the PRIVACY-PRESERVING CIRCUIT image id — the
    // receipt\'s top-level claim. The predicate guest runs composed inside the
    // circuit, so its id is bound into the journal, NOT the verification key.
    // (See nssa Proof::is_valid_for: receipt.verify(PRIVACY_PRESERVING_CIRCUIT_ID).)
    let image_id_hex = PRIVACY_PRESERVING_CIRCUIT_ID
        .iter()
        .map(|w| format!("{w:08x}"))
        .collect::<String>();
    fs::write(dir.join("image_id.txt"), &image_id_hex).expect("write image_id.txt");

    println!("Wrote artifacts/:");
    println!("  proof.bin    {} bytes", proof_bytes.len());
    println!("  journal.bin  {} bytes", journal_bytes.len());
    println!("  image_id.txt {image_id_hex}");
    println!("\nThese are the inputs to the M0 succinct->Groth16 wrap step (x86).");
}
