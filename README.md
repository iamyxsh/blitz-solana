# Ordering receipts for Ephemeral Rollups

**Make unnoticeable MEV noticeable.**

An ephemeral rollup executes transactions first-come-first-served. That is a
promise, not a proof — nothing the validator produces commits to the order
transactions *arrived*, so nothing can contradict it if the order changes.

This adds the missing half. Every transaction gets a signed receipt naming its
arrival position. The block hash already commits to the executed order. Two
commitments, one comparison, and FIFO stops being a promise and becomes a
checkable equation.

And then a consequence, on chain: the operator posts a bond, anyone can stake
coverage beside it, and a proven fault slashes the bond — part burned, part paid
to the trader who was lied to, part to whoever was staked when it happened.

Your block hashes already commit the executed order. These receipts commit the
arrival order. You are ninety percent of the way to having both.

---

## The problem, in one example

Alice has 60 SOL. She owes Bob 50 and Charlie 30, and Dick owes her 20.

| Order | Result |
|---|---|
| A | Bob paid, Charlie fails |
| B | Charlie paid, Bob fails |
| C | Both paid |

Three different winners. **The fraud proof accepts all three**, because each is
the correct execution of *some* order. It is not broken — it is answering a
different question.

Commitments are account diffs:
`CommitStateArgs { nonce, lamports, data, allow_undelegation }`. A commitment
format with no ordering field cannot adjudicate ordering. That is a property of
the type, not an opinion about it.

The validator's own determinism rules say only the scheduler-chosen
serialization order may influence committed state. Determinism is fully
specified. *Which* order gets chosen is not constrained anywhere.

---

## What this is

A fork of `magicblock-validator` v0.13.19 that issues signed ordering receipts,
plus an independent watchtower that checks them.

```
client ──▶ validator ──▶ receipt (signed, sequenced, chained, persisted)
                    │
                    └──▶ block  (blake3 fold over executed signatures)

                    watchtower compares the two
                              │
                              └──▶ Solana: bond slashed, victim paid, pool paid
```

**Three signatures, three hops.** The client signs over a recent block hash,
giving an unforgeable lower bound on creation time. The operator signs a
receipt, binding ingress order. The block hash binds executed order — and does
so as a literal blake3 fold over the ordered signature list, so the executed
side is self-checking.

---

## What it catches

Ten fault types, each carrying enough evidence to be checked by a stranger.

| | |
|---|---|
| **Reorder** | conflicting transactions executed against their receipted order |
| **Equivocation** | two different receipts for one position |
| **Withholding** | ran far later than the operator's own receipt says it arrived |
| **Absent** | receipted and never run |
| **Unticketed** | a transaction holding a block position with no receipt |
| **Broken chain** | a log rewritten after the fact |
| **Bad origin** | a log that does not begin from a genesis link |
| **Withdrawn but executed** | a position publicly taken back, and the transaction run anyway |
| **Impossible ingress** | a receipt whose own fields contradict each other |
| **Not revealed** | a position promised blind, contents never produced |

Every fault re-derives from its own evidence. The watchtower prints
`[verified against the operator key]` only after re-running the accusation
against the carried objects — both receipts, the transaction bytes, and the
block hash fold. Nothing asks you to trust the watchtower.

---

## Seeing it work

```bash
# an honest node
magicblock-validator --lifecycle offline &
cargo run --example send -- http://127.0.0.1:8899 8
cargo run --bin mb-watchtower -- http://127.0.0.1:8899 --once
```

```
· 25 receipts · 25 transactions in 8 blocks · 571 slots scanned
· 0 faults, 9 undetermined
    9 × operator-issued pair
· execution order recovered reversed in 8 blocks
```

Same binary, one environment variable:

```bash
MB_ATTACK=reorder-swap magicblock-validator --lifecycle offline &
```

```
FAULT in slot 473
  reorder in slot 473
    64L6pK… arrived at seq 4  and ran at index 2
    qNaQkU… arrived at seq 0  and ran at index 3

  These two transactions touch the same account, so the order
  between them was the operator's to keep, and it did not.

  [verified against the operator key]
```

`MB_ATTACK` also takes `equivocate` and `withhold`.

The equivocation case is the sharper one: the node's published log is perfectly
self-consistent, and a watchtower reading only that log finds nothing. The
fault appears when the client's own copy of its receipt is added. **The
evidence is not in the operator's log — it is in the hands of whoever it lied
to.**

---

## What it costs the operator

`8VMsFLGQEF4x3wrFUfoipjjyzYFNe8DhNGAjXeDTSey7` — deployed on devnet.

```bash
cargo run -p mb-demo                       # the whole arc, against devnet
```

```
register the operator and post its bond
stake coverage against it
  bond    100000000   staked     50000000   index 0

operator asks for its bond back
operator tries to withdraw immediately
  refused: the bond stays slashable until the delay runs

prove the equivocation
  bond            0   staked     50000000   index 400000000000
  position 50000000   earned     20000000

victim produces the transaction and collects
  victim 0 -> 34995000 lamports
```

An operator registers a signing key and posts a bond. Anyone can stake coverage
against it. Proven equivocation slashes the bond and splits it **5000 / 3000 /
2000** basis points: burned, escrowed for the trader who was lied to, and paid
to whoever was staked at the moment of the fault.

**The burn is the security budget, and that is a compile-time assertion.** An
operator picks which transactions it equivocates over, so it can arrange to be
its own victim, and nothing stops it staking its own pool. Both of those shares
can come back to it. Only the burn is a loss it cannot recover, so
`BURN_BPS >= VICTIM_BPS + POOL_BPS` is enforced by the compiler rather than by
good intentions.

Stakers hold a claim on faults that happen *while they are staked*, tracked by a
reward index rather than a balance split — otherwise anyone watching for an
evidence transaction could stake in front of it and take a cut of a fault they
carried no risk on.

The victim share is escrowed rather than sent, because a receipt names a
transaction *signature* and not an address. Whoever produces bytes hashing to
what the operator committed to, and signs as their fee payer, collects.

Getting out takes longer than misbehaving: `BeginUnbond` starts a timelock and
the bond stays slashable for the whole of it. The watchtower can submit evidence
itself with `--slash`, off by default, and only for faults it re-derived first.

`cargo run -p mb-demo -- --receipts client-receipts.json --signer <pubkey>`
convicts on a log the attack rig actually produced, with the contradicting pair
chosen by the watchtower's own scan.

---

## Not detecting — preventing

Detection leaves a residual: the operator sees content at the moment it assigns
position. Within one 50ms slot it can read your transaction, mint its own, and
stamp its own first.

Commit-reveal cuts that wire. You send a hash; the operator signs a ticket
naming your position while holding nothing else; you then reveal.

> In an open auction the auctioneer sees every bid before deciding anything. In
> a sealed-bid auction the envelopes are ordered first and opened second. You
> did not make the auctioneer honest — you removed their ability to be
> dishonest.

All three rules run in this fork:

1. **Every transaction carries a ticket.** Every producer stamps — including
   the account cloner, task scheduler, undelegation and committor services —
   so a transaction with no receipt is an injection.
2. **Tickets are hash-chained.** Rewriting one entry breaks its own link *and*
   every link after it.
3. **An unrevealed ticket is a fault.** Otherwise an operator pre-commits a
   menu, reveals only the profitable entry, and abandons the rest for free.

```bash
cargo run --example sealed -- http://127.0.0.1:8899 3
```
```
0: committed blind at seq 0
0: revealed into seq 0 · 4EGZoxGhv498woL2MuH95P9ythMCJ2SM8LN8Rprd2EkU…
```

Costs roughly one millisecond, and only for transactions that ask for it.
Co-location is a hard deployment requirement, not a tuning tip: cross-region
adds 60–140ms, which is two to three slots.

See [`docs/V2_COMMIT_REVEAL.md`](docs/V2_COMMIT_REVEAL.md).

---

## What this does not do

A fault proof that overclaims is worse than none.

- **The sub-slot residual remains for plaintext transactions.** Commit-reveal
  closes it; ordinary sends do not get that guarantee. Same residual as
  preconfirmations and BAM.
- **Commit-reveal's residual is economic, not cryptographic.** An operator
  willing to absorb penalties can still speculate. Whether that pays is a
  parameter. Threshold decryption removes it, and that is the stage after this.
- **The outcome annotation is unsigned.** It can suppress an accusation but
  never create one. The signed form is `RETRACT` mode, which is built; wiring
  the node to emit one on a refused forward is not.
- **Nothing bounds how freely an operator retracts.** No cryptography separates
  an honest refused forward from a convenient one. What is bounded is the
  profit: running a withdrawn transaction anyway is a fault.
- **Only equivocation settles on chain.** Reorder and withholding need a signed
  block attestation with a Merkle root over the ordered signature list, and that
  needs an ER identifier this validator does not expose — `getGenesisHash` is a
  placeholder returning zeros. Specified, not built.
- **The pool is bond-funded.** Coverage capital adds depth and earns a share,
  but the operator's bond is what pays. Premiums and pricing are roadmap
  sentences. The accurate name for v1 is a **surety bond with a parametric
  payout**, not insurance — the honest distinction being that a bond is posted
  by the party at fault.
- **The receipt log is never truncated.** Roughly 325 bytes per transaction,
  growing without bound. Arguably correct for evidence, but stated rather than
  hidden.
- **Nothing has run against a hosted devnet ER**, and cannot, because receipts
  require this fork. The slashing program *is* on devnet.

On fair ordering in general: this never claims it is achievable. Aequitas shows
strict receive-order fairness is unachievable across multiple nodes — Condorcet
cycles. A single-sequencer ER collapses the problem from consensus to
accountability, which costs one signature per transaction.

---

## Why a fork and not a sidecar

The first design was a proxy in front of an unmodified validator: zero changes,
working against hosted ERs immediately.

Rule 1 killed it. A proxy can only receipt what passes through it, so an
operator injecting a transaction straight into its own scheduler produces
something the proxy never saw — and its absence from the log would mean
nothing. Inside the validator every producer stamps, and **absence becomes
evidence**.

The trade is real: a sidecar demos against live infrastructure today; this
needs the fork deployed. What it buys is the difference between detecting
reordering and detecting insertion.

The suggested framing for upstream is a **protected ER** — a per-node mode, not
a global protocol change. Flash Trade's ER runs protected; a game's does not.

---

## Repository

| | |
|---|---|
| `crates/constants`, `crates/receipt` | the 261-byte receipt format |
| `crates/watchtower` | the detector and its binary |
| `crates/slashing` | the split and the pool arithmetic |
| `crates/program` | the Solana program |
| `crates/court`, `crates/demo` | evidence transactions, and the end-to-end demo |
| `magicblock-validator/magicblock-receipts` | the stamper |
| `magicblock-validator/magicblock-aperture` | RPC endpoints, fan-out, attack rig |
| [`ENGINEERING_NOTES.md`](ENGINEERING_NOTES.md) | how it was built and what the code says |
| [`docs/V2_COMMIT_REVEAL.md`](docs/V2_COMMIT_REVEAL.md) | the commit-reveal design |

The watchtower depends on `mb-receipt`, `solana-*` and `blake3` — **nothing
from the validator**. That is deliberate: a watchtower linking the validator's
own crates is an insider tool. Anything it reports can be reproduced by anyone
who can reach the same RPC endpoint.

1021 tests in the validator workspace, 160 outside it.

---

## Two findings for upstream

**`getBlock` returns a slot's transactions in reverse index order.** It seeks
from `(slot, u32::MAX)` backwards and pushes in iteration order without
re-sorting, while its sibling `get_transaction_signatures_for_slot` documents
ascending as canonical. Reproduction test in
`magicblock-ledger/tests/block_ordering.rs`. Anything deriving execution order
from `getBlock` sees a correct slot as fully reversed.

**Signing the block hash** would close block-level equivocation on its own, at
one signature per 50ms slot — about three lines at
`prepare_block_as_primary()`. Offered as a small, obviously correct change that
happens to strengthen everything above.

**A third, smaller:** `getGenesisHash` returns `BlockHash::default()` — all
zeros, identical on every ER (`requests/http/mocked.rs`). Anything using it to
tell one rollup from another gets the same answer everywhere, which is why the
receipts here carry a log identifier the node mints for itself instead.

---

## Prior art

Arbitrum's signed sequencer feed is an ordering commitment in production, but
with no per-transaction ingress receipts and no slash condition, and it is not
SVM. TimeBoost shows why FCFS invites latency wars, hence attestation that is
policy-agnostic. Preconfirmations share the detect-and-slash trust model —
receipts are ordering preconfs. Chainlink FSS proposed this in 2020 and never
shipped it. Jito BAM attests L1 sequencing inside a TEE, trading TDX trust for
cryptographic accountability. MagicBlock's own TDX PERs are complementary: a
receipt signer inside the enclave closes the ingress instant, and the best
design is both.

Commit-reveal ordering has prior art in sealed-bid auctions and encrypted
mempools. The contribution here is binding it to a per-transaction signed
ticket chain that plugs into an existing fraud-proof stack. None of the above
target SVM ephemeral rollups.

The payout side is *attributable security* in EigenLayer's sense: the fault
proof names the party at fault and the party harmed, so the same object that
convicts also addresses the compensation. The shape is parametric — proven
fault, fixed share, no loss assessment — which is the trade being made
deliberately, since assessing harm is exactly the process parametric payouts
exist to avoid.

---

*Ordering is now a slashable offence.*
