import { TransactionInstruction } from '@solana/web3.js'
import { ED25519_PROGRAM } from './constants'
import type { Receipt } from './receipt'

const HEADER_LEN = 2
const OFFSETS_LEN = 14
/** Tells the precompile the bytes it needs are in this same instruction. */
const THIS_INSTRUCTION = 0xffff

/**
 * Asks the precompile to verify both receipts under one key.
 *
 * The precompile proves *some* signature over *some* bytes and says nothing
 * about whose or over what — those live at byte offsets in this data, and every
 * entry points back here so the program can refuse anything that does not. A
 * single byte wrong and the program reads different bytes from the ones that
 * were verified, which is exactly the failure it is written to catch.
 */
export function verifyTwo(signingKey: Uint8Array, a: Receipt, b: Receipt): TransactionInstruction {
  const entries: number[] = []
  const payload: number[] = []
  const payloadStart = HEADER_LEN + 2 * OFFSETS_LEN

  for (const receipt of [a, b]) {
    const signatureAt = payloadStart + payload.length
    payload.push(...receipt.signature)
    const keyAt = payloadStart + payload.length
    payload.push(...signingKey)
    const messageAt = payloadStart + payload.length
    payload.push(...receipt.message)

    entries.push(
      signatureAt, THIS_INSTRUCTION,
      keyAt, THIS_INSTRUCTION,
      messageAt, receipt.message.length, THIS_INSTRUCTION,
    )
  }

  const data = new Uint8Array(payloadStart + payload.length)
  data[0] = 2
  const view = new DataView(data.buffer)
  entries.forEach((value, i) => view.setUint16(HEADER_LEN + i * 2, value, true))
  data.set(payload, payloadStart)

  return new TransactionInstruction({ programId: ED25519_PROGRAM, keys: [], data: Buffer.from(data) })
}
