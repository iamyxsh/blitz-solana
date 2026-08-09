import { useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import type { Conviction } from '../lib/accounts'
import { EXPLORER } from '../lib/constants'
import { hex, short, sol } from '../lib/format'
import { decodeBase64 } from '../lib/receipt'
import * as ix from '../lib/instructions'
import { Empty, Section, Stat } from './Section'
import { TxButton } from './TxButton'

export function ConvictionList({ convictions, refresh }: {
  convictions: Conviction[]
  refresh: () => void
}) {
  const slashed = convictions.reduce((sum, c) => sum + c.slashed, 0n)
  const owed = convictions.reduce((sum, c) => sum + c.owedToVictim, 0n)

  return (
    <Section title="Proven faults"
      note="Every one of these is a sequencer that signed two contradictory statements about one position, checked on chain by the program rather than asserted by anyone.">
      <div className="stats">
        <Stat label="Convictions" value={convictions.length} />
        <Stat label="Slashed in total" value={`${sol(slashed)} SOL`} tone={slashed > 0n ? 'danger' : undefined} />
        <Stat label="Escrow unclaimed" value={`${sol(owed)} SOL`} sub="waiting for the wronged transaction" />
      </div>
      {convictions.length === 0
        ? <Empty>No sequencer has been convicted.</Empty>
        : convictions.map((c) => <Row key={c.address.toBase58()} conviction={c} refresh={refresh} />)}
    </Section>
  )
}

function Row({ conviction, refresh }: { conviction: Conviction; refresh: () => void }) {
  const { publicKey } = useWallet()
  const [wire, setWire] = useState('')
  const bytes = tryDecode(wire)

  return (
    <div className="conv">
      <div className="conv-hd">
        <a className="mono sm lnk" href={EXPLORER(conviction.address.toBase58())} target="_blank" rel="noreferrer">
          {short(conviction.address.toBase58(), 8, 8)}
        </a>
        <span className="dim">slot {conviction.slot.toLocaleString()}</span>
        <b className="danger">−{sol(conviction.slashed)} SOL</b>
      </div>
      <div className="conv-bd">
        <span className="dim">sequencer </span>
        <a className="mono sm lnk" href={EXPLORER(conviction.operator.toBase58())} target="_blank" rel="noreferrer">
          {short(conviction.operator.toBase58(), 6, 6)}
        </a>
        <span className="dim"> · lied to transaction </span>
        <span className="mono sm">{hex(conviction.wronged, 10)}</span>
      </div>

      {conviction.owedToVictim > 0n && (
        <div className="claimbox">
          <p className="note">
            <b>{sol(conviction.owedToVictim)} SOL</b> is escrowed here. A receipt names a transaction
            signature, not an address, so it is paid to whoever produces bytes hashing to what the
            operator committed to — and signs as their fee payer.
          </p>
          <textarea rows={2} spellCheck={false} value={wire} onChange={(e) => setWire(e.target.value)}
            placeholder="the wronged transaction, base64" />
          <TxButton label="Claim as the victim" disabled={!bytes || !publicKey}
            build={() => [ix.claimVictim(publicKey!, conviction.address, bytes!)]} onDone={refresh} />
        </div>
      )}
    </div>
  )
}

function tryDecode(value: string): Uint8Array | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  try { const b = decodeBase64(trimmed); return b.length > 64 ? b : null } catch { return null }
}
