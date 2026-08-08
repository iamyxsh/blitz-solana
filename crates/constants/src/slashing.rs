pub const BPS_DENOMINATOR: u64 = 10_000;

/// Destroyed outright.
pub const BURN_BPS: u64 = 5_000;
/// Paid to the signer the evidence names as wronged.
pub const VICTIM_BPS: u64 = 3_000;
/// Paid to whoever staked the coverage pool.
pub const POOL_BPS: u64 = 2_000;

const _: () = assert!(BURN_BPS + VICTIM_BPS + POOL_BPS == BPS_DENOMINATOR);

/// The collusion rule, as a compile-time assertion.
///
/// Both of the other shares can return to a dishonest operator: it chooses
/// which transactions it equivocates over, so it can name itself the victim,
/// and nothing stops it staking the pool. Only the burned share is a loss it
/// cannot recover, so the burn alone is the security budget and it must be at
/// least everything else combined.
const _: () = assert!(BURN_BPS >= VICTIM_BPS + POOL_BPS);

/// Fixed-point scale for the pool's reward index.
///
/// Rewards accrue per unit staked, and a unit is one lamport, so the per-unit
/// figure is almost always a fraction. Scaling it keeps the remainder instead
/// of truncating every distribution to zero.
pub const REWARD_SCALE: u128 = 1_000_000_000_000;

/// Slots between an operator asking for its bond back and being able to take
/// it.
///
/// Must exceed the widest window in which evidence for an already-committed
/// fault could still arrive: a watchtower has to observe the log, assemble the
/// object and land a transaction on the base chain. Otherwise an operator
/// front-runs its own conviction by withdrawing the moment it misbehaves.
/// Roughly a day at devnet block times.
pub const UNBOND_SLOTS: u64 = 216_000;
