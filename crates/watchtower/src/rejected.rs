use crate::undetermined::Undetermined;

/// Why a receipt is not a statement this watchtower can reason from.
///
/// Neither reason is an accusation. One means somebody other than the operator
/// wrote it; the other means the operator wrote it about a different run of
/// the log. In both cases the honest answer is that it says nothing about the
/// log being watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    ForeignLog,
    Unverifiable,
}

impl Rejected {
    pub fn undetermined(self, seq: u64) -> Undetermined {
        match self {
            Rejected::ForeignLog => Undetermined::ForeignLog { seq },
            Rejected::Unverifiable => Undetermined::UnverifiableReceipt { seq },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_reason_maps_to_its_own_silent_outcome() {
        assert_eq!(
            Rejected::ForeignLog.undetermined(7),
            Undetermined::ForeignLog { seq: 7 }
        );
        assert_eq!(
            Rejected::Unverifiable.undetermined(7),
            Undetermined::UnverifiableReceipt { seq: 7 }
        );
    }
}
