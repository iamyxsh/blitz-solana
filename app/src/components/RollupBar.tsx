import { useEffect, useRef, useState } from 'react'
import { usePoll } from '../hooks/usePoll'
import { describeEr, type ErInfo } from '../lib/erClient'
import { short } from '../lib/format'

const KEY = 'mb.er'
export const readErUrl = () => localStorage.getItem(KEY) ?? 'http://127.0.0.1:8799'

/**
 * Which rollup this session is watching, kept current.
 *
 * `onChange` fires only when the node's identity or address changes, not on
 * every poll: downstream work is keyed on it, and handing back a fresh object
 * twice a second would re-verify every signature for no reason.
 */
export function RollupBar({ onChange }: { onChange: (info: ErInfo | null) => void }) {
  const [url, setUrl] = useState(readErUrl)
  const [watching, setWatching] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const announced = useRef<string>('')

  const { value: live, ok } = usePoll(() => describeEr(watching!), 2000, watching)

  useEffect(() => {
    const signature = live ? `${live.url}|${live.identity}` : ''
    if (signature === announced.current) return
    announced.current = signature
    onChange(live)
  }, [live, onChange])

  async function connect(target = url) {
    setBusy(true); setError(null)
    try {
      await describeEr(target)
      localStorage.setItem(KEY, target)
      setWatching(target)
    } catch (caught) {
      setWatching(null)
      setError((caught as Error).message)
    } finally { setBusy(false) }
  }

  useEffect(() => { void connect(url) }, [])

  return (
    <div className={`rollup${ok ? ' on' : ''}`}>
      <span className={`led${ok ? ' led-on' : live || error ? ' led-off' : ''}`} />
      <input value={url} onChange={(e) => setUrl(e.target.value)} spellCheck={false}
        placeholder="http://127.0.0.1:8799" />
      <button className="btn btn-ghost sm" onClick={() => connect()} disabled={busy}>
        {busy ? '…' : live ? 'reconnect' : 'connect'}
      </button>
      {live && (
        <span className="rollup-i">
          sequencer <b className="mono">{short(live.identity, 5, 5)}</b>
          <span className="dim"> · slot {live.slot.toLocaleString()} · {live.receipts} receipts published</span>
          {ok
            ? <span className="watching">watching</span>
            : <span className="stalled">not answering</span>}
        </span>
      )}
      {!live && error && (
        <span className="rollup-i dim">no rollup at this address — you can still import receipts</span>
      )}
    </div>
  )
}
