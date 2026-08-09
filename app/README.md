# Dashboard

Sign in as a **trader** or a **sequencer**. Both sides see what is theirs, both
can stake coverage, and a trader who was wronged can take the sequencer's bond.

```bash
npm install
npm run dev          # http://127.0.0.1:5173
```

Devnet, Phantom or Solflare. On-chain state is read straight from
`getProgramAccounts` — no indexer between you and the thing you are checking.

## As a trader

1. **Point at your rollup.** The bar at the top takes an ER RPC address and
   reports who signs receipts there.
2. **Send a transaction.** The wallet signs, this app submits — because the
   receipt arrives in the send response and a wallet's own submission path
   would throw it away. Your copy is kept in this browser, under your address.
3. **Watch the verdict.** Your receipts are checked against the sequencer's own
   published log. Silence is the ordinary answer.
4. **Challenge.** If a position was promised to you *and* to somebody else, the
   sequencer signed both statements. One transaction takes its bond: half burned,
   a share escrowed for you, a share to whoever staked coverage.
5. **Claim.** Produce the displaced transaction and the escrow is released.

Receipts you hold from elsewhere can be pasted under **Import** — base64, one
per line or a JSON array, the same format the watchtower reads.

## As a sequencer

Post a bond against your own ordering, see what a proven fault would cost you
before it happens, read back the log you have published, and unbond — on a
timelock, because the bond has to keep standing behind the log for long enough
that evidence can still arrive.

## What is checked where

Both receipt signatures are verified **in the browser** before any transaction is
built: evidence a client cannot verify itself is evidence the program is about to
reject, and the only difference is who pays the fee. The split shown before you
sign uses the same integer arithmetic the program runs, so it is the outcome
rather than an estimate.
