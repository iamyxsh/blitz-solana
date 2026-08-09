import { PublicKey } from '@solana/web3.js'

export const PROGRAM_ID = new PublicKey('8VMsFLGQEF4x3wrFUfoipjjyzYFNe8DhNGAjXeDTSey7')
export const CLUSTER = 'devnet'
export const RPC_URL = 'https://api.devnet.solana.com'
export const EXPLORER = (what: string, kind: 'address' | 'tx' = 'address') =>
  `https://explorer.solana.com/${kind}/${what}?cluster=devnet`

export const INCINERATOR = new PublicKey('1nc1nerator11111111111111111111111111111111')
export const INSTRUCTIONS_SYSVAR = new PublicKey('Sysvar1nstructions1111111111111111111111111')
export const ED25519_PROGRAM = new PublicKey('Ed25519SigVerify111111111111111111111111111')

export const OPERATOR_SEED = 'operator'
export const POSITION_SEED = 'position'
export const CONVICTION_SEED = 'conviction'

export const RECEIPT_LEN = 261
export const SIGNED_RECEIPT_LEN = 325

/** Byte offsets of the frozen receipt layout. Must match `mb-constants`. */
export const OFF = {
  DOMAIN_TAG: 0,
  LOG_ID: 12,
  MODE: 44,
  SEQ: 45,
  TX_SIG: 53,
  TX_HASH: 117,
  RECENT_BLOCKHASH: 149,
  PREV_RECEIPT_HASH: 181,
  COMMITTER: 213,
  INGRESS_SLOT: 245,
  T_INGRESS_MICROS: 253,
} as const

export const DOMAIN_TAG = 'MBRECEIPT_V1'

export const MODE = { PLAIN: 1, COMMIT: 2, REVEAL: 3, RETRACT: 4 } as const
export const MODE_NAME: Record<number, string> = {
  1: 'plain', 2: 'commit', 3: 'reveal', 4: 'retract',
}

export const BPS = 10_000n
export const BURN_BPS = 5_000n
export const VICTIM_BPS = 3_000n
export const POOL_BPS = 2_000n
export const REWARD_SCALE = 1_000_000_000_000n
export const LAMPORTS_PER_SOL = 1_000_000_000n

export const OPERATOR_LEN = 106
export const POSITION_LEN = 98
export const CONVICTION_LEN = 154
export const TAG = { OPERATOR: 1, POSITION: 2, CONVICTION: 3 } as const
