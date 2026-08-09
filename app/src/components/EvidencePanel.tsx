import { useMemo, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import bs58 from 'bs58'
import type { Operator } from '../lib/accounts'
import { convictionPda } from '../lib/addresses'
import { EXPLORER } from '../lib/constants'
import { type Contradiction, ordered, scan } from '../lib/evidence'
import { hex, lamports, short, sol } from '../lib/format'
import { fetchReceipts, operatorIdentity } from '../lib/erRpc'
import { decodeBase64, decodeReceipt, type Receipt } from '../lib/receipt'
import { splitOf } from '../lib/split'
import * as ix from '../lib/instructions'
import { Empty, Section, Stat } from './Section'
import { SplitBar } from './SplitBar'
import { TxButton } from './TxButton'

/**
 * Where a trader turns a receipt into a slashing.
 *
 * The operator's own log is not enough and is not meant to be: a node that
 * lies about a position publishes a log that is perfectly self-consistent. The
 * contradiction only appears once the copy it handed the client is added, which
 * is why the second box exists.
 */
export function EvidencePanel({ operators, refresh }: { operators: Operator[]; refresh: () => void }) {
  const { publicKey } = useWallet()
  const [erUrl, setErUrl] = useState('http://127.0.0.1:8899')
  const [pasted, setPasted] = useState('')
  const [logReceipts, setLogReceipts] = useState<Receipt[]>([])
  const [identity, setIdentity] = useState<string>('')
  const [status, setStatus] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const myReceipts = useMemo(() => parsePasted(pasted), [pasted])
  const signingKey = useMemo(() => tryDecodeKey(identity), [identity])

  const operator = useMemo(
    () => operators.find((o) => signingKey && bs58.encode(o.signingKey) === identity.trim()) ?? null,
    [operators, identity, signingKey],
  )

  const result = useMemo(() => {
    if (!signingKey) return null
    const all = [...logReceipts, ...myReceipts.receipts]
    if (all.length === 0) return null
    // Follow one run of the log. Entries from an earlier run occupy the same
    // positions without contradicting them, because the sequence counter
    // restarts when the node does and the signing key does not.
    return scan(all, signingKey, all[0].logId)
  }, [logReceipts, myReceipts.receipts, signingKey])

  async function pull() {
    setBusy(true); setStatus(null)
    try {
      const who = await operatorIdentity(erUrl)
      setIdentity(who)
      const receipts = await fetchReceipts(erUrl)
      setLogReceipts(receipts)
      setStatus(`${receipts.length} receipts from the node’s published log · identity ${short(who, 6, 6)}`)
    } catch (error) {
      setStatus(`could not reach ${erUrl}: ${(error as Error).message}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Section title="1 · The operator’s published log"
        note="Optional. A node that lies about one position still publishes a log that checks out end to end, so this on its own will usually find nothing.">
        <div className="form">
          <label>
            <span>Ephemeral rollup RPC</span>
            <input value={erUrl} onChange={(e) => setErUrl(e.target.value)} spellCheck={false} />
          </label>
          <button className="btn btn-ghost" onClick={pull} disabled={busy}>
            {busy ? 'reading…' : 'Read the log'}
          </button>
        </div>
        {status && <p className="note">{status}</p>}
      </Section>

      <Section title="2 · Your own receipts"
        note="The copies the node handed you when you sent transactions. Base64, one per line, or a JSON array — the same format the watchtower takes."
        right={<span className="dim mono sm">{myReceipts.receipts.length} parsed{myReceipts.rejected ? ` · ${myReceipts.rejected} rejected` : ''}</span>}>
        <textarea rows={4} spellCheck={false} value={pasted} onChange={(e) => setPasted(e.target.value)}
          placeholder="TUJSRUNFSVBUX1YxAAAA… " />
        <label className="inline">
          <span>Signing key to check against <em>— base58, filled in by step 1</em></span>
          <input value={identity} onChange={(e) => setIdentity(e.target.value)} spellCheck={false}
            placeholder="GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB" />
        </label>
      </Section>

      <Section title="3 · What the evidence says"
        note="Checked here, in your browser, before anything is signed. Evidence a client cannot verify itself is evidence the program is about to reject — the only difference is who pays the fee.">
        {!result ? (
          <Empty>Add receipts and a signing key above.</Empty>
        ) : (
          <>
            <div className="stats">
              <Stat label="Examined" value={result.examined} sub="receipts across both sources" />
              <Stat label="Set aside" value={result.unverifiable + result.foreignLog}
                sub={`${result.unverifiable} unsigned · ${result.foreignLog} from another run`}
                tone={result.unverifiable + result.foreignLog > 0 ? 'warn' : undefined} />
              <Stat label="Contradictions" value={result.contradictions.length}
                tone={result.contradictions.length ? 'danger' : 'ok'}
                sub={result.contradictions.length ? 'the operator signed both' : 'nothing to escalate'} />
            </div>
            {result.contradictions.length === 0 && (
              <p className="note">
                Silence is the ordinary result. An entry that fails its signature is attributable to
                nobody and one from another run of the log is not a contradiction, so neither is held
                against the operator.
              </p>
            )}
            {result.contradictions.map((pair) => (
              <Escalation key={pair.seq.toString()} pair={pair} operator={operator}
                signingKey={signingKey!} accuser={publicKey?.toBase58() ?? null} refresh={refresh} />
            ))}
          </>
        )}
      </Section>
    </>
  )
}

function Escalation({ pair, operator, signingKey, accuser, refresh }: {
  pair: Contradiction
  operator: Operator | null
  signingKey: Uint8Array
  accuser: string | null
  refresh: () => void
}) {
  const { publicKey } = useWallet()
  const [low, high] = ordered(pair)
  const conviction = operator ? convictionPda(operator.address, pair) : null
  const preview = operator ? splitOf(operator.bond, operator.poolStaked) : null

  return (
    <div className="evi">
      <div className="evi-hd">
        <b>Two receipts at position {pair.seq.toString()}</b>
        <span className="dim mono sm">log {hex(pair.logId, 6)}</span>
      </div>
      <table className="evi-t">
        <thead>
          <tr><th /><th>receipt hash</th><th>mode</th><th>names transaction</th></tr>
        </thead>
        <tbody>
          <tr><td className="dim">a</td><td className="mono sm">{hex(low.receiptHash, 12)}</td>
            <td className="dim">{low.modeName}</td><td className="mono sm">{hex(low.txSig, 10)}</td></tr>
          <tr><td className="dim">b</td><td className="mono sm">{hex(high.receiptHash, 12)}</td>
            <td className="dim">{high.modeName}</td><td className="mono sm">{hex(high.txSig, 10)}</td></tr>
        </tbody>
      </table>
      <p className="note">
        Both verify under the operator’s key and both name position {pair.seq.toString()} in the same
        run of the log. It made two contradictory statements about one position, and signed each.
      </p>

      {!operator ? (
        <p className="warn-note">
          No sequencer has registered this signing key on chain, so there is no bond to slash. The
          evidence is still valid — it just has nowhere to go yet.
        </p>
      ) : operator.bond === 0n ? (
        <p className="warn-note">This sequencer’s bond is already gone. Nothing left to slash.</p>
      ) : (
        <>
          <h3>What escalating does</h3>
          <SplitBar split={preview!} total={operator.bond} />
          <p className="note">
            Conviction account <span className="mono sm">{short(conviction!.toBase58(), 8, 8)}</span> —
            addressed by the contradiction itself, so the same evidence cannot be submitted twice.
            The victim share is escrowed there until someone produces the transaction the log lied to.
          </p>
          <TxButton
            label={`Escalate — slash ${sol(operator.bond)} SOL`}
            tone="danger"
            disabled={!accuser}
            build={() => ix.proveEquivocation(publicKey!, operator.address, signingKey, pair)}
            onDone={refresh}
          />
          <p className="note dim">
            Two instructions: the ed25519 precompile verifies both receipts under the registered key,
            then the program reads back what it verified and compares them. Nothing else may sit
            between them.
          </p>
        </>
      )}
    </div>
  )
}

function parsePasted(text: string): { receipts: Receipt[]; rejected: number } {
  const trimmed = text.trim()
  if (!trimmed) return { receipts: [], rejected: 0 }
  let entries: string[]
  try {
    const parsed = JSON.parse(trimmed)
    entries = Array.isArray(parsed) ? parsed.map(String) : [String(parsed)]
  } catch {
    entries = trimmed.split(/[\s,"'\[\]]+/).filter(Boolean)
  }
  const receipts: Receipt[] = []
  let rejected = 0
  for (const entry of entries) {
    try { receipts.push(decodeReceipt(decodeBase64(entry))) } catch { rejected++ }
  }
  return { receipts, rejected }
}

function tryDecodeKey(value: string): Uint8Array | null {
  try {
    const bytes = bs58.decode(value.trim())
    return bytes.length === 32 ? bytes : null
  } catch { return null }
}
