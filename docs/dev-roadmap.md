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
M0 connected them; M1 turned that connection into a working on-chain program.
**Both are now green.** Everything downstream is shaped by real artifacts, not
hypotheses.

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
Groth16 on x86 - producing a 256-byte Solana-verifiable seal. The moat and the
settlement, previously demonstrated on *different artifacts*, now connect on *one*.

- ✅ Reproduce the Exp 2 confidential predicate in-repo (`predicate-engine`);
  pass case + `should_panic` soundness case, real proofs.
- ✅ Capture proof artifacts to disk (`proof.bin` 223,835 B, `journal.bin` 696 B,
  `image_id.txt`) via the `prove` example.
- ✅ Local pre-flight: reconstruct the receipt and verify against
  `PRIVACY_PRESERVING_CIRCUIT_ID` (`verify` example) - artifact intact.
- ✅ Wrap the succinct receipt → Groth16 on x86 (`prover/`), producing `seal.bin`
  (256 B). Composed-assumption wrap succeeded empirically - the one caveat the
  litepaper flagged is resolved.

**Result:** the full shielded path - confidential execution on Logos → succinct
receipt → Groth16 seal - is demonstrated end to end. Half B is no longer assessed;
it is proven.

Depends on: Foundation (Exp 2, Exp 3).

---

## M1 - First end-to-end product slice ✅

**GREEN.** The coordinator Anchor program verifies the shielded proof on Solana
through the real Verifier Router CPI and `groth_16_verifier`. The full CPI chain
fires on real artifacts: `seal.bin` + `journal.bin` + `PRIVACY_PRESERVING_CIRCUIT_ID`.
The nullifier PDA replay guard works. The secret never appears on the Solana side.

- ✅ Decode `journal.bin` against `PrivacyPreservingCircuitOutput` (commitments /
  nullifiers) - the Half B parse, made concrete. See `decode.rs`.
- ✅ Coordinator Anchor program: CPI into Verifier Router (v3.0.0), nullifier-PDA
  replay guard, `PredicateVerified` event emission.
- ✅ **Verify `seal.bin` through the on-chain `verify_groth16` path** — CPI into the
  Verifier Router (`boundless-xyz/risc0-solana` v3.0.0), `groth_16_verifier` pairing
  check passes on the *shielded* seal. This was the task moved from M0; it is the
  decisive connection between the moat and Solana settlement.
- ✅ `verify_predicate` litesvm integration test: full CPI chain on real artifacts.
  Commit: `de06ce3`.
- ✅ Two non-obvious encoding facts established (see `6.m1-green.md`):
  - `pi_a` must be **pre-negated** (BN254 G1) before passing to the verifier.
  - `image_id` must be **LE bytes per word** (not raw hex parse).
- ⬜ Wire the coordinator result to a downstream consumer program. *(Carries into
  M2 - verification is done; the consumer forward is the remaining product slice.)*

**Done when:** ~~a consumer program acts on the result~~ The Verifier Router CPI
is green. Consumer wiring is M2's first task.

**Result:**

```
initialize: ok
VERIFY_PREDICATE: ok - shielded proof verified on Solana!
M1 gate: GREEN
test verify_shielded_predicate_on_solana ... ok
```

Depends on: M0 ✅.

---

## M2 - Consumer wiring + proving service 🔜

Turn the verified coordinator into a complete product slice: a consumer program
that acts on the result, and a callable proving service.

### Consumer program (Solana)
- 🔜 Write a minimal consumer program that the coordinator CPIs into on
  `PredicateVerified` (e.g. gates access, issues a token, records eligibility).
- ⬜ Tighten the nullifier binding: decode the journal on-chain to extract
  `new_nullifiers` precisely and verify the supplied nullifier matches. (The
  substring scan was removed in M1; this is the rigorous replacement.)
- ⬜ Remove dead code: `journal_contains_nullifier` fn + `NullifierNotInJournal`
  error in `coordinator/lib.rs`.

### Proving worker (offchain, x86)
- ⬜ Wrap the `prover/` droplet binary into a callable service (request in →
  succinct receipt → Groth16 seal + journal + image ID out).
- ⬜ Define the request/response contract the SDK will later call.
- ⬜ Document the trust model: sees the succinct receipt, never the cleartext
  secret (trust-light, not trustless).
- ⬜ Capture a reusable droplet build cache / snapshot (cold build was ~63 min).

**Done when:** a single confidential predicate proves on Logos → wraps to Groth16
→ verifies on Solana → a consumer program acts on the result, end to end, with the
secret never exposed.

Depends on: M1 ✅.

---

## M3 - Accelerators ⬜

Keep crypto-adjacent confidential programs affordable. Confidential predicates are
disproportionately crypto-heavy, so this is where accelerators help most.

- ⬜ Route `sol_sha256` (and other crypto syscalls) to RISC Zero accelerators.
- ⬜ Add signature-verification syscall support where predicates need it.
- ⬜ Re-measure proving cost with crypto syscalls vs. the baseline.

**Done when:** a crypto-adjacent predicate (e.g. a hash-gated eligibility check)
proves within acceptable cost and time.

Depends on: M2.

---

## M4 - Opcode + syscall coverage ⬜

Widen the interpreter from "demo predicate" to "the class of confidential
predicates we actually target."

- ⬜ Expand sBPF opcode coverage for realistic predicates.
- ⬜ Implement the minimal syscall surface the target use cases require.
- ⬜ Conformance-test interpreter behavior against the `solana-labs/rbpf` reference.
- ⬜ Define and document the supported-program boundary.

**Done when:** a representative target predicate compiles from normal Solana
tooling and runs through the interpreter unmodified.

Depends on: M2 (M3 can run in parallel).

---

## M5 - Flagship demo ⬜

The pitch-worthy artifact: a real confidential application over shielded Logos
data, verified on Solana.

- ⬜ Choose the showcase use case (sealed-bid auction *or* private eligibility).
- ⬜ Build the full application flow on top of M2–M4.
- ⬜ Record an end-to-end demo with the secret provably never exposed.
- ⬜ Publish results and write-up.

**Done when:** a real confidential program over shielded Logos data is demonstrated
and recorded, settling on Solana.

Depends on: M2, M4 (M3 strongly recommended).

---

## M6 - Developer SDK ⬜

Let external developers build confidential Solana programs without leaving their stack.

- ⬜ Annotation surface for marking confidential inputs (`#[confidential]` macro).
- ⬜ Build pipeline: annotated program → sBPF + confidential manifest.
- ⬜ Client orchestration library: submit → prove → retrieve → submit to coordinator.
- ⬜ Developer docs + a minimal starter example.

**Done when:** an external developer can write, build, and run a confidential
Solana program against Tuniq from their existing toolchain.

Depends on: M2, M5.

---

## Cross-cutting (ongoing)

- ⬜ **Proving-service hardening** - latency budget, availability, decentralization
  question (who runs the x86 worker, and its trust implications).
- ⬜ **Devnet deployment** - move from local validator to a public Solana devnet.
- ⬜ **Version-drift watch** - keep the LEZ ↔ RISC Zero ↔ Solana-verifier versions
  aligned (a known integration risk).
- 🔜 **Build-in-public cadence** - M0-green post (ready now), M1-green post
  (ready now), M2 and M5 posts as they land.

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
      M2  (consumer wiring + proving service) 🔜
        ├────────────► M3 (accelerators)
        ├────────────► M4 (opcode/syscall coverage)
        │                     │
        ▼                     ▼
      M5  (flagship demo) ◄───┘
        │
        ▼
      M6  (developer SDK)
```

M0 and M1 are green. M2 is the active milestone - consumer program first,
then the proving service wrapper.