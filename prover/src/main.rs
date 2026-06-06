//! Tuniq prover — succinct -> Groth16 wrap (the x86-only M0 step).
//!
//! Reads the three artifacts captured by `predicate-engine` and produces a
//! Groth16 seal verifiable on Solana:
//!
//!   in:  artifacts/proof.bin     (borsh InnerReceipt)
//!        artifacts/journal.bin    (PrivacyPreservingCircuitOutput bytes)
//!        artifacts/image_id.txt   (PRIVACY_PRESERVING_CIRCUIT_ID, hex)
//!   out: artifacts/seal.bin       (256-byte Groth16 seal)
//!
//! REQUIRES x86 + Docker: the succinct->Groth16 (stark2snark) step uses the
//! RISC Zero Docker prover. Will NOT run on Apple Silicon — that's why this is a
//! separate box. (It DOES compile on any host, so check it builds locally first.)
//!
//! Usage (on the x86 droplet, from a dir containing artifacts/):
//!   cargo run --release

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use risc0_zkvm::{default_prover, InnerReceipt, ProverOpts, Receipt};

/// Parse the 64-hex-char image id (8 u32 words, each big-endian 8 hex) we wrote.
fn parse_image_id(hex: &str) -> Result<[u32; 8]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err(anyhow!(
            "image_id.txt must be 64 hex chars, got {}",
            hex.len()
        ));
    }
    let mut id = [0u32; 8];
    for (i, word) in id.iter_mut().enumerate() {
        let start = i * 8;
        let chunk = &hex[start..start + 8];
        *word =
            u32::from_str_radix(chunk, 16).with_context(|| format!("parsing image-id word {i}"))?;
    }
    Ok(id)
}

fn main() -> Result<()> {
    let dir = Path::new("artifacts");

    let proof_bytes = fs::read(dir.join("proof.bin")).context("read artifacts/proof.bin")?;
    let journal_bytes = fs::read(dir.join("journal.bin")).context("read artifacts/journal.bin")?;
    let image_id_hex =
        fs::read_to_string(dir.join("image_id.txt")).context("read artifacts/image_id.txt")?;
    let image_id = parse_image_id(&image_id_hex)?;

    println!(
        "Loaded: proof.bin {} bytes, journal.bin {} bytes",
        proof_bytes.len(),
        journal_bytes.len()
    );

    // Reconstruct the succinct receipt (same as the local pre-flight).
    let inner: InnerReceipt =
        borsh::from_slice(&proof_bytes).context("deserialize InnerReceipt")?;
    let receipt = Receipt::new(inner, journal_bytes);

    // Sanity: the succinct receipt must verify against the circuit id before we
    // pay to compress it.
    receipt
        .verify(image_id)
        .map_err(|e| anyhow!("succinct receipt failed to verify before wrap: {e}"))?;
    println!("Succinct receipt verifies against image id — compressing to Groth16...");

    // --- the x86-only step: succinct -> Groth16 (stark2snark via Docker) ---
    let prover = default_prover();
    let groth16 = prover
        .compress(&ProverOpts::groth16(), &receipt)
        .context("Groth16 compression (needs x86 + Docker)")?;

    // Confirm the compressed receipt still verifies against the same id.
    groth16
        .verify(image_id)
        .map_err(|e| anyhow!("Groth16 receipt failed to verify: {e}"))?;

    // Extract the 256-byte Groth16 seal.
    let seal = groth16
        .inner
        .groth16()
        .map_err(|e| anyhow!("expected a Groth16 receipt: {e}"))?
        .seal
        .clone();

    fs::write(dir.join("seal.bin"), &seal).context("write artifacts/seal.bin")?;
    println!("WRAP OK — wrote artifacts/seal.bin ({} bytes).", seal.len());
    println!("This is the Groth16 proof the Solana verifier checks (<200k CU).");
    Ok(())
}
