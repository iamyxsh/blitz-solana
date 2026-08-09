# Engineering notes

Working notes from building signed ordering receipts into the MagicBlock
ephemeral validator. Written for someone who will read the code, so every
claim points at a file and a line.

Checkout: `magicblock-validator` v0.13.19 @ `49d84172`.

---

## 1. What the validator does today

The scheduler assigns two numbers to every transaction and never compares
them.

A **monotonic arrival ID** comes from `next_transaction_id()` at
`magicblock-processor/src/scheduler/locks.rs:36`. An **execution index** is
taken at `magicblock-processor/src/scheduler/mod.rs:547` and advanced at
`:549`. Both are in scope in the same function. Neither is signed, neither is
exposed, and nothing in the codebase compares one to the other.

Ordering fairness is not enforced because it is not represented.

That is not a criticism of the design; it is a gap in what the design can
express. The fraud-proof system adjudicates account diffs —
`CommitStateArgs { nonce, lamports, data, allow_undelegation }` at
`magicblock-committor-service/src/tasks/commit_task.rs:47`. A commitment
format with no ordering field cannot adjudicate ordering. That is a property
of the type, not an opinion about it.

The determinism rules say as much without saying it:
`.agents/rules/invariants.md:182` states that *only the scheduler-chosen
serialization order may influence committed state*. Determinism is fully
specified. Which order gets chosen is not constrained anywhere.

---

## 2. Findings from reading the code

Three of these changed what got built rather than merely informing it.

### 2.1 Execution is parallel, so the reorder test must be conflict-relative

`(num_cpus::get() / 2).saturating_sub(1).max(1)` executors
(`magicblock-api/src/magic_validator.rs:395`), each on its own OS thread, each
with an inbound channel of capacity 1 (`scheduler/mod.rs:93`). Account locks
are all-or-nothing (`scheduler/coordinator.rs:150`). A transaction that cannot
take its locks is parked on the blocking executor's min-heap, keyed by arrival
ID, and receives a **later** block index than transactions that arrived after
it.

So honest execution reorders relative to arrival, routinely and by design.

Consequence: a detector comparing global arrival order to global execution
order would accuse an honest validator under any load at all. The comparison
must be restricted to pairs that could have influenced each other — account
sets intersecting on at least one account writable in either. Read-read
sharing is not a conflict.

This is also the economically correct definition. Transactions sharing no
account cannot front-run each other.

### 2.2 `MAX_PROCESSING_AGE` does not exist here

There is no `check_age`, no `BlockhashQueue`, and no slot-count check on the
transaction path. Enforcement is `prepare_transaction` →
`blocks.contains(hash)` (`magicblock-aperture/src/requests/http/mod.rs:306`)
against a **60-second** `ExpiringCache` (`aperture/src/state/blocks.rs:65`).

Advertised validity is `slot + (400 / blocktime) * 150`, which is 1200 slots
at the 50ms default — scaled so the window is always about 60 seconds
regardless of block time.

Consequence: the withholding threshold is derivable rather than guessed. A
client's block hash can legitimately be up to 1200 slots old when it arrives,
so `exec_slot − blockhash_slot > 1200 + X` proves a delay of at least X.

### 2.3 Durable nonces are unsupported, and structurally so

No nonce handling anywhere. It is not an omission that could be added by
configuration: a nonce value would not be in the block hash cache, so it is
rejected at ingress before anything else runs.

Consequence: a planned carve-out disappeared. Every transaction reaching the
node carries a real, age-bounded block hash — which is what makes the 1200
bound meaningful.

### 2.4 The block hash is a streaming blake3 fold

Reset and seeded with the previous block hash at slot start
(`scheduler/mod.rs:728`), each signature stirred in at dispatch (`:554`),
finalised at slot end (`:596`). So:

```
blockhash(n) = blake3(blockhash(n-1) ‖ sig₀ ‖ … ‖ sigₖ)
```

Consequence, and it is the most useful single fact in this document: the
**executed** side of any ordering evidence is self-checking. Hand a verifier
the previous hash and the ordered signature list and they recompute the block
hash themselves rather than trusting anyone's report of the order.

Confirmed empirically — see §3.2.

### 2.5 Blocks are unsigned

`VersionedConfirmedBlock` is built at `magicblock-ledger/src/store/api.rs:485`
with no signature field and `rewards: vec![]`. Nothing in a block is
attributable to the operator.

Receipts are therefore the first operator-attributable artifact in the system,
on either side.

### 2.6 There is no challenge window

`magicblock-committor-service/src/intent_executor/mod.rs:63` carries the
comment *"TODO: with arrival of challenge window remove SingleStage / Protocol
requires 2 stage: Commit, Finalize"*. The commit→finalize split exists to make
room for a dispute period that is not built. Settlement today is fully
optimistic and uncontestable.

### 2.7 Smaller things that shaped the work

- **Address lookup tables are rejected at ingress**
  (`aperture/src/requests/http/transaction_validation.rs:17`), so account keys
  are readable straight off the message with no resolution step. This is why
  the watchtower can derive conflict sets without an ALT resolver.
- **Airdrops are hard-disabled** (`request_airdrop.rs:15`), which is why the
  demo funds nothing and relies on transactions that reach the scheduler and
  fail there.
- **`commit_frequency_ms` is dead code** behind `#[allow(dead_code)]`
  (`magicblock-account-cloner/src/lib.rs:285`, issue #625). Settlement happens
  only when a program asks. Do not build a demo assuming a timer.
- **Delegation names exactly one authority**
  (`chainlink/src/chainlink/fetch_cloner/delegation.rs:112`), so a delegated
  account lives on exactly one ER at a time. A shared order book relocates
  wholesale, and one operator sequences the entire venue. That is the setting
  where an unfalsifiable FIFO promise is worth least.

---

## 3. Experiments

Both are permanent regression tests rather than scratch scripts, so they fail
loudly if upstream behaviour changes.

### 3.1 `getBlock` returns transactions in reverse index order

`magicblock-ledger/tests/block_ordering.rs`

Written at indices 0, 1, 2, 3; returned 3, 2, 1, 0.

`get_block` seeks from `(slot, u32::MAX)` with `IteratorDirection::Reverse`
and pushes in iteration order without re-sorting
(`magicblock-ledger/src/store/api.rs:450`). Its sibling
`get_transaction_signatures_for_slot` seeks forward from `(slot, 0)` and
documents ascending as canonical (`:394`). The two readers disagree about the
same data.

This is a real upstream bug, and the fix is a one-line direction change plus
the reproduction test.

**It is also load-bearing here.** Anything deriving execution order from
`getBlock` sees a correctly ordered slot as fully reversed. The watchtower
therefore recomputes the blake3 fold in both directions and keeps whichever
reproduces the block hash. Against a live node it recovers `reversed` in every
populated block — so without this the honest-run output would be a wall of
false faults.

*A note on how the test was built:* the first version compared the wrong
identities, because the shared `write_dummy_transaction` helper keys rows by an
unrelated random signature. The test now writes transactions keyed by their own
signature, the way `executor/processing.rs:304` does.

### 3.2 The block hash is exactly the blake3 fold

`magicblock-processor/tests/blockhash_fold.rs`

```
slot   1  txs  0  reproduces false
slot   2  txs  0  reproduces true
slot   3  txs  4  reproduces true
...
reproduced 8 populated and 5 empty blocks
```

Two things the code reading did not reveal:

- **Empty slots reproduce too**, as `blake3(prev)` alone. A watchtower can
  therefore verify quiet slots, not just busy ones — which matters for
  withholding, where an empty slot needs to be verifiably empty rather than
  merely unreported.
- **The first slot after boot does not reproduce.** A freshly started
  validator's hasher has no predecessor to seed from. Pinned as a named
  constant in the test so nobody later reads it as a bug.

A companion test reverses the signature list and asserts the reproduction
breaks. Without it, the main test would pass on a hash that ignored order
entirely.

---

## 4. What was built

Roughly 1,700 lines in the validator and 2,450 outside it.

### 4.1 Receipt format

`crates/constants`, `crates/receipt` — 923 lines, 45 tests.

261 bytes, fixed offsets, little-endian throughout, ed25519 over the raw
concatenation. Chain link is `sha256(message ‖ signature)`, which means it
covers the signature and not just the message.

```
domain_tag        "MBRECEIPT_V1"   12
log_id            run of the log   32
mode              u8                1
seq               u64 LE            8
tx_sig            first signature  64
tx_hash           sha256(wire)     32
recent_blockhash  copied from tx   32
prev_receipt_hash sha256           32
committer         pubkey           32
ingress_slot      u64 LE            8
t_ingress_micros  u64 LE            8
```

Transport encoding is the 261 message bytes followed by the 64-byte signature,
325 in total, and `receipt_hash` is the sha256 of exactly those bytes.

Four modes, each with its own invariants enforced in `validate()` on every path
that can produce a receipt — including `sign()`, so a malformed one cannot be
signed even by constructing the struct directly. `PLAIN` names a transaction.
`COMMIT` zeroes `tx_sig`, because at commit time the signature is inside bytes
the operator has not seen, and requires a `committer`. `RETRACT` withdraws a
position — see §4.7. `REVEAL` is reserved and rejected.

**`log_id` was the field that was missing, and its absence was a live
false-positive generator.** The sequence counter starts again at zero every
time a writer is built (`writer.rs`, `seq: 0`, no resume from the ledger) while
the signing key does not — it is the validator identity. Two runs of one node
therefore produce two entries at every position, both genuinely signed. Before
this field, a plain restart yielded four `Equivocation` faults that each
verified standalone against an operator that had done nothing but reboot.

It is not a genesis hash. `getGenesisHash` on this validator is a placeholder
returning `BlockHash::default()` — all zeros on every ER
(`aperture/src/requests/http/mocked.rs:152`), so there is nothing chain-shaped
to domain-separate with. The node mints one per writer from
`sha256(identity ‖ nanos ‖ process counter)`; the counter is there because two
writers can be constructed inside a single clock tick, and a repeated log id
would undo the point of having one.

**Five bug classes** are treated as fatal to the idea rather than to the code,
and each has a test that fails without the fix: hashing an encoded form instead
of raw wire bytes; assigning a sequence number after forwarding; computing the
chain link over the wrong scope; endianness drift; and — added when the program
was built — trusting that an ed25519 instruction is present rather than
checking what it verified (§6).

### 4.2 The stamper

`magicblock-validator/magicblock-receipts` — 1,221 lines, 20 tests.

One task owns the sequence number, the chain, and the signing key. Everything
else holds a cloneable handle. The single-writer property is enforced by
ownership rather than by discipline at each call site.

Two ordering decisions inside it matter more than the structure:

**A receipt is persisted before the caller ever sees it.** A receipt handed out
but not recorded would leave a client holding a signed statement the node has
no memory of, which is indistinguishable from equivocation produced by an
honest node.

**The sequence advances only after both the signature and the write succeed.**
A refused receipt must not consume a position, because a dense log is what
makes a missing sequence number mean something.

The chain forces sequential signing: receipt *n+1* cannot be built until *n* is
signed, because the link covers *n*'s signature. Ed25519 signing is about 25µs,
so the ceiling is roughly 40k receipts/second — far above this validator, but
it is inherent to the format rather than an implementation choice.

### 4.3 Where the stamp happens, and why exactly there

`magicblock-aperture/src/requests/http/send_transaction.rs`, between the replay
check and `ensure_transaction_accounts`.

After signature verification and deduplication, so the node only ever signs
statements about well-formed transactions it has not already seen. **Before**
account resolution, because that step can take up to 30 seconds cloning from
the base chain, and everything downstream of it is latency the operator
controls. An ordering claim made after an operator-controlled delay proves
nothing about arrival order.

This position has a cost: a transaction stamped here can still fail account
resolution and never execute, which a naive watchtower reads as withholding.
That is why receipts carry an outcome — see §5.2.

### 4.4 Every producer is receipted, not just RPC traffic

The stamp lives inside `TransactionSchedulerHandle` itself
(`magicblock-core/src/link/transactions.rs`), so `schedule()` and `execute()`
receipt by default. The four internal producers — JIT account cloner, task
scheduler, undelegation service, committor — required **no changes**. Opting
out requires calling `schedule_receipted()` / `execute_receipted()`, which the
RPC path does because it already stamped at ingress.

`magicblock-core` cannot depend on the receipts crate (that would be
`core → ledger → receipts`, a cycle), so core declares an `IngressStamper`
trait and the receipts crate implements it.

This inverts the default: previously you had to remember to stamp; now you have
to deliberately opt out, and the opt-out is visible in review.

**Consequence:** a transaction in a block with no receipt is now a fault. That
is commit-reveal's Rule 1, obtained in v1 because this is a fork rather than a
proxy in front of one.

Verified against a live node: 25 receipts, 25 transactions in 8 blocks. Exact
match.

### 4.5 Storage

Two RocksDB column families in `magicblock-ledger`:
`receipt_by_seq` (u64 → outcome byte + 325 receipt bytes) and `receipt_by_sig`
(signature → seq).

Keys are **big-endian**, because RocksDB orders keys lexicographically and only
big-endian makes byte order match numeric order — which is what turns "walk the
chain from sequence N" into a range scan. Note the split brain and keep it
straight: receipt *wire* bytes are little-endian per the spec; storage *keys*
are big-endian. Two byte orders, one system, different jobs.

**A trap worth recording.** The slot-based compaction filter
(`database/compaction_filter.rs:93`) deletes any key whose `slot()` falls below
`oldest_slot`. Every pre-existing column is slot-addressed; these are keyed by
sequence number and have no slot, so `slot()` returns 0 — below `oldest_slot`
the moment the truncator advances. RocksDB then drops the rows during ordinary
background compaction, with no error and no log line.

That is evidence disappearing, silently, in exactly the shape a withheld
transaction has. Both columns override `keep_all_on_compaction() -> true`,
making them the first columns in the codebase to do so. Confirmed by flipping
the flag back: all eight test receipts vanished.

### 4.6 Reading the log

- `sendTransaction` returns `{signature, receipt}` — **not** wire-compatible
  with stock Solana, deliberately.
- `getReceipts(from_seq, limit)` — the backfill path, capped at 1000.
- `getReceipt(signature)`.
- `receiptSubscribe` — live stream over the existing websocket.

The websocket is a latency optimisation. **Storage is the source of truth.**
`UpdateSubscriber::send` uses `try_send`, so a stalled subscriber silently
misses receipts; the stamper's broadcast drops with `Lagged(n)` if the fan-out
falls behind. Neither is fixable from inside the node — a dropped message is
invisible to the client by construction. The fix belongs in the watchtower:
sequence numbers are dense and monotonic, so it detects its own gaps and closes
them from `getReceipts`.

Receipt fan-out is a dedicated task rather than a field on `EventProcessor`,
because `event_processors` is configurable and each processor would otherwise
deliver its own copy of every receipt to the same subscribers.

*Possibly an existing bug, unverified:* `slotSubscribe` appears to have exactly
that shape — `block_update_rx` is a tokio broadcast, so every processor
receives every block and each calls `send_slot`. Worth a five-minute check and
a small PR.

### 4.7 Retraction, and why it takes its own position

A forward the node refuses leaves a receipt for a transaction that will never
run, which a watchtower reads as withholding. The stated production answer was
a signed retraction. Building it turned up a collision: a retraction *at the
same sequence number* is, byte for byte, a second signed statement about one
position — which is the definition of equivocation. The system's own
recommended repair would have convicted the operator using it.

A withdrawal therefore takes **its own** sequence number and names the receipt
it voids by hash. The log keeps exactly one statement per position, the chain
stays linear, and the equivocation check needed no change at all. The
alternative — two statements at position 7 — also forks the chain, because
position 8 could only link to one of them.

`tx_hash` holds the withdrawn receipt's hash rather than a transaction hash.
That is the format's only genuine pun; the mode byte disambiguates it and
`validate()` enforces it. The alternative was a new field and a broken length
freeze for a case that is rare by construction.

What stops "retract anything I dislike" is not in the format. It is that a
withdrawn transaction which executes anyway is a fault —
`WithdrawnButExecuted`, and unlike the two absence claims it re-derives from
its own object: two signatures, a hash match, a blake3 fold and a block
position. It is the first fault fit to go on chain unchanged.

A withdrawal retires the promise, not the statement. `check_ingress_is_possible`
still runs on a withdrawn receipt: one claiming it arrived before its own block
hash existed said something false when it was signed, and taking the position
back afterwards does not unsay it.

---

## 5. The watchtower

`crates/watchtower` — 2,323 lines, 64 tests, plus a binary.

It depends on `mb-receipt`, `solana-*` and `blake3`. **Nothing from the
validator.** That is the claim made structural: a watchtower linking
`magicblock-ledger` is an insider tool. If this crate ever needs a validator
dependency, the evidence design has failed.

### 5.1 Three outcomes, in the type system

```rust
enum Verdict { Fault(Box<Fault>), Clean, CannotDetermine(Undetermined) }
```

Not a bool and not an `Option<Fault>`. A detector that can only say "fault" or
"fine" eventually reports a bug in its own ingestion as misbehaviour by the
operator. Making the third case a variant the compiler insists on handling is
what prevents it. Only `Fault` reaches output; undetermined checks are counted
and stay silent.

Against a live honest node the undetermined count is non-zero and every entry
is the same category — see §5.3. That number is a feature: it is the detector
stating the boundary of what it examined.

### 5.2 Faults, and what each one proves

| Fault | Claim |
|---|---|
| `Equivocation` | two different receipts at one sequence number |
| `BrokenChain` | a link that does not follow from its predecessor |
| `BadOrigin` | the log does not begin from a genesis link |
| `Unticketed` | a transaction holding a block position with no receipt |
| `WithdrawnButExecuted` | a position publicly taken back, and the transaction run anyway |
| `Reorder` | conflicting transactions executed against their sequence |
| `Withheld` | ran, far later than the operator's own receipt says it arrived |
| `Absent` | receipted and never run |
| `ImpossibleIngress` | a receipt whose own fields contradict each other |
| `NotRevealed` | a position promised blind, contents never produced |

Every fault carries enough to be checked by someone holding nothing but the
object and the operator's public key, and exposes `verify()` so that is
executable rather than asserted. The binary re-derives each fault before
printing it.

`Fault::Reorder::verify()` re-runs the whole accusation: both receipts verify,
`sha256(wire_bytes)` matches each `tx_hash`, the signature list folds to the
block hash, both transactions sit at their claimed indices, and the inversion
holds. Two tests confirm it can *fail* — swapping one side's transaction bytes
breaks the binding, reversing the claimed order breaks the fold — because a
`verify()` that always passes proves nothing.

**A stated boundary:** `verify()` proves the *inversion*, not the *conflict*.
Deriving account sets needs a Solana parser, so the fault carries both
transactions' wire bytes and the consumer re-derives. Better a boundary written
down than a silent gap.

### 5.3 The false-positive surfaces, and how each is handled

False positives are the unforgivable bug here: a fault proof that fires on
honest behaviour is worth less than no fault proof, because the one real fault
it eventually finds carries no weight. Each of these has a test.

**Duplicate delivery.** A reconnect overlapping a backfill delivers the same
receipt twice. Byte-identical at one sequence number is re-delivery, not
contradiction.

**Sequence gaps.** Paging, a lagged stream, a late subscriber. A gap is
something the detector could not see, never something the operator did.

**Arrival order.** The stream and the backfill interleave; the engine sorts.

**Parallel execution.** Transactions writing different accounts execute in any
order the scheduler likes. Without the conflict test the detector fires on
every busy honest slot. Read-read sharing is not a conflict either.

**The clone inversion.** The sharpest one, and it was introduced by this work.
A user's transaction is stamped at ingress; the JIT clone it triggers is
stamped a moment later and must execute *first*, because it creates the
account. Sequence order and execution order disagree, on two transactions that
conflict — on every cold-account transaction, against an honest node.

The resolution is `CannotDetermine`, not `Clean`. An earlier plan was to exempt
operator-issued transactions from ordering checks; that is a loophole, because
an operator can fund an account and front-run through the exemption.
`CannotDetermine` is the honest statement: this detector cannot adjudicate this
pair. It never accuses the innocent and never silently clears the guilty.

Against a live honest node, **every** undetermined verdict is this category and
nothing else.

**Recent receipts with no execution yet**, and **block hashes aged out of the
ring** — both undetermined rather than faults.

### 5.4 Two framing bugs, found by asking who else can write

The surfaces above are all about honest behaviour that *looks* guilty. These
two are different: an outsider making an honest operator look guilty on
purpose. Both were live, and neither is exotic.

**Anyone could manufacture an equivocation.** The scan recorded signature
verdicts and then grouped *all* receipts by sequence number — including ones
whose signature had failed. Append a receipt at seq 7 with sixty-four bytes of
junk, and `Fault::Equivocation` fires against the genuine receipt at seq 7. It
cost nothing to try and required no key.

Removing it took one change, but the right one is not "filter the junk". A
receipt that does not verify is attributable to *nobody* — anyone can write
bytes, only the operator can sign them — so it is not a fault at all. It is
`CannotDetermine`. `Fault`'s own doc comment promises attributable operator
misbehaviour, and there was a tell: `Fault::Unverifiable` was the only variant
whose `verify()` succeeded by proving something *fails*. It never re-derived
misbehaviour because there was none to derive.

Disabling the fix to check the test showed a second route nobody had predicted.
*Substituting* junk for an entry rather than appending it produces
`BrokenChain` at the **next** sequence number — because the tampered entry
hashes differently, so its honest successor no longer points at it. That
accusation names two receipts that are both genuinely the operator's, and both
genuinely fail to link. It verifies standalone. The forgery is invisible in the
evidence. One fix closed both.

**The same hole existed one layer up, in the evidence itself.** `Fault::verify`
takes only a public key, and no variant checked that the receipts it carries
come from one run of the log. Take entry 2 of a node's first run and entry 3 of
its second: adjacent sequence numbers, genuine signatures, and of course the
second's chain link points at its own predecessor rather than the first's.
`Fault::BrokenChain { receipt: B3, predecessor: A2 }` returns `Ok(())`. An
honest operator convicted of rewriting its log for the crime of restarting,
assembled by anyone who read the public log across a reboot.

The four multi-receipt variants now require their receipts to agree on
`log_id`, checked against **each other** rather than against a configured
value, so the object stays answerable by someone holding nothing but it and a
public key. That property is not decoration: `verify` is the predicate the
on-chain program runs, and a program built on the earlier version would have
slashed operators for rebooting.

The lesson generalises, and it is the rule the code now follows: **claims about
an absence are safe to build from unverified input; claims about a presence are
not.** A missing receipt at worst produces an accusation in the operator's
favour. A *present* forged one produces an accusation against it. So
`ReceiptIndex` tolerates gaps while `Withdrawals` demands the operator's key to
build at all, and everything that could name the operator passes through a
single `Operator::accepts` — key and log together, in one place, because a check
spread across four call sites is a check one of them will forget.

### 5.5 The live result

```
· 25 receipts · 25 transactions in 8 blocks · 571 slots scanned
· 0 faults, 9 undetermined
    9 × operator-issued pair
· execution order recovered reversed in 8 blocks
```

Zero false positives on real data with real parsing. Every undetermined entry
is the predicted category. Order recovered reversed in every populated block,
which is §3.1 doing load-bearing work.

---

## 6. Catching it

Four deliberate misbehaviours, off by default, enabled by `MB_ATTACK` and
announced loudly at startup.

**`reorder-swap`.** The naive implementation — swapping sequence numbers in the
writer — breaks the hash chain, and the watchtower would report `BrokenChain`.
That is a *weaker* story: "the operator tampered with its own log."

The real attack leaves the log immaculate. Sequence numbers are still issued in
arrival order, the chain still links, every receipt still verifies. Only
**execution** is reordered, by holding one transaction back and letting the
next overtake it. The fault then says: the block hash commits to one order, the
receipts commit to another, and both are signed by you.

The rig sits between the receipt and the scheduler, which is the only place a
reorder can be staged without disturbing the log — itself a decent illustration
of why the stamp goes where it goes.

Four swapped pairs, four faults, deterministic across runs. Zero on the honest
run with identical traffic.

**`equivocate`.** The published receipt differs from the one returned to the
client, in `tx_hash` only. Tampering `tx_sig` would have orphaned every real
transaction and buried the demo under `Unticketed` faults.

The result is the sharper demonstration: the node's own log is perfectly
self-consistent, and a watchtower reading only the log finds nothing. The fault
appears only when the client's copy is added. **The evidence is not in the
operator's log — it is in the hands of whoever it lied to.**

**`withhold`.** Receipt a transaction and never forward it. Five lines, because
the rig already knew how to hold one.

**Abandonment** (`sealed --abandon`) commits positions and never produces the
contents — the shape of speculation.

---

## 7. Commit-reveal

The residual v1 cannot close: the operator sees content at the moment it
assigns position. Inside one 50ms slot it can read tx1, mint tx2, and stamp
tx2 first before acknowledging either. Same residual as preconfirmations and
BAM.

Commit-reveal cuts that wire. At ticket-assignment time the operator holds only
a hash.

All three rules run in this fork:

1. **Every transaction carries a ticket** — true since §4.4, detected as
   `Unticketed`.
2. **Tickets are hash-chained** — true since §4.2, with tamper-evidence
   propagating forward: rewriting one entry breaks its own link *and* the next.
3. **An unrevealed ticket is a fault** — `NotRevealed`, with expiry recorded by
   the node as a fourth outcome.

Two design decisions worth keeping:

**Reveal issues no receipt.** The commit ticket already binds the contents
through `tx_hash`. A second statement about one position is exactly the shape
of equivocation, and would have forced an exception into the detector —
exceptions to equivocation being precisely where a real one would hide. So
`MODE_REVEAL` stays reserved and unused, deliberately. The join key for a
commit ticket is `tx_hash`, not `tx_sig`.

**The committer signs the commitment.** Without it, `committer` would be
unauthenticated and Rule 3 could not distinguish a user who failed to reveal
(spam) from an operator who never intended to (speculation).

Live, against a running node:

```
0: committed blind at seq 0
0: revealed into seq 0 · 4EGZoxGhv498woL2MuH95P9ythMCJ2SM8LN8Rprd2EkU…

· 10 receipts · 10 transactions in 3 blocks · 0 faults
```

Ten receipts, ten transactions — the commit tickets matched by content hash.

And abandonment caught two independent ways: the node records `expired`, while
the watchtower derives `NotRevealed` **without believing it**, from the tickets
and the absence of matching contents. An operator hiding speculation would
simply omit those entries from any endpoint it controls; it cannot omit them
from the chain it signed.

---

## 8. Settling it on chain

`crates/slashing`, `crates/program`, `crates/court`, `crates/demo` — 2,711
lines, 51 tests. Deployed to devnet as
`8VMsFLGQEF4x3wrFUfoipjjyzYFNe8DhNGAjXeDTSey7`.

Detection is a log line until something is at stake. The program holds an
operator's bond, holds capital other people stake against it, and pays out when
a fault is proven. It lives on the **base chain**, never the ER: the operator
orders transactions inside its own rollup and nowhere else, so on Solana it is
one fee payer among everyone else with no say in inclusion. You do not file the
lawsuit in the defendant's courtroom.

The bond sits in a program-owned PDA and the program never CPIs to the
delegation program, so the stake cannot be leased into an ER the accused
controls. That is structural rather than a rule someone has to remember.

### 8.1 The one that would have been fatal

Solana verifies ed25519 in a precompile, and a program reads back what it
verified by introspecting the instructions sysvar. The precompile proves *some*
signature over *some* bytes and says nothing about whose or over what — the
public key and message live at byte offsets inside its own instruction data,
and each offset entry names which instruction to read them from.

**A program that checks an ed25519 instruction is present has checked
nothing.** Point the precompile at a harmless message in a different
instruction, which it genuinely verifies, while the receipt bytes sitting in
*this* instruction — the ones a careless program reads and adjudicates — were
signed by nobody. Every offset entry must resolve inside the verifying
instruction, and `an_entry_pointing_at_another_instruction_is_refused` is that
attack written out.

The parsing is a pure function over `&[u8]`, so all of it is tested without an
SVM, and a test in `crates/court` runs the client's instruction builder
straight into the program's reader — they live in different crates and a
disagreement between them would be silent.

Evidence is adjudicated as **raw bytes at the frozen offsets**, not through a
parsed struct. Parsing first and judging the struct leaves a gap where a field
could be read differently on chain from the way it was signed off it.

### 8.2 Conviction addresses are derived from the fault

`[b"conviction", operator, min(hash_a, hash_b), max(hash_a, hash_b)]`. Keying
by operator alone would let the same evidence be submitted twice for two
payouts; without the canonical ordering, presenting the pair backwards would
mint a second conviction for one offence.

### 8.3 The burn is the security budget

The slash splits 5000 / 3000 / 2000 basis points — burn, victim, coverage pool.
The rule that matters is a compile-time assertion:

```rust
const _: () = assert!(BURN_BPS >= VICTIM_BPS + POOL_BPS);
```

An earlier draft had "victim + bounty < slash, burn > 0", which assumed an
honest victim. It is wrong. **An operator chooses which transactions it
equivocates over**, so it can arrange to be its own victim, and nothing stops it
staking the coverage pool. Both of those shares can flow back to it. Only the
burned share is a loss it cannot recover, so the burn alone is the security
budget — and the compiler now refuses a table where it is not at least
everything else combined. There is no separate whistleblower bounty for the
same reason: another recoverable share would weaken the only one that is not.

### 8.4 The pool pays whoever carried the risk

Stakers are not buying a share of a growing balance. They are buying a claim on
faults that happen *while they are staked*, so the pool tracks a cumulative
reward-per-lamport index and each position records the index it entered at.

A balance split would be exploitable in an obvious way: anyone watching for an
evidence transaction in the mempool could stake in front of it and take a cut of
a fault they carried no risk on. `staking_after_a_slash_earns_nothing_from_it`
is that property. A slash with nothing staked burns the pool share rather than
parking it where the next arrival collects it.

### 8.5 The victim share is escrowed, not sent

A receipt names a transaction *signature*, not an address, so cold evidence has
no wallet to pay. The share waits in the conviction account until someone
produces the transaction.

The first design matched the stored signature against the transaction's first
signature. That is wrong and badly so: **nothing on chain verifies that
signature.** Anyone could read the conviction account, paste the signature into
a transaction naming themselves as fee payer, and collect. The binding has to be
the receipt's `tx_hash` — bytes that hash to what the operator already committed
to genuinely are the transaction — and the fee payer read out of them must sign
the claim.

Reading that fee payer has its own trap. A versioned message carries a version
byte before the header; treating it as a header byte shifts every field and
hands the payout to whoever the misaligned bytes happen to name. Its own test.

Equivocation is the only fault settled so far, and the reason is size rather
than preference: two 261-byte receipts pack into one ed25519 instruction at
about 744 bytes, whereas `WithdrawnButExecuted` needs the block's whole
signature list for the fold — 16 KB for a busy slot — and therefore needs the
Merkle attestation that does not exist yet.

### 8.6 Leaving takes longer than misbehaving

`BeginUnbond` starts a timelock; `WithdrawBond` refuses until it runs. The bond
keeps standing behind the log throughout, so evidence for anything the operator
did *before* asking still slashes it. Otherwise misbehaving and withdrawing in
the same breath costs nothing. Asking twice does not restart the clock, which
would keep a withdrawal permanently one instruction away.

Key registration is one-way. An earlier plan called for slot-bounded key
validity; it cannot work, because receipts carry an *ER* slot and the program
lives on the *base* chain, so the two numbers are not comparable. What actually
defends against rotation is that a registered key is never removed and
revocation never releases the bond early — the timelock does the work the slot
range was being asked to do.

### 8.7 The whole arc, on devnet

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

0.1 SOL bonded: 50,000,000 lamports burned to the incinerator, 30,000,000
escrowed and collected, 20,000,000 to the pool. The index lands at
`20,000,000 × 10¹² / 50,000,000`. A fresh authority every run, so it can be
filmed twice.

The watchtower submits with `--slash` and `--operator`, off by default, and only
for faults it re-derived itself. Putting an accusation on chain that the process
could not verify would spend real lamports on something the program is about to
reject anyway.

---

## 9. What this does not do

Stated plainly, because a fault proof that overclaims is worse than none.

**The sub-slot residual remains in plaintext mode.** Within a single slot the
operator can still see and stamp in an order of its choosing before
acknowledging anything. Commit-reveal closes it; plaintext transactions do not
get that guarantee.

**The outcome byte is unsigned.** It can only ever *suppress* an accusation,
never create one — a client holding a signed receipt for a transaction it knows
was valid still has counter-evidence against a false `Rejected`. But an
operator marking everything rejected degrades withholding detection to "the
client must complain." The signed answer is `RETRACT` mode, which **is** built
(§4.7); wiring the node to emit one on a refused forward is not.

**Nothing bounds how freely an operator retracts.** A withdrawal that is never
followed by execution is indistinguishable from an honest refused forward, and
no cryptography separates them. What is bounded is the profit: running the
withdrawn transaction anyway is a fault. A retraction *rate* is a dashboard
signal, not evidence, and it is left as one rather than dressed up.

**Commit-reveal's residual is economic, not cryptographic.** An operator
willing to absorb penalties can still speculate: commit a menu, reveal the
winner, eat the faults on the losers. Whether that pays is a parameter, not a
guarantee. Threshold decryption removes the sender-controlled reveal and hence
the last economic assumption — that is the stage after this one.

**The receipt log is never truncated.** The ledger truncator purges by slot
range and these columns are sequence-keyed, so the log grows at roughly 294
bytes per transaction. Arguably correct for evidence, but it is a stated
property rather than an oversight.

**The conflict test is not verified inside `Fault::verify()`.** See §5.2.

**Ticket replication is unhandled.** The validator has primary/replica over
NATS; how tickets replicate is not addressed.

**Only equivocation settles on chain.** Reorder and withholding both need a
signed block attestation with a Merkle root over the ordered signature list —
about fifty lines at slot close — and that attestation needs an ER identifier to
domain-separate with, which this validator does not expose (§4.1). Specified,
not built.

**The pool is bond-funded.** Coverage capital adds depth and earns a share, but
the operator's bond is what actually pays. Premiums, coverage terms and pricing
are roadmap sentences, not code, and the two capitals are kept apart on purpose:
a bond is a licence deposit posted by the party at fault, and calling it
insurance would mean the insurer and the insured risk are the same entity. The
correct name for what v1 is, is a **surety bond with a parametric payout**.

**The watchtower can be misconfigured into silence.** Give it the wrong
`log_id` and every receipt reads as foreign, every transaction reads as
unticketed. A loud startup check against what the node serves is ops work that
is not done; the receipt log id is discoverable only from a receipt.

**Nothing has run against a hosted devnet ER**, and cannot, because receipts
require this fork. That is the cost of the route chosen — see §10.

---

## 10. The route not taken

The original plan was a sidecar: a receipt gateway in front of an unmodified
validator, zero changes, working against hosted devnet ERs on day one.

This is a fork instead. The reason is Rule 1. A proxy can only receipt what
passes through it, so an operator injecting a transaction directly into its own
scheduler produces something the proxy never saw — and its absence from the log
would mean nothing. Inside the validator, every producer stamps, and absence
becomes evidence.

The trade is real and worth stating rather than glossing: a sidecar demos
against live infrastructure today, whereas this needs the fork deployed. What
it buys is the strongest available claim — *a transaction in a block with no
receipt is a fault* — which is the whole difference between detecting
reordering and detecting insertion.

---

## 11. Numbers

| | |
|---|---|
| Outer workspace | 160 tests |
| Validator workspace | 1021 tests |
| Receipt spec (`constants`, `receipt`) | 923 lines |
| Stamper (`magicblock-receipts`) | 1,271 lines |
| Aperture endpoints, fan-out, attack rig | 517 lines |
| Watchtower | 2,323 lines |
| Pool and split arithmetic (`slashing`) | 420 lines |
| Slashing program (`program`) | 1,531 lines |
| Evidence transactions and demo (`court`, `demo`) | 760 lines |

Every fault type is demonstrated against a running validator; every fault
printed by the binary has been re-derived from its own evidence first; and the
conviction path has run end to end on devnet.

Three bugs in this document were found by disabling the fix and watching the
test fail, rather than by reading. Two of them turned out to have a second route
nobody had predicted. That is the argument for the practice, and it is the only
methodology note here worth keeping.
