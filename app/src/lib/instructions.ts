import {
  PublicKey,
  SystemProgram,
  type TransactionInstruction,
  TransactionInstruction as Ix,
} from '@solana/web3.js'
import { INCINERATOR, INSTRUCTIONS_SYSVAR, PROGRAM_ID } from './constants'
import { convictionPda, operatorPda, positionPda } from './addresses'
import type { Contradiction } from './evidence'
import { ordered } from './evidence'
import { verifyTwo } from './ed25519Ix'

const writable = (pubkey: PublicKey, isSigner = false) => ({ pubkey, isSigner, isWritable: true })
const readonly = (pubkey: PublicKey) => ({ pubkey, isSigner: false, isWritable: false })

function encode(tag: number, ...parts: Uint8Array[]): Buffer {
  const size = 1 + parts.reduce((n, p) => n + p.length, 0)
  const data = new Uint8Array(size)
  data[0] = tag
  let at = 1
  for (const part of parts) { data.set(part, at); at += part.length }
  return Buffer.from(data)
}

function u64(value: bigint): Uint8Array {
  const out = new Uint8Array(8)
  new DataView(out.buffer).setBigUint64(0, value, true)
  return out
}

const ix = (keys: ReturnType<typeof readonly>[], data: Buffer) =>
  new Ix({ programId: PROGRAM_ID, keys, data })

export function register(authority: PublicKey, signingKey: Uint8Array, bond: bigint) {
  return ix(
    [writable(authority, true), writable(operatorPda(authority)), readonly(SystemProgram.programId)],
    encode(0, signingKey, u64(bond)),
  )
}

export function stake(owner: PublicKey, operator: PublicKey, amount: bigint) {
  return ix(
    [
      writable(owner, true),
      writable(operator),
      writable(positionPda(operator, owner)),
      readonly(SystemProgram.programId),
    ],
    encode(1, u64(amount)),
  )
}

export function unstake(owner: PublicKey, operator: PublicKey, amount: bigint) {
  return ix(
    [writable(owner, true), writable(operator), writable(positionPda(operator, owner))],
    encode(2, u64(amount)),
  )
}

export function claim(owner: PublicKey, operator: PublicKey) {
  return ix(
    [writable(owner, true), writable(operator), writable(positionPda(operator, owner))],
    encode(3),
  )
}

/**
 * The two instructions that convict, in the order the program expects.
 *
 * The precompile call must sit immediately before the program call: the program
 * looks back exactly one instruction for what was verified, so anything wedged
 * between them breaks the link.
 */
export function proveEquivocation(
  accuser: PublicKey,
  operator: PublicKey,
  signingKey: Uint8Array,
  pair: Contradiction,
): TransactionInstruction[] {
  const [a, b] = ordered(pair)
  return [
    verifyTwo(signingKey, a, b),
    ix(
      [
        writable(accuser, true),
        writable(operator),
        writable(convictionPda(operator, pair)),
        writable(INCINERATOR),
        readonly(INSTRUCTIONS_SYSVAR),
        readonly(SystemProgram.programId),
      ],
      encode(4),
    ),
  ]
}

export function claimVictim(claimant: PublicKey, conviction: PublicKey, wireBytes: Uint8Array) {
  return ix([writable(claimant, true), writable(conviction)], encode(5, wireBytes))
}

export const beginUnbond = (authority: PublicKey) =>
  ix([writable(authority, true), writable(operatorPda(authority))], encode(6))

export const withdrawBond = (authority: PublicKey) =>
  ix([writable(authority, true), writable(operatorPda(authority))], encode(7))
