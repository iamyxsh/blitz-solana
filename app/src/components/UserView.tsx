import { useEffect, useMemo, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import bs58 from 'bs58'
import type { ProgramState } from '../hooks/useProgramState'
import { useReceiptStore } from '../hooks/useReceiptStore'
import { fetchLog, type ErInfo } from '../lib/erClient'
import { scan } from '../lib/evidence'
import { compareBytes } from '../lib/format'
import type { Receipt } from '../lib/receipt'
import { RollupBar } from './RollupBar'
import { TransactionFeed } from './TransactionFeed'
import { Challenge } from './Challenge'
import { StakeCard } from './StakeCard'
import { OwedToYou } from './OwedToYou'
import { Section } from './Section'

/** Everything a trader needs, in the order they need it. */
export function UserView({ state }: { state: ProgramState }) {
  const { publicKey } = useWallet()
  const owner = publicKey?.toBase58() ?? null
  const { kept, add, clear } = useReceiptStore(owner)
  const [rollup, setRollup] = useState<ErInfo | null>(null)
  const [log, setLog] = useState<Receipt[]>([])

  useEffect(() => {
    if (!rollup) return setLog([])
    fetchLog(rollup.url).then(setLog).catch(() => setLog([]))
  }, [rollup])

  const signingKey = useMemo(() => {
    if (!rollup) return null
    try {
      const bytes = bs58.decode(rollup.identity)
      return bytes.length === 32 ? bytes : null
    } catch { return null }
  }, [rollup])

  // Your copies plus the operator's published log. Neither alone is enough:
  // a node that lies about one position publishes a log that agrees with itself.
  const result = useMemo(() => {
    if (!signingKey) return null
    const mine = kept.map((k) => k.receipt)
    const all = [...log, ...mine]
    if (all.length === 0) return null
    return scan(all, signingKey, all[0].logId)
  }, [log, kept, signingKey])

  const mineHashes = useMemo(() => kept.map((k) => k.receipt.receiptHash), [kept])
  const yours = (result?.contradictions ?? []).filter((pair) =>
    mineHashes.some((h) =>
      compareBytes(h, pair.a.receiptHash) === 0 || compareBytes(h, pair.b.receiptHash) === 0))
  const others = (result?.contradictions ?? []).filter((pair) => !yours.includes(pair))

  const operator = useMemo(
    () => state.operators.find((o) => rollup && bs58.encode(o.signingKey) === rollup.identity) ?? null,
    [state.operators, rollup],
  )

  return (
    <>
      <RollupBar onChange={setRollup} />

      <Verdict yours={yours.length} others={others.length} watched={kept.length} rollup={!!rollup} />

      {yours.map((pair) => (
        <Challenge key={`mine-${pair.seq}`} pair={pair} operator={operator}
          signingKey={signingKey!} refresh={state.refresh} />
      ))}

      <TransactionFeed kept={kept} rollup={rollup} disputed={result?.contradictions ?? []}
        add={add} clear={clear} />

      {others.length > 0 && (
        <Section title="Faults against other traders"
          note="Found in the same log. Not yours to claim from, but they are the same sequencer.">
          {others.map((pair) => (
            <Challenge key={`other-${pair.seq}`} pair={pair} operator={operator}
              signingKey={signingKey!} refresh={state.refresh} />
          ))}
        </Section>
      )}

      <OwedToYou convictions={state.convictions} refresh={state.refresh} />

      <StakeCard operators={state.operators} positions={state.positions} refresh={state.refresh}
        blurb="Back a sequencer you use. If it is ever caught, you take a share of what it loses — but only for faults proven while your stake was open." />
    </>
  )
}

function Verdict({ yours, others, watched, rollup }: {
  yours: number; others: number; watched: number; rollup: boolean
}) {
  if (yours > 0) {
    return (
      <div className="verdict bad">
        <b>The sequencer sold your position twice.</b>
        <p>
          {yours === 1 ? 'One of your transactions was' : `${yours} of your transactions were`} promised a
          place in the queue that was promised to somebody else as well. It signed both. You can take
          its bond.
        </p>
      </div>
    )
  }
  if (others > 0) {
    return (
      <div className="verdict warn">
        <b>This sequencer has broken its promise to someone.</b>
        <p>{others} contradiction{others === 1 ? '' : 's'} in its log, none involving your transactions.</p>
      </div>
    )
  }
  if (!rollup) {
    return (
      <div className="verdict idle">
        <b>Not watching a rollup yet.</b>
        <p>Point the bar above at your rollup’s RPC, or import receipts you already hold.</p>
      </div>
    )
  }
  return (
    <div className="verdict ok">
      <b>Your positions are intact.</b>
      <p>
        {watched === 0
          ? 'No receipts of yours yet. Send a transaction below and one will arrive with it.'
          : `${watched} receipt${watched === 1 ? '' : 's'} of yours checked against the sequencer’s own log. Nothing contradicts.`}
      </p>
    </div>
  )
}
