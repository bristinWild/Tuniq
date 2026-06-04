# Tuniq monorepo task runner.
# Polyglot monorepo: each build unit is self-contained with its own toolchain.
# This file ties them together WITHOUT a shared Cargo workspace (the risc0,
# anchor, and plain-x86 toolchains conflict if forced into one lock).

# List available recipes.
default:
    @just --list

# --- shared-types: the cross-boundary contract (keep it tiny & dep-light) ---
test-shared:
    cd shared-types && cargo test

# --- predicate-engine (LEZ / RISC Zero) ---
# Host-side build of the predicate program + methods (excludes the guest).
build-predicate:
    cd predicate-engine && cargo build

# The guest is a separate build (excluded from the workspace), driven by the
# methods crate's build.rs (risc0_build::embed_methods).
build-guest:
    cd predicate-engine && CC_aarch64_apple_darwin="$(xcrun --find clang)" HOST_CC="$(xcrun --find clang)" cargo build -p predicate-methods

# Run predicate tests with REAL proofs (no dev mode) — slow, the honest path.
test-predicate:
    cd predicate-engine && RISC0_DEV_MODE=0 cargo test

# Fast iteration only: dev-mode proofs (NOT a real proof — never cite timings).
test-predicate-dev:
    cd predicate-engine && RISC0_DEV_MODE=1 cargo test

# --- M0: the gate (placeholders — wire up once the crates below exist) ---
# The single connecting run: take the Experiment 2 shielded, commitment-bound
# succinct receipt -> wrap to Groth16 (x86) -> verify via Solana logic ->
# parse the circuit output. See ROADMAP.md M0.
gate:
    @echo "M0 gate not yet wired. Steps:"
    @echo "  1. produce the shielded predicate succinct receipt (predicate-engine)"
    @echo "  2. reconstruct Receipt from nssa Proof, wrap succinct->groth16 (prover, x86)"
    @echo "  3. verify via Solana verify_groth16 + parse circuit output (coordinator)"

# --- coordinator (Solana / Anchor) — scaffold pending M0 output shape ---
build-coordinator:
    cd coordinator && anchor build --no-idl

# --- prover (offchain x86 worker) — scaffold pending M0 ---
build-prover:
    cd prover && cargo build --release

# Lint everything that has a Rust toolchain.
fmt:
    cd shared-types && cargo fmt
    cd predicate-engine && cargo fmt
