import { useMemo, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { claimable, type Operator, type Position } from '../lib/accounts'
import { LAMPORTS_PER_SOL } from '../lib/constants'
import { short, sol } from '../lib/format'
import * as ix from '../lib/instructions'
import { Empty, Section, Stat } from './Section'
import { TxButton } from './TxButton'

/** Coverage, from either side of the table. Both roles can stake. */
export function StakeCard({ operators, positions, refresh, blurb }: {
  operators: Operator[]
  positions: Position[]
  refresh: () => void
  blurb: string
}) {
  const { publicKey } = useWallet()
  const [selected, setSelected] = useState('')
  const [amount, setAmount] = useState('0.05')

  const mine = useMemo(
    () => (publicKey ? positions.filter((p) => p.owner.equals(publicKey)) : []),
    [positions, publicKey],
  )
  const target = operators.find((o) => o.address.toBase58() === selected) ?? operators[0] ?? null

  return (
    <Section title="Coverage" note={blurb}>
      {operators.length === 0 ? (
        <Empty>No sequencer has bonded yet, so there is nothing to back.</Empty>
      ) : (
        <div className="form">
          <label>
            <span>Sequencer</span>
            <select value={selected || target?.address.toBase58()} onChange={(e) => setSelected(e.target.value)}>
              {operators.map((o) => (
                <option key={o.address.toBase58()} value={o.address.toBase58()}>
                  {short(o.address.toBase58(), 8, 8)} — bond {sol(o.bond)} SOL · backed by {sol(o.poolStaked)} SOL
                </option>
              ))}
            </select>
          </label>
          <label className="narrow">
            <span>Amount <em>— SOL</em></span>
            <input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" />
          </label>
          <TxButton label="Stake" disabled={!target || !publicKey || toLamports(amount) === 0n}
            build={() => [ix.stake(publicKey!, target!.address, toLamports(amount))]} onDone={refresh} />
        </div>
      )}

      {mine.map((position) => {
        const operator = operators.find((o) => o.address.equals(position.operator))
        const earned = operator ? claimable(position, operator) : position.reward
        return (
          <div key={position.address.toBase58()} className="pos">
            <div className="stats">
              <Stat label="Backing" value={<span className="mono sm">{short(position.operator.toBase58(), 6, 6)}</span>} />
              <Stat label="Staked" value={`${sol(position.staked)} SOL`} />
              <Stat label="Earned" value={`${sol(earned)} SOL`} tone={earned > 0n ? 'ok' : undefined}
                sub={earned > 0n ? 'from faults proven while you were staked' : 'no faults while staked'} />
            </div>
            <div className="row">
              <TxButton label="Claim" tone="ghost" disabled={earned === 0n || !publicKey}
                build={() => [ix.claim(publicKey!, position.operator)]} onDone={refresh} />
              <TxButton label="Unstake all" tone="ghost" disabled={position.staked === 0n || !publicKey}
                build={() => [ix.unstake(publicKey!, position.operator, position.staked)]} onDone={refresh} />
            </div>
          </div>
        )
      })}
    </Section>
  )
}

export function toLamports(value: string): bigint {
  const n = Number(value)
  return Number.isFinite(n) && n > 0 ? BigInt(Math.round(n * Number(LAMPORTS_PER_SOL))) : 0n
}
