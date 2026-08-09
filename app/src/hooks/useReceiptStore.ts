import { useCallback, useEffect, useState } from 'react'
import { decodeBase64, decodeReceipt, encodeBase64, receiptBytes, type Receipt } from '../lib/receipt'

export type Kept = { receipt: Receipt; signature: string; at: number; note: string }

type Stored = { r: string; s: string; t: number; n: string }

/**
 * The receipts this wallet was handed, kept locally.
 *
 * Deliberately not fetched from the operator on demand: a node that lies about
 * a position publishes a log that agrees with itself, so the copy that
 * contradicts it has to live somewhere the operator cannot reach.
 */
export function useReceiptStore(owner: string | null) {
  const key = owner ? `mb.receipts.${owner}` : null
  const [kept, setKept] = useState<Kept[]>([])

  useEffect(() => {
    if (!key) return setKept([])
    try {
      const raw = JSON.parse(localStorage.getItem(key) ?? '[]') as Stored[]
      setKept(raw.flatMap((entry) => {
        try {
          return [{ receipt: decodeReceipt(decodeBase64(entry.r)), signature: entry.s, at: entry.t, note: entry.n }]
        } catch { return [] }
      }))
    } catch { setKept([]) }
  }, [key])

  const persist = useCallback((next: Kept[]) => {
    setKept(next)
    if (!key) return
    const raw: Stored[] = next.map((k) => ({
      r: encodeBase64(receiptBytes(k.receipt)), s: k.signature, t: k.at, n: k.note,
    }))
    localStorage.setItem(key, JSON.stringify(raw))
  }, [key])

  const add = useCallback((entries: Kept[]) => {
    setKept((current) => {
      const seen = new Set(current.map((k) => k.receipt.receiptHash.toString()))
      const fresh = entries.filter((e) => !seen.has(e.receipt.receiptHash.toString()))
      const next = [...current, ...fresh].sort((a, b) => Number(a.receipt.seq - b.receipt.seq))
      if (key) {
        localStorage.setItem(key, JSON.stringify(next.map((k) => ({
          r: encodeBase64(receiptBytes(k.receipt)), s: k.signature, t: k.at, n: k.note,
        }))))
      }
      return next
    })
  }, [key])

  const clear = useCallback(() => persist([]), [persist])

  return { kept, add, clear }
}
