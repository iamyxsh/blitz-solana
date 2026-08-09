import { useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { useProgramState } from './hooks/useProgramState'
import { Header } from './components/Header'
import { OperatorPanel } from './components/OperatorPanel'
import { StakerPanel } from './components/StakerPanel'
import { EvidencePanel } from './components/EvidencePanel'
import { ConvictionList } from './components/ConvictionList'
import { Section, Stat } from './components/Section'
import { sol } from './lib/format'

type Tab = 'overview' | 'validator' | 'coverage' | 'escalate'

export default function App() {
  const state = useProgramState()
  const { publicKey } = useWallet()
  const [tab, setTab] = useState<Tab>('overview')

  const staked = state.operators.reduce((sum, o) => sum + o.poolStaked, 0n)
  const bonded = state.operators.reduce((sum, o) => sum + o.bond, 0n)
  const mine = publicKey ? state.positions.filter((p) => p.owner.equals(publicKey)) : []

  const tabs: [Tab, string, string | null][] = [
    ['overview', 'Overview', null],
    ['validator', 'Validator', state.mine ? 'registered' : null],
    ['coverage', 'Coverage', mine.length ? `${mine.length}` : null],
    ['escalate', 'Escalate', state.convictions.length ? `${state.convictions.length}` : null],
  ]

  return (
    <div className="app">
      <Header balance={state.balance} slot={state.slot} />

      <nav className="tabs">
        {tabs.map(([id, label, badge]) => (
          <button key={id} className={tab === id ? 'tab on' : 'tab'} onClick={() => setTab(id)}>
            {label}{badge && <span className="badge">{badge}</span>}
          </button>
        ))}
        <button className="tab ghost" onClick={state.refresh} title="re-read every account">
          {state.loading ? '…' : '↻'}
        </button>
      </nav>

      {state.error && <p className="err">Could not read the chain: {state.error}</p>}

      <main>
        {tab === 'overview' && (
          <>
            <Section title="What this is"
              note="A sequencer promises first-come-first-served. This makes the promise checkable, and gives it a price.">
              <div className="stats">
                <Stat label="Sequencers bonded" value={state.operators.length} sub={`${sol(bonded)} SOL at risk`} />
                <Stat label="Coverage staked" value={`${sol(staked)} SOL`} sub="earning on proven faults" />
                <Stat label="Proven faults" value={state.convictions.length}
                  tone={state.convictions.length ? 'danger' : 'ok'} />
              </div>
              <ol className="how">
                <li><b>A sequencer posts a bond</b> and registers the key it signs receipts with.</li>
                <li><b>Anyone stakes coverage</b> beside it, and earns from faults proven while they are staked.</li>
                <li><b>Every transaction gets a signed receipt</b> naming its arrival position. You keep your copy.</li>
                <li><b>If the operator lies about your position</b>, your copy plus its published log is the proof.</li>
                <li><b>You escalate.</b> The program verifies both signatures, slashes the bond, and splits it —
                  burned, escrowed for you, paid to the stakers.</li>
              </ol>
            </Section>
            <ConvictionList convictions={state.convictions} refresh={state.refresh} />
          </>
        )}

        {tab === 'validator' && (
          <OperatorPanel mine={state.mine} convictions={state.convictions}
            slot={state.slot} refresh={state.refresh} />
        )}

        {tab === 'coverage' && (
          <StakerPanel operators={state.operators} positions={state.positions} refresh={state.refresh} />
        )}

        {tab === 'escalate' && (
          <>
            <EvidencePanel operators={state.operators} refresh={state.refresh} />
            <ConvictionList convictions={state.convictions} refresh={state.refresh} />
          </>
        )}
      </main>
    </div>
  )
}
