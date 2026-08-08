use crate::error::SlashError;

/// What a transaction is asking the program to do.
///
/// Hand-decoded rather than derived: the discriminants are wire values that
/// clients in other languages have to reproduce, so they belong somewhere a
/// reader can see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// Create the operator account and post its bond.
    Register { signing_key: [u8; 32], bond: u64 },
    /// Add lamports to the coverage pool.
    Stake { amount: u64 },
    /// Remove lamports from the coverage pool. Rewards already earned stay.
    Unstake { amount: u64 },
    /// Pay out everything this position has earned.
    Claim,
    /// Present two contradictory receipts and slash the bond.
    ProveEquivocation,
    /// Produce the wronged transaction and collect its escrowed share.
    ClaimVictim { wire_bytes: Vec<u8> },
}

impl Instruction {
    pub fn read(data: &[u8]) -> Result<Self, SlashError> {
        let (tag, rest) = data.split_first().ok_or(SlashError::BadInstruction)?;
        let amount = || -> Result<u64, SlashError> {
            rest.get(..8)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u64::from_le_bytes)
                .ok_or(SlashError::BadInstruction)
        };

        match tag {
            0 => Ok(Self::Register {
                signing_key: rest
                    .get(..32)
                    .and_then(|bytes| bytes.try_into().ok())
                    .ok_or(SlashError::BadInstruction)?,
                bond: rest
                    .get(32..40)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(SlashError::BadInstruction)?,
            }),
            1 => Ok(Self::Stake { amount: amount()? }),
            2 => Ok(Self::Unstake { amount: amount()? }),
            3 => Ok(Self::Claim),
            4 => Ok(Self::ProveEquivocation),
            5 => Ok(Self::ClaimVictim {
                wire_bytes: rest.to_vec(),
            }),
            _ => Err(SlashError::BadInstruction),
        }
    }

    pub fn write(&self) -> Vec<u8> {
        match self {
            Self::Register { signing_key, bond } => {
                let mut data = vec![0];
                data.extend_from_slice(signing_key);
                data.extend_from_slice(&bond.to_le_bytes());
                data
            }
            Self::Stake { amount } => {
                let mut data = vec![1];
                data.extend_from_slice(&amount.to_le_bytes());
                data
            }
            Self::Unstake { amount } => {
                let mut data = vec![2];
                data.extend_from_slice(&amount.to_le_bytes());
                data
            }
            Self::Claim => vec![3],
            Self::ProveEquivocation => vec![4],
            Self::ClaimVictim { wire_bytes } => {
                let mut data = vec![5];
                data.extend_from_slice(wire_bytes);
                data
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_instruction_round_trips() {
        for instruction in [
            Instruction::Register {
                signing_key: [0x9c; 32],
                bond: 5_000_000_000,
            },
            Instruction::Stake { amount: 1 },
            Instruction::Unstake { amount: u64::MAX },
            Instruction::Claim,
            Instruction::ProveEquivocation,
            Instruction::ClaimVictim {
                wire_bytes: vec![1, 2, 3],
            },
        ] {
            assert_eq!(
                Instruction::read(&instruction.write()).unwrap(),
                instruction.clone()
            );
        }
    }

    /// Truncated data must be refused rather than read as a smaller number.
    /// A `Stake` whose amount was cut short would otherwise stake whatever
    /// the remaining bytes happened to say.
    #[test]
    fn truncated_data_is_refused() {
        let full = Instruction::Stake { amount: u64::MAX }.write();
        for length in 0..full.len() {
            assert_eq!(
                Instruction::read(&full[..length]),
                Err(SlashError::BadInstruction),
                "length {length}"
            );
        }
    }

    #[test]
    fn an_unknown_tag_is_refused() {
        assert_eq!(Instruction::read(&[9]), Err(SlashError::BadInstruction));
    }
}
