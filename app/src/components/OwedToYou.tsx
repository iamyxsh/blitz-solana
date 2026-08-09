import { useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import type { Conviction } from '../lib/accounts'
import { EXPLORER } from '../lib/constants'
import { hex, short, sol } from '../lib/format'
import { decodeBase64 } from '../lib/receipt'
import * as ix from '../lib/instructions'
import { Empty, Section } from './Section'
import { TxButton } from './TxButton'

/**
 * Compensation waiting to be collected.
 *
 * A receipt names a transaction signature, not an address, so cold evidence has
 * no wallet to pay. It is released to whoever can produce bytes hashing to what
 * the sequencer committed to — and signs as the account that paid for them.
 */
export function OwedToYou({ convictions, refresh }: { convictions: Conviction[]; refresh: () => void }) {
  const open = convictions.filter((c) => c.owedToVictim > 0n)
  if (open.length === 0) {
    return (
      <Section title="Compensation" note="Paid out of a slashed bond, to the trader whose position was sold twice.">
        <Empty>Nothing outstanding.</Empty>
      </Section>
    )
  }
  return (
    <Section title="Compensation waiting to be claimed"
      note="Produce the transaction that was displaced and the escrow is yours.">
      {open.map((c) => <Claim key={c.address.toBase58()} conviction={c} refresh={refresh} />)}
    </Section>
  )
}

function Claim({ conviction, refresh }: { conviction: Conviction; refresh: () => void }) {
  const { publicKey } = useWallet()
  const [wire, setWire] = useState('')
  const bytes = decode(wire)

  return (
    <div className="pos">
      <div className="conv-hd">
        <b className="ok">{sol(conviction.owedToVictim)} SOL</b>
        <span className="dim">for transaction <span className="mono sm">{hex(conviction.wronged, 10)}</span></span>
        <a className="lnk" style={{ marginLeft: 'auto' }} href={EXPLORER(conviction.address.toBase58())}
          target="_blank" rel="noreferrer">{short(conviction.address.toBase58(), 5, 5)}</a>
      </div>
      <textarea rows={2} spellCheck={false} value={wire} onChange={(e) => setWire(e.target.value)}
        placeholder="the displaced transaction, base64" />
      <div className="row">
        <TxButton label="Claim compensation" disabled={!bytes || !publicKey}
          build={() => [ix.claimVictim(publicKey!, conviction.address, bytes!)]} onDone={refresh} />
      </div>
    </div>
  )
}

function decode(value: string): Uint8Array | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  try { const b = decodeBase64(trimmed); return b.length > 64 ? b : null } catch { return null }
}
