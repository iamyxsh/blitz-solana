import { useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { SystemProgram, Transaction } from '@solana/web3.js'
import type { Kept } from '../hooks/useReceiptStore'
import type { ErInfo } from '../lib/erClient'
import { latestBlockhash, sendAndCollect } from '../lib/erClient'
import type { Contradiction } from '../lib/evidence'
import { compareBytes, hex, short } from '../lib/format'
import { decodeBase64, decodeReceipt } from '../lib/receipt'
import { Empty, Section } from './Section'

/**
 * What you sent, and what the sequencer promised you for it.
 *
 * A receipt arrives in the send response, not from a lookup afterwards. That is
 * the difference that matters: your copy is yours, and it is the half of the
 * evidence an operator cannot quietly revise.
 */
export function TransactionFeed({ kept, rollup, disputed, add, clear }: {
  kept: Kept[]
  rollup: ErInfo | null
  disputed: Contradiction[]
  add: (entries: Kept[]) => void
  clear: () => void
}) {
  const { publicKey, signTransaction } = useWallet()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const [pasted, setPasted] = useState('')

  const displaced = (entry: Kept) =>
    disputed.some((pair) =>
      compareBytes(pair.a.receiptHash, entry.receipt.receiptHash) === 0 ||
      compareBytes(pair.b.receiptHash, entry.receipt.receiptHash) === 0)

  async function send() {
    if (!publicKey || !signTransaction || !rollup) return
    setBusy(true); setError(null)
    try {
      const transaction = new Transaction().add(
        SystemProgram.transfer({ fromPubkey: publicKey, toPubkey: publicKey, lamports: 0 }),
      )
      transaction.feePayer = publicKey
      transaction.recentBlockhash = await latestBlockhash(rollup.url)
      const signed = await signTransaction(transaction)
      const { signature, receipt } = await sendAndCollect(rollup.url, signed.serialize())
      if (!receipt) throw new Error('the rollup accepted it but returned no receipt')
      add([{ receipt, signature, at: Date.now(), note: 'sent from here' }])
    } catch (caught) {
      setError((caught as Error).message.slice(0, 200))
    } finally { setBusy(false) }
  }

  function importPasted() {
    const entries = pasted.trim()
    if (!entries) return
    let list: string[]
    try {
      const parsed = JSON.parse(entries)
      list = Array.isArray(parsed) ? parsed.map(String) : [String(parsed)]
    } catch { list = entries.split(/[\s,"'\[\]]+/).filter(Boolean) }

    const kept: Kept[] = []
    for (const item of list) {
      try {
        kept.push({ receipt: decodeReceipt(decodeBase64(item)), signature: '', at: Date.now(), note: 'imported' })
      } catch { /* not a receipt */ }
    }
    add(kept)
    setPasted(''); setImporting(false)
  }

  return (
    <Section
      title="Your transactions"
      note="Each one came back with a signed receipt naming the position it was promised. This is your copy."
      right={
        <div className="row tight">
          <button className="btn btn-primary sm" onClick={send} disabled={busy || !rollup || !signTransaction}
            title={rollup ? undefined : 'connect to a rollup first'}>
            {busy ? 'sending…' : 'Send a transaction'}
          </button>
          <button className="btn btn-ghost sm" onClick={() => setImporting(!importing)}>Import</button>
        </div>
      }
    >
      {error && <p className="warn-note">Could not send: {error}</p>}

      {importing && (
        <div className="importbox">
          <p className="note">
            Receipts you were handed elsewhere — base64, one per line or a JSON array.
            The same format the watchtower reads, so <code>client-receipts.json</code> pastes straight in.
          </p>
          <textarea rows={3} spellCheck={false} value={pasted} onChange={(e) => setPasted(e.target.value)}
            placeholder="TUJSRUNFSVBUX1YxAAAA…" />
          <div className="row">
            <button className="btn btn-primary sm" onClick={importPasted} disabled={!pasted.trim()}>Add</button>
            <button className="btn btn-ghost sm" onClick={() => setImporting(false)}>Cancel</button>
          </div>
        </div>
      )}

      {kept.length === 0 ? (
        <Empty>
          Nothing yet. Send one through the rollup above, or import receipts you already hold.
        </Empty>
      ) : (
        <table className="feed">
          <thead>
            <tr><th>position</th><th>transaction</th><th>arrived</th><th>mode</th><th>status</th></tr>
          </thead>
          <tbody>
            {kept.map((entry) => {
              const bad = displaced(entry)
              return (
                <tr key={entry.receipt.receiptHash.toString()} className={bad ? 'bad' : ''}>
                  <td className="mono">#{entry.receipt.seq.toString()}</td>
                  <td className="mono sm">
                    {entry.signature ? short(entry.signature, 6, 6) : hex(entry.receipt.txSig, 8)}
                  </td>
                  <td className="dim mono sm">slot {entry.receipt.ingressSlot.toString()}</td>
                  <td className="dim sm">{entry.receipt.modeName}</td>
                  <td>
                    {bad
                      ? <span className="tag tag-bad">position sold twice</span>
                      : <span className="tag tag-ok">promise intact</span>}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}

      {kept.length > 0 && (
        <p className="note dim">
          Kept in this browser, under your wallet address.{' '}
          <button className="linkbtn" onClick={clear}>forget them</button>
        </p>
      )}
    </Section>
  )
}
