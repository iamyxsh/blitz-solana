import { PublicKey } from '@solana/web3.js'
import { sha256 } from '@noble/hashes/sha256'
import { CONVICTION_SEED, OPERATOR_SEED, POSITION_SEED, PROGRAM_ID } from './constants'
import { type Contradiction, ordered } from './evidence'

const seed = (s: string) => new TextEncoder().encode(s)

export const operatorPda = (authority: PublicKey) =>
  PublicKey.findProgramAddressSync([seed(OPERATOR_SEED), authority.toBytes()], PROGRAM_ID)[0]

export const positionPda = (operator: PublicKey, owner: PublicKey) =>
  PublicKey.findProgramAddressSync(
    [seed(POSITION_SEED), operator.toBytes(), owner.toBytes()],
    PROGRAM_ID,
  )[0]

/**
 * Addressed by the contradiction itself, under the same canonical ordering the
 * program applies — so the pair presented either way round names one account,
 * and the same evidence cannot be submitted twice for two payouts.
 */
export function convictionPda(operator: PublicKey, pair: Contradiction): PublicKey {
  const [low, high] = ordered(pair)
  return PublicKey.findProgramAddressSync(
    [seed(CONVICTION_SEED), operator.toBytes(), sha256(low.message), sha256(high.message)],
    PROGRAM_ID,
  )[0]
}
