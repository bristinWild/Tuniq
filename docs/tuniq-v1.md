# Tuniq V1 - Scope & Definition

What the first version of Tuniq is, precisely: what it does, what it deliberately
does not do, the components that make it up, and what "V1 is done" concretely
means.

Companion docs:
- `TUNIQ-LITEPAPER.md` - the research, architecture, and why Logos is irreplaceable
- `ROADMAP.md` - the milestone plan and ordering

---

## V1 in one sentence

A developer can run a single confidential predicate over one shielded, on-chain-
commitment-bound input on Logos, and have the verified result settle on Solana -
with the secret value never exposed to anyone, including the prover's ability to
forge it.

V1 is the first version where the **moat is present end-to-end**: not the
standalone (non-bound) path, but the commitment-bound privacy-circuit path proven
through to Solana settlement.

---

## Precondition

V1 depends on the M0 gate (see `ROADMAP.md`) going green: Experiment 2's shielded,
commitment-bound proof verifying through Solana logic end-to-end. Everything below
assumes that connection holds. If M0 goes red, V1 scope pauses pending a resolution
with the Logos team.

---

## Scope

### In scope for V1

- **One confidential predicate class:** a threshold-style check (e.g.
  `balance >= threshold`) evaluated over a single shielded input, revealing only a
  boolean result plus public parameters.
- **The commitment-bound privacy path:** the shielded value is bound to its
  on-chain commitment via the LEZ privacy circuit, so the prover cannot substitute
  a fake value. (The moat, not Half A.)
- **Offchain proving worker (x86):** a callable service that takes a succinct
  receipt and returns a Groth16 seal + journal + image ID.
- **Solana coordinator program:** verifies the Groth16 proof via the Verifier
  Router, parses the circuit output, and forwards the verified result to a
  consumer program.
- **One reference confidential application:** the end-to-end eligibility check,
  demonstrating the full flow.
- **Devnet target:** runs against a public Solana devnet (after a local-validator
  bring-up).

### Out of scope for V1 (deferred)

- **Developer SDK / annotations** - writing confidential programs from a normal
  toolchain via a `#[confidential]`-style surface. (Roadmap M5.)
- **Crypto-syscall accelerators** - sha256, signature verification at low cost.
  (Roadmap M2.)
- **Broad opcode / syscall coverage** - V1 supports the predicate class above, not
  arbitrary Solana programs. (Roadmap M3.)
- **Large / business-logic-heavy programs** - out of scope by design; interpreter
  overhead makes these impractical. Not a V1 limitation but a permanent product
  boundary.
- **Multiple shielded inputs / complex disclosure** - V1 is one shielded input,
  one boolean output.
- **Decentralized proving** - the V1 worker is a single managed x86 service.
- **Mainnet** - V1 targets devnet.

---

## The V1 confidential program

What a confidential program can express in V1:

- Read **one** value as a private witness, sourced from a shielded Logos account
  and bound to that account's on-chain commitment.
- Take **public** parameters (e.g. a threshold) openly.
- Evaluate a **threshold-style predicate** over the private value.
- Reveal **only** the boolean result plus the public parameters - never the secret,
  and never a value from which the secret can be derived.

The idiomatic expression is assert-and-panic: a valid proof existing *is* the
statement that the predicate held (per the fixed `ProgramOutput` shape). A false
predicate produces no proof.

---

## Components shipped in V1

| Component | Role | Build basis |
|---|---|---|
| sBPF interpreter guest | Runs the predicate bytecode inside the LEZ privacy circuit | Experiments 1 & 2 |
| Privacy-circuit binding | Binds the shielded witness to its on-chain commitment | LEZ / SPEL (Experiment 2) |
| Offchain proving worker (x86) | Wraps the succinct receipt → Groth16 seal | Experiment 3 droplet workflow, productized |
| Solana coordinator program | Verifies Groth16 via Verifier Router; parses output; forwards result | Experiment 3 L1, extended with circuit-output parsing |
| Reference confidential app | The eligibility-check demonstration | New for V1 |

Reuse vs. custom: verification cryptography (the RISC Zero Solana verifier, the
Groth16 wrap, the LEZ privacy circuit) is reused, audited, and already exercised in
the experiments. The custom surface is the coordinator's output parsing + result
forwarding, the worker's service wrapper, and the reference app.

---

## V1 end-to-end flow

1. The user's secret value lives in a shielded Logos account, bound to an on-chain
   commitment.
2. A request triggers the confidential predicate over that shielded value with a
   public threshold.
3. The sBPF interpreter guest runs the predicate inside the LEZ privacy circuit;
   the circuit asserts the witness opens the commitment. `execute_and_prove`
   produces a **succinct receipt**; the journal carries only the public result.
4. The offchain worker reconstructs the receipt and wraps succinct → **Groth16**
   (256-byte seal + journal + image ID).
5. The Solana coordinator verifies the Groth16 proof via the Verifier Router
   (`alt_bn128`, under 200k compute units), parses the
   `PrivacyPreservingCircuitOutput`, and forwards the verified result.
6. The consumer program acts on a trusted result derived from data it never saw.

The secret crosses zero boundaries: a private witness in the circuit, absent from
the receipt's journal, absent from the seal that reaches Solana.

---

## Interfaces (V1 contracts)

Defined here at the contract level; exact types land during M1.

- **Worker request:** a succinct receipt (plus the metadata needed to reconstruct
  the `Receipt` from LEZ's `Proof`).
- **Worker response:** Groth16 seal (256 bytes) + journal + image ID.
- **Coordinator input:** seal + journal + image ID (not the full receipt).
- **Coordinator output:** the parsed predicate result, forwarded to the consumer
  program. Note the journal is a `PrivacyPreservingCircuitOutput`
  (commitments / nullifiers), so the coordinator owns the parsing that turns it
  into a usable result.

---

## Tech stack

- **Language:** Rust
- **zkVM / privacy:** Logos LEZ, RISC Zero zkVM, SPEL framework, the
  privacy-preserving circuit
- **Proving:** `execute_and_prove` with `ProverOpts::succinct()`; succinct→Groth16
  wrap on x86 (`risc0-groth16`)
- **Settlement:** Solana, Anchor, the RISC Zero Solana verifier
  (`boundless-xyz/risc0-solana` v3.0.0), `alt_bn128`
- **Mode:** `RISC0_DEV_MODE=0` (real proofs) throughout

---

## V1 acceptance criteria

V1 is done when **all** of the following hold:

- [ ] The M0 gate is green (commitment-bound shielded proof verifies via Solana
      logic end-to-end).
- [ ] A confidential threshold predicate runs over a single shielded, commitment-
      bound input on the real LEZ stack (`RISC0_DEV_MODE=0`).
- [ ] The proof wraps to Groth16 via the offchain worker as a callable service.
- [ ] The Solana coordinator verifies the proof, parses the circuit output, and
      forwards the result to a consumer program.
- [ ] The consumer program acts on the verified result.
- [ ] The secret value is provably absent from the journal, the seal, and all
      Solana state.
- [ ] The full flow runs against a public Solana devnet.
- [ ] The reference eligibility-check application demonstrates the flow end-to-end.

---

## Known limitations in V1 (stated honestly)

- One shielded input, one boolean output - richer disclosure and multi-input
  predicates come later.
- The predicate class is threshold-style; broad program support is post-V1.
- Crypto-heavy predicates are expensive until accelerators (M2) land.
- The proving worker is a single managed x86 service - trust-light (sees the
  succinct receipt, never the cleartext), not trustless, and not yet decentralized.
- No developer SDK yet; building a confidential program in V1 is hands-on, not
  self-serve.

---

## After V1

The roadmap continues with accelerators (M2), opcode/syscall coverage (M3), the
flagship demo (M4), and the developer SDK (M5) - the step that turns Tuniq from a
working product into a platform external developers can build on. See `ROADMAP.md`.