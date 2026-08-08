use mb_constants::slashing::REWARD_SCALE;

/// One staker's claim on the coverage pool.
///
/// `entry_index` is the pool's reward index at the moment this position last
/// settled. Everything the pool has distributed since then is owed to it, and
/// everything before belongs to whoever was staked at the time — which is why
/// the index is recorded on the way in rather than a share count on the way
/// out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub staked: u64,
    pub entry_index: u128,
    /// Settled but unwithdrawn rewards, banked when the stake changes.
    pub reward: u64,
}

impl Position {
    /// Rewards accrued since this position last settled.
    pub fn accrued(&self, index: u128) -> u64 {
        // Bounded by the total ever distributed: `staked` never exceeds the
        // pool's total, and the index moves by amount * SCALE / total.
        let delta = index.saturating_sub(self.entry_index);
        ((self.staked as u128 * delta) / REWARD_SCALE) as u64
    }

    pub fn claimable(&self, index: u128) -> u64 {
        self.reward + self.accrued(index)
    }

    /// Moves everything accrued into `reward` and re-anchors to `index`.
    pub(crate) fn settle(&mut self, index: u128) {
        self.reward += self.accrued(index);
        self.entry_index = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_anchored_at_the_current_index_has_accrued_nothing() {
        let position = Position {
            staked: 1_000,
            entry_index: 5 * REWARD_SCALE,
            reward: 0,
        };
        assert_eq!(position.accrued(5 * REWARD_SCALE), 0);
    }

    #[test]
    fn accrual_is_stake_times_index_movement() {
        let position = Position {
            staked: 1_000,
            entry_index: 0,
            reward: 0,
        };
        // The index moved by 2 whole units of reward per lamport staked.
        assert_eq!(position.accrued(2 * REWARD_SCALE), 2_000);
    }

    /// The index only ever rises, but a caller passing a stale one must not
    /// wrap into an enormous payout.
    #[test]
    fn an_index_below_the_entry_accrues_nothing() {
        let position = Position {
            staked: 1_000,
            entry_index: 9 * REWARD_SCALE,
            reward: 0,
        };
        assert_eq!(position.accrued(0), 0);
    }

    #[test]
    fn settling_banks_the_accrual_and_re_anchors() {
        let mut position = Position {
            staked: 1_000,
            entry_index: 0,
            reward: 7,
        };
        position.settle(2 * REWARD_SCALE);

        assert_eq!(position.reward, 2_007);
        assert_eq!(position.entry_index, 2 * REWARD_SCALE);
        assert_eq!(position.accrued(2 * REWARD_SCALE), 0);
    }
}
