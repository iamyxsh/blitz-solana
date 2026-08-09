#!/usr/bin/env bash
# Starts an ephemeral rollup that stamps ordering receipts.
#
#   ./run/rollup.sh honest    — behaves
#   ./run/rollup.sh mev       — publishes a receipt different from the one it
#                               hands the client, which is equivocation
#
# Both run at once. They share a signing key on purpose, so one on-chain
# registration covers either, and they still cannot be confused for each other:
# a log id is minted per run, so entries from one are foreign to the other
# rather than contradictions.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-honest}"
BIN=magicblock-validator/target/release/magicblock-validator
KEY="${MB_IDENTITY:?set MB_IDENTITY to the validator identity, base58 secret key}"

case "$MODE" in
  honest) PORT=8799; METRICS=9799; STORE=run/.honest; ATTACK="" ;;
  mev)    PORT=9899; METRICS=9901; STORE=run/.mev;    ATTACK="equivocate" ;;
  *) echo "usage: $0 [honest|mev]" >&2; exit 2 ;;
esac

echo "rollup: $MODE · rpc 127.0.0.1:$PORT · storage $STORE"
[ -n "$ATTACK" ] && echo "  ATTACK MODE — this node will publish receipts that differ from the ones it returns"

exec env ${ATTACK:+MB_ATTACK=$ATTACK} MBV_METRICS__ADDRESS="127.0.0.1:$METRICS" "$BIN" \
  --lifecycle offline \
  --keypair "$KEY" \
  --listen "127.0.0.1:$PORT" \
  --storage "$STORE" \
  --reset
