import { useWallet } from '@solana/wallet-adapter-react'
import type { Operator } from '../lib/accounts'
import { convictionPda } from '../lib/addresses'
import { type Contradiction, ordered } from '../lib/evidence'
import { hex, short, sol } from '../lib/format'
import { splitOf } from '../lib/split'
import * as ix from '../lib/instructions'
import { Section } from './Section'
import { SplitBar } from './SplitBar'
import { TxButton } from './TxButton'

/**
 * One position, promised to two different transactions.
 *
 * Everything shown here was checked in this browser first. Evidence a client
 * cannot verify itself is evidence the program is about to reject, and the only
 * difference is who pays the fee.
 */
export function Challenge({ pair, operator, signingKey, refresh }: {
  pair: Contradiction
  operator: Operator | null
  signingKey: Uint8Array
  refresh: () => void
}) {
  const { publicKey } = useWallet()
  const [low, high] = ordered(pair)
  const preview = operator ? splitOf(operator.bond, operator.poolStaked) : null

  return (
    <Section
      title={`Position ${pair.seq.toString()} was sold twice`}
      note="The sequencer signed two different statements about one place in the queue. Both verify under its own key, and both name the same run of its log."
    >
      <div className="pair">
        <div className="pair-side">
          <span className="pair-k">it told one party</span>
          <b className="mono sm">{hex(low.txSig, 10)}</b>
          <span className="dim mono sm">receipt {hex(low.receiptHash, 8)}</span>
        </div>
        <div className="pair-vs">vs</div>
        <div className="pair-side">
          <span className="pair-k">and another</span>
          <b className="mono sm">{hex(high.txSig, 10)}</b>
          <span className="dim mono sm">receipt {hex(high.receiptHash, 8)}</span>
        </div>
      </div>

      {!operator ? (
        <p className="warn-note">
          This sequencer has not bonded on chain, so there is nothing to take. The evidence stands —
          it just has nowhere to go yet.
        </p>
      ) : operator.bond === 0n ? (
        <p className="warn-note">This sequencer’s bond is already gone. It has been slashed before.</p>
      ) : (
        <>
          <h3>What challenging does</h3>
          <SplitBar split={preview!} total={operator.bond} />
          <p className="note">
            The victim share is held at{' '}
            <span className="mono sm">{short(convictionPda(operator.address, pair).toBase58(), 6, 6)}</span>{' '}
            until someone produces the transaction that was displaced. That account is addressed by
            the contradiction itself, so this evidence can only ever be spent once.
          </p>
          <TxButton
            label={`Challenge — take ${sol(operator.bond)} SOL`}
            tone="danger"
            disabled={!publicKey}
            build={() => ix.proveEquivocation(publicKey!, operator.address, signingKey, pair)}
            onDone={refresh}
          />
          <p className="note dim">
            Two instructions in one transaction: Solana’s signature precompile verifies both receipts
            under the key the sequencer registered, then the program reads back exactly what was
            verified and compares them. Nothing may sit between the two.
          </p>
        </>
      )}
    </Section>
  )
}
