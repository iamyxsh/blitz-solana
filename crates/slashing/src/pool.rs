use mb_constants::slashing::REWARD_SCALE;

use crate::{error::PoolError, position::Position, split::Split};

/// Capital staked against an operator's misbehaviour, and what it has earned.
///
/// Stakers are not buying a share of a growing balance; they are buying a
/// claim on slashes that happen *while they are staked*. So the pool records a
/// running reward-per-lamport rather than dividing a balance at withdrawal
/// time, and a position that arrives after a slash inherits nothing from it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pool {
    pub staked: u64,
    /// Cumulative reward per lamport staked, scaled by `REWARD_SCALE`.
    pub reward_index: u128,
}

impl Pool {
    pub fn stake(&mut self, position: &mut Position, amount: u64) -> Result<(), PoolError> {
        if amount == 0 {
            return Err(PoolError::StakeIsZero);
        }
        if position.staked > 0 && position.entry_index > self.reward_index {
            return Err(PoolError::ForeignPosition);
        }

        position.settle(self.reward_index);
        position.staked += amount;
        self.staked += amount;
        Ok(())
    }

    pub fn unstake(&mut self, position: &mut Position, amount: u64) -> Result<u64, PoolError> {
        if amount > position.staked {
            return Err(PoolError::Overdraw {
                requested: amount,
                held: position.staked,
            });
        }

        position.settle(self.reward_index);
        position.staked -= amount;
        self.staked -= amount;
        Ok(amount)
    }

    /// Pays out everything this position has earned, leaving the stake alone.
    pub fn claim(&self, position: &mut Position) -> u64 {
        position.settle(self.reward_index);
        std::mem::take(&mut position.reward)
    }

    /// Credits the pool's share of a slash to whoever is staked right now.
    ///
    /// Returns the split actually applied: with nothing staked there is no
    /// denominator to divide by and no one who carried the risk, so the pool's
    /// share is burned instead.
    pub fn distribute(&mut self, split: Split) -> Split {
        if self.staked == 0 || split.pool == 0 {
            return split.without_pool();
        }

        // Truncation here leaves at most `staked` lamports undistributed. They
        // stay in the index's remainder and accrue to later distributions
        // rather than being credited to anyone twice.
        self.reward_index += (split.pool as u128 * REWARD_SCALE) / self.staked as u128;
        split
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slash(amount: u64) -> Split {
        Split::of(amount)
    }

    #[test]
    fn two_stakers_share_a_slash_in_proportion() {
        let (mut pool, mut small, mut large) =
            (Pool::default(), Position::default(), Position::default());
        pool.stake(&mut small, 1_000).unwrap();
        pool.stake(&mut large, 3_000).unwrap();

        pool.distribute(slash(10_000)); // pool share is 2_000

        assert_eq!(small.claimable(pool.reward_index), 500);
        assert_eq!(large.claimable(pool.reward_index), 1_500);
    }

    /// The reason the pool tracks an index instead of dividing a balance.
    /// Capital that arrived after the fault carried none of the risk, so it
    /// must earn nothing from it — otherwise anyone watching the mempool for
    /// evidence transactions could stake in front of one and take a cut.
    #[test]
    fn staking_after_a_slash_earns_nothing_from_it() {
        let (mut pool, mut early, mut late) =
            (Pool::default(), Position::default(), Position::default());
        pool.stake(&mut early, 1_000).unwrap();

        pool.distribute(slash(10_000));
        pool.stake(&mut late, 1_000).unwrap();

        assert_eq!(late.claimable(pool.reward_index), 0);
        assert_eq!(early.claimable(pool.reward_index), 2_000);
    }

    #[test]
    fn a_second_slash_is_shared_by_whoever_is_staked_then() {
        let (mut pool, mut early, mut late) =
            (Pool::default(), Position::default(), Position::default());
        pool.stake(&mut early, 1_000).unwrap();
        pool.distribute(slash(10_000));
        pool.stake(&mut late, 1_000).unwrap();
        pool.distribute(slash(10_000));

        assert_eq!(early.claimable(pool.reward_index), 3_000);
        assert_eq!(late.claimable(pool.reward_index), 1_000);
    }

    /// Nobody carried the risk, so nobody is paid: the share is destroyed
    /// rather than parked where the next arrival would collect it.
    #[test]
    fn a_slash_with_nothing_staked_burns_the_pool_share() {
        let mut pool = Pool::default();

        let applied = pool.distribute(slash(10_000));

        assert_eq!(applied.pool, 0);
        assert_eq!(applied.burn, 7_000);
        assert_eq!(applied.total(), 10_000);
        assert_eq!(pool.reward_index, 0);
    }

    #[test]
    fn unstaking_keeps_rewards_already_earned() {
        let (mut pool, mut position) = (Pool::default(), Position::default());
        pool.stake(&mut position, 1_000).unwrap();
        pool.distribute(slash(10_000));

        pool.unstake(&mut position, 1_000).unwrap();

        assert_eq!(pool.staked, 0);
        assert_eq!(position.staked, 0);
        assert_eq!(pool.claim(&mut position), 2_000);
        assert_eq!(pool.claim(&mut position), 0);
    }

    /// Withdrawing does not entitle the position to later slashes.
    #[test]
    fn an_emptied_position_earns_nothing_afterwards() {
        let (mut pool, mut leaver, mut stayer) =
            (Pool::default(), Position::default(), Position::default());
        pool.stake(&mut leaver, 1_000).unwrap();
        pool.stake(&mut stayer, 1_000).unwrap();
        pool.unstake(&mut leaver, 1_000).unwrap();

        pool.distribute(slash(10_000));

        assert_eq!(leaver.claimable(pool.reward_index), 0);
        assert_eq!(stayer.claimable(pool.reward_index), 2_000);
    }

    #[test]
    fn a_position_cannot_overdraw() {
        let (mut pool, mut position) = (Pool::default(), Position::default());
        pool.stake(&mut position, 100).unwrap();

        assert_eq!(
            pool.unstake(&mut position, 101),
            Err(PoolError::Overdraw {
                requested: 101,
                held: 100
            })
        );
        assert_eq!(pool.staked, 100);
    }

    #[test]
    fn staking_nothing_is_refused() {
        let (mut pool, mut position) = (Pool::default(), Position::default());
        assert_eq!(pool.stake(&mut position, 0), Err(PoolError::StakeIsZero));
    }

    /// A position carrying an index from some other pool would claim rewards
    /// this one never distributed, or forfeit ones it did.
    #[test]
    fn a_position_from_another_pool_is_refused() {
        let (mut pool, mut position) = (Pool::default(), Position::default());
        pool.stake(&mut position, 1_000).unwrap();
        position.entry_index = pool.reward_index + 1;

        assert_eq!(
            pool.stake(&mut position, 1_000),
            Err(PoolError::ForeignPosition)
        );
    }

    /// Every lamport credited to the index must be claimable by someone. The
    /// remainder left by integer division stays in the pool rather than being
    /// paid twice.
    #[test]
    fn distributions_never_pay_out_more_than_they_took() {
        let (mut pool, mut a, mut b, mut c) = (
            Pool::default(),
            Position::default(),
            Position::default(),
            Position::default(),
        );
        pool.stake(&mut a, 333).unwrap();
        pool.stake(&mut b, 777).unwrap();
        pool.stake(&mut c, 1).unwrap();

        let applied = pool.distribute(slash(9_999));
        let paid = a.claimable(pool.reward_index)
            + b.claimable(pool.reward_index)
            + c.claimable(pool.reward_index);

        assert!(paid <= applied.pool, "paid {paid} of {}", applied.pool);
        assert!(applied.pool - paid < pool.staked);
    }
}
