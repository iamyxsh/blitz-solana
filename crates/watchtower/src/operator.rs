use ed25519_dalek::VerifyingKey;
use mb_receipt::{LEN_LOG_ID, SignedReceipt};

use crate::rejected::Rejected;

/// The node being watched: whose signature counts, and which run of its log.
///
/// Both halves are needed to decide whether a receipt is a statement about
/// this log. The key alone is not enough, because sequence numbers restart
/// whenever the node does while the key does not, so entries from an earlier
/// run collide with the current one at every position.
///
/// Every check that could name the operator goes through [`Operator::accepts`]
/// first. Keeping that decision in one place is the point of the type: a check
/// that forgets to make it produces an accusation from bytes anybody could
/// have written.
#[derive(Debug, Clone, Copy)]
pub struct Operator {
    pub key: VerifyingKey,
    pub log_id: [u8; LEN_LOG_ID],
}

impl Operator {
    pub fn new(key: VerifyingKey, log_id: [u8; LEN_LOG_ID]) -> Self {
        Self { key, log_id }
    }

    pub fn accepts(&self, signed: &SignedReceipt) -> Result<(), Rejected> {
        if signed.receipt.log_id != self.log_id {
            return Err(Rejected::ForeignLog);
        }
        if signed.verify(&self.key).is_err() {
            return Err(Rejected::Unverifiable);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use mb_receipt::{Mode, Receipt, ZERO_PUBKEY};

    const LOG_ID: [u8; 32] = [0x9c; 32];

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[0x07; 32])
    }

    fn operator() -> Operator {
        Operator::new(key().verifying_key(), LOG_ID)
    }

    fn receipt(log_id: [u8; 32]) -> Receipt {
        Receipt {
            log_id,
            mode: Mode::Plain,
            seq: 0,
            tx_sig: [0xa1; 64],
            tx_hash: [0xb2; 32],
            recent_blockhash: [0xc3; 32],
            prev_receipt_hash: [0u8; 32],
            committer: ZERO_PUBKEY,
            ingress_slot: 1_000,
            t_ingress_micros: 1_700_000_000_000_000,
        }
    }

    #[test]
    fn a_receipt_from_this_log_and_this_key_is_accepted() {
        let signed = receipt(LOG_ID).sign(&key()).unwrap();
        assert_eq!(operator().accepts(&signed), Ok(()));
    }

    /// The restart case. Same operator, same key, same sequence number, an
    /// earlier run of the log.
    #[test]
    fn a_receipt_from_another_run_of_the_log_is_foreign() {
        let signed = receipt([0x01; 32]).sign(&key()).unwrap();
        assert_eq!(operator().accepts(&signed), Err(Rejected::ForeignLog));
    }

    #[test]
    fn a_receipt_signed_by_a_stranger_is_unverifiable() {
        let signed = receipt(LOG_ID)
            .sign(&SigningKey::from_bytes(&[0x09; 32]))
            .unwrap();
        assert_eq!(operator().accepts(&signed), Err(Rejected::Unverifiable));
    }

    /// The log is checked first, so a stranger forging entries for a log this
    /// watchtower does not follow is reported as somebody else's business
    /// rather than as a forgery against this one.
    #[test]
    fn a_foreign_log_is_reported_before_the_signature() {
        let signed = receipt([0x01; 32])
            .sign(&SigningKey::from_bytes(&[0x09; 32]))
            .unwrap();
        assert_eq!(operator().accepts(&signed), Err(Rejected::ForeignLog));
    }
}
