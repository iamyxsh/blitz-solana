# Dashboard

Coverage staking and slashing for ordering receipts, against the program on devnet.

```bash
npm install
npm run dev          # http://127.0.0.1:5173
```

Connect Phantom or Solflare on **devnet**. Everything is read straight from
`getProgramAccounts` rather than through an indexer — the dashboard is as
checkable as the evidence it displays.

## Tabs

| | |
|---|---|
| **Overview** | who is bonded, what is staked, every proven fault |
| **Validator** | register a signing key, post a bond, see your exposure, unbond |
| **Coverage** | stake against a sequencer, watch rewards accrue, claim, unstake |
| **Escalate** | turn a receipt into a slashing |

## Escalating

Three steps, and the second is the one that matters.

1. **Read the operator's published log** from an ER RPC. On its own this usually
   finds nothing — a node that lies about one position still publishes a log
   that checks out end to end.
2. **Paste your own receipts.** The copies the node handed you. The
   contradiction appears here, because the evidence is not in the operator's
   log — it is in the hands of whoever it lied to.
3. **Escalate.** Both signatures are verified in the browser first, the split is
   previewed with the same integer arithmetic the program runs, and the
   transaction carries the ed25519 precompile call immediately before the
   program call.

Receipts are base64, one per line or a JSON array — the same format the
watchtower takes, so `client-receipts.json` can be pasted straight in.
