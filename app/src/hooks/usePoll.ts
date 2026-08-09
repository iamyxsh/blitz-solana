import { useEffect, useRef, useState } from 'react'

export type Polled<T> = {
  /** The last value read successfully, kept through a failed attempt. */
  value: T | null
  /** Whether the most recent attempt succeeded. */
  ok: boolean
}

/**
 * Re-runs a read on a timer.
 *
 * A rollup keeps sequencing whether or not anyone is looking, so a page that
 * reads once only ever tells you what was true when it loaded. `key` decides
 * when the timer restarts — the reader is held in a ref, so callers do not have
 * to memoise a closure to avoid resetting the interval every render.
 *
 * A failed read keeps the last good value rather than blanking the view, and
 * reports the failure separately. Those are different facts and a display that
 * conflates them says a node is healthy when it has stopped answering.
 */
export function usePoll<T>(read: () => Promise<T>, everyMs: number, key: string | null): Polled<T> {
  const [state, setState] = useState<Polled<T>>({ value: null, ok: false })
  const latest = useRef(read)
  latest.current = read

  useEffect(() => {
    if (key === null) { setState({ value: null, ok: false }); return }
    let live = true

    const tick = async () => {
      try {
        const value = await latest.current()
        if (live) setState({ value, ok: true })
      } catch {
        if (live) setState((previous) => ({ value: previous.value, ok: false }))
      }
    }

    void tick()
    const timer = setInterval(tick, everyMs)
    return () => { live = false; clearInterval(timer) }
  }, [key, everyMs])

  return state
}
