import { BURN_BPS, POOL_BPS, VICTIM_BPS } from '../lib/constants'
import { lamports } from '../lib/format'
import type { Split } from '../lib/split'

/**
 * What a slash of this size would do, to scale.
 *
 * Shown before signing rather than after, because the numbers come from the
 * same integer arithmetic the program runs — this is the outcome, not an
 * estimate of it.
 */
export function SplitBar({ split, total }: { split: Split; total: bigint }) {
  const pct = (part: bigint) => (total === 0n ? 0 : Number((part * 10000n) / total) / 100)
  const rows = [
    { key: 'burn', label: 'burn', value: split.burn, bps: BURN_BPS, note: 'destroyed — the only share an operator cannot recover' },
    { key: 'victim', label: 'victim', value: split.victim, bps: VICTIM_BPS, note: 'escrowed for whoever the log lied to' },
    { key: 'pool', label: 'pool', value: split.pool, bps: POOL_BPS, note: split.pool === 0n ? 'nobody staked, so this is burned too' : 'to whoever is staked right now' },
  ]
  return (
    <div className="split">
      <div className="split-bar">
        {rows.map((row) => (
          <div key={row.key} className={`seg seg-${row.key}`} style={{ width: `${pct(row.value)}%` }} />
        ))}
      </div>
      <table className="split-t">
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              <td><span className={`dot dot-${row.key}`} />{row.label}</td>
              <td className="num">{lamports(row.value)}</td>
              <td className="num dim">{pct(row.value).toFixed(0)}%</td>
              <td className="dim">{row.note}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
