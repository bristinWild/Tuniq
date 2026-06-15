# Tuniq — Development Roadmap

Confidential sBPF on Logos. This is the working build plan from validated research
to v1. It is a living document — update statuses as milestones land.

Litepaper (architecture + experiment results): see `architectural-research-litepaper.md`
Experiments: https://github.com/bristinWild/tuniq-experiments

---

## How to read this

The roadmap is ordered by a single principle: **prove the one unproven connection
before building anything on top of it.** Three experiments validated the hard
questions independently (interpreter cost, the privacy moat, Solana settlement).
M0 connected them; M1 verified the proof on Solana; M2 wired a consumer program;
M3 added security and a proving service; M4 deployed the whole stack to a public
devnet. **M0 through M4 are all green.** Everything downstream is shaped by real
artifacts, not hypotheses.

Status legend:

- ✅ done
- 🔜 next / in progress
- ⬜ not started
- 🚧 blocked on a decision or an upstream dependency

---

## Foundation — validated research ✅

Already complete; this is what the roadmap builds on. Details in the litepaper.

- ✅ **Experiment 1 — interpreter overhead.** ~314 zkVM cycles/instruction;
  100-insn predicate ~36k cycles; proves in ~9.4s on an unaccelerated laptop,
  verifies in 12ms. Small confidential predicates are economically viable.
- ✅ **Experiment 2 — the moat.** `balance >= threshold` over a shielded,
  commitment-bound account on the real LEZ stack (`RISC0_DEV_MODE=0`). Valid case
  proved; false case unprovable. Commitment binding + selective disclosure +
  soundness, all native to Logos.
- ✅ **Experiment 3 — Solana settlement.** Anchor verifier program deploys; RISC
  Zero Solana verifier passes 12/12 vectors; our own confidential predicate proof
  verifies through the real Solana logic (<200k CU), secret absent from the
  journal. Proven for a *standalone* (non-commitment-bound) guest.

---

## M0 — The Gate: connect the moat to settlement ✅

**GREEN.** Experiment 2's shielded, commitment-bound proof was reproduced inside
the monorepo (real proofs, `RISC0_DEV_MODE=0`), captured to disk, and wrapped to
Groth16 on x86 — producing a 256-byte Solana-verifiable seal.

- ✅ Reproduce the Exp 2 confidential predicate in-repo (`predicate-engine`).
- ✅ Capture proof artifacts to disk (`proof.bin`, `journal.bin`, `image_id.txt`).
- ✅ Local pre-flight: verify receipt against `PRIVACY_PRESERVING_CIRCUIT_ID`.
- ✅ Wrap succinct receipt → Groth16 on x86 (`prover/`), producing `seal.bin`.

**Result:** the full shielded path — Logos → succinct receipt → Groth16 seal —
demonstrated end to end. Half B proven.

Depends on: Foundation (Exp 2, Exp 3).

---

## M1 — First end-to-end product slice ✅

**GREEN.** The coordinator verifies the shielded proof in litesvm through the real
Verifier Router CPI and `groth_16_verifier`, on real artifacts. Nullifier PDA
replay guard works. Secret never appears on the Solana side.

- ✅ Decode `journal.bin` against `PrivacyPreservingCircuitOutput` (`decode.rs`).
- ✅ Coordinator: Verifier Router CPI, nullifier-PDA replay guard, event emission.
- ✅ Verify `seal.bin` through the on-chain `verify_groth16` path.
- ✅ Two encoding facts (see `6.m1-green.md`): `pi_a` pre-negated (BN254 G1);
  `image_id` LE bytes per word.

**Result:** `test verify_shielded_predicate_on_solana ... ok`. Commit: `de06ce3`.

Depends on: M0 ✅.

---

## M2 — Consumer wiring ✅

**GREEN.** The coordinator CPIs into a consumer program after a shielded proof
verifies; the consumer's `Registry` PDA increments `verified_count`. The chain is
a composable building block.

- ✅ Consumer program: `Registry` PDA, `record_verification`, event emission.
- ✅ Coordinator CPIs into consumer (passed as account — any compliant consumer).
- ✅ Test asserts `registry.verified_count == 1`. Commit: `d42b238`.

Depends on: M1 ✅.

---

## M3 — Security + proving service ✅

**GREEN.** Authorized prover constraint live; dead code removed; proving service
tested as a callable HTTP API on an x86 droplet. Commit: `c6038f0`.

- ✅ `authorized_prover: Pubkey` in `Config`; only the registered prover keypair
  can call `verify_predicate`.
- ✅ Dead code removed: `journal_contains_nullifier` + `NullifierNotInJournal`.
- ✅ `prover/`: `wrap()` fn + HTTP service behind `--features serve`;
  `POST /wrap` → `{seal_b64, journal_b64, image_id_hex}`.
- ✅ Live test on x86 droplet: ~90s round-trip, 256-byte seal over HTTP.

Depends on: M2 ✅.

---

## M4 — Devnet deployment ✅

**GREEN.** The full stack verified a shielded proof on a **public Solana devnet
validator**, not litesvm. Consumer registry incremented on-chain. Transaction
finalized. Commit: `5a1bb75`.

- ✅ Coordinator + consumer deployed to devnet.
- ✅ Verifier router + groth16 verifier deployed + initialized on devnet (own copies,
  consistent declare_ids, router PDA as groth16 upgrade authority).
- ✅ **`verify_predicate` confirmed on devnet** — full CPI chain on a public
  validator: coordinator → router → groth16 → consumer.
- ✅ Consumer registry reads `verified_count = 1` on devnet.
- ✅ `journal_digest` refactor: coordinator takes `[u8; 32]` digest (off-chain
  sha256), removing the transaction-size constraint. Cleaner API; raw journal
  never on-chain.

**Result:**
```
✓ verify_predicate on devnet!  (Finalized)
tx: 5KhzR7vQyQ7wwM6pe2kyzmFqHrTAUvWFpX78cRvGowN9e5EamvktzHqMpTtXJiNAYT9su6G5N6m9LoGD1xqX3W9s
```
Explorer: https://explorer.solana.com/tx/5KhzR7vQyQ7wwM6pe2kyzmFqHrTAUvWFpX78cRvGowN9e5EamvktzHqMpTtXJiNAYT9su6G5N6m9LoGD1xqX3W9s?cluster=devnet

Devnet IDs: router `CfAo7ygm...`, groth16 `8RPusmPr...`, coordinator `39jHP7Hs...`,
consumer `Gv1x7gNn...`. See `9.m4-green.md` for the full deployment lessons.

Carries to M5: cryptographic nullifier binding (still key-based via
`authorized_prover`); fully stateless proving service; a one-shot devnet deploy
script.

Depends on: M3 ✅.

---

## M5 — Nullifier binding + accelerators ⬜

Tighten the deferred security gap and keep crypto-adjacent predicates affordable.

### Nullifier binding
- ⬜ On-chain `PrivacyPreservingCircuitOutput` decode (risc0-serde word-aligned), or
  a commitment scheme the prover generates off-chain and the coordinator verifies.
- ⬜ Replace the `authorized_prover` constraint with cryptographic nullifier binding.

### Accelerators
- ⬜ Route `sol_sha256` (and other crypto syscalls) to RISC Zero accelerators.
- ⬜ Add signature-verification syscall support.
- ⬜ Re-measure proving cost with crypto syscalls vs. baseline.

**Done when:** the nullifier binding is cryptographic (not key-based), and a
crypto-adjacent predicate proves within acceptable cost and time.

Depends on: M4.

---

## M6 — Opcode + syscall coverage ⬜

Widen the interpreter from "demo predicate" to "the class of confidential
predicates we actually target."

- ⬜ Expand sBPF opcode coverage for realistic predicates.
- ⬜ Implement the minimal syscall surface the target use cases require.
- ⬜ Conformance-test against `solana-labs/rbpf`.
- ⬜ Define and document the supported-program boundary.

**Done when:** a representative target predicate compiles from normal Solana
tooling and runs through the interpreter unmodified.

Depends on: M4 (M5 can run in parallel).

---

## M7 — Flagship demo ⬜

The pitch-worthy artifact: a real confidential application over shielded Logos
data, verified on Solana.

- ⬜ Choose the showcase use case (sealed-bid auction *or* private eligibility).
- ⬜ Build the full application flow on top of M4–M6.
- ⬜ Record an end-to-end demo with the secret provably never exposed.
- ⬜ Publish results and write-up.

**Done when:** a real confidential program over shielded Logos data is
demonstrated and recorded, settling on Solana.

Depends on: M4, M6 (M5 strongly recommended).

---

## M8 — Developer SDK ⬜

Let external developers build confidential Solana programs without leaving their stack.

- ⬜ Annotation surface for marking confidential inputs (`#[confidential]` macro).
- ⬜ Build pipeline: annotated program → sBPF + confidential manifest.
- ⬜ Client orchestration library.
- ⬜ Developer docs + minimal starter example.

**Done when:** an external developer can write, build, and run a confidential
Solana program against Tuniq from their existing toolchain.

Depends on: M4, M7.

---

## Cross-cutting (ongoing)

- ✅ **Devnet deployment** — done in M4.
- 🔜 **One-shot devnet deploy script** — codify the M4 deployment sequence.
- ⬜ **Proving-service hardening** — latency budget, availability, decentralization.
- ⬜ **Version-drift watch** — keep LEZ ↔ RISC Zero ↔ Solana-verifier aligned.
- 🔜 **Build-in-public cadence** — M0–M4 posts ready; M4 has a live devnet tx link.

---

## Dependency order at a glance

```
Foundation (Exp 1,2,3) ✅
        │
        ▼
      M0  (moat connected to settlement) ✅
        │
        ▼
      M1  (coordinator verifies shielded proof — litesvm) ✅
        │
        ▼
      M2  (consumer program wired via coordinator CPI) ✅
        │
        ▼
      M3  (authorized prover + proving service) ✅
        │
        ▼
      M4  (verified on public Solana devnet) ✅
        ├────────────► M5 (nullifier binding + accelerators)
        ├────────────► M6 (opcode/syscall coverage)
        │                     │
        ▼                     ▼
      M7  (flagship demo) ◄───┘
        │
        ▼
      M8  (developer SDK)
```

M0 through M4 are green — the full stack is verified on a public validator. M5 is
the active milestone: cryptographic nullifier binding and accelerators.