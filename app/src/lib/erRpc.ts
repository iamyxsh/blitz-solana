import { decodeBase64, decodeReceipt, type Receipt } from './receipt'

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

/** The signing key every receipt from this node must verify against. */
export async function operatorIdentity(url: string): Promise<string> {
  const result = await call(url, 'getIdentity', [])
  return result.identity as string
}

/** The operator's own published log. */
export async function fetchReceipts(url: string, from = 0, limit = 1000): Promise<Receipt[]> {
  const result = await call(url, 'getReceipts', [from, limit])
  return (result as { receipt: string }[]).map((entry) => decodeReceipt(decodeBase64(entry.receipt)))
}
