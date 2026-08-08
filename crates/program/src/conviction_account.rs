use solana_program::pubkey::Pubkey;

use crate::error::SlashError;

pub const CONVICTION_LEN: usize = 1 + 32 + 64 + 32 + 8 + 8 + 8 + 1;
pub const CONVICTION_TAG: u8 = 3;
pub const CONVICTION_SEED: &[u8] = b"conviction";

/// A proven fault, and the compensation still owed for it.
///
/// Its address is derived from the contradiction itself, so the same evidence
/// cannot be presented twice: the second attempt collides with an account that
/// already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvictionAccount {
    pub operator: Pubkey,
    /// The transaction the log lied to. A signature, not an address, so the
    /// share is held here until someone produces the transaction itself.
    pub wronged: [u8; 64],
    /// `sha256` of that transaction's wire bytes, copied from the receipt.
    /// Claiming means producing bytes that hash to this — the signature alone
    /// would not do, because nothing here verifies it and anyone could paste
    /// it into a transaction naming themselves as the payer.
    pub wronged_tx_hash: [u8; 32],
    pub slashed: u64,
    pub owed_to_victim: u64,
    pub slot: u64,
    pub bump: u8,
}

impl ConvictionAccount {
    pub fn write(&self, into: &mut [u8]) -> Result<(), SlashError> {
        let out = into
            .get_mut(..CONVICTION_LEN)
            .ok_or(SlashError::BadAccountData)?;
        out[0] = CONVICTION_TAG;
        out[1..33].copy_from_slice(self.operator.as_ref());
        out[33..97].copy_from_slice(&self.wronged);
        out[97..129].copy_from_slice(&self.wronged_tx_hash);
        out[129..137].copy_from_slice(&self.slashed.to_le_bytes());
        out[137..145].copy_from_slice(&self.owed_to_victim.to_le_bytes());
        out[145..153].copy_from_slice(&self.slot.to_le_bytes());
        out[153] = self.bump;
        Ok(())
    }

    pub fn read(from: &[u8]) -> Result<Self, SlashError> {
        let data = from
            .get(..CONVICTION_LEN)
            .ok_or(SlashError::BadAccountData)?;
        if data[0] != CONVICTION_TAG {
            return Err(SlashError::BadAccountData);
        }
        let arr = |at: usize, len: usize| -> &[u8] { &data[at..at + len] };
        Ok(Self {
            operator: Pubkey::try_from(arr(1, 32)).map_err(|_| SlashError::BadAccountData)?,
            wronged: arr(33, 64)
                .try_into()
                .map_err(|_| SlashError::BadAccountData)?,
            wronged_tx_hash: arr(97, 32)
                .try_into()
                .map_err(|_| SlashError::BadAccountData)?,
            slashed: u64::from_le_bytes(arr(129, 8).try_into().unwrap()),
            owed_to_victim: u64::from_le_bytes(arr(137, 8).try_into().unwrap()),
            slot: u64::from_le_bytes(arr(145, 8).try_into().unwrap()),
            bump: data[153],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_its_account_bytes() {
        let conviction = ConvictionAccount {
            operator: Pubkey::new_from_array([0x55; 32]),
            wronged: [0x66; 64],
            wronged_tx_hash: [0x77; 32],
            slashed: 1_000_000,
            owed_to_victim: 300_000,
            slot: 99,
            bump: 252,
        };
        let mut buffer = [0u8; CONVICTION_LEN];
        conviction.write(&mut buffer).unwrap();
        assert_eq!(ConvictionAccount::read(&buffer).unwrap(), conviction);
    }
}
