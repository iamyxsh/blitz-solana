import { compareBytes } from './format'
import { type Receipt, verifyReceipt } from './receipt'

export type Contradiction = {
  seq: bigint
  logId: Uint8Array
  a: Receipt
  b: Receipt
}

export type ScanResult = {
  contradictions: Contradiction[]
  examined: number
  /** Set aside, never held against anybody. */
  unverifiable: number
  foreignLog: number
}

/**
 * Finds positions the operator made two different statements about.
 *
 * Mirrors the watchtower, including the parts that exist to avoid accusing
 * honest nodes: an entry that does not verify is attributable to nobody, and
 * an entry from another run of the log occupies the same position without
 * contradicting anything — the sequence counter restarts when the node does
 * and the signing key does not.
 */
export function scan(
  receipts: Receipt[],
  operatorKey: Uint8Array,
  logId: Uint8Array | null,
): ScanResult {
  const result: ScanResult = { contradictions: [], examined: receipts.length, unverifiable: 0, foreignLog: 0 }

  const attributable = receipts.filter((receipt) => {
    if (logId && compareBytes(receipt.logId, logId) !== 0) {
      result.foreignLog++
      return false
    }
    if (!verifyReceipt(receipt, operatorKey)) {
      result.unverifiable++
      return false
    }
    return true
  })

  const bySeq = new Map<string, Receipt[]>()
  for (const receipt of attributable) {
    const key = receipt.seq.toString()
    const group = bySeq.get(key)
    if (group) group.push(receipt)
    else bySeq.set(key, [receipt])
  }

  for (const group of bySeq.values()) {
    const first = group[0]
    // Byte-identical re-delivery is a reconnect overlapping a backfill, not a
    // contradiction.
    const other = group.find((candidate) => compareBytes(candidate.receiptHash, first.receiptHash) !== 0)
    if (other) {
      result.contradictions.push({ seq: first.seq, logId: first.logId, a: first, b: other })
    }
  }

  result.contradictions.sort((x, y) => (x.seq < y.seq ? -1 : x.seq > y.seq ? 1 : 0))
  return result
}

/** The canonical ordering the conviction address is derived from. */
export function ordered(pair: Contradiction): [Receipt, Receipt] {
  return compareBytes(pair.a.message, pair.b.message) <= 0 ? [pair.a, pair.b] : [pair.b, pair.a]
}
