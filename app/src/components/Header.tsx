import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { CLUSTER, EXPLORER, PROGRAM_ID } from '../lib/constants'
import { short, sol } from '../lib/format'

export function Header({ balance, slot }: { balance: bigint; slot: number }) {
  return (
    <header className="top">
      <div className="top-l">
        <h1>Ordering receipts</h1>
        <p>Sequencer coverage and slashing · <span className="pill">{CLUSTER}</span></p>
      </div>
      <div className="top-r">
        <a className="prog" href={EXPLORER(PROGRAM_ID.toBase58())} target="_blank" rel="noreferrer">
          program {short(PROGRAM_ID.toBase58(), 6, 6)}
        </a>
        {slot > 0 && <span className="dim mono">slot {slot.toLocaleString()}</span>}
        {balance > 0n && <span className="dim mono">{sol(balance)} SOL</span>}
        <WalletMultiButton />
      </div>
    </header>
  )
}
