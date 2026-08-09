import { decodeBase64, decodeReceipt, type Receipt } from './receipt'

export type ErInfo = { url: string; identity: string; slot: number; receipts: number }

async function call(url: string, method: string, params: unknown[]): Promise<any> {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
  })
  const body = await response.json()
  if (body.error) throw new Error(body.error.message ?? 'rpc error')
  return body.result
}

/** Who signs receipts here, how far along it is, and how much log there is. */
export async function describeEr(url: string): Promise<ErInfo> {
  const [identity, slot, log] = await Promise.all([
    call(url, 'getIdentity', []),
    call(url, 'getSlot', []),
    call(url, 'getReceipts', [0, 1000]).catch(() => []),
  ])
  return { url, identity: identity.identity, slot, receipts: (log as unknown[]).length }
}

export async function fetchLog(url: string, from = 0, limit = 1000): Promise<Receipt[]> {
  const result = await call(url, 'getReceipts', [from, limit])
  return (result as { receipt: string }[]).map((e) => decodeReceipt(decodeBase64(e.receipt)))
}

export async function latestBlockhash(url: string): Promise<string> {
  const result = await call(url, 'getLatestBlockhash', [{ commitment: 'confirmed' }])
  return result.value.blockhash
}

/**
 * Sends a signed transaction to the rollup and keeps the receipt it answers with.
 *
 * The wallet signs but does not send: the receipt arrives in this response and a
 * wallet's own submission path would discard it. That copy is the whole reason a
 * client can later prove anything — the operator's log alone never will.
 */
export async function sendAndCollect(url: string, signed: Uint8Array): Promise<{
  signature: string
  receipt: Receipt | null
}> {
  const encoded = btoa(String.fromCharCode(...signed))
  const result = await call(url, 'sendTransaction', [encoded, { encoding: 'base64' }])
  if (typeof result === 'string') return { signature: result, receipt: null }
  return {
    signature: result.signature,
    receipt: result.receipt ? decodeReceipt(decodeBase64(result.receipt)) : null,
  }
}
