import { useEffect, useState } from 'react'
import { describeEr, type ErInfo } from '../lib/erClient'
import { short } from '../lib/format'

const KEY = 'mb.er'
export const readErUrl = () => localStorage.getItem(KEY) ?? 'http://127.0.0.1:8899'

/** Which rollup this session is talking to, and whether it is answering. */
export function RollupBar({ onChange }: { onChange: (info: ErInfo | null) => void }) {
  const [url, setUrl] = useState(readErUrl)
  const [info, setInfo] = useState<ErInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function connect(target = url) {
    setBusy(true); setError(null)
    try {
      const described = await describeEr(target)
      localStorage.setItem(KEY, target)
      setInfo(described); onChange(described)
    } catch (caught) {
      setInfo(null); onChange(null)
      setError((caught as Error).message)
    } finally { setBusy(false) }
  }

  useEffect(() => { void connect(url) }, [])

  return (
    <div className={`rollup${info ? ' on' : ''}`}>
      <span className={`led${info ? ' led-on' : error ? ' led-off' : ''}`} />
      <input value={url} onChange={(e) => setUrl(e.target.value)} spellCheck={false}
        placeholder="http://127.0.0.1:8899" />
      <button className="btn btn-ghost sm" onClick={() => connect()} disabled={busy}>
        {busy ? '…' : info ? 'reconnect' : 'connect'}
      </button>
      {info && (
        <span className="rollup-i">
          sequencer <b className="mono">{short(info.identity, 5, 5)}</b>
          <span className="dim"> · slot {info.slot.toLocaleString()} · {info.receipts} receipts published</span>
        </span>
      )}
      {!info && error && <span className="rollup-i dim">no rollup at this address — you can still import receipts</span>}
    </div>
  )
}
