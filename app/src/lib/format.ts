import { LAMPORTS_PER_SOL } from './constants'

export function sol(lamports: bigint | number, places = 4): string {
  const n = BigInt(lamports)
  const whole = n / LAMPORTS_PER_SOL
  const frac = (n % LAMPORTS_PER_SOL).toString().padStart(9, '0').slice(0, places)
  return places > 0 ? `${whole}.${frac}` : `${whole}`
}

export const lamports = (n: bigint | number) => BigInt(n).toLocaleString('en-US')

export const short = (s: string, head = 4, tail = 4) =>
  s.length <= head + tail + 1 ? s : `${s.slice(0, head)}…${s.slice(-tail)}`

export function hex(bytes: Uint8Array, max = 8): string {
  const head = Array.from(bytes.slice(0, max), (b) => b.toString(16).padStart(2, '0')).join('')
  return bytes.length > max ? `${head}…` : head
}

/** Lexicographic byte comparison, matching the program's canonical ordering. */
export function compareBytes(a: Uint8Array, b: Uint8Array): number {
  const n = Math.min(a.length, b.length)
  for (let i = 0; i < n; i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1
  return a.length - b.length
}
