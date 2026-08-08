use mb_constants::slashing::{BPS_DENOMINATOR, POOL_BPS, VICTIM_BPS};

/// Where a slashed bond goes.
///
/// The three shares always sum to exactly what was taken. Integer division
/// leaves a remainder of at most two lamports, and it is added to the burn
/// rather than dropped: a slash that destroys one lamport more than the table
/// says is conservative, whereas one that pays out more is a hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub burn: u64,
    pub victim: u64,
    pub pool: u64,
}

impl Split {
    pub fn of(slashed: u64) -> Self {
        let share = |bps: u64| ((slashed as u128 * bps as u128) / BPS_DENOMINATOR as u128) as u64;

        let victim = share(VICTIM_BPS);
        let pool = share(POOL_BPS);
        Self {
            burn: slashed - victim - pool,
            victim,
            pool,
        }
    }

    pub fn total(&self) -> u64 {
        self.burn + self.victim + self.pool
    }

    /// What the pool cannot take, because nobody has staked it.
    ///
    /// Rather than hold an unowned balance, the pool's share is burned. Paying
    /// it to the next staker to arrive would reward capital that carried none
    /// of the risk.
    pub fn without_pool(self) -> Self {
        Self {
            burn: self.burn + self.pool,
            pool: 0,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant that matters most: a slash neither creates nor destroys
    /// lamports beyond what it took. Checked across sizes that all divide
    /// badly by the basis points.
    #[test]
    fn the_shares_always_sum_to_what_was_taken() {
        for slashed in [0, 1, 2, 3, 7, 9_999, 10_001, 1_234_567, u64::MAX] {
            assert_eq!(Split::of(slashed).total(), slashed, "slashed {slashed}");
        }
    }

    /// Rounding is conservative. A payout share that rounded up would pay out
    /// lamports the slash never took.
    #[test]
    fn rounding_never_favours_a_payout() {
        for slashed in 0..1_000u64 {
            let split = Split::of(slashed);
            assert!(split.victim * BPS_DENOMINATOR <= slashed * VICTIM_BPS);
            assert!(split.pool * BPS_DENOMINATOR <= slashed * POOL_BPS);
        }
    }

    /// The burn is computed as the remainder so the sum is always exact, which
    /// means nothing else ties it to `BURN_BPS`. This does.
    #[test]
    fn a_round_slash_splits_by_the_table() {
        let split = Split::of(BPS_DENOMINATOR);
        assert_eq!(split.burn, mb_constants::slashing::BURN_BPS);
        assert_eq!(split.victim, VICTIM_BPS);
        assert_eq!(split.pool, POOL_BPS);
    }

    /// An empty pool must not silently hold lamports nobody can claim.
    #[test]
    fn an_unstaked_pool_share_is_burned_instead() {
        let split = Split::of(10_000).without_pool();
        assert_eq!(split.pool, 0);
        assert_eq!(split.burn, 7_000);
        assert_eq!(split.total(), 10_000);
    }
}
