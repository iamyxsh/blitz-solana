import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import type { Role } from '../lib/role'

/** The door. Connect, then say which side of the promise you are on. */
export function Login({ connected, onPick }: { connected: boolean; onPick: (role: Role) => void }) {
  return (
    <div className="login">
      <div className="login-hero">
        <h1>Ordering receipts</h1>
        <p>
          A rollup sequencer promises to run transactions in the order they arrive.
          Here that promise is signed, checkable, and backed by a bond you can take.
        </p>
      </div>

      {!connected ? (
        <div className="login-step">
          <span className="step-n">1</span>
          <div>
            <h2>Connect a wallet</h2>
            <p className="note">Devnet. Nothing is signed until you ask for it.</p>
          </div>
          <WalletMultiButton />
        </div>
      ) : (
        <>
          <p className="note login-note">Wallet connected. Which are you?</p>
          <div className="roles">
            <button className="role" onClick={() => onPick('user')}>
              <span className="role-k">Trader</span>
              <b>I send transactions</b>
              <p>
                Every transaction you send comes back with a signed receipt naming its
                position. Keep them, watch for a position that was sold twice, and
                challenge the sequencer if it was.
              </p>
              <span className="role-go">Enter as a trader →</span>
            </button>
            <button className="role" onClick={() => onPick('validator')}>
              <span className="role-k">Sequencer</span>
              <b>I run the rollup</b>
              <p>
                Post a bond against your own ordering, register the key you sign
                receipts with, and see what a proven fault would cost you before it
                happens.
              </p>
              <span className="role-go">Enter as a sequencer →</span>
            </button>
          </div>
          <p className="note dim login-foot">
            You can switch at any time, and both sides can stake coverage.
          </p>
        </>
      )}
    </div>
  )
}
