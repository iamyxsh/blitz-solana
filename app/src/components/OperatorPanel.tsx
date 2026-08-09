import { useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import bs58 from 'bs58'
import type { Conviction, Operator } from '../lib/accounts'
import { EXPLORER, LAMPORTS_PER_SOL } from '../lib/constants'
import { lamports, short, sol } from '../lib/format'
import { splitOf } from '../lib/split'
import * as ix from '../lib/instructions'
import { Empty, Section, Stat } from './Section'
import { SplitBar } from './SplitBar'
import { TxButton } from './TxButton'

/** What a sequencer sees: its own exposure, and what it would lose. */
export function OperatorPanel({ mine, convictions, slot, refresh }: {
  mine: Operator | null
  convictions: Conviction[]
  slot: number
  refresh: () => void
}) {
  const { publicKey } = useWallet()
  const [signingKey, setSigningKey] = useState('')
  const [bond, setBond] = useState('0.1')

  if (!publicKey) return <Section title="Validator"><Empty>Connect a wallet.</Empty></Section>

  if (!mine) {
    return (
      <Section
        title="Register as a sequencer"
        note="One signing key, fixed at registration. Rotation would let an operator disown receipts it had already issued, so to use a new key you register again and post a new bond."
      >
        <div className="form">
          <label>
            <span>Receipt signing key <em>— the validator identity, base58</em></span>
            <input value={signingKey} onChange={(e) => setSigningKey(e.target.value)}
              placeholder="GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB" spellCheck={false} />
          </label>
          <label className="narrow">
            <span>Bond <em>— SOL</em></span>
            <input value={bond} onChange={(e) => setBond(e.target.value)} inputMode="decimal" />
          </label>
          <TxButton
            label="Post bond"
            disabled={!parseKey(signingKey) || toLamports(bond) === 0n}
            build={() => [ix.register(publicKey, parseKey(signingKey)!, toLamports(bond))]}
            onDone={refresh}
          />
        </div>
        <p className="note">
          The bond is held in a program-owned account and the program never delegates it, so it
          cannot be leased into a rollup the accused controls.
        </p>
      </Section>
    )
  }

  const against = convictions.filter((c) => c.operator.equals(mine.address))
  const slashed = against.reduce((sum, c) => sum + c.slashed, 0n)
  const unbonding = mine.unbondAt > 0n
  const withdrawable = unbonding && BigInt(slot) >= mine.unbondAt
  const exposure = splitOf(mine.bond, mine.poolStaked)

  return (
    <>
      <Section title="Your sequencer" note={mine.address.toBase58()}
        right={<a className="lnk" href={EXPLORER(mine.address.toBase58())} target="_blank" rel="noreferrer">explorer</a>}>
        <div className="stats">
          <Stat label="Bond at risk" value={`${sol(mine.bond)} SOL`} sub={`${lamports(mine.bond)} lamports`}
            tone={mine.bond === 0n ? 'danger' : undefined} />
          <Stat label="Coverage staked on you" value={`${sol(mine.poolStaked)} SOL`}
            sub={`by others, earning on your faults`} />
          <Stat label="Convictions" value={against.length}
            sub={slashed > 0n ? `${sol(slashed)} SOL slashed` : 'clean'}
            tone={against.length ? 'danger' : 'ok'} />
          <Stat label="Signing key" value={<span className="mono sm">{short(bs58.encode(mine.signingKey), 6, 6)}</span>}
            sub="every receipt must verify against this" />
        </div>

        {mine.bond > 0n && (
          <>
            <h3>If a fault is proven against you right now</h3>
            <SplitBar split={exposure} total={mine.bond} />
          </>
        )}
      </Section>

      <Section title="Leaving"
        note="The bond keeps standing behind the log for the whole delay. Evidence for anything done before you asked still slashes it — otherwise misbehaving and withdrawing in the same breath costs nothing.">
        <div className="row">
          {!unbonding && (
            <TxButton label="Begin unbond" tone="ghost" disabled={mine.bond === 0n}
              build={() => [ix.beginUnbond(publicKey)]} onDone={refresh} />
          )}
          {unbonding && (
            <>
              <Stat label="Withdrawable at slot" value={mine.unbondAt.toLocaleString()}
                sub={withdrawable ? 'now' : `${(mine.unbondAt - BigInt(slot)).toLocaleString()} slots to go`}
                tone={withdrawable ? 'ok' : 'warn'} />
              <TxButton label="Withdraw bond" tone="ghost" disabled={!withdrawable}
                title={withdrawable ? undefined : 'the timelock has not run'}
                build={() => [ix.withdrawBond(publicKey)]} onDone={refresh} />
            </>
          )}
        </div>
      </Section>
    </>
  )
}

function parseKey(value: string): Uint8Array | null {
  try {
    const bytes = bs58.decode(value.trim())
    return bytes.length === 32 ? bytes : null
  } catch { return null }
}

export function toLamports(value: string): bigint {
  const n = Number(value)
  return Number.isFinite(n) && n > 0 ? BigInt(Math.round(n * Number(LAMPORTS_PER_SOL))) : 0n
}
