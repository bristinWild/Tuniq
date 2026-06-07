# Tuniq - Development Roadmap

Confidential sBPF on Logos. This is the working build plan from validated research
to v1. It is a living document - update statuses as milestones land.

Litepaper (architecture + experiment results): see `architectural-research-litepaper.md`
Experiments: https://github.com/bristinWild/tuniq-experiments

---

## How to read this

The roadmap is ordered by a single principle: **prove the one unproven connection
before building anything on top of it.** Three experiments validated the hard
questions independently (interpreter cost, the privacy moat, Solana settlement).
M0 connected them; M1 verified the proof on Solana; M2 wired a consumer program.
**M0, M1, and M2 are all green.** Everything downstream is shaped by real
artifacts, not hypotheses.

Status legend:

- ✅ done
- 🔜 next / in progress
- ⬜ not started
- 🚧 blocked on a decision or an upstream dependency

---

## Foundation - validated research ✅

Already complete; this is what the roadmap builds on. Details in the litepaper.

- ✅ **Experiment 1 - interpreter overhead.** ~314 zkVM cycles/instruction;
  100-insn predicate ~36k cycles; proves in ~9.4s on an unaccelerated laptop,
  verifies in 12ms. Small confidential predicates are economically viable.
- ✅ **Experiment 2 - the moat.** `balance >= threshold` over a shielded,
  commitment-bound account on the real LEZ stack (`RISC0_DEV_MODE=0`). Valid case
  proved; false case unprovable. Commitment binding + selective disclosure +
  soundness, all native to Logos.
- ✅ **Experiment 3 - Solana settlement.** Anchor verifier program deploys; RISC
  Zero Solana verifier passes 12/12 vectors; our own confidential predicate proof
  verifies through the real Solana logic (<200k CU), secret absent from the
  journal. Proven for a *standalone* (non-commitment-bound) guest.

---

## M0 - The Gate: connect the moat to settlement ✅

**GREEN.** Experiment 2's shielded, commitment-bound proof was reproduced inside
the monorepo (real proofs, `RISC0_DEV_MODE=0`), captured to disk, and wrapped to
Groth16 on x86 - producing a 256-byte Solana-verifiable seal.

- ✅ Reproduce the Exp 2 confidential predicate in-repo (`predicate-engine`).
- ✅ Capture proof artifacts to disk (`proof.bin`, `journal.bin`, `image_id.txt`).
- ✅ Local pre-flight: verify receipt against `PRIVACY_PRESERVING_CIRCUIT_ID`.
- ✅ Wrap succinct receipt → Groth16 on x86 (`prover/`), producing `seal.bin`.
  Composed-assumption wrap succeeded empirically.

**Result:** the full shielded path - Logos → succinct receipt → Groth16 seal -
is demonstrated end to end. Half B is proven.

Depends on: Foundation (Exp 2, Exp 3).

---

## M1 - First end-to-end product slice ✅

**GREEN.** The coordinator Anchor program verifies the shielded proof on Solana
through the real Verifier Router CPI and `groth_16_verifier`. The full CPI chain
fires on real artifacts. The nullifier PDA replay guard works. The secret never
appears on the Solana side.

- ✅ Decode `journal.bin` against `PrivacyPreservingCircuitOutput`. See `decode.rs`.
- ✅ Coordinator Anchor program: CPI into Verifier Router (v3.0.0), nullifier-PDA
  replay guard, `PredicateVerified` event emission.
- ✅ **Verify `seal.bin` through the on-chain `verify_groth16` path** - the
  decisive connection between the moat and Solana settlement.
- ✅ `verify_predicate` litesvm integration test passes on real artifacts.
- ✅ Two non-obvious encoding facts confirmed (see `6.m1-green.md`):
  `pi_a` must be pre-negated (BN254 G1); `image_id` must be LE bytes per word.

**Result:** `test verify_shielded_predicate_on_solana ... ok`. Commit: `de06ce3`.

Depends on: M0 ✅.

---

## M2 - Consumer wiring ✅

**GREEN.** The coordinator CPIs into a consumer program after a shielded proof
verifies. The consumer's `Registry` PDA increments its `verified_count` - on-chain
state changes as a direct result of the verified proof. The full chain is now a
composable building block.

- ✅ Consumer program (`programs/consumer`): `Registry` PDA, `record_verification`
  increments `verified_count`, `VerificationRecorded` event emission.
- ✅ Coordinator updated: `verify_predicate` CPIs into consumer after the Verifier
  Router CPI succeeds. Consumer program is passed as an account - any compliant
  consumer can be wired without redeploying the coordinator.
- ✅ Test updated: asserts `registry.verified_count == 1` after the full chain runs.
  Not just "no error thrown" - on-chain state actually changed. Commit: `d42b238`.
- 🔜 Tighten nullifier binding: decode the journal on-chain to extract
  `new_nullifiers` precisely. The substring scan was removed in M1; this is the
  rigorous replacement.
- ⬜ Remove dead code: `journal_contains_nullifier` fn + `NullifierNotInJournal`
  error in `coordinator/lib.rs`.

### Proving worker (offchain, x86) - carries forward
- ⬜ Wrap `prover/` into a callable service (request in → seal + journal + image ID out).
- ⬜ Define the request/response contract the SDK will later call.
- ⬜ Document the trust model explicitly (trust-light, not trustless).

**Done when:** ~~a single confidential predicate proves on Logos → wraps →
verifies → a consumer acts on the result~~ **Met** for the on-chain consumer.
Nullifier binding tightening + proving service carry into M3.

**Result:**

```
consumer initialize: ok
coordinator initialize: ok
VERIFY_PREDICATE + CONSUMER CPI: ok
Registry verified_count = 1 ✓
M2 gate: GREEN - shielded proof verified AND consumer notified.
test verify_and_forward_to_consumer ... ok
```

Depends on: M1 ✅.

---

## M3 - Security + proving service 🔜

Tighten the one deferred security gap and wrap the proving worker into a callable
service - the two pieces that carry from M2.

### Nullifier binding (security)
- 🔜 On-chain journal decode: parse `PrivacyPreservingCircuitOutput` from the
  journal bytes to extract `new_nullifiers` precisely. Bind the caller-supplied
  nullifier to the journal (the seal authenticates the journal; the binding
  ensures the nullifier is *in* it).
- 🔜 Remove dead code: `journal_contains_nullifier` + `NullifierNotInJournal`.

### Proving worker
- ⬜ Wrap `prover/` into a callable HTTP service.
- ⬜ Define the request/response contract the SDK will later call.
- ⬜ Document the trust model: sees the succinct receipt, never the cleartext secret.
- ⬜ Devnet deployment - move from litesvm to a real Solana devnet.

**Done when:** the nullifier binding is tight (on-chain decode, not substring
scan), the proving service is callable, and the coordinator + consumer deploy to
devnet.

Depends on: M2 ✅.

---

## M4 - Accelerators ⬜

Keep crypto-adjacent confidential programs affordable.

- ⬜ Route `sol_sha256` (and other crypto syscalls) to RISC Zero accelerators.
- ⬜ Add signature-verification syscall support.
- ⬜ Re-measure proving cost with crypto syscalls vs. baseline.

**Done when:** a crypto-adjacent predicate proves within acceptable cost and time.

Depends on: M3.

---

## M5 - Opcode + syscall coverage ⬜

Widen the interpreter from "demo predicate" to "the class of confidential
predicates we actually target."

- ⬜ Expand sBPF opcode coverage for realistic predicates.
- ⬜ Implement the minimal syscall surface the target use cases require.
- ⬜ Conformance-test against `solana-labs/rbpf`.
- ⬜ Define and document the supported-program boundary.

**Done when:** a representative target predicate compiles from normal Solana
tooling and runs through the interpreter unmodified.

Depends on: M3 (M4 can run in parallel).

---

## M6 - Flagship demo ⬜

The pitch-worthy artifact: a real confidential application over shielded Logos
data, verified on Solana.

- ⬜ Choose the showcase use case (sealed-bid auction *or* private eligibility).
- ⬜ Build the full application flow on top of M3–M5.
- ⬜ Record an end-to-end demo with the secret provably never exposed.
- ⬜ Publish results and write-up.

**Done when:** a real confidential program over shielded Logos data is
demonstrated and recorded, settling on Solana.

Depends on: M3, M5 (M4 strongly recommended).

---

## M7 - Developer SDK ⬜

Let external developers build confidential Solana programs without leaving their stack.

- ⬜ Annotation surface for marking confidential inputs (`#[confidential]` macro).
- ⬜ Build pipeline: annotated program → sBPF + confidential manifest.
- ⬜ Client orchestration library.
- ⬜ Developer docs + minimal starter example.

**Done when:** an external developer can write, build, and run a confidential
Solana program against Tuniq from their existing toolchain.

Depends on: M3, M6.

---

## Cross-cutting (ongoing)

- 🔜 **Proving-service hardening** - latency budget, availability, decentralization.
- 🔜 **Devnet deployment** - move from litesvm to a public Solana devnet (M3).
- ⬜ **Version-drift watch** - keep LEZ ↔ RISC Zero ↔ Solana-verifier aligned.
- 🔜 **Build-in-public cadence** - M0, M1, M2 posts ready now; M3 and M6 as they land.

---

## Dependency order at a glance

```
Foundation (Exp 1,2,3) ✅
        │
        ▼
      M0  (moat connected to settlement) ✅
        │
        ▼
      M1  (coordinator verifies shielded proof on Solana) ✅
        │
        ▼
      M2  (consumer program wired via coordinator CPI) ✅
        │
        ▼
      M3  (nullifier binding + proving service + devnet) 🔜
        ├────────────► M4 (accelerators)
        ├────────────► M5 (opcode/syscall coverage)
        │                     │
        ▼                     ▼
      M6  (flagship demo) ◄───┘
        │
        ▼
      M7  (developer SDK)
```

M0, M1, and M2 are green. M3 is the active milestone - nullifier binding first,
then the proving service, then devnet.