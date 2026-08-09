import { useCallback, useEffect, useState } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey } from '@solana/web3.js'
import {
  type Conviction, type Operator, type Position,
  decodeConviction, decodeOperator, decodePosition,
} from '../lib/accounts'
import { PROGRAM_ID, TAG } from '../lib/constants'
import { operatorPda } from '../lib/addresses'

export type ProgramState = {
  operators: Operator[]
  positions: Position[]
  convictions: Conviction[]
  /** The operator account this wallet is the authority for, if any. */
  mine: Operator | null
  balance: bigint
  slot: number
  loading: boolean
  error: string | null
  refresh: () => void
}

/**
 * Everything on chain, read directly rather than through an indexer.
 *
 * The program is small enough that one `getProgramAccounts` per account type is
 * cheap, and reading it straight keeps the dashboard as checkable as the
 * evidence it displays.
 */
export function useProgramState(): ProgramState {
  const { connection } = useConnection()
  const { publicKey } = useWallet()
  const [state, setState] = useState<Omit<ProgramState, 'refresh'>>({
    operators: [], positions: [], convictions: [], mine: null,
    balance: 0n, slot: 0, loading: true, error: null,
  })
  const [nonce, setNonce] = useState(0)
  const refresh = useCallback(() => setNonce((n) => n + 1), [])

  // Re-read on a timer as well as on demand. A bond posted or a conviction
  // landed in another window is still a change to what this one is showing,
  // and devnet is slow enough that eight seconds is polite.
  useEffect(() => {
    const timer = setInterval(refresh, 8000)
    return () => clearInterval(timer)
  }, [refresh])

  useEffect(() => {
    let live = true
    ;(async () => {
      try {
        const all = await connection.getProgramAccounts(PROGRAM_ID)
        const operators: Operator[] = []
        const positions: Position[] = []
        const convictions: Conviction[] = []

        for (const { pubkey, account } of all) {
          const data = new Uint8Array(account.data)
          if (data[0] === TAG.OPERATOR) {
            const decoded = decodeOperator(pubkey, data)
            if (decoded) operators.push(decoded)
          } else if (data[0] === TAG.POSITION) {
            const decoded = decodePosition(pubkey, data)
            if (decoded) positions.push(decoded)
          } else if (data[0] === TAG.CONVICTION) {
            const decoded = decodeConviction(pubkey, data)
            if (decoded) convictions.push(decoded)
          }
        }

        const slot = await connection.getSlot()
        const balance = publicKey ? BigInt(await connection.getBalance(publicKey)) : 0n
        const mineAddress = publicKey ? operatorPda(publicKey).toBase58() : null
        const mine = operators.find((o) => o.address.toBase58() === mineAddress) ?? null

        if (live) {
          convictions.sort((a, b) => Number(b.slot - a.slot))
          setState({ operators, positions, convictions, mine, balance, slot, loading: false, error: null })
        }
      } catch (error) {
        // Keep whatever was last read: a dropped poll is this browser losing
        // sight of the chain, not the chain losing the accounts.
        if (live) setState((s) => ({ ...s, loading: false, error: (error as Error).message }))
      }
    })()
    return () => { live = false }
  }, [connection, publicKey, nonce])

  return { ...state, refresh }
}
