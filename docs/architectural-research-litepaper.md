# Tuniq - Confidential sBPF on Logos

### An architectural research litepaper

> Run confidential Solana programs. Developers write Solana logic the way they
> already do - same Rust, same Anchor, same sBPF bytecode - but now over private
> data. The program runs inside Logos's privacy-preserving zkVM over shielded
> inputs that never become public, and only a verified result settles back on
> Solana.

This document is a refresher of the research behind Tuniq, a primer for the
community, and the decision base for choosing the v1 technical architecture and
roadmap. It is deliberately not a whitepaper - there is no formal protocol spec
or proofs here. Call it a litepaper: enough to understand what was validated,
what is irreplaceable, what remains open, and what to build first.

Experiments and raw results: https://github.com/bristinWild/tuniq-experiments

---

## 1. The problem

Every public blockchain, Solana included, verifies a program by re-executing it.
Validators agree on a result because they all re-run the same code over the same
inputs and get the same answer. This works only because the inputs are public.

The moment an input is private, re-execution is impossible - no validator can
re-run a program over data it is not allowed to see. So the data has to be
exposed. That is the wall: public chains cannot compute over secrets, because
their trust model is "redo the work," and redoing the work means seeing the
secret.

There is exactly one way around the wall. Instead of re-running the program to
trust it, produce a mathematical proof that it ran correctly. A proof can be
checked without re-execution, so the inputs can stay hidden. That requires a
zero-knowledge virtual machine - and to keep the inputs hidden end-to-end, a
*privacy-preserving* one.

That is Logos.

---

## 2. The core insight

Tuniq's premise is that Solana - the largest developer ecosystem in crypto -
cannot build confidential applications today, and that Logos's privacy-preserving
zkVM is the one mechanism that can unlock them without forcing developers off
their existing tooling.

The defensibility of the idea rests on a single property that is easy to state
and easy to get wrong. Consider the canonical confidential task: prove that a
balance is at least some threshold, without revealing the balance.

- **Plain Solana** verifies by re-running, so it must see the balance. The secret
  leaks. Impossible by construction.
- **A plain zkVM (vanilla RISC Zero)** can read the balance as a private witness
  and reveal only the boolean result. The secret stays hidden - but nothing ties
  that witness to a *real* balance. The prover can supply any number they like.
  The proof attests that *some* value cleared the bar, not that *the real
  on-chain value* did. This is not trustworthy; it is confidential-compute
  theater.
- **Logos** keeps the balance hidden *and* binds it to an on-chain commitment.
  The privacy circuit forces the witnessed value to be the one that opens that
  commitment, so the prover cannot substitute a fake number. The proof now
  attests that the real, on-chain-anchored value cleared the bar - and still
  reveals only the boolean.

That combination - a secret that is simultaneously invisible and provably the
real value - exists nowhere else. Remove Logos and the guarantee collapses to the
plain-zkVM case, which has no on-chain binding and therefore no moat. Logos is
not an optimization in this design. It is the load-bearing wall.

A useful way to hold the data flow in your head: the result is *read* on Solana,
but it is *trusted* because of what happened on Logos. Solana is where you read
the boolean; Logos is why the boolean means anything. The secret itself is never
read from anywhere - it stays with its owner. What lives on Logos is the
commitment (a hash that anchors the value), never the cleartext.

---

## 3. What you can actually compute

A common misread is that confidential execution means "comparisons only" - that
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
USER's private data - lives shielded on Logos, bound to an on-chain commitment
        |
        v
+------------------------------------------------------------------+
| LOGOS (LEZ) - the irreplaceable layer                            |
|                                                                  |
|   sBPF interpreter (the guest)                                   |
|     - loads the developer's bytecode                             |
|     - private witness: the shielded balance (bound to commitment)|
|     - public input: the threshold / parameters                   |
|     - fetch → decode → execute over the secret data              |
|     - asserts the witness opens the on-chain commitment          |
|                                                                  |
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
| SOLANA - settlement                                              |
|   Tuniq coordinator program:                                     |
|     - verify_groth16(...) via the Verifier Router (alt_bn128)     |
|     - parse the journal / circuit output                          |
|     - forward the verified result to the developer's program     |
|   Developer's program acts on a trusted result it never saw raw  |
+------------------------------------------------------------------+
```

### Three zones, three realities

The diagram has a trust boundary running between Logos and Solana. Everything on
the Logos side happens where the secret exists; everything on the Solana side
sees only the verified result. The single invariant the whole product enforces:
the shielded value crosses *zero* boundaries - it is a private witness in the
circuit, absent from the receipt's journal, and absent from the seal that reaches
Solana.

- **Logos (the privacy circuit)** is the only irreplaceable component. It does two
  things no other layer can: it runs the interpreter *and* binds the witness to an
  on-chain commitment. That binding is the moat.
- **The offchain worker** is the one unavoidable operational dependency. The
  succinct→Groth16 wrap requires x86 hardware (the Circom witness generator is x86
  assembly; rapidsnark runs via Docker on x86), so it cannot run inside the Logos
  guest and cannot run on-chain. In production it becomes a managed proving
  service rather than a hand-spun cloud box. It is trust-light, not trustless: it
  sees the succinct receipt but never the cleartext secret. This should be stated
  honestly in any pitch.
- **The Solana coordinator** does two jobs: verify the Groth16 proof (solved - see
  Experiment 3) and parse the circuit output to extract the result (the remaining
  integration work - see §7).

---

## 6. What was validated - the three experiments

Three experiments tested the three things that had to be true. All three passed,
the third with one honestly-marked bridge step remaining.

### Experiment 1 - Is interpreting sBPF fast enough?

A minimal sBPF interpreter was built as a RISC Zero guest and run over programs of
increasing size; the zkVM cycle counter measured cost, and a linear fit separated
fixed overhead from per-instruction cost.

| Metric | Value |
|---|---|
| Per-instruction cost (realistic opcode mix) | ~314 zkVM cycles |
| Fixed overhead (setup, I/O, halt) | ~4,672 cycles |
| 100-instruction predicate | ~36,000 cycles |
| Real proving time (100-insn, unaccelerated laptop CPU) | 9.39 s |
| Verification time | 12 ms |

The marginal cost is stable across memory, branch, compare, and arithmetic
opcodes because the dominant cost is the interpreter's fetch-decode-dispatch
loop. A realistic predicate fits in a single RISC Zero proving segment (under
2^16 = 65,536 cycles), the cheapest category. The 9.4 s figure is the pessimistic
floor on the worst hardware; GPU or a proving service brings it to seconds or
sub-second.

Verdict: viable for the small confidential predicates Tuniq is scoped to.
Caveat: crypto syscalls (sha256, signatures) raise cost sharply unless routed to
accelerators - a later-stage item.

### Experiment 2 - Can the secret stay secret *and* unforgeable?

Built in two layers.

*Layer 1 (privacy mechanic, plain RISC Zero).* A guest reads a secret balance as a
private witness and commits only a public result. A forensic scan confirmed the
secret was absent from the 12-byte journal, and a deliberately-planted leak line,
when enabled, was caught - proving the test genuinely detects leakage.

*Layer 2 (the moat, real LEZ).* A standalone LEZ program (`confidential_predicate`)
evaluated `balance >= threshold` over a private account whose balance is bound to
its on-chain commitment, through LEZ's `execute_and_prove` / privacy-preserving
circuit, built in the SPEL framework. Real proofs, `RISC0_DEV_MODE=0`.

- The valid case (balance 5,000 ≥ 1,000) produced a real proof (~112 s on an
  unaccelerated laptop CPU); the balance never appeared in any public field.
- The false case (balance 500) could not be proven at all - the guest panics and
  `execute_and_prove` fails. A false statement is unprovable. This is soundness,
  demonstrated.

Three properties were shown together, all requiring LEZ: commitment binding
(anti-substitution), selective disclosure (only the result is public), and
soundness (a false predicate has no proof). Vanilla RISC Zero provides none of the
binding on-chain. This is the experiment that makes Logos irreplaceable.

Environment: `RISC0_DEV_MODE=0`, `r0vm` 3.0.5, `nssa_core` `v0.2.0-rc3`,
`spel-framework` v0.3.0.

### Experiment 3 - Can Solana cheaply verify and act on the result?

*L1 - on-chain verifier program.* An Anchor program
(`confidential_predicate_verifier`) verifies a RISC Zero Groth16 proof by CPI into
the deployed Verifier Router (the recommended integration path; verification is
not reimplemented). It compiles against the real verifier crates, deploys, and
runs on a local validator.

*L2 / Path B - the machinery works.* The RISC Zero Solana verifier's own test suite
passed 12/12 against a bundled real proof: a valid proof verifies via `alt_bn128`,
a tampered proof is rejected, wrong public inputs are rejected, and the
claim-digest construction matches. On-chain verification costs under 200,000
compute units.

*L2 / Path 1 - our own proof verifies.* A Groth16 proof of our own confidential
predicate guest was generated on a short-lived x86 droplet (the wrap step requires
x86) and then verified through the exact Solana verification logic - taking only
the seal, journal, and image ID, reconstructing the claim digest, and running the
same `verify_groth16` the on-chain program calls. The secret was absent from the
9-byte journal (`EligibilityResult { eligible: true, threshold: 1000 }`).

Environment: `r0vm` 3.0.5, `risc0-groth16` 3.0.4, Anchor 0.31.1, RISC Zero Solana
verifier `boundless-xyz/risc0-solana` v3.0.0.

---

## 7. The one honest open item - Half A vs Half B

This is the most important section for deciding what to build, and it is subtle
enough to miss even in the experiment notes.

What Experiment 3 settled end-to-end is **Half A**: a *standalone* confidential
predicate guest whose proof carries a clean `EligibilityResult` journal and
verifies on Solana. But that standalone guest is the Layer-1-style mechanic - it
does **not** carry the commitment binding from Experiment 2 Layer 2. In other
words, Half A as proven is essentially vanilla-RISC-Zero confidential compute: the
secret stays out of the journal, but nothing on-chain forces the prover to have
used the real value. The moat is not in it.

The moat lives in Experiment 2's privacy-circuit proof, which was proven on Logos
but settled only on Logos - it was never wrapped to Groth16 and verified on
Solana. So today there are two proven things that have never touched each other:
a commitment-bound proof that lives on Logos, and a Solana settlement path proven
for a *non-bound* proof. The moat and the settlement were demonstrated on
*different artifacts*.

Joining them is **Half B**: taking Experiment 2's shielded, commitment-bound proof
through the worker and the Solana coordinator. Its feasibility was assessed
against the LEZ source and found reachable, not blocked:

- LEZ's `Proof` type is `Proof(Vec<u8>)`, a borsh-serialized RISC Zero
  `InnerReceipt`, with public `into_inner()` / `from_inner()`; the full `Receipt`
  can be reconstructed via `Receipt::new(inner, circuit_output)`.
- `execute_and_prove` proves with `ProverOpts::succinct()` against the privacy
  circuit, and a succinct receipt is exactly the input the STARK→SNARK (Groth16)
  wrap operates on.

Remaining Half B work is integration, not research:

1. The Solana-verified journal is the `PrivacyPreservingCircuitOutput`
   (commitments / nullifiers), **not** a clean `EligibilityResult` - the
   coordinator must parse the circuit output.
2. The succinct→Groth16 wrap of the larger privacy-circuit receipt should be
   confirmed empirically on x86, not just assumed.
3. `execute_and_prove` composes the program proof as an assumption resolved by the
   succinct receipt; wrapping a receipt-with-resolved-assumptions is standard but
   worth one empirical confirmation.

**This single connecting run is the gate.** Until it is green, Tuniq has a moat on
Logos and a settlement on Solana that have never shaken hands - and "confidential
Solana programs" is not yet demonstrated end-to-end. Importantly, the run invents
no new research: every component it depends on has individually passed. It is a
connection task, and it should be done *before* building the coordinator, SDK, or
proving service around it, because everything downstream is shaped by the stamp it
produces.

Half A remains useful as an optional "fast lane" for use cases that genuinely do
not need on-chain binding (where the prover is trusted), but it is not the
headline and should not be the product thesis - shipping it as the thesis would
quietly drop the one property the whole positioning rests on.

---

## 8. Build it from proven parts - reuse vs. custom

A guiding principle: never reinvent verification cryptography. The reuse/custom
split is clean.

**Reuse (audited, already exercised in the experiments):**

- RISC Zero Solana verifier - `boundless-xyz/risc0-solana` v3.0.0 (Verifier Router
  + `groth_16_verifier`). Used in Experiment 3; passed 12/12 vectors. On-chain
  verification via `alt_bn128`, under 200k CU.
- The succinct→Groth16 wrap - `risc0-groth16` and the RISC Zero proving stack.
- LEZ's privacy-preserving circuit, commitment scheme, and `execute_and_prove` -
  the SPEL framework primitives.

**Custom (the thin glue that is the actual product):**

- **Coordinator program (Solana).** CPI into the Verifier Router, parse *our*
  journal / `PrivacyPreservingCircuitOutput`, forward the result to the
  developer's program. This is the `confidential_predicate_verifier` from
  Experiment 3 L1, extended with the Half B output parsing.
- **Proving service (offchain, x86).** The hand-spun droplet grown up into a
  managed worker that takes a succinct receipt and returns a Groth16 seal.
  Trust-light: sees the succinct receipt, never the cleartext secret.
- **Developer SDK.** Build-time annotations for marking confidential inputs,
  compilation to sBPF + a confidential manifest, and client-side orchestration
  (submit request, retrieve proof, submit to the coordinator). The SDK *calls* the
  worker; it does not replace it and holds no x86 wrap logic itself.

---

## 9. Competitive positioning

|  | Arcium | Tuniq |
|---|---|---|
| Trust model | MPC (multi-party computation) | ZK proofs |
| Programming model | Arcium DSL (`#[encrypted]`) | Real Solana sBPF + annotations |
| Privacy source | MPC network | Logos shielded zkVM |
| Verification | Attestation | Groth16 on Solana |

Tuniq differentiates on two axes: ZK rather than MPC for the trust model, and
native Solana tooling rather than a foreign DSL for the developer experience.

---

## 10. Risks and honest caveats

- Interpreter overhead bounds scope to small programs. Large programs are out of
  scope by design, not by accident.
- Crypto syscalls inside a predicate raise cost sharply until accelerators are
  added (a later stage).
- The x86 wrap is an unavoidable offchain, trust-light step; the product can never
  be fully on-chain.
- Half B's end-to-end run is assessed feasible but not yet demonstrated - this is
  the project's single load-bearing open item.
- Market demand for confidential compute on Solana is plausible but unproven; a
  working end-to-end demo de-risks both the technology and the pitch.
- Solo founder, frontier tech, two ecosystems - heavy. The proven experiments
  already remove most of the technical risk; the connecting run removes the rest.

---

## 11. Roadmap to v1

The order is dictated by §7: prove the connection first, then build around it.

**Stage 0 - the gate (do this first).** Run the single Half B connecting test:
wrap Experiment 2's shielded, commitment-bound succinct receipt to Groth16 on x86,
and verify it through the Solana logic, parsing the `PrivacyPreservingCircuitOutput`.
Green here means the architecture is demonstrated end-to-end; red means a precise,
early question for the Logos team.

**Stage 1 - coordinator + minimal proving worker.** Extend the Experiment 3
coordinator to parse the privacy-circuit output and forward the result; wrap the
droplet workflow into a callable proving service. Deliver one confidential
predicate over one shielded input, settled on Solana, end-to-end.

**Stage 2 - accelerators.** Route crypto syscalls (sha256, signatures) to RISC
Zero accelerators so crypto-adjacent confidential programs stay affordable.

**Stage 3 - opcode + syscall coverage.** Expand the sBPF interpreter's opcode and
minimal-syscall coverage to the surface real confidential predicates need.

**Stage 4 - flagship demo.** A real confidential program (sealed-bid auction or
private eligibility) over shielded Logos data, verified on Solana - the
pitch-worthy artifact.

**Stage 5 - SDK.** Developer annotations and tooling so external developers can
write confidential Solana programs without leaving their stack.

---

## Appendix - the intuition, in one analogy

A secret travels down a single road, and the three experiments are three stretches
of it.

You need to prove a private fact to your home government (Solana) - "my balance is
over $1,000" - without showing the number. Home offices verify by redoing the
work, which would mean seeing your statement. So you go to a foreign notary
(Logos) with a sealed, tamper-proof booth.

- The booth reads your statement line by line in a slow certified procedure - the
  sBPF interpreter running your program one instruction at a time. **Experiment 1**
  timed that reading and found it fast enough for a short document.
- Inside the booth, your statement goes in sealed and never comes out; the booth
  stamps only the outside - "balance exceeds $1,000." And because the envelope was
  sealed by the official registry (the on-chain commitment), the notary cannot
  swap in a fake statement, and cannot stamp a claim that isn't true.
  **Experiment 2** built and tested that booth on real Logos machinery. Its one
  quirk: the stamp is in the foreign notary's own ornate format.
- Back home, a fast desk checks a foreign seal against an international registry in
  seconds, without redoing the work - the Groth16 verifier on Solana.
  **Experiment 3** proved the desk accepts genuine seals, rejects forgeries, and
  accepts a stamp on *our own* document.

The missing link: the document the home desk has accepted so far was stamped at a
*simpler* booth - one without the registry seal. Nobody has yet taken the *real
sealed booth's* stamp (Experiment 2's proof), run it through the re-pressing
machine that converts the ornate stamp into the compact international format (the
x86 worker), and walked it up to the home desk. Every piece of that has been shown
to work separately. Connecting them is the gate to v1.