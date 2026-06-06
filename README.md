# Tuniq — Confidential sBPF on Logos

Run **confidential Solana programs**: Solana logic (sBPF) interpreted inside
Logos's privacy-preserving zkVM, operating on **shielded inputs that never become
public**, with the verified result settled back on Solana.

The core idea: Solana developers write programs the way they already do — same
Rust, same Anchor, same sBPF bytecode — but now over private data. The program
runs privately on Logos; the secret never touches Solana; Solana only ever sees a
verified result.

> **Status: early, building in public.** The core zero-knowledge guarantee (the
> "moat") is implemented and proven on the real Logos stack. The product layers
> around it (Solana coordinator, proving service, developer SDK) are not built
> yet. See [What works today](#what-works-today) for exactly what you can run.

Research write-up and experiment results: see [`docs/`](docs/) and the
[experiments repo](https://github.com/bristinWild/tuniq-experiments).

---

## What works today

One thing is implemented and reproducible end-to-end on real proofs: a
**confidential predicate over a shielded account**. It evaluates `balance >=
threshold` over a private balance bound to its on-chain commitment, producing a
verifiable proof that the predicate held **without revealing the balance**, and
**without the prover being able to forge it**.

This is the irreplaceable part — the guarantee no public chain and no plain zkVM
can provide. Reproducing it is the test below.

### What is NOT built yet

So expectations are honest:

- **Solana coordinator** — verifying the proof on Solana and forwarding the
  result. Not built.
- **Proving service** — the offchain x86 worker that wraps the proof to Groth16.
  Not built.
- **Developer SDK** — annotations + build pipeline for external developers. Not
  built.

You can reproduce the ZK guarantee. You cannot yet "write a confidential Solana
program and settle it on Solana" — that's the roadmap.

---

## Reproduce the moat

### Prerequisites

- **Rust 1.94.0** (pinned via `rust-toolchain.toml`; rustup will honor it).
- **RISC Zero toolchain** — install via [`rzup`](https://dev.risczero.com/api/zkvm/install):
  `curl -L https://risczero.com/install | bash && rzup install`
- **Network access to GitHub** — the Logos (`nssa`) and SPEL crates resolve as git
  dependencies on first build.
- ~4 GB free RAM and a few minutes. The first build compiles the guest and the
  Logos/SPEL deps (~3 min); each real proof takes ~90s on an unaccelerated laptop.

### Run it

**macOS (Apple Silicon):** the guest cross-compile needs the host C builds pointed
at Apple clang (otherwise `ring` is handed the riscv compiler and fails):

```bash
cd predicate-engine
RISC0_DEV_MODE=0 \
  CC_aarch64_apple_darwin="$(xcrun --find clang)" \
  HOST_CC="$(xcrun --find clang)" \
  cargo test -p integration_tests --test confidential_predicate -- --nocapture
```

**Linux:** no clang shims needed:

```bash
cd predicate-engine
RISC0_DEV_MODE=0 cargo test -p integration_tests --test confidential_predicate -- --nocapture
```

> `RISC0_DEV_MODE=0` produces **real** proofs (slow, honest). Set `RISC0_DEV_MODE=1`
> for fast mock proofs to confirm wiring — but a dev-mode pass is not a real proof.

### What you should see

```
running 2 tests
test confidential_predicate_fails_when_below_threshold - should panic ... ok
test confidential_predicate_passes_over_shielded_balance ... ok
test result: ok. 2 passed; ...
```

- **passes_over_shielded_balance** — a real proof that a shielded balance of 5000
  clears the 1000 threshold; the balance never appears in any public field.
- **fails_when_below_threshold** — proving `500 >= 1000` is *impossible*: the guest
  panics and no proof can be produced. This `should_panic` passing **is** the
  soundness/unforgeability guarantee, not a test artifact.

There's also a fast contract-level check with no proving:

```bash
just test-shared    # or: cd shared-types && cargo test
```

---

## Repository layout

```
tuniq/
├── shared-types/      cross-boundary contract types (borsh-only; the result schema)
├── predicate-engine/  the confidential predicate — IMPLEMENTED & PROVEN
│   ├── core/          instruction types
│   ├── src/           the predicate handler (check.rs)
│   ├── methods/       guest (risc0) — the sBPF/predicate guest + image ID
│   └── integration_tests/  the real-proof moat test
├── coordinator/       Solana/Anchor settlement — NOT BUILT YET
├── prover/            offchain x86 Groth16 wrap worker — NOT BUILT YET
├── reference-app/     the v1 demo app — NOT BUILT YET
├── docs/              litepaper, roadmap, v1 scope
└── justfile           task runner (just --list)
```

This is a polyglot monorepo: each unit is self-contained with its own toolchain,
tied together by the `justfile` rather than a single Cargo workspace (the risc0,
Anchor, and x86 toolchains conflict if forced into one lock).

---

## Roadmap (short version)

1. **M0 — the gate:** wrap the proof above to Groth16 (x86) and verify it through
   Solana's verification logic. The one connecting step between the proven moat and
   Solana settlement.
2. **M1:** coordinator + proving worker → one confidential predicate, settled on
   Solana end-to-end.
3. **M2–M5:** accelerators, opcode coverage, flagship demo, developer SDK.

Full detail in [`docs/dev-roadmap.md`](docs/dev-roadmap.md).

---

## License

TBD.