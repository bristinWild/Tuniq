# Tuniq — Confidential sBPF on Logos

### An architectural research litepaper

> Run confidential Solana programs. Developers write Solana logic the way they
> already do — same Rust, same Anchor, same sBPF bytecode — but now over private
> data. The program runs inside Logos's privacy-preserving zkVM over shielded
> inputs that never become public, and only a verified result settles back on
> Solana.

This document is a refresher of the research behind Tuniq, a primer for the
community, and the decision base for the v1 technical architecture and roadmap.
It is deliberately not a whitepaper — there is no formal protocol spec or proofs
here. Call it a litepaper: enough to understand what was validated, what is
irreplaceable, and what to build first.

**Status (update):** two milestones are now green. M0 wrapped the shielded,
commitment-bound Logos proof to a 256-byte Groth16 seal (§6, §7). M1 verified
that seal on Solana through the real Verifier Router CPI — the `alt_bn128` pairing
check passed on real artifacts, with the secret provably absent. What was once
"the gate, not yet green" is closed. What was once "M1 integration, not research"
is done.

Experiments and raw results: https://github.com/bristinWild/tuniq-experiments

---

## 1. The problem

Every public blockchain, Solana included, verifies a program by re-executing it.
Validators agree on a result because they all re-run the same code over the same
inputs and get the same answer. This works only because the inputs are public.

The moment an input is private, re-execution is impossible — no validator can
re-run a program over data it is not allowed to see. So the data has to be
exposed. That is the wall: public chains cannot compute over secrets, because
their trust model is "redo the work," and redoing the work means seeing the
secret.

There is exactly one way around the wall. Instead of re-running the program to
trust it, produce a mathematical proof that it ran correctly. A proof can be
checked without re-execution, so the inputs can stay hidden. That requires a
zero-knowledge virtual machine — and to keep the inputs hidden end-to-end, a
*privacy-preserving* one.

That is Logos.

---

## 2. The core insight

Tuniq's premise is that Solana — the largest developer ecosystem in crypto —
cannot build confidential applications today, and that Logos's privacy-preserving
zkVM is the one mechanism that can unlock them without forcing developers off
their existing tooling.

The defensibility of the idea rests on a single property that is easy to state
and easy to get wrong. Consider the canonical confidential task: prove that a
balance is at least some threshold, without revealing the balance.

- **Plain Solana** verifies by re-running, so it must see the balance. The secret
  leaks. Impossible by construction.
- **A plain zkVM (vanilla RISC Zero)** can read the balance as a private witness
  and reveal only the boolean result. The secret stays hidden — but nothing ties
  that witness to a *real* balance. The prover can supply any number they like.
  The proof attests that *some* value cleared the bar, not that *the real
  on-chain value* did. This is not trustworthy; it is confidential-compute
  theater.
- **Logos** keeps the balance hidden *and* binds it to an on-chain commitment.
  The privacy circuit forces the witnessed value to be the one that opens that
  commitment, so the prover cannot substitute a fake number. The proof now
  attests that the real, on-chain-anchored value cleared the bar — and still
  reveals only the boolean.

That combination — a secret that is simultaneously invisible and provably the
real value — exists nowhere else. Remove Logos and the guarantee collapses to the
plain-zkVM case, which has no on-chain binding and therefore no moat. Logos is
not an optimization in this design. It is the load-bearing wall.

A useful way to hold the data flow in your head: the result is *read* on Solana,
but it is *trusted* because of what happened on Logos. Solana is where you read
the boolean; Logos is why the boolean means anything. The secret itself is never
read from anywhere — it stays with its owner. What lives on Logos is the
commitment (a hash that anchors the value), never the cleartext.

---

## 3. What you can actually compute

A common misread is that confidential execution means "comparisons only" — that
you can only use secret values inside require/assert checks. That is too narrow.

You can run arbitrary sBPF logic over secret values: arithmetic, hashing,
branching, derivations. The real constraint is about *output*, not *operations*:
whatever you commit to the journal becomes public; everything else stays secret.
A boolean predicate (`balance >= threshold` → reveal `true`) is simply the
safest, lowest-leakage output, which is why it is the natural beachhead.

There is one informational caution worth stating plainly: revealing a *derived*
value can leak the secret backward. If you reveal `x = secret + 6`, anyone can
recover `secret = x - 6`. So predicates that reveal a fact without the underlying
number are popular for good reason. Mechanically you are free to output more; you
just choose what is safe to expose.

This also clarifies a scope boundary in the opposite direction: a computation
with *no* secret input does not need Tuniq at all. `5 + 6 = 11` over public
values already runs on plain Solana for free. Tuniq earns its place only when at
least one input must stay private.

---

## 4. Scope discipline

Tuniq targets *small confidential programs*, not arbitrary large DeFi logic.
Interpreting sBPF inside a zkVM carries a per-instruction overhead (measured
below), so business-logic-heavy programs remain impractical to interpret and are
intentionally out of scope.

The sweet spot is the class of programs that:

1. are small enough that interpreter overhead is tolerable,
2. cannot run on public Solana because inputs are private,
3. must use a privacy zkVM, and
4. are often crypto-adjacent, where hardware accelerators help most.

Beachhead use cases: private eligibility and threshold checks (balance ≥ X, age ≥
18), sealed-bid auctions, solvency proofs, confidential KYC predicates, and
private allowlist or voting membership.

---

## 5. Architecture

The lifecycle of a single confidential call, top to bottom:

```
DEVELOPER (build time)
  writes Solana program (Rust/Anchor), marks confidential inputs via the SDK
  → compiles to sBPF bytecode + a confidential manifest
        |
USER's private data — lives shielded on Logos, bound to an on-chain commitment
        |
        v
+------------------------------------------------------------------+
| LOGOS (LEZ) — the irreplaceable layer                            |
|   sBPF interpreter (the guest)                                   |
|     - loads the developer's bytecode                             |
|     - private witness: the shielded balance (bound to commitment)|
|     - public input: the threshold / parameters                   |
|     - fetch → decode → execute over the secret data              |
|     - asserts the witness opens the on-chain commitment          |
|   execute_and_prove → succinct receipt                           |
|     journal = public result only (never the private input)       |
+-------------------------------+----------------------------------+
                                | succinct receipt
                                v
+------------------------------------------------------------------+
| OFFCHAIN PROVING WORKER (x86)                                    |
|   reconstruct the receipt → wrap succinct → Groth16              |
|   output: 256-byte seal + journal + image ID                     |
+-------------------------------+----------------------------------+
                                | Groth16 seal + journal + image ID
                                v
+------------------------------------------------------------------+
| SOLANA — settlement                                              |
|   Tuniq coordinator program:                                     |
|     - verify_groth16(...) via the Verifier Router (alt_bn128)     |
|     - nullifier PDA replay guard (one proof, one use)            |
|     - forward the verified result to the developer's program     |
|   Developer's program acts on a trusted result it never saw raw  |
+------------------------------------------------------------------+
```

### Three zones, three realities

The diagram has a trust boundary running between Logos and Solana. Everything on
the Logos side happens where the secret exists; everything on the Solana side
sees only the verified result. The single invariant the whole product enforces:
the shielded value crosses *zero* boundaries — it is a private witness in the
circuit, absent from the receipt's journal, and absent from the seal that reaches
Solana.

- **Logos (the privacy circuit)** is the only irreplaceable component. It does two
  things no other layer can: it runs the interpreter *and* binds the witness to an
  on-chain commitment. That binding is the moat.
- **The offchain worker** is the one unavoidable operational dependency. The
  succinct→Groth16 wrap requires x86 hardware, so it cannot run inside the Logos
  guest and cannot run on-chain. In production it becomes a managed proving service
  rather than a hand-spun cloud box. It is trust-light, not trustless: it sees the
  succinct receipt but never the cleartext secret.
- **The Solana coordinator** verifies the Groth16 proof via the Verifier Router CPI
  and guards against replay via a nullifier PDA. Both are proven and working (M1).
  The remaining product work is wiring a consumer program to act on the result.

---

## 6. What was validated — the experiments + the connecting runs

Four runs tested and connected the stack. All passed.

### Experiment 1 — Is interpreting sBPF fast enough?

| Metric | Value |
|---|---|
| Per-instruction cost (realistic opcode mix) | ~314 zkVM cycles |
| Fixed overhead (setup, I/O, halt) | ~4,672 cycles |
| 100-instruction predicate | ~36,000 cycles |
| Real proving time (100-insn, unaccelerated laptop CPU) | 9.39 s |
| Verification time | 12 ms |

A realistic predicate fits in a single RISC Zero proving segment (under
2^16 = 65,536 cycles). Caveat: crypto syscalls raise cost sharply unless routed to
accelerators — a later-stage item (M3).

### Experiment 2 — Can the secret stay secret *and* unforgeable?

`balance >= threshold` over a shielded, commitment-bound LEZ account. Real proofs,
`RISC0_DEV_MODE=0`. Valid case proved; false case unprovable (soundness). Three
properties demonstrated together, all requiring LEZ: commitment binding, selective
disclosure, soundness. This is the experiment that makes Logos irreplaceable.

Environment: `RISC0_DEV_MODE=0`, `r0vm` 3.0.5, `nssa_core` v0.2.0-rc3,
`spel-framework` v0.3.0.

### Experiment 3 — Can Solana cheaply verify and act on the result?

Anchor verifier deploys; RISC Zero Solana verifier passes 12/12 vectors;
on-chain verification under 200k CU. Proven for a *standalone*
(non-commitment-bound) guest — the Half A path.

Environment: `r0vm` 3.0.5, `risc0-groth16` 3.0.4, Anchor 0.31.1,
`boundless-xyz/risc0-solana` v3.0.0.

### M0 — the connecting run (DONE)

Experiment 2's shielded, commitment-bound proof was reproduced in-repo, captured
to disk, locally verified, and wrapped to Groth16 on x86 — producing a 256-byte
Solana-verifiable seal. The composed-assumption wrap succeeded with no special
handling — the one caveat §7 flagged for empirical confirmation.

Artifacts: `proof.bin` (223,835 B), `journal.bin` (696 B), `seal.bin` (256 B),
`image_id.txt`. All regenerable; the prover crate (`prover/`) documents how.

### M1 — on-chain verification (DONE)

The coordinator Anchor program verified `seal.bin` on a local Solana VM (litesvm)
through the real Verifier Router CPI and `groth_16_verifier`. The `alt_bn128`
pairing check passed. The nullifier PDA replay guard fired correctly. The secret
was absent from everything Solana saw. Commit: `de06ce3`.

Two non-obvious encoding requirements discovered and confirmed (see
`docs/dev-core-notes/6.m1-green.md`): `pi_a` must be pre-negated (BN254 G1), and
`image_id` must be passed as little-endian bytes per u32 word. Both are caller
responsibilities; the verifier does not handle them internally.

---

## 7. Half A vs Half B — both now connected

**Half A** is a *standalone* confidential predicate guest whose proof verifies on
Solana but carries no commitment binding — vanilla-RISC-Zero confidential compute.
The secret stays out of the journal, but nothing on-chain forces the prover to
have used the real value. Useful as a fast lane for trusted-prover cases, but not
the product thesis.

**Half B** is the real thing: taking Experiment 2's shielded, commitment-bound
proof through the worker and onto Solana. **It is now demonstrated end to end:**

- The succinct receipt wraps to Groth16 cleanly (M0) — including for
  receipts-with-resolved-assumptions, the one caveat from the earlier litepaper.
- The Groth16 seal verifies on Solana through the real Verifier Router CPI (M1).
- The journal (`PrivacyPreservingCircuitOutput`) carries commitments and nullifiers
  — no boolean result field, because verification *is* the result. The coordinator
  parses it; the replay guard consumes the nullifier.

The moat and the settlement, once demonstrated on *different artifacts*, now run
on *one*. "Confidential Solana programs" is demonstrated end to end.

**What remains is M2 product wiring, not research:**

- Wire a consumer program that acts on the coordinator's verified result.
- Tighten the nullifier binding (on-chain journal decode for precise extraction).
- Wrap the proving service into a callable API.

---

## 8. Build it from proven parts — reuse vs. custom

**Reuse (audited, already exercised):**

- RISC Zero Solana verifier — `boundless-xyz/risc0-solana` v3.0.0 (Verifier Router
  + `groth_16_verifier`). Passed 12/12 vectors; `alt_bn128`, under 200k CU.
- The succinct→Groth16 wrap — `risc0-groth16` and the RISC Zero proving stack.
- LEZ's privacy-preserving circuit, commitment scheme, and `execute_and_prove`.

**Custom (the thin glue that is the actual product):**

- **Coordinator program (Solana).** CPI into the Verifier Router, nullifier-PDA
  replay guard, `PrivacyPreservingCircuitOutput` parsing, consumer forward. Working
  as of M1.
- **Proving service (offchain, x86).** The M0 wrap binary grown into a managed
  worker that takes a succinct receipt and returns a Groth16 seal. Trust-light:
  sees the succinct receipt, never the cleartext secret.
- **Developer SDK.** Build-time annotations for marking confidential inputs,
  compilation to sBPF + a confidential manifest, and client-side orchestration.

---

## 9. Competitive positioning

|  | Arcium | Tuniq |
|---|---|---|
| Trust model | MPC (multi-party computation) | ZK proofs |
| Programming model | Arcium DSL (`#[encrypted]`) | Real Solana sBPF + annotations |
| Privacy source | MPC network | Logos shielded zkVM |
| Verification | Attestation | Groth16 on Solana |

Tuniq differentiates on two axes: ZK rather than MPC for the trust model, and
native Solana tooling rather than a foreign DSL.

---

## 10. Risks and honest caveats

- Interpreter overhead bounds scope to small programs. Large programs are out of
  scope by design, not by accident.
- Crypto syscalls inside a predicate raise cost sharply until accelerators are
  added (M3).
- The x86 wrap is an unavoidable offchain, trust-light step; the product can never
  be fully on-chain.
- ~~Half B's end-to-end run is assessed feasible but not yet demonstrated.~~
  **Resolved (M0 + M1):** the shielded receipt wraps to Groth16 and verifies on
  Solana through the real `alt_bn128` pairing check. The architecture is
  demonstrated end to end on real proofs.
- Market demand for confidential compute on Solana is plausible but unproven; a
  working end-to-end demo (M5) de-risks the pitch.
- Solo founder, frontier tech, two ecosystems — heavy. The core technical risk is
  now retired. M2 onwards is product work on a proven foundation.

---

## 11. Roadmap to v1

See `dev-roadmap.md` for the tracked, granular version.

**M0 — DONE.** Shielded receipt wraps to Groth16 on x86. Composed-assumption wrap
confirmed.

**M1 — DONE.** Coordinator verifies the shielded seal on Solana via Verifier
Router CPI. Nullifier replay guard works. The full stack is demonstrated.

**M2 — NEXT.** Consumer program wiring + proving service. Wire a downstream
program that acts on the coordinator's verified result. Wrap the droplet workflow
into a callable proving service.

**M3 — Accelerators.** Route crypto syscalls to RISC Zero accelerators so
crypto-adjacent predicates stay affordable.

**M4 — Opcode + syscall coverage.** Expand the sBPF interpreter to the surface
real confidential predicates need.

**M5 — Flagship demo.** A real confidential application (sealed-bid auction or
private eligibility) over shielded Logos data, verified on Solana.

**M6 — SDK.** Developer annotations and tooling so external developers can write
confidential Solana programs without leaving their stack.

---

## Appendix — the intuition, in one analogy

A secret travels down a single road, and the experiments are stretches of it.

You need to prove a private fact to your home government (Solana) — "my balance is
over $1,000" — without showing the number. Home offices verify by redoing the
work, which would mean seeing your statement. So you go to a foreign notary
(Logos) with a sealed, tamper-proof booth.

- The booth reads your statement line by line in a slow certified procedure — the
  sBPF interpreter running your program one instruction at a time. **Experiment 1**
  timed that reading and found it fast enough for a short document.
- Inside the booth, your statement goes in sealed and never comes out; the booth
  stamps only the outside — "balance exceeds $1,000." Because the envelope was
  sealed by the official registry (the on-chain commitment), the notary cannot
  swap in a fake statement, and cannot stamp a claim that isn't true.
  **Experiment 2** built and tested that booth on real Logos machinery.
- Back home, a fast desk checks a foreign seal against an international registry
  in seconds, without redoing the work — the Groth16 verifier on Solana.
  **Experiment 3** proved the desk accepts genuine seals and rejects forgeries.

**Both runs are now complete.** The real sealed booth's stamp (Experiment 2's
proof) was run through the re-pressing machine (the x86 worker, M0) and came out
as a valid 256-byte seal. That seal was handed across the home desk (the Solana
coordinator, M1) and the desk accepted it — the `alt_bn128` pairing check passed,
with the secret provably never exposed.

The road is proven end to end. What remains is paving it: consumer program wiring,
a callable proving service, accelerators, broader opcode coverage, and the
flagship demo.