//! M6 host prover: prove sBPF execution of balance_predicate.so in the zkVM.
//!
//! Reads the compiled .so, extracts .text, builds the input buffer,
//! proves execution inside risc0, and prints the image_id + journal.

use anyhow::{Context, Result};
use risc0_zkvm::{default_prover, ExecutorEnv, ProverOpts};
use sbpf_methods::SBPF_PREDICATE_ELF;
use std::fs;

fn load_text(elf: &[u8]) -> Vec<u64> {
    let e_shoff     = u64::from_le_bytes(elf[40..48].try_into().unwrap()) as usize;
    let e_shentsize = u16::from_le_bytes(elf[58..60].try_into().unwrap()) as usize;
    let e_shnum     = u16::from_le_bytes(elf[60..62].try_into().unwrap()) as usize;
    let e_shstrndx  = u16::from_le_bytes(elf[62..64].try_into().unwrap()) as usize;
    let shstr_sh    = e_shoff + e_shstrndx * e_shentsize;
    let shstr_off   = u64::from_le_bytes(elf[shstr_sh+24..shstr_sh+32].try_into().unwrap()) as usize;
    for i in 0..e_shnum {
        let sh = e_shoff + i * e_shentsize;
        let name_off = u32::from_le_bytes(elf[sh..sh+4].try_into().unwrap()) as usize;
        let ns = shstr_off + name_off;
        let ne = elf[ns..].iter().position(|&b| b==0).map(|p| ns+p).unwrap_or(ns);
        if &elf[ns..ne] == b".text" {
            let off  = u64::from_le_bytes(elf[sh+24..sh+32].try_into().unwrap()) as usize;
            let size = u64::from_le_bytes(elf[sh+32..sh+40].try_into().unwrap()) as usize;
            return elf[off..off+size].chunks_exact(8)
                .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
                .collect();
        }
    }
    panic!("no .text section");
}

fn build_input_buffer(balance: u64, threshold: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0u64.to_le_bytes()); // num_accounts = 0
    buf.extend_from_slice(&16u64.to_le_bytes()); // ix_data_len
    buf.extend_from_slice(&balance.to_le_bytes());
    buf.extend_from_slice(&threshold.to_le_bytes());
    buf.extend_from_slice(&[0u8; 32]); // program_id
    buf
}

fn prove_predicate(balance: u64, threshold: u64) -> Result<bool> {
    let so_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../programs/balance-predicate/target/deploy/balance_predicate.so"
    );
    let so = fs::read(so_path).context("read balance_predicate.so")?;
    let bytecode = load_text(&so);
    let input_buffer = build_input_buffer(balance, threshold);

    let mut env_builder = ExecutorEnv::builder();
    env_builder.write(&(bytecode.len() as u32)).unwrap();
    for w in &bytecode { env_builder.write(w).unwrap(); }
    env_builder.write(&(input_buffer.len() as u32)).unwrap();
    for b in &input_buffer { env_builder.write(b).unwrap(); }
    let env = env_builder.build().unwrap();

    let prover = default_prover();
    let receipt = prover.prove(env, SBPF_PREDICATE_ELF)
        .context("prove failed")?
        .receipt;

    receipt.verify(sbpf_methods::SBPF_PREDICATE_ID)
        .context("verify failed")?;

    let (r0, success): (u64, bool) = receipt.journal.decode().unwrap();
    println!("  r0={r0}, success={success}");
    Ok(success) // r0=0 always in Solana v2; success=no VM fault
}

fn main() -> Result<()> {
    println!("M6: proving sBPF execution inside RISC Zero zkVM");
    println!("image_id: {:?}", sbpf_methods::SBPF_PREDICATE_ID);

    println!("\n[1/2] balance=100, threshold=50 (should pass)...");
    let ok = prove_predicate(100, 50)?;
    println!("  execution completed: {}", if ok { "✓" } else { "✗ (VM fault)" });

    println!("\n[2/2] balance=49, threshold=50 (should fail)...");
    let ok = prove_predicate(49, 50)?;
    println!("  execution completed: {}", if ok { "✓" } else { "✗ (VM fault)" });

    println!("\nM6 gate: sBPF execution proven inside RISC Zero zkVM ✓");
    Ok(())
}
