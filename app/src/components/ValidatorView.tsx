import { useCallback, useEffect, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import bs58 from 'bs58'
import type { ProgramState } from '../hooks/useProgramState'
import { fetchLog, type ErInfo } from '../lib/erClient'
import { usePoll } from '../hooks/usePoll'
import { EXPLORER } from '../lib/constants'
import { hex, lamports, short, sol } from '../lib/format'
import type { Receipt } from '../lib/receipt'
import { splitOf } from '../lib/split'
import * as ix from '../lib/instructions'
import { Empty, Section, Stat } from './Section'
import { SplitBar } from './SplitBar'
import { TxButton } from './TxButton'
import { RollupBar } from './RollupBar'
import { StakeCard, toLamports } from './StakeCard'

const EMPTY: Receipt[] = []

/** Everything a sequencer needs: its exposure, its log, and the way out. */
export function ValidatorView({ state }: { state: ProgramState }) {
  const { publicKey } = useWallet()
  const [rollup, setRollup] = useState<ErInfo | null>(null)
  const onRollup = useCallback((info: ErInfo | null) => setRollup(info), [])
  const [signingKey, setSigningKey] = useState('')
  const [bond, setBond] = useState('0.1')

  const log = usePoll(() => fetchLog(rollup!.url), 2000, rollup?.url ?? null).value ?? EMPTY

  useEffect(() => {
    if (rollup) setSigningKey((current) => current || rollup.identity)
  }, [rollup?.identity])

  if (!publicKey) return null
  const mine = state.mine
  const against = state.convictions.filter((c) => mine && c.operator.equals(mine.address))
  const unbonding = mine ? mine.unbondAt > 0n : false
  const withdrawable = unbonding && BigInt(state.slot) >= mine!.unbondAt

  return (
    <>
      <RollupBar onChange={onRollup} />

      {!mine ? (
        <Section
          title="Bond against your own ordering"
          note="One signing key, fixed at registration. Rotation would let a sequencer disown receipts it had already issued, so a new key means a new registration and a new bond."
        >
          <div className="form">
            <label>
              <span>Receipt signing key <em>— your validator identity, base58</em></span>
              <input value={signingKey} onChange={(e) => setSigningKey(e.target.value)}
                spellCheck={false} placeholder="filled in when you connect to your rollup above" />
            </label>
            <label className="narrow">
              <span>Bond <em>— SOL</em></span>
              <input value={bond} onChange={(e) => setBond(e.target.value)} inputMode="decimal" />
            </label>
            <TxButton label="Post bond" disabled={!parseKey(signingKey) || toLamports(bond) === 0n}
              build={() => [ix.register(publicKey, parseKey(signingKey)!, toLamports(bond))]}
              onDone={state.refresh} />
          </div>
          <p className="note">
            The bond sits in a program-owned account, and the program has no way to delegate it — so
            it can never be leased into a rollup you control.
          </p>
        </Section>
      ) : (
        <>
          <div className={`verdict ${against.length ? 'bad' : mine.bond === 0n ? 'warn' : 'ok'}`}>
            <b>
              {against.length
                ? `Convicted ${against.length} time${against.length === 1 ? '' : 's'}.`
                : mine.bond === 0n ? 'No bond posted.' : 'Bonded and clean.'}
            </b>
            <p>
              {against.length
                ? `${sol(against.reduce((s, c) => s + c.slashed, 0n))} SOL taken. Every conviction names two receipts you signed.`
                : mine.bond === 0n
                  ? 'Nothing at stake means nothing to prove. Traders have no reason to believe the ordering.'
                  : `${sol(mine.bond)} SOL says your ordering is honest, and ${sol(mine.poolStaked)} SOL of other people’s money agrees.`}
            </p>
          </div>

          <Section title="Your bond" note={mine.address.toBase58()}
            right={<a className="lnk" href={EXPLORER(mine.address.toBase58())} target="_blank" rel="noreferrer">explorer</a>}>
            <div className="stats">
              <Stat label="At risk" value={`${sol(mine.bond)} SOL`} sub={`${lamports(mine.bond)} lamports`}
                tone={mine.bond === 0n ? 'danger' : undefined} />
              <Stat label="Backed by others" value={`${sol(mine.poolStaked)} SOL`}
                sub="earns from your faults" />
              <Stat label="Signing key" value={<span className="mono sm">{short(bs58.encode(mine.signingKey), 5, 5)}</span>}
                sub="every receipt must verify against this" />
              <Stat label="Convictions" value={against.length} tone={against.length ? 'danger' : 'ok'} />
            </div>
            {mine.bond > 0n && (
              <>
                <h3>What one proven fault would cost you</h3>
                <SplitBar split={splitOf(mine.bond, mine.poolStaked)} total={mine.bond} />
              </>
            )}
          </Section>

          <Section title="Leaving"
            note="The bond keeps standing behind your log for the whole delay. Evidence for anything you did before asking still takes it — otherwise misbehaving and withdrawing in the same breath would cost nothing.">
            <div className="row">
              {!unbonding ? (
                <TxButton label="Begin unbond" tone="ghost" disabled={mine.bond === 0n}
                  build={() => [ix.beginUnbond(publicKey)]} onDone={state.refresh} />
              ) : (
                <>
                  <Stat label="Withdrawable at slot" value={mine.unbondAt.toLocaleString()}
                    tone={withdrawable ? 'ok' : 'warn'}
                    sub={withdrawable ? 'now' : `${(mine.unbondAt - BigInt(state.slot)).toLocaleString()} slots to go`} />
                  <TxButton label="Withdraw bond" tone="ghost" disabled={!withdrawable}
                    title={withdrawable ? undefined : 'the timelock has not run'}
                    build={() => [ix.withdrawBond(publicKey)]} onDone={state.refresh} />
                </>
              )}
            </div>
          </Section>
        </>
      )}

      <Section title="What you have signed"
        note="Your published log, as any trader sees it. Dense and gapless: a missing sequence number is itself something to explain."
        right={<span className="dim mono sm">{log.length} receipts</span>}>
        {log.length === 0 ? (
          <Empty>Connect to your rollup above to read its log.</Empty>
        ) : (
          <table className="feed">
            <thead><tr><th>position</th><th>transaction</th><th>arrived</th><th>mode</th></tr></thead>
            <tbody>
              {log.slice(-12).reverse().map((receipt) => (
                <tr key={receipt.receiptHash.toString()}>
                  <td className="mono">#{receipt.seq.toString()}</td>
                  <td className="mono sm">{hex(receipt.txSig, 8)}</td>
                  <td className="dim mono sm">slot {receipt.ingressSlot.toString()}</td>
                  <td className="dim sm">{receipt.modeName}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {log.length > 12 && <p className="note dim">Showing the last 12 of {log.length}.</p>}
      </Section>

      <StakeCard operators={state.operators} positions={state.positions} refresh={state.refresh}
        blurb="Sequencers can stake coverage too — on others, or on themselves. Backing yourself is not a way to soften a slash: half of any bond is burned outright, which is more than the victim and the pool put together." />
    </>
  )
}

function parseKey(value: string): Uint8Array | null {
  try {
    const bytes = bs58.decode(value.trim())
    return bytes.length === 32 ? bytes : null
  } catch { return null }
}
