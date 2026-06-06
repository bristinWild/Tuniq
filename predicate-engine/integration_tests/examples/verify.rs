//! Local pre-flight: verify the captured proof artifacts WITHOUT x86 or Groth16.
//!
//! Mirrors nssa's internal `Proof::is_valid_for`:
//!   let inner: InnerReceipt = borsh::from_slice(&proof_bytes)?;
//!   let receipt = Receipt::new(inner, journal_bytes);
//!   receipt.verify(PRIVACY_PRESERVING_CIRCUIT_ID)
//!
//! Confirms the on-disk artifact round-trips and verifies against the privacy
//! circuit image id — so if the later x86 Groth16 wrap fails, you know it's the
//! wrap, not the artifact.
//!
//! Run:
//!   cargo run -p integration_tests --example verify --release

use std::fs;
use std::path::Path;

use nssa::PRIVACY_PRESERVING_CIRCUIT_ID;
use risc0_zkvm::{InnerReceipt, Receipt};

fn main() {
    let dir = Path::new("artifacts");
    let proof_bytes = fs::read(dir.join("proof.bin")).expect("read artifacts/proof.bin");
    let journal_bytes = fs::read(dir.join("journal.bin")).expect("read artifacts/journal.bin");

    println!(
        "Read artifacts: proof.bin {} bytes, journal.bin {} bytes",
        proof_bytes.len(),
        journal_bytes.len()
    );

    // Same reconstruction nssa performs internally.
    let inner: InnerReceipt =
        borsh::from_slice(&proof_bytes).expect("deserialize InnerReceipt from proof.bin");
    let receipt = Receipt::new(inner, journal_bytes);

    match receipt.verify(PRIVACY_PRESERVING_CIRCUIT_ID) {
        Ok(()) => {
            println!(
                "PRE-FLIGHT PASS — receipt verifies against PRIVACY_PRESERVING_CIRCUIT_ID.\n\
                 Artifact is intact and ready for the x86 succinct->Groth16 wrap."
            );
        }
        Err(e) => {
            eprintln!("PRE-FLIGHT FAIL — {e}");
            std::process::exit(1);
        }
    }
}
