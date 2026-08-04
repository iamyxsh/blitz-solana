pub const OUTCOME_PENDING: u8 = 0x01;
pub const OUTCOME_ACCEPTED: u8 = 0x02;
pub const OUTCOME_REJECTED: u8 = 0x03;

const _: () = assert!(OUTCOME_PENDING != 0 && OUTCOME_ACCEPTED != 0 && OUTCOME_REJECTED != 0);

/// What became of a transaction after its receipt was issued.
///
/// Unsigned, and stored only alongside the receipt rather than inside it. An
/// outcome can therefore only ever *suppress* an accusation, never create one:
/// a client holding a signed receipt for a transaction it knows was valid
/// still has counter-evidence against a false `Rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Outcome {
    /// Sequenced and signed, but its fate is not yet known.
    Pending = OUTCOME_PENDING,
    /// Handed to the scheduler. It occupied a position in a block, whether or
    /// not execution then succeeded.
    Accepted = OUTCOME_ACCEPTED,
    /// Never reached the scheduler, so no position was ever assigned.
    Rejected = OUTCOME_REJECTED,
}

impl Outcome {
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            OUTCOME_PENDING => Some(Outcome::Pending),
            OUTCOME_ACCEPTED => Some(Outcome::Accepted),
            OUTCOME_REJECTED => Some(Outcome::Rejected),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_the_stored_bytes() {
        assert_eq!(Outcome::Pending.as_u8(), 0x01);
        assert_eq!(Outcome::Accepted.as_u8(), 0x02);
        assert_eq!(Outcome::Rejected.as_u8(), 0x03);
    }

    #[test]
    fn every_valid_byte_round_trips() {
        for outcome in [Outcome::Pending, Outcome::Accepted, Outcome::Rejected] {
            assert_eq!(Outcome::from_u8(outcome.as_u8()), Some(outcome));
        }
    }

    #[test]
    fn zero_is_never_a_valid_outcome() {
        assert_eq!(Outcome::from_u8(0x00), None);
    }

    #[test]
    fn rejects_unknown_bytes() {
        for byte in [0x04, 0x7f, 0x80, 0xff] {
            assert_eq!(Outcome::from_u8(byte), None);
        }
    }
}
