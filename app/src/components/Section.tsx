import type { ReactNode } from 'react'

export function Section({ title, note, right, children }: {
  title: string
  note?: string
  right?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="card">
      <header className="card-hd">
        <div>
          <h2>{title}</h2>
          {note && <p className="note">{note}</p>}
        </div>
        {right}
      </header>
      <div className="card-bd">{children}</div>
    </section>
  )
}

export function Stat({ label, value, sub, tone }: {
  label: string
  value: ReactNode
  sub?: ReactNode
  tone?: 'ok' | 'warn' | 'danger'
}) {
  return (
    <div className={`stat${tone ? ` stat-${tone}` : ''}`}>
      <span className="stat-l">{label}</span>
      <b>{value}</b>
      {sub && <span className="stat-s">{sub}</span>}
    </div>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>
}
