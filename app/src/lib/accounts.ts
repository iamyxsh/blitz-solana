import { PublicKey } from '@solana/web3.js'
import { CONVICTION_LEN, OPERATOR_LEN, POSITION_LEN, REWARD_SCALE, TAG } from './constants'

const view = (b: Uint8Array) => new DataView(b.buffer, b.byteOffset, b.byteLength)
const u64 = (b: Uint8Array, at: number) => view(b).getBigUint64(at, true)
const u128 = (b: Uint8Array, at: number) =>
  view(b).getBigUint64(at, true) + (view(b).getBigUint64(at + 8, true) << 64n)
const key = (b: Uint8Array, at: number) => new PublicKey(b.slice(at, at + 32))

export type Operator = {
  address: PublicKey
  authority: PublicKey
  signingKey: Uint8Array
  bond: bigint
  poolStaked: bigint
  rewardIndex: bigint
  unbondAt: bigint
}

export type Position = {
  address: PublicKey
  owner: PublicKey
  operator: PublicKey
  staked: bigint
  entryIndex: bigint
  reward: bigint
}

export type Conviction = {
  address: PublicKey
  operator: PublicKey
  wronged: Uint8Array
  wrongedTxHash: Uint8Array
  slashed: bigint
  owedToVictim: bigint
  slot: bigint
}

export function decodeOperator(address: PublicKey, data: Uint8Array): Operator | null {
  if (data.length < OPERATOR_LEN || data[0] !== TAG.OPERATOR) return null
  return {
    address,
    authority: key(data, 1),
    signingKey: data.slice(33, 65),
    bond: u64(data, 65),
    poolStaked: u64(data, 73),
    rewardIndex: u128(data, 81),
    unbondAt: u64(data, 97),
  }
}

export function decodePosition(address: PublicKey, data: Uint8Array): Position | null {
  if (data.length < POSITION_LEN || data[0] !== TAG.POSITION) return null
  return {
    address,
    owner: key(data, 1),
    operator: key(data, 33),
    staked: u64(data, 65),
    entryIndex: u128(data, 73),
    reward: u64(data, 89),
  }
}

export function decodeConviction(address: PublicKey, data: Uint8Array): Conviction | null {
  if (data.length < CONVICTION_LEN || data[0] !== TAG.CONVICTION) return null
  return {
    address,
    operator: key(data, 1),
    wronged: data.slice(33, 97),
    wrongedTxHash: data.slice(97, 129),
    slashed: u64(data, 129),
    owedToVictim: u64(data, 137),
    slot: u64(data, 145),
  }
}

/**
 * What this position could claim right now.
 *
 * The index only moves on faults that happen while the position is open, so a
 * stake opened after a slash inherits nothing from it.
 */
export function claimable(position: Position, operator: Operator): bigint {
  const moved = operator.rewardIndex > position.entryIndex
    ? operator.rewardIndex - position.entryIndex
    : 0n
  return position.reward + (position.staked * moved) / REWARD_SCALE
}
