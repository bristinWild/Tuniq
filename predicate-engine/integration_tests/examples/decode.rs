//! Decode the captured journal (PrivacyPreservingCircuitOutput) and print it.
//!
//! `to_bytes()` is `bytemuck::cast_slice(&risc0_zkvm::serde::to_vec(&self))`,
//! so the inverse is: bytes -> u32 words -> risc0_zkvm::serde::from_slice.
//! (Confirmed by nssa_core's own circuit_io round-trip test.)
//!
//! Run:
//!   cargo run -p integration_tests --example decode --release

use std::fs;
use std::path::Path;

use nssa_core::PrivacyPreservingCircuitOutput;

fn main() {
    let dir = Path::new("artifacts");
    let bytes = fs::read(dir.join("journal.bin")).expect("read artifacts/journal.bin");
    println!("journal.bin: {} bytes", bytes.len());

    // risc0's serde operates on a &[u32]. to_bytes() cast u32 words -> bytes,
    // so cast back. (Length is a multiple of 4 by construction.)
    let words: &[u32] = bytemuck::cast_slice(&bytes);
    println!("as u32 words: {}", words.len());

    let output: PrivacyPreservingCircuitOutput =
        risc0_zkvm::serde::from_slice(words).expect("decode PrivacyPreservingCircuitOutput");

    println!("\n=== PrivacyPreservingCircuitOutput ===");
    println!(
        "public_pre_states:  {} entries",
        output.public_pre_states.len()
    );
    println!(
        "public_post_states: {} entries",
        output.public_post_states.len()
    );
    println!("ciphertexts:        {} entries", output.ciphertexts.len());
    println!(
        "new_commitments:    {} entries",
        output.new_commitments.len()
    );
    println!(
        "new_nullifiers:     {} entries",
        output.new_nullifiers.len()
    );
    println!("\nfull dump:\n{output:#?}");

    // The secret balance (5000) must NOT appear. 5000 = 0x1388 -> LE bytes 88,13.
    let needle = 5000u128.to_le_bytes();
    let leaked = bytes.windows(needle.len()).any(|w| w == needle);
    println!(
        "\nsecret-absence check: 5000 {} in journal bytes",
        if leaked {
            "PRESENT (LEAK!)"
        } else {
            "absent (good)"
        }
    );
}
