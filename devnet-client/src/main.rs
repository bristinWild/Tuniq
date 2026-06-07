//! Tuniq devnet submit client.
//!
//! Reads /tmp/wrap_response.json and submits coordinator::initialize +
//! verify_predicate to Solana devnet via RPC.
//!
//! Place at ~/tuniq/devnet-client/src/main.rs
//! Run: cargo run --release

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::hashv,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use std::fs;
use std::str::FromStr;

const VERIFIER_ROUTER_ID: &str = "6C7Xkz3jC19aEm5i3fiP4AicfWVy6wdxWWhFtq4Vkr3Q";
const GROTH16_VERIFIER_ID: &str = "BRFJjGGWBWmb53P48rU3P4MrxMcsXqypPq4JzGi3gctZ";
const COORDINATOR_ID: &str = "39jHP7Hs6zvCWsG3gJHVPfZfdFwAjhGfnFiyGDcPN7bY";
const CONSUMER_ID: &str = "Gv1x7gNnbL94uuQf5s92j6DZ93u4e5aaWWt1nYfDPHWQ";
const NULLIFIER_HEX: &str = "39e15eadcbc684bfca46f76bec4182d71cf5b26833d6e20af0b283515d9f92b2";

#[derive(Deserialize)]
struct WrapResponse {
    seal_b64: String,
    journal_b64: String,
    image_id_hex: String,
}

fn disc(name: &str) -> [u8; 8] {
    let h = solana_sdk::hash::hash(format!("global:{name}").as_bytes());
    h.to_bytes()[..8].try_into().unwrap()
}

fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

fn negate_g1(pi_a: &[u8; 64]) -> [u8; 64] {
    const Q: [u8; 32] = [
        0x30, 0x64, 0x4e, 0x72, 0xe1, 0x31, 0xa0, 0x29, 0xb8, 0x50, 0x45, 0xb6, 0x81, 0x81, 0x58,
        0x5d, 0x97, 0x81, 0x6a, 0x91, 0x68, 0x71, 0xca, 0x8d, 0x3c, 0x20, 0x8c, 0x16, 0xd8, 0x7c,
        0xfd, 0x47,
    ];
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&pi_a[..32]);
    let y = &pi_a[32..64];
    let mut neg_y = [0u8; 32];
    let mut borrow: i16 = 0;
    for i in (0..32).rev() {
        let diff = Q[i] as i16 - y[i] as i16 - borrow;
        if diff < 0 {
            neg_y[i] = (diff + 256) as u8;
            borrow = 1;
        } else {
            neg_y[i] = diff as u8;
            borrow = 0;
        }
    }
    out[32..64].copy_from_slice(&neg_y);
    out
}

fn parse_image_id(hex: &str) -> [u8; 32] {
    let hex = hex.trim();
    (0..8usize)
        .flat_map(|i| {
            let w = u32::from_str_radix(&hex[i * 8..(i + 1) * 8], 16).unwrap();
            w.to_le_bytes().to_vec()
        })
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap()
}

fn main() -> anyhow::Result<()> {
    // Load wrap response
    let resp: WrapResponse = serde_json::from_str(&fs::read_to_string("/tmp/wrap_response.json")?)?;
    let seal = B64.decode(&resp.seal_b64)?;
    let journal = B64.decode(&resp.journal_b64)?;
    let image_id = parse_image_id(&resp.image_id_hex);
    let nullifier: [u8; 32] = hex::decode(NULLIFIER_HEX)?.try_into().unwrap();
    let selector: [u8; 4] = seal[0..4].try_into().unwrap();

    println!(
        "seal: {} bytes, journal: {} bytes",
        seal.len(),
        journal.len()
    );
    println!("selector: {}", hex::encode(selector));

    // Connect to devnet
    let rpc = RpcClient::new_with_commitment(
        "https://api.devnet.solana.com",
        CommitmentConfig::confirmed(),
    );

    // Load keypair
    let keypair_path = shellexpand::tilde("~/.config/solana/id.json").to_string();
    let payer =
        read_keypair_file(&keypair_path).map_err(|e| anyhow::anyhow!("keypair error: {e}"))?;
    let payer_pk = payer.pubkey();
    println!("payer: {payer_pk}");
    println!("balance: {} lamports", rpc.get_balance(&payer_pk)?);

    // Program IDs
    let router_id = Pubkey::from_str(VERIFIER_ROUTER_ID)?;
    let groth16_id = Pubkey::from_str(GROTH16_VERIFIER_ID)?;
    let coord_id = Pubkey::from_str(COORDINATOR_ID)?;
    let consumer_id = Pubkey::from_str(CONSUMER_ID)?;

    // PDAs
    let config_pda = pda(&[b"config"], &coord_id);
    let spent_nul_pda = pda(&[b"nullifier", &nullifier], &coord_id);
    let registry_pda = pda(&[b"registry"], &consumer_id);
    let router_state = pda(&[b"router"], &router_id);
    let verifier_entry = pda(&[b"verifier", &selector], &router_id);

    println!("config_pda:     {config_pda}");
    println!("registry_pda:   {registry_pda}");
    println!("spent_nul_pda:  {spent_nul_pda}");
    println!("router_state:   {router_state}");
    println!("verifier_entry: {verifier_entry}");

    let bh = rpc.get_latest_blockhash()?;


    // ── 0a. verifier_router::initialize ─────────────────────────────────────
    println!("\n[0a] verifier_router::initialize...");
    {
        let mut d = disc("initialize").to_vec();
        let ix = Instruction {
            program_id: router_id,
            accounts: vec![
                AccountMeta::new(router_state, false),
                AccountMeta::new(payer_pk, true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: d,
        };
        let bh = rpc.get_latest_blockhash()?;
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], bh);
        match rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => println!("router initialize ok — {sig}"),
            Err(e)  => println!("router initialize: {e} (may already exist, continuing)"),
        }
    }

    // ── 0b. verifier_router::add_verifier ───────────────────────────────────
    println!("\n[0b] verifier_router::add_verifier...");
    {
        // selector = first 4 bytes of seal
        let mut d = disc("add_verifier").to_vec();
        d.extend_from_slice(&selector); // selector: [u8;4]
        // groth16 ProgramData address (from `solana program show`)
        let groth16_program_data = Pubkey::from_str("9ARmMtyf5oLG4b9FebynH2oM9m4pcLDd9D8tA9BZgf9w")?;
        let ix = Instruction {
            program_id: router_id,
            accounts: vec![
                AccountMeta::new_readonly(router_state, false),
                AccountMeta::new(verifier_entry, false),
                AccountMeta::new_readonly(groth16_program_data, false),
                AccountMeta::new_readonly(groth16_id, false),
                AccountMeta::new(payer_pk, true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: d,
        };
        let bh = rpc.get_latest_blockhash()?;
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], bh);
        match rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => println!("add_verifier ok — {sig}"),
            Err(e)  => println!("add_verifier: {e} (may already exist, continuing)"),
        }
    }

    // ── 1. consumer::initialize ──────────────────────────────────────────────
    println!("\n[1/3] consumer::initialize...");
    let mut d = disc("initialize").to_vec();
    let ix = Instruction {
        program_id: consumer_id,
        accounts: vec![
            AccountMeta::new(registry_pda, false),
            AccountMeta::new(payer_pk, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: d,
    };
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], bh);
    match rpc.send_and_confirm_transaction(&tx) {
        Ok(sig) => println!("consumer::initialize ok — {sig}"),
        Err(e) => println!("consumer::initialize: {e} (may already exist, continuing)"),
    }

    // ── 2. coordinator::initialize ───────────────────────────────────────────
    println!("\n[2/3] coordinator::initialize...");
    let mut d = disc("initialize").to_vec();
    d.extend_from_slice(&image_id);
    d.extend_from_slice(&payer_pk.to_bytes()); // authorized_prover = payer
    let ix = Instruction {
        program_id: coord_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(payer_pk, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: d,
    };
    let bh = rpc.get_latest_blockhash()?;
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], bh);
    match rpc.send_and_confirm_transaction(&tx) {
        Ok(sig) => println!("coordinator::initialize ok — {sig}"),
        Err(e) => println!("coordinator::initialize: {e} (may already exist, continuing)"),
    }

    // ── 3. coordinator::verify_predicate ─────────────────────────────────────
    println!("\n[3/3] coordinator::verify_predicate...");
    let pi_a_neg = negate_g1(&seal[0..64].try_into().unwrap());
    let mut d = disc("verify_predicate").to_vec();
    d.extend_from_slice(&selector);
    d.extend_from_slice(&pi_a_neg);
    d.extend_from_slice(&seal[64..192]);   // pi_b
    d.extend_from_slice(&seal[192..256]);  // pi_c
    // journal_digest = sha256(journal), pre-computed by prover service
    let journal_digest: [u8; 32] = hex::decode("69ccdaed14e35c57e7203f5d754e7e3e62edf35133b3e39c1e632d2d62175e84").unwrap().try_into().unwrap();
    d.extend_from_slice(&journal_digest);
    d.extend_from_slice(&nullifier);

    let ix = Instruction {
        program_id: coord_id,
        accounts: vec![
            AccountMeta::new(config_pda, false),
            AccountMeta::new(spent_nul_pda, false),
            AccountMeta::new_readonly(router_id, false),
            AccountMeta::new_readonly(router_state, false),
            AccountMeta::new_readonly(verifier_entry, false),
            AccountMeta::new_readonly(groth16_id, false),
            AccountMeta::new_readonly(consumer_id, false),
            AccountMeta::new(registry_pda, false),
            AccountMeta::new(payer_pk, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: d,
    };
    let bh = rpc.get_latest_blockhash()?;
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer_pk), &[&payer], bh);
    let sig = rpc.send_and_confirm_transaction_with_spinner(&tx)?;
    println!("\n✓ verify_predicate on devnet!");
    println!("Signature: {sig}");
    println!("Explorer: https://explorer.solana.com/tx/{sig}?cluster=devnet");

    Ok(())
}
