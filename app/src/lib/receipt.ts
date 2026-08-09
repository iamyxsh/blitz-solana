import { sha256 } from '@noble/hashes/sha256'
import { ed25519 } from '@noble/curves/ed25519'
import { DOMAIN_TAG, MODE_NAME, OFF, RECEIPT_LEN, SIGNED_RECEIPT_LEN } from './constants'

export type Receipt = {
  /** The 261 signed bytes, exactly as the operator signed them. */
  message: Uint8Array
  signature: Uint8Array
  /** sha256 over message ‖ signature — the chain link, and the identity used on chain. */
  receiptHash: Uint8Array
  logId: Uint8Array
  mode: number
  modeName: string
  seq: bigint
  txSig: Uint8Array
  txHash: Uint8Array
  prevReceiptHash: Uint8Array
  ingressSlot: bigint
}

const u64 = (b: Uint8Array, at: number) =>
  new DataView(b.buffer, b.byteOffset + at, 8).getBigUint64(0, true)

const text = new TextDecoder()

export function decodeReceipt(bytes: Uint8Array): Receipt {
  if (bytes.length !== SIGNED_RECEIPT_LEN) {
    throw new Error(`receipt is ${bytes.length} bytes, expected ${SIGNED_RECEIPT_LEN}`)
  }
  const message = bytes.slice(0, RECEIPT_LEN)
  if (text.decode(message.slice(0, OFF.LOG_ID)) !== DOMAIN_TAG) {
    throw new Error('not a receipt: wrong domain tag')
  }
  const mode = message[OFF.MODE]
  return {
    message,
    signature: bytes.slice(RECEIPT_LEN),
    receiptHash: sha256(bytes),
    logId: message.slice(OFF.LOG_ID, OFF.MODE),
    mode,
    modeName: MODE_NAME[mode] ?? `unknown(${mode})`,
    seq: u64(message, OFF.SEQ),
    txSig: message.slice(OFF.TX_SIG, OFF.TX_HASH),
    txHash: message.slice(OFF.TX_HASH, OFF.RECENT_BLOCKHASH),
    prevReceiptHash: message.slice(OFF.PREV_RECEIPT_HASH, OFF.COMMITTER),
    ingressSlot: u64(message, OFF.INGRESS_SLOT),
  }
}

/**
 * Whether the operator really signed this.
 *
 * Checked in the browser before any transaction is built. Evidence the client
 * cannot verify itself is evidence the program is about to reject, and the
 * only difference is who pays the fee.
 */
export function verifyReceipt(receipt: Receipt, operatorKey: Uint8Array): boolean {
  try {
    return ed25519.verify(receipt.signature, receipt.message, operatorKey)
  } catch {
    return false
  }
}

export function decodeBase64(encoded: string): Uint8Array {
  const binary = atob(encoded.trim())
  return Uint8Array.from(binary, (c) => c.charCodeAt(0))
}

export function encodeBase64(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
}

export function receiptBytes(receipt: Receipt): Uint8Array {
  const out = new Uint8Array(SIGNED_RECEIPT_LEN)
  out.set(receipt.message, 0)
  out.set(receipt.signature, RECEIPT_LEN)
  return out
}
