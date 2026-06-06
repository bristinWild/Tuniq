//! Integration test: verify the real shielded confidential-predicate proof on
//! a local Solana VM (litesvm). First on-chain verification of the shielded proof.
//!
//! Run from coordinator/: cargo test -p coordinator --test verify_predicate -- --nocapture

use std::path::Path;
use std::{fs, str::FromStr};

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::hash::hash;
use litesvm::LiteSVM;
use solana_account::Account as SolanaAccount;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction::{AccountMeta, Instruction};

// ---- program IDs (from keypair files) ----
const VERIFIER_ROUTER_ID: &str = "6JvFfBrvCcWgANKh1Eae9xDq4RC6cfJuBcf71rp2k9Y7";
const GROTH16_VERIFIER_ID: &str = "THq1qFYQoh7zgcjXoMXduDBqiZRCPeg3PvvMbrVQUge";
const COORDINATOR_ID: &str = "39jHP7Hs6zvCWsG3gJHVPfZfdFwAjhGfnFiyGDcPN7bY";

// ---- artifact paths (relative to coordinator/programs/coordinator/) ----
const ARTIFACTS: &str = "../../../predicate-engine/artifacts";
const ROUTER_SO: &str  = "/Users/bristinborah/.cargo/git/checkouts/risc0-solana-1c84ab93fd21abb4/ee41593/solana-verifier/target/deploy/verifier_router.so";
const GROTH16_SO: &str = "/Users/bristinborah/.cargo/git/checkouts/risc0-solana-1c84ab93fd21abb4/ee41593/solana-verifier/target/deploy/groth_16_verifier.so";
const COORDINATOR_SO: &str = "../../target/deploy/coordinator.so";

// ---- helpers ----
fn pubkey(s: &str) -> Pubkey {
    Pubkey::from_str(s).unwrap()
}

/// anchor account discriminator = sha256("account:<Name>")[0..8]
fn acct_disc(name: &str) -> [u8; 8] {
    hash(format!("account:{name}").as_bytes()).to_bytes()[..8]
        .try_into()
        .unwrap()
}

/// anchor instruction discriminator = sha256("global:<name>")[0..8]
fn ix_disc(name: &str) -> [u8; 8] {
    hash(format!("global:{name}").as_bytes()).to_bytes()[..8]
        .try_into()
        .unwrap()
}

fn pda(seeds: &[&[u8]], prog: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, prog).0
}

/// Convert a Pubkey (anchor v2.x) to the [u8;32] that litesvm's Address::from expects.
fn addr(pk: &Pubkey) -> [u8; 32] {
    pk.to_bytes()
}

/// Convert a newer solana_keypair pubkey to an anchor Pubkey via bytes.
fn kp_pubkey(kp: &Keypair) -> Pubkey {
    Pubkey::new_from_array(kp.pubkey().to_bytes())
}

/// Build a newer solana_instruction::Instruction with anchor AccountMeta.
/// Both live in solana_instruction so they are the same type (different ver).
/// We route via bytes to stay version-agnostic.
fn make_ix(program_id: &Pubkey, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: program_id.to_bytes().into(),
        accounts,
        data,
    }
}

fn make_meta(pk: &Pubkey, is_writable: bool, is_signer: bool) -> AccountMeta {
    let pk_new = Pubkey::new_from_array(pk.to_bytes());
    if is_writable {
        AccountMeta::new(pk_new.to_bytes().into(), is_signer)
    } else {
        AccountMeta::new_readonly(pk_new.to_bytes().into(), is_signer)
    }
}

/// BN254 G1 negation: (x, y) -> (x, Q - y).
/// The groth_16_verifier's verify_groth16 requires pi_a to be negated.
fn negate_g1(pi_a: &[u8; 64]) -> [u8; 64] {
    const Q: [u8; 32] = [
        0x30,0x64,0x4e,0x72,0xe1,0x31,0xa0,0x29,
        0xb8,0x50,0x45,0xb6,0x81,0x81,0x58,0x5d,
        0x97,0x81,0x6a,0x91,0x68,0x71,0xca,0x8d,
        0x3c,0x20,0x8c,0x16,0xd8,0x7c,0xfd,0x47,
    ];
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&pi_a[..32]);
    let y = &pi_a[32..64];
    let mut neg_y = [0u8; 32];
    let mut borrow: i16 = 0;
    for i in (0..32).rev() {
        let diff = Q[i] as i16 - y[i] as i16 - borrow;
        if diff < 0 { neg_y[i] = (diff + 256) as u8; borrow = 1; }
        else         { neg_y[i] = diff as u8;          borrow = 0; }
    }
    out[32..64].copy_from_slice(&neg_y);
    out
}


#[test]
fn verify_shielded_predicate_on_solana() {
    // ---- load artifacts ----
    let arts = Path::new(ARTIFACTS);
    let seal = fs::read(arts.join("seal.bin")).expect("read seal.bin — re-run prove example first");
    let journal = fs::read(arts.join("journal.bin")).unwrap();
    let img_hex = fs::read_to_string(arts.join("image_id.txt")).unwrap();
    assert_eq!(seal.len(), 256);
    assert_eq!(journal.len(), 696);

    let img_hex = img_hex.trim();
    // groth_16_verifier expects each u32 word in LE bytes: my_proof_test proved
    // this with val.to_le_bytes(). image_id.txt stores words as big-endian hex.
    let image_id: [u8; 32] = (0..8usize)
        .flat_map(|i| {
            let w = u32::from_str_radix(&img_hex[i*8..(i+1)*8], 16).unwrap();
            w.to_le_bytes().to_vec()
        })
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap();

    let nul_hex = "39e15eadcbc684bfca46f76bec4182d71cf5b26833d6e20af0b283515d9f92b2";
    let nullifier: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&nul_hex[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    let selector: [u8; 4] = seal[0..4].try_into().unwrap();

    // ---- program IDs ----
    let router_id = pubkey(VERIFIER_ROUTER_ID);
    let groth16_id = pubkey(GROTH16_VERIFIER_ID);
    let coord_id = pubkey(COORDINATOR_ID);
    let sys_id = pubkey("11111111111111111111111111111111");

    // ---- PDAs ----
    let router_state = pda(&[b"router"], &router_id);
    let verifier_entry = pda(&[b"verifier", &selector], &router_id);
    let config_pda = pda(&[b"config"], &coord_id);
    let spent_nul_pda = pda(&[b"nullifier", &nullifier], &coord_id);

    // ---- litesvm ----
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(addr(&router_id), ROUTER_SO)
        .expect("load verifier_router.so");
    svm.add_program_from_file(addr(&groth16_id), GROTH16_SO)
        .expect("load groth_16_verifier.so");
    svm.add_program_from_file(addr(&coord_id), COORDINATOR_SO)
        .expect("load coordinator.so");

    let payer = Keypair::new();
    let payer_pk = kp_pubkey(&payer);
    svm.airdrop(&addr(&payer_pk).into(), 10_000_000_000)
        .unwrap();

    // ---- seed VerifierRouter (bypass INITIAL_OWNER) ----
    {
        let mut data = acct_disc("VerifierRouter").to_vec();
        // Ownership { owner: Some(payer), pending_owner: None }
        data.push(1u8);
        data.extend_from_slice(&payer_pk.to_bytes());
        data.push(0u8);
        svm.set_account(
            addr(&router_state).into(),
            SolanaAccount {
                lamports: 1_000_000,
                data,
                owner: router_id.to_bytes().into(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }

    // ---- seed VerifierEntry ----
    {
        let mut data = acct_disc("VerifierEntry").to_vec();
        data.extend_from_slice(&selector);
        data.extend_from_slice(&groth16_id.to_bytes());
        data.push(0u8); // estopped = false
        svm.set_account(
            addr(&verifier_entry).into(),
            SolanaAccount {
                lamports: 1_000_000,
                data,
                owner: router_id.to_bytes().into(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }

    // ---- coordinator::initialize ----
    {
        let mut d = ix_disc("initialize").to_vec();
        d.extend_from_slice(&image_id);
        let ix = make_ix(
            &coord_id,
            vec![
                make_meta(&config_pda, true, false),
                make_meta(&payer_pk, true, true),
                make_meta(&sys_id, false, false),
            ],
            d,
        );
        let bh = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], bh);
        svm.send_transaction(tx).expect("initialize");
        println!("initialize: ok");
    }

    // ---- coordinator::verify_predicate ----
    {
        let mut d = ix_disc("verify_predicate").to_vec();
        d.extend_from_slice(&selector);
        let pi_a_neg = negate_g1(&seal[0..64].try_into().unwrap());
        d.extend_from_slice(&pi_a_neg);
        d.extend_from_slice(&seal[64..192]);
        d.extend_from_slice(&seal[192..256]);
        let jlen = (journal.len() as u32).to_le_bytes();
        d.extend_from_slice(&jlen);
        d.extend_from_slice(&journal);
        d.extend_from_slice(&nullifier);

        let ix = make_ix(
            &coord_id,
            vec![
                make_meta(&config_pda, true, false),
                make_meta(&spent_nul_pda, true, false),
                make_meta(&router_id, false, false),
                make_meta(&router_state, false, false),
                make_meta(&verifier_entry, false, false),
                make_meta(&groth16_id, false, false),
                make_meta(&payer_pk, true, true),
                make_meta(&sys_id, false, false),
            ],
            d,
        );
        let bh = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], bh);
        svm.send_transaction(tx)
            .expect("verify_predicate — shielded proof verifies on Solana!");
        println!("VERIFY_PREDICATE: ok — shielded proof verified on Solana!");
        println!("M1 gate: GREEN");
    }
}
