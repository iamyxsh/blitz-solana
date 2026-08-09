# Commit-reveal ordering for Ephemeral Rollups

A design note, and a description of a working implementation.

Signed ordering receipts make unfair sequencing *detectable*. They leave one
residual: the operator sees a transaction's contents at the moment it assigns
that transaction a position. Commit-reveal removes that, and with it the
ability to order by contents at all.

All three rules below run in the fork this document accompanies. What remains
open is stated in §8.

---

## 1. The residual this closes

An operator receives tx1, reads it, mints tx2 in response, and stamps tx2 first
— all within one 50ms slot, before acknowledging either. Every receipt is
honest. Every chain link holds. Nothing is detectable, because the operator
never contradicted itself; it simply chose the order while knowing what it was
choosing between.

This is the same residual preconfirmations and BAM carry. Detection cannot
reach it, because there is nothing inconsistent to detect.

> In an open auction the auctioneer sees every bid before deciding anything. In
> a sealed-bid auction the envelopes are ordered first and opened second. You
> did not make the auctioneer honest — you removed their ability to be
> dishonest.

---

## 2. Flow

1. Client sends the full transaction to a committing service, self-hosted or
   hosted.
2. The service checks it is **well-formed** — signature valid, block hash
   fresh, not a duplicate. Not *valid*: without executing you cannot know
   whether it will succeed. The weaker word is used deliberately throughout.
3. The service sends `sha256(wire_bytes)` to the operator, signed by the
   committer.
4. The operator returns a **signed ticket** — `(mode, seq, tx_hash,
   prev_receipt_hash, committer, …)` — within a deadline.
5. The service reveals the full transaction.
6. The operator executes it.
7. A watchtower verifies.

The operator's only input at step 4 is a 32-byte digest. A position assigned to
content the operator cannot read cannot have been chosen *because of* that
content.

---

## 3. Three rules

Rule 1 alone does not work. All three are required.

### Rule 1 — every transaction in a block must carry a valid ticket

Without it the operator ignores your reveal, inserts its own liquidation, and
runs your transaction afterwards. Its liquidation carries no ticket, so it
should not be there.

**This is why the design needs a validator change rather than a proxy.** A
proxy can only receipt what passes through it; a transaction injected straight
into the operator's own scheduler is something the proxy never saw, and its
absence proves nothing.

*Running.* Every producer stamps — the RPC path, the JIT account cloner, the
task scheduler, the undelegation service, the committor. Detected as
`Unticketed`. Verified against a live node: 25 receipts, 25 transactions in 8
blocks.

### Rule 2 — tickets are hash-chained

Once ticket 7 exists containing `H(ticket 6)`, no new ticket 6 can be minted
afterwards. To insert before you the operator must have committed **before**
issuing your ticket — that is, blind.

The chain link was designed against log insertion. It happens to close
backdating too.

*Running.* `prev_receipt_hash = sha256(message ‖ signature)`, covering the
signature and not merely the message. Tamper-evidence propagates forward:
rewriting one entry breaks its own link *and* the next, because the rewritten
entry hashes differently. Detected as `BrokenChain`.

### Rule 3 — an issued ticket that is never revealed is a fault

Without this the operator **speculates**: pre-commit a menu — "liquidate
Alice", "liquidate Bob", "no-op" — at tickets 4, 5 and 6, wait for your reveal
at 7, then reveal only the profitable one and abandon the rest. This is the
standard weakness of commit-reveal, and the same shape as last-revealer
manipulation in on-chain randomness.

Penalising non-reveal makes every wrong guess a recorded fault. It doubles as
the anti-spam mechanism.

*Running.* Detected as `NotRevealed`. The node also records an `Expired`
outcome, and caps how many positions one committer may hold open.

---

## 4. What a watchtower checks

| Check | Catches |
|---|---|
| Every block transaction has a valid ticket | unticketed insertion |
| Ticket chain links intact | backdating a ticket into an earlier position |
| Ticket order matches block order, **for conflicting transactions** | ordinary reordering |
| Every issued ticket revealed within its deadline | speculation, and spam |

The conflict restriction is not an optimisation. Execution is parallel — the
validator runs `(num_cpus / 2 − 1).max(1)` executors with all-or-nothing
account locks — so transactions sharing no written account are reordered
constantly and legitimately. A global order comparison would accuse an honest
operator under any load.

Two transactions conflict when their account sets intersect on at least one
account **writable in either**. Read-read sharing is not a conflict.

---

## 5. Design decisions worth keeping

### 5.1 Reveal issues no receipt

The instinct is that revealing produces a second receipt at the same position,
now carrying `tx_sig`. That is two signed statements about one position —
exactly the shape of equivocation — and would force an exception into the
detector. Exceptions to equivocation are precisely where a real one would hide.

It is also unnecessary. The commit ticket already binds the contents through
`tx_hash`, so a watchtower recomputes `sha256(wire_bytes)` from the block and
matches. One ticket per position, no exception, no weakening.

`MODE_REVEAL` therefore remains reserved and unused, deliberately. The join key
for a commit-mode ticket is `tx_hash`, not `tx_sig`.

### 5.2 Gap semantics: relative order, not absolute position

Tickets impose relative order among *revealed* transactions. They do not
reserve an absolute block position.

If the rule were "execute in exact ticket order", **one withheld reveal halts
the chain** — ticket 7 never arrives and 8, 9, 10 wait forever. One attacker,
one message, the whole rollup stopped.

Relative ordering loses nothing. The operator still committed blind, still
cannot backdate, and still cannot reorder you against anyone. It removes a
total-halt vector for free.

### 5.3 The committer signs the commitment

Rule 3's fault attribution depends on knowing *whose* commitment went
unrevealed: a user failing to reveal is spamming, an operator failing to reveal
was speculating. An unauthenticated `committer` field would let anyone commit
as anyone and make that distinction worthless.

The commitment is therefore an ed25519 signature by the committer over the
32-byte hash, verified before any ticket is issued.

### 5.4 A missed deadline is a fault, never a silent downgrade

If missing the deadline fell back to plaintext, the operator would simply
always be slow for transactions it wanted to see, and the protection would
disable itself.

Honest networks do hiccup, so the threshold is generous and configurable
(`MB_REVEAL_DEADLINE_SLOTS`, default 150 slots). If a fallback ever fires it
must be loud in the response, so the client knows the guarantee lapsed.

### 5.5 Anti-spam

Given relative gap semantics, an unrevealed commitment costs a signature and
some bandwidth. Bounded by:

- the commitment is **signed**, so rate limiting is per key rather than per IP;
- a **cap on outstanding unrevealed commitments** per key (default 8), which
  directly bounds the size of a speculative menu;
- reveal-ratio reputation, throttling low ratios;
- Rule 3 doing double duty.

For the deployment that matters — a market maker self-hosting for their own
flow — spam is not a threat at all; they are the only user. It is a business
problem for a hosted offering, not a protocol problem.

### 5.6 Latency

One extra round trip, service to operator.

| Service location | Extra RTT | Share of a 50ms slot |
|---|---|---|
| Same machine | ~0.2 ms | 0.4% |
| Same datacenter | ~0.5–1 ms | ~2% |
| Same region, other AZ | ~2–4 ms | ~6% |
| Cross-region | **60–140 ms** | **2–3 slots — fatal** |

**Co-location is a hard deployment requirement, not a tuning tip.** Measure the
actual RTT and set the deadline at roughly 5×; a round 10ms is too generous at
a fifth of a slot.

Costs about one millisecond, and only for transactions that ask for it.
Liquidations already tolerate far more.

---

## 6. Deployment shape

**A protected ER, not a global change.** The "all transactions ticketed" rule
is a per-node mode. Flash Trade's ER runs protected; a game's does not. This
softens the ask enormously — it is a product, not a protocol migration.

---

## 7. New false-positive surfaces

Detection that fires on honest behaviour is worse than no detection. Three
surfaces exist here that plaintext receipts do not have.

**A missed deadline caused by network trouble** would falsely accuse the
operator. Generous threshold, and repeated misses before convicting.

**A client that crashes between commit and reveal** looks exactly like
speculation. This is why the fault must be attributed to the *committer* — see
§5.3.

**A reveal arriving after the operator legitimately closed the slot** looks
like withholding. The deadline definition has to be explicit about which slot
boundary it refers to.

Implemented handling: a commit ticket names no block hash — it is signed before
any transaction exists — so it is exempt from the ingress-plausibility check
rather than failing it. A commitment still inside its deadline returns
*cannot determine* rather than a fault.

---

## 8. What remains open

**Mode as a signal.** If commit-reveal is opt-in for risky operations only,
choosing it tells the operator "this one is worth money" — contents hidden,
importance advertised. Options: make it the default for everything (preferred,
since at one millisecond there is little reason to gate it), send decoys, or
make the paths indistinguishable on the wire. Undecided.

**Replication.** The validator has primary/replica over NATS. How tickets
replicate is unhandled.

**The residual, stated plainly.** An operator willing to eat penalties can
still speculate: commit a menu, reveal the winner, absorb faults on the losers.
Whether that pays is an economic parameter, not a cryptographic guarantee.

The fully cryptographic version is threshold encryption — no sender-controlled
reveal at all, ordering fixed first and decryption afterwards by a committee.
That is the stage after this one, and it now has a sharper motivation than
confidentiality: **it removes the last economic assumption.**

---

## 9. Cost to implement

Everything above is running. For an upstream reader, the shape of the change:

| Piece | Where |
|---|---|
| `commitTransaction` | ~110 lines, aperture |
| `revealTransaction` | ~100 lines, aperture |
| Blind commit path, pending table, expiry | ~250 lines, receipts crate |
| Optional signature index | ~15 lines, ledger |

Rules 1 and 2 required no commit-reveal-specific work: they fell out of
receipting every producer and chaining the log, both of which are useful on
their own.

---

## 10. Prior art

Sealed-bid auctions and encrypted mempools are the ancestors — Shutter,
Fairblock, Radius. Chainlink FSS proposed ordering fairness in 2020 and never
shipped. Aequitas (CRYPTO 2020) proves strict receive-order fairness is
unachievable across multiple nodes; a single-sequencer ER collapses the problem
from consensus to accountability.

The contribution here is binding commit-reveal to a per-transaction signed
ticket chain that plugs into an existing fraud-proof stack, on SVM ephemeral
rollups, where none of the above currently reach.
