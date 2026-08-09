import { useEffect, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { useProgramState } from './hooks/useProgramState'
import { Login } from './components/Login'
import { UserView } from './components/UserView'
import { ValidatorView } from './components/ValidatorView'
import { CLUSTER, EXPLORER, PROGRAM_ID } from './lib/constants'
import { readRole, type Role, writeRole } from './lib/role'
import { short, sol } from './lib/format'

export default function App() {
  const state = useProgramState()
  const { connected } = useWallet()
  const [role, setRole] = useState<Role | null>(readRole)

  useEffect(() => { writeRole(role) }, [role])

  function pick(next: Role | null) { setRole(next) }

  if (!connected || !role) {
    return <div className="app"><Login connected={connected} onPick={pick} /></div>
  }

  return (
    <div className="app">
      <header className="top">
        <div className="top-l">
          <h1>Ordering receipts</h1>
          <p>
            <span className={`chip chip-${role}`}>{role === 'user' ? 'Trader' : 'Sequencer'}</span>
            <button className="linkbtn" onClick={() => pick(null)}>switch</button>
          </p>
        </div>
        <div className="top-r">
          <a className="prog" href={EXPLORER(PROGRAM_ID.toBase58())} target="_blank" rel="noreferrer">
            {short(PROGRAM_ID.toBase58(), 5, 5)}
          </a>
          <span className="pill">{CLUSTER}</span>
          {state.balance > 0n && <span className="dim mono">{sol(state.balance, 3)} SOL</span>}
          <button className="btn btn-ghost sm" onClick={state.refresh} title="re-read every account">
            {state.loading ? '…' : '↻'}
          </button>
          <WalletMultiButton />
        </div>
      </header>

      {state.error && <p className="err">Could not read the chain: {state.error}</p>}

      <main>
        {role === 'user' ? <UserView state={state} /> : <ValidatorView state={state} />}
      </main>
    </div>
  )
}
