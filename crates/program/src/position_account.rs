use solana_program::pubkey::Pubkey;

use crate::error::SlashError;

pub const POSITION_LEN: usize = 1 + 32 + 32 + 8 + 16 + 8 + 1;
pub const POSITION_TAG: u8 = 2;
pub const POSITION_SEED: &[u8] = b"position";

/// One staker's claim on an operator's coverage pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionAccount {
    pub owner: Pubkey,
    pub operator: Pubkey,
    pub staked: u64,
    pub entry_index: u128,
    pub reward: u64,
    pub bump: u8,
}

impl PositionAccount {
    pub fn write(&self, into: &mut [u8]) -> Result<(), SlashError> {
        let out = into
            .get_mut(..POSITION_LEN)
            .ok_or(SlashError::BadAccountData)?;
        out[0] = POSITION_TAG;
        out[1..33].copy_from_slice(self.owner.as_ref());
        out[33..65].copy_from_slice(self.operator.as_ref());
        out[65..73].copy_from_slice(&self.staked.to_le_bytes());
        out[73..89].copy_from_slice(&self.entry_index.to_le_bytes());
        out[89..97].copy_from_slice(&self.reward.to_le_bytes());
        out[97] = self.bump;
        Ok(())
    }

    pub fn read(from: &[u8]) -> Result<Self, SlashError> {
        let data = from.get(..POSITION_LEN).ok_or(SlashError::BadAccountData)?;
        if data[0] != POSITION_TAG {
            return Err(SlashError::BadAccountData);
        }
        let arr = |at: usize, len: usize| -> &[u8] { &data[at..at + len] };
        Ok(Self {
            owner: Pubkey::try_from(arr(1, 32)).map_err(|_| SlashError::BadAccountData)?,
            operator: Pubkey::try_from(arr(33, 32)).map_err(|_| SlashError::BadAccountData)?,
            staked: u64::from_le_bytes(arr(65, 8).try_into().unwrap()),
            entry_index: u128::from_le_bytes(arr(73, 16).try_into().unwrap()),
            reward: u64::from_le_bytes(arr(89, 8).try_into().unwrap()),
            bump: data[97],
        })
    }

    pub fn position(&self) -> mb_slashing::Position {
        mb_slashing::Position {
            staked: self.staked,
            entry_index: self.entry_index,
            reward: self.reward,
        }
    }

    pub fn set_position(&mut self, position: mb_slashing::Position) {
        self.staked = position.staked;
        self.entry_index = position.entry_index;
        self.reward = position.reward;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PositionAccount {
        PositionAccount {
            owner: Pubkey::new_from_array([0x33; 32]),
            operator: Pubkey::new_from_array([0x44; 32]),
            staked: 7_000,
            entry_index: 123_456_789,
            reward: 55,
            bump: 253,
        }
    }

    #[test]
    fn round_trips_through_its_account_bytes() {
        let mut buffer = [0u8; POSITION_LEN];
        sample().write(&mut buffer).unwrap();
        assert_eq!(PositionAccount::read(&buffer).unwrap(), sample());
    }

    /// The tags differ so an operator account can never be read as a position,
    /// which would reinterpret the bond as somebody's stake.
    #[test]
    fn an_operator_account_does_not_read_as_a_position() {
        let mut buffer = [0u8; crate::operator_account::OPERATOR_LEN];
        buffer[0] = crate::operator_account::OPERATOR_TAG;
        assert_eq!(
            PositionAccount::read(&buffer),
            Err(SlashError::BadAccountData)
        );
    }
}
