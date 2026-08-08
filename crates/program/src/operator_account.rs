use solana_program::pubkey::Pubkey;

use crate::error::SlashError;

pub const OPERATOR_LEN: usize = 1 + 32 + 32 + 8 + 8 + 16 + 8 + 1;
pub const OPERATOR_TAG: u8 = 1;
pub const OPERATOR_SEED: &[u8] = b"operator";

/// A staked sequencer, and the coverage pool riding on it.
///
/// `signing_key` is fixed at registration and never changes. Rotation would
/// let an operator disown receipts it had already issued; to use a new key it
/// registers again and posts a new bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorAccount {
    pub authority: Pubkey,
    pub signing_key: [u8; 32],
    pub bond: u64,
    pub pool_staked: u64,
    pub reward_index: u128,
    /// Base-chain slot the bond becomes withdrawable. Zero while bonded.
    pub unbond_at: u64,
    pub bump: u8,
}

impl OperatorAccount {
    pub fn write(&self, into: &mut [u8]) -> Result<(), SlashError> {
        let out = into
            .get_mut(..OPERATOR_LEN)
            .ok_or(SlashError::BadAccountData)?;
        out[0] = OPERATOR_TAG;
        out[1..33].copy_from_slice(self.authority.as_ref());
        out[33..65].copy_from_slice(&self.signing_key);
        out[65..73].copy_from_slice(&self.bond.to_le_bytes());
        out[73..81].copy_from_slice(&self.pool_staked.to_le_bytes());
        out[81..97].copy_from_slice(&self.reward_index.to_le_bytes());
        out[97..105].copy_from_slice(&self.unbond_at.to_le_bytes());
        out[105] = self.bump;
        Ok(())
    }

    pub fn read(from: &[u8]) -> Result<Self, SlashError> {
        let data = from.get(..OPERATOR_LEN).ok_or(SlashError::BadAccountData)?;
        if data[0] != OPERATOR_TAG {
            return Err(SlashError::BadAccountData);
        }
        let arr = |at: usize, len: usize| -> &[u8] { &data[at..at + len] };
        Ok(Self {
            authority: Pubkey::try_from(arr(1, 32)).map_err(|_| SlashError::BadAccountData)?,
            signing_key: arr(33, 32)
                .try_into()
                .map_err(|_| SlashError::BadAccountData)?,
            bond: u64::from_le_bytes(arr(65, 8).try_into().unwrap()),
            pool_staked: u64::from_le_bytes(arr(73, 8).try_into().unwrap()),
            reward_index: u128::from_le_bytes(arr(81, 16).try_into().unwrap()),
            unbond_at: u64::from_le_bytes(arr(97, 8).try_into().unwrap()),
            bump: data[105],
        })
    }

    pub fn pool(&self) -> mb_slashing::Pool {
        mb_slashing::Pool {
            staked: self.pool_staked,
            reward_index: self.reward_index,
        }
    }

    pub fn set_pool(&mut self, pool: mb_slashing::Pool) {
        self.pool_staked = pool.staked;
        self.reward_index = pool.reward_index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OperatorAccount {
        OperatorAccount {
            authority: Pubkey::new_from_array([0x11; 32]),
            signing_key: [0x22; 32],
            bond: 5_000_000_000,
            pool_staked: 1_234,
            reward_index: 987_654_321_098_765_432_109,
            unbond_at: 42,
            bump: 254,
        }
    }

    #[test]
    fn round_trips_through_its_account_bytes() {
        let mut buffer = [0u8; OPERATOR_LEN];
        sample().write(&mut buffer).unwrap();
        assert_eq!(OperatorAccount::read(&buffer).unwrap(), sample());
    }

    /// A zeroed account is not an operator. Without the tag, a freshly created
    /// account reads as one holding no bond and any key.
    #[test]
    fn an_untagged_account_is_refused() {
        assert_eq!(
            OperatorAccount::read(&[0u8; OPERATOR_LEN]),
            Err(SlashError::BadAccountData)
        );
    }

    #[test]
    fn a_short_account_is_refused() {
        let mut buffer = [0u8; OPERATOR_LEN];
        sample().write(&mut buffer).unwrap();
        assert_eq!(
            OperatorAccount::read(&buffer[..OPERATOR_LEN - 1]),
            Err(SlashError::BadAccountData)
        );
    }
}
