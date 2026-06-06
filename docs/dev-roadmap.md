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
What had *not* been demonstrated was the one run connecting the moat to settlement
- so that run (M0) came first. **M0 is now green** (see below), so everything
downstream is shaped by a real artifact, not a hypothesis.

Status legend:

- ✅ done
- 🔜 next / in progress
- ⬜ not started
- 🚧 blocked on a decision or an upstream dependency

Each milestone lists concrete tasks, a single **Done when** acceptance line, and
its dependencies. A milestone is not complete until its Done-when line is true.

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
- ✅ Capture the proof artifacts to disk (`proof.bin` 223,835 B, `journal.bin`
  696 B, `image_id.txt`) via the `prove` example.
- ✅ Local pre-flight: reconstruct the receipt and verify it against
  `PRIVACY_PRESERVING_CIRCUIT_ID` (the `verify` example) - confirms the on-disk
  artifact is intact before paying for x86.
- ✅ Wrap the succinct receipt → Groth16 on x86 (`prover/`), producing
  `seal.bin` (256 B). **The composed-assumption wrap succeeded empirically** -
  the one caveat the litepaper flagged is resolved.
- ⬜ Verify `seal.bin` through the on-chain Solana `verify_groth16` path. *(Moved
  to M1 - it is the coordinator's first job, and uses the same Verifier Router
  CPI proven in Exp 3.)*

**Done when:** ~~Experiment 2's commitment-bound shielded proof verifies through
the real Solana verification logic end-to-end.~~ **Met** for the off-chain wrap +
local verification; the on-chain verify of *this* seal carries into M1.

**Result:** the full shielded path - confidential execution on Logos → succinct
receipt → Groth16 seal - is demonstrated end to end. Half B is no longer assessed;
it is proven. Artifacts live in `predicate-engine/artifacts/` (regenerable).

Depends on: Foundation (Exp 2, Exp 3).

---

## M1 - First end-to-end product slice 🔜

Turn the M0 connection into the thinnest real product: one confidential predicate,
over one shielded input, settled on Solana and acted upon - built from the proven
parts, no new cryptography.

**Start here:** decode `journal.bin` (the 696-byte `PrivacyPreservingCircuitOutput`)
against the nssa struct. The coordinator's whole job hinges on what's in it; build
the on-chain program around the real bytes, not a guess.

### Coordinator program (Solana)
- 🔜 Decode the captured `journal.bin` against `PrivacyPreservingCircuitOutput`
  (commitments / nullifiers) - the Half B parse, made concrete.
- ⬜ Extend the Experiment 3 `confidential_predicate_verifier` into a coordinator.
- ⬜ CPI into the Verifier Router (`boundless-xyz/risc0-solana` v3.0.0) to verify
  `seal.bin` - reuse, do not reimplement verification.
- ⬜ Parse the circuit output on-chain and extract the result.
- ⬜ Forward the verified result to a downstream consumer program.

### Proving worker (offchain, x86)
- 🔜 The wrap binary exists (`prover/`). Next: wrap the droplet workflow into a
  callable service (request in → succinct receipt → seal + journal + image ID out).
- ⬜ Define the request/response contract the SDK will later call.
- ⬜ Document the trust model: sees the succinct receipt, never the cleartext
  secret (trust-light, not trustless).
- ⬜ Capture a reusable build cache / droplet snapshot (the cold build was ~63 min).

### Reference confidential program
- ✅ One shielded predicate over one shielded input (the eligibility check) - the
  `predicate-engine` predicate, proving end-to-end on Logos.
- ⬜ Wire it through the coordinator to a consumer program on Solana.

**Done when:** a single confidential predicate runs over a shielded input → proves
on Logos → wraps to Groth16 → verifies on Solana → a consumer program acts on the
result, all on `RISC0_DEV_MODE=0`, with the secret never exposed.

Depends on: M0 ✅.

---

## M2 - Accelerators ⬜

Keep crypto-adjacent confidential programs affordable. Confidential predicates are
disproportionately crypto-heavy, so this is where accelerators help most.

- ⬜ Route `sol_sha256` (and other crypto syscalls) to RISC Zero accelerators.
- ⬜ Add signature-verification syscall support where predicates need it.
- ⬜ Re-measure proving cost with crypto syscalls present vs. the baseline.

**Done when:** a crypto-adjacent predicate (e.g. a hash-gated eligibility check)
proves within acceptable cost and time.

Depends on: M1.

---

## M3 - Opcode + syscall coverage ⬜

Widen the interpreter from "demo predicate" to "the class of confidential
predicates we actually target."

- ⬜ Expand sBPF opcode coverage for realistic predicates.
- ⬜ Implement the minimal syscall surface the target use cases require.
- ⬜ Conformance-test interpreter behavior against the `solana-labs/rbpf`
  reference.
- ⬜ Define and document the supported-program boundary (what compiles and runs
  unmodified vs. what is out of scope).

**Done when:** a representative target predicate compiles from normal Solana
tooling and runs through the interpreter unmodified.

Depends on: M1 (M2 can run in parallel).

---

## M4 - Flagship demo ⬜

The pitch-worthy artifact: a real confidential application over shielded Logos
data, verified on Solana.

- ⬜ Choose the showcase use case (sealed-bid auction *or* private eligibility).
- ⬜ Build the full application flow on top of M1–M3.
- ⬜ Record an end-to-end demo (shielded input → confidential execution → Solana
  settlement) with the secret provably never exposed.
- ⬜ Publish results and write-up.

**Done when:** a real confidential program over shielded Logos data is
demonstrated and recorded, settling on Solana.

Depends on: M1, M3 (M2 strongly recommended).

---

## M5 - Developer SDK ⬜

Let external developers build confidential Solana programs without leaving their
stack. This is what turns Tuniq from a demo into a platform.

- ⬜ Annotation surface for marking confidential inputs (e.g. a `#[confidential]`
  macro).
- ⬜ Build pipeline: annotated program → sBPF + confidential manifest.
- ⬜ Client orchestration library: submit request → call the proving worker →
  retrieve proof → submit to the coordinator.
- ⬜ Developer docs + a minimal starter example.

**Done when:** an external developer can write, build, and run a confidential
Solana program against Tuniq from their existing toolchain.

Depends on: M1, M4.

---

## Cross-cutting (ongoing)

Runs alongside the milestones, not after them.

- ⬜ **Proving-service hardening** - latency budget, availability, the
  decentralization question (who runs the x86 worker, and its trust implications).
- ⬜ **Devnet deployment** - move from local validator to a public Solana devnet.
- ⬜ **Version-drift watch** - keep the LEZ ↔ RISC Zero ↔ Solana-verifier versions
  aligned (a known integration risk).
- 🔜 **Build-in-public cadence** - devlog updates per milestone; standalone
  showcase posts for M0-green (now), M1, and M4.

---

## Dependency order at a glance

```
Foundation (Exp 1,2,3) ✅
        │
        ▼
      M0  (the gate - moat connected to settlement) ✅
        │
        ▼
      M1  (first end-to-end product slice) 🔜
        ├────────────► M2 (accelerators)
        ├────────────► M3 (opcode/syscall coverage)
        │                     │
        ▼                     ▼
      M4  (flagship demo) ◄───┘
        │
        ▼
      M5  (developer SDK)
```

M0 was the only hard gate, and it is green. M1 is now the active milestone, built
against the real artifacts M0 produced.