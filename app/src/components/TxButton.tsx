import { useState } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { Transaction, type TransactionInstruction } from '@solana/web3.js'
import { EXPLORER } from '../lib/constants'
import { short } from '../lib/format'

type Props = {
  label: string
  build: () => TransactionInstruction[] | Promise<TransactionInstruction[]>
  onDone?: () => void
  disabled?: boolean
  tone?: 'primary' | 'danger' | 'ghost'
  title?: string
}

/**
 * Sends one transaction and reports what happened, including the failure.
 *
 * Program errors surface as a custom code; showing it rather than a generic
 * failure is the difference between "it did not work" and knowing the evidence
 * was refused for a nameable reason.
 */
export function TxButton({ label, build, onDone, disabled, tone = 'primary', title }: Props) {
  const { connection } = useConnection()
  const { publicKey, sendTransaction } = useWallet()
  const [busy, setBusy] = useState(false)
  const [signature, setSignature] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  async function run() {
    if (!publicKey) return
    setBusy(true); setError(null); setSignature(null)
    try {
      const instructions = await build()
      const transaction = new Transaction().add(...instructions)
      const sent = await sendTransaction(transaction, connection)
      await connection.confirmTransaction(sent, 'confirmed')
      setSignature(sent)
      onDone?.()
    } catch (caught) {
      setError(describe(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <span className="txb">
      <button className={`btn btn-${tone}`} onClick={run} disabled={busy || disabled || !publicKey} title={title}>
        {busy ? 'confirming…' : label}
      </button>
      {signature && (
        <a className="txb-ok" href={EXPLORER(signature, 'tx')} target="_blank" rel="noreferrer">
          ✓ {short(signature, 6, 6)}
        </a>
      )}
      {error && <span className="txb-err">{error}</span>}
    </span>
  )
}

const PROGRAM_ERRORS: Record<number, string> = {
  0: 'bad instruction', 1: 'bad account data', 2: 'wrong owner', 3: 'wrong PDA', 4: 'not a signer',
  10: 'no ed25519 instruction', 11: 'unregistered signing key',
  12: 'ed25519 offsets point outside this instruction', 13: 'wrong signature count',
  14: 'malformed receipt',
  20: 'receipts are from different runs of the log', 21: 'receipts sit at different positions',
  22: 'the receipts are identical', 23: 'already convicted',
  30: 'insufficient bond', 31: 'nothing staked', 32: 'overdraw', 33: 'bond still timelocked',
}

function describe(caught: unknown): string {
  const message = caught instanceof Error ? caught.message : String(caught)
  const custom = message.match(/custom program error: 0x([0-9a-f]+)/i)
  if (custom) {
    const code = parseInt(custom[1], 16)
    return PROGRAM_ERRORS[code] ?? `program error ${code}`
  }
  return message.replace(/^Error: /, '').slice(0, 160)
}
