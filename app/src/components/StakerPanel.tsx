import { useMemo, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import type { Operator, Position } from '../lib/accounts'
import { claimable } from '../lib/accounts'
import { EXPLORER } from '../lib/constants'
import { lamports, short, sol } from '../lib/format'
import * as ix from '../lib/instructions'
import { Empty, Section, Stat } from './Section'
import { TxButton } from './TxButton'
import { toLamports } from './OperatorPanel'

/** What a coverage staker sees: who they back, and what it has earned. */
export function StakerPanel({ operators, positions, refresh }: {
  operators: Operator[]
  positions: Position[]
  refresh: () => void
}) {
  const { publicKey } = useWallet()
  const [selected, setSelected] = useState<string>('')
  const [amount, setAmount] = useState('0.05')

  const mine = useMemo(
    () => (publicKey ? positions.filter((p) => p.owner.equals(publicKey)) : []),
    [positions, publicKey],
  )
  const operatorAt = (address: string) => operators.find((o) => o.address.toBase58() === address)
  const target = operatorAt(selected) ?? operators[0]

  if (!publicKey) return <Section title="Coverage"><Empty>Connect a wallet.</Empty></Section>

  return (
    <>
      <Section
        title="Stake coverage"
        note="You are buying a claim on faults that happen while you are staked. Staking after a fault earns nothing from it — otherwise anyone watching for an evidence transaction could stake in front of it."
      >
        {operators.length === 0 ? (
          <Empty>No sequencer has registered yet.</Empty>
        ) : (
          <div className="form">
            <label>
              <span>Sequencer</span>
              <select value={selected || operators[0]?.address.toBase58()} onChange={(e) => setSelected(e.target.value)}>
                {operators.map((o) => (
                  <option key={o.address.toBase58()} value={o.address.toBase58()}>
                    {short(o.address.toBase58(), 8, 8)} — bond {sol(o.bond)} SOL · staked {sol(o.poolStaked)} SOL
                  </option>
                ))}
              </select>
            </label>
            <label className="narrow">
              <span>Amount <em>— SOL</em></span>
              <input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" />
            </label>
            <TxButton label="Stake" disabled={!target || toLamports(amount) === 0n}
              build={() => [ix.stake(publicKey, target!.address, toLamports(amount))]} onDone={refresh} />
          </div>
        )}
      </Section>

      <Section title="Your positions">
        {mine.length === 0 ? (
          <Empty>Nothing staked yet.</Empty>
        ) : (
          mine.map((position) => {
            const operator = operatorAt(position.operator.toBase58())
            const earned = operator ? claimable(position, operator) : position.reward
            return (
              <div key={position.address.toBase58()} className="pos">
                <div className="stats">
                  <Stat label="Backing" value={<span className="mono sm">{short(position.operator.toBase58(), 6, 6)}</span>}
                    sub={<a className="lnk" href={EXPLORER(position.operator.toBase58())} target="_blank" rel="noreferrer">explorer</a>} />
                  <Stat label="Staked" value={`${sol(position.staked)} SOL`} sub={`${lamports(position.staked)} lamports`} />
                  <Stat label="Earned" value={`${sol(earned)} SOL`} tone={earned > 0n ? 'ok' : undefined}
                    sub={earned > 0n ? 'from proven faults while you were staked' : 'no faults while staked'} />
                </div>
                <div className="row">
                  <TxButton label="Claim rewards" tone="ghost" disabled={earned === 0n}
                    build={() => [ix.claim(publicKey, position.operator)]} onDone={refresh} />
                  <TxButton label="Unstake all" tone="ghost" disabled={position.staked === 0n}
                    build={() => [ix.unstake(publicKey, position.operator, position.staked)]} onDone={refresh} />
                </div>
              </div>
            )
          })
        )}
      </Section>
    </>
  )
}
