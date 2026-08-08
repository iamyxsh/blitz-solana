pub const MODE_PLAIN: u8 = 0x01;
pub const MODE_COMMIT: u8 = 0x02;
pub const MODE_REVEAL: u8 = 0x03;
pub const MODE_RETRACT: u8 = 0x04;

const _: () = assert!(MODE_PLAIN != 0 && MODE_COMMIT != 0 && MODE_REVEAL != 0 && MODE_RETRACT != 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Plain = MODE_PLAIN,
    Commit = MODE_COMMIT,
    Reveal = MODE_REVEAL,
    /// Withdraws a position the operator issued and could not honour. It takes
    /// its own sequence number rather than the one it voids, so the log keeps
    /// exactly one statement per position and the chain stays linear.
    Retract = MODE_RETRACT,
}

impl Mode {
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            MODE_PLAIN => Some(Mode::Plain),
            MODE_COMMIT => Some(Mode::Commit),
            MODE_REVEAL => Some(Mode::Reveal),
            MODE_RETRACT => Some(Mode::Retract),
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
    fn discriminants_are_the_wire_bytes() {
        assert_eq!(Mode::Plain.as_u8(), 0x01);
        assert_eq!(Mode::Commit.as_u8(), 0x02);
        assert_eq!(Mode::Reveal.as_u8(), 0x03);
        assert_eq!(Mode::Retract.as_u8(), 0x04);
    }

    #[test]
    fn every_valid_byte_round_trips() {
        for mode in [Mode::Plain, Mode::Commit, Mode::Reveal, Mode::Retract] {
            assert_eq!(Mode::from_u8(mode.as_u8()), Some(mode));
        }
    }

    #[test]
    fn zero_is_never_a_valid_mode() {
        assert_eq!(Mode::from_u8(0x00), None);
    }

    #[test]
    fn rejects_unknown_bytes() {
        for byte in [0x05, 0x7f, 0x80, 0xff] {
            assert_eq!(Mode::from_u8(byte), None);
        }
    }
}
