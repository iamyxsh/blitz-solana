use base64::{prelude::BASE64_STANDARD, Engine};
use json::Serialize;
use mb_receipt::Outcome;

/// One row of the receipt log as it appears over JSON-RPC.
///
/// `receipt` is the signed 293 transport bytes, base64 encoded, identical to
/// what `sendTransaction` and `receiptNotification` carry. `seq` and `outcome`
/// sit outside it for different reasons: `seq` is a convenience already inside
/// the signed bytes, while `outcome` is a local, unsigned annotation that has
/// nowhere else to live.
#[derive(Serialize)]
pub(crate) struct ReceiptEntry {
    seq: u64,
    outcome: &'static str,
    receipt: String,
}

impl ReceiptEntry {
    pub(crate) fn new(seq: u64, outcome: u8, receipt: &[u8]) -> Self {
        Self {
            seq,
            outcome: describe(outcome),
            receipt: BASE64_STANDARD.encode(receipt),
        }
    }
}

fn describe(outcome: u8) -> &'static str {
    match Outcome::from_u8(outcome) {
        Some(Outcome::Pending) => "pending",
        Some(Outcome::Accepted) => "accepted",
        Some(Outcome::Rejected) => "rejected",
        Some(Outcome::Expired) => "expired",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_byte_has_a_stable_name() {
        assert_eq!(describe(Outcome::Pending.as_u8()), "pending");
        assert_eq!(describe(Outcome::Accepted.as_u8()), "accepted");
        assert_eq!(describe(Outcome::Rejected.as_u8()), "rejected");
        assert_eq!(describe(Outcome::Expired.as_u8()), "expired");
    }

    /// A byte written by a newer node must not be silently reported as one of
    /// the outcomes this build understands.
    #[test]
    fn an_unrecognised_byte_is_reported_as_unknown() {
        for byte in [0x00, 0x05, 0xff] {
            assert_eq!(describe(byte), "unknown");
        }
    }
}
