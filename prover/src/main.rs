//! Tuniq prover — succinct → Groth16 wrap.
//!
//! Two modes:
//!   cargo run --release              # one-shot: reads artifacts/, writes seal.bin
//!   cargo run --release -- serve     # HTTP service on :3000
//!
//! HTTP API (serve mode):
//!   POST /wrap
//!   Body:     {"proof_b64": "<base64>", "image_id_hex": "<64 hex chars>"}
//!   Response: {"seal_b64": "<base64>", "journal_b64": "<base64>", "image_id_hex": "..."}
//!
//! REQUIRES x86 + Docker for the Groth16 compression step.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use risc0_zkvm::{default_prover, InnerReceipt, ProverOpts, Receipt};

//  Core wrap logic

pub struct WrapInput {
    pub proof_bytes: Vec<u8>,
    pub journal_bytes: Vec<u8>,
    pub image_id: [u32; 8],
}

pub struct WrapOutput {
    pub seal: Vec<u8>,    // 256 bytes
    pub journal: Vec<u8>, // PrivacyPreservingCircuitOutput bytes
    pub image_id_hex: String,
}

pub fn parse_image_id(hex: &str) -> Result<[u32; 8]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err(anyhow!("image_id must be 64 hex chars, got {}", hex.len()));
    }
    let mut id = [0u32; 8];
    for (i, word) in id.iter_mut().enumerate() {
        let chunk = &hex[i * 8..(i + 1) * 8];
        *word =
            u32::from_str_radix(chunk, 16).with_context(|| format!("parsing image-id word {i}"))?;
    }
    Ok(id)
}

pub fn image_id_to_hex(id: &[u32; 8]) -> String {
    id.iter().map(|w| format!("{w:08x}")).collect()
}

pub fn wrap(input: WrapInput) -> Result<WrapOutput> {
    let inner: InnerReceipt =
        borsh::from_slice(&input.proof_bytes).context("deserialize InnerReceipt")?;
    let receipt = Receipt::new(inner, input.journal_bytes.clone());

    receipt
        .verify(input.image_id)
        .map_err(|e| anyhow!("succinct receipt failed to verify before wrap: {e}"))?;
    eprintln!("Succinct receipt verified — compressing to Groth16...");

    let prover = default_prover();
    let groth16 = prover
        .compress(&ProverOpts::groth16(), &receipt)
        .context("Groth16 compression (needs x86 + Docker)")?;

    groth16
        .verify(input.image_id)
        .map_err(|e| anyhow!("Groth16 receipt failed to verify: {e}"))?;

    let seal = groth16
        .inner
        .groth16()
        .map_err(|e| anyhow!("expected Groth16 receipt: {e}"))?
        .seal
        .clone();

    eprintln!("Wrap OK — seal {} bytes.", seal.len());

    Ok(WrapOutput {
        seal,
        journal: input.journal_bytes,
        image_id_hex: image_id_to_hex(&input.image_id),
    })
}

// ─── One-shot CLI mode ───────────────────────────────────────────────────────

fn run_oneshot() -> Result<()> {
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

    let out = wrap(WrapInput {
        proof_bytes,
        journal_bytes,
        image_id,
    })?;
    fs::write(dir.join("seal.bin"), &out.seal).context("write artifacts/seal.bin")?;
    println!(
        "WRAP OK — wrote artifacts/seal.bin ({} bytes).",
        out.seal.len()
    );
    println!("This is the Groth16 proof the Solana verifier checks (<200k CU).");
    Ok(())
}

//  HTTP service mode

#[cfg(feature = "serve")]
mod server {
    use super::*;
    use axum::{extract::Json, http::StatusCode, response::IntoResponse, routing::post, Router};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize)]
    pub struct WrapRequest {
        pub proof_b64: String,
        pub image_id_hex: String,
    }

    #[derive(Serialize)]
    pub struct WrapResponse {
        pub seal_b64: String,
        pub journal_b64: String,
        pub image_id_hex: String,
    }

    async fn handle_wrap(Json(req): Json<WrapRequest>) -> impl IntoResponse {
        let proof_bytes = match B64.decode(&req.proof_b64) {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("invalid base64 proof: {e}"),
                )
                    .into_response()
            }
        };
        // journal_bytes: the prover reconstructs the receipt from proof bytes;
        // journal comes from the captured artifacts alongside the proof.
        // For the service, the caller must also supply journal_b64.
        // (See request schema comment — kept simple for M3.)
        let image_id = match parse_image_id(&req.image_id_hex) {
            Ok(id) => id,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("invalid image_id: {e}")).into_response()
            }
        };

        // The journal is embedded in the receipt — we extract it after verify.
        // For the wrap, we read from a side-channel artifacts/journal.bin on the
        // service host (the x86 box keeps the journal alongside the proof).
        // M4: include journal_b64 in the request for full stateless operation.
        let journal_bytes = match fs::read("artifacts/journal.bin") {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("cannot read journal: {e}"),
                )
                    .into_response()
            }
        };

        match wrap(WrapInput {
            proof_bytes,
            journal_bytes,
            image_id,
        }) {
            Ok(out) => Json(WrapResponse {
                seal_b64: B64.encode(&out.seal),
                journal_b64: B64.encode(&out.journal),
                image_id_hex: out.image_id_hex,
            })
            .into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }

    pub async fn serve() {
        let app = Router::new().route("/wrap", post(handle_wrap));
        let addr = "0.0.0.0:3000";
        eprintln!("Tuniq prover service listening on {addr}");
        eprintln!("POST /wrap  {{\"proof_b64\": \"...\", \"image_id_hex\": \"...\"}}");
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    }
}

//  Entry point

#[cfg(feature = "serve")]
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("serve") {
        server::serve().await;
        Ok(())
    } else {
        run_oneshot()
    }
}

#[cfg(not(feature = "serve"))]
fn main() -> Result<()> {
    run_oneshot()
}
