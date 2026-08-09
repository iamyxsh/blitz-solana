import { BPS, BURN_BPS, POOL_BPS, VICTIM_BPS } from './constants'

export type Split = { burn: bigint; victim: bigint; pool: bigint }

/**
 * The same integer arithmetic the program runs, so the preview shown before
 * signing is the outcome and not an estimate. Burn takes the remainder, which
 * is why the three shares always sum to exactly what was taken.
 */
export function splitOf(slashed: bigint, poolStaked: bigint): Split {
  const victim = (slashed * VICTIM_BPS) / BPS
  const pool = (slashed * POOL_BPS) / BPS
  const burn = slashed - victim - pool
  // Nobody staked means nobody carried the risk, so that share is destroyed
  // rather than parked where the next arrival would collect it.
  return poolStaked === 0n ? { burn: burn + pool, victim, pool: 0n } : { burn, victim, pool }
}

export const BURN_IS_THE_BUDGET = BURN_BPS >= VICTIM_BPS + POOL_BPS
