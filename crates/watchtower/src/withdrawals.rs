use std::collections::HashMap;

use mb_receipt::{LEN_HASH, Mode, SignedReceipt};

use crate::operator::Operator;

/// Which promises the operator has publicly taken back.
///
/// Keyed by the hash of the receipt being withdrawn, which is what a
/// retraction carries in `tx_hash`.
///
/// Nothing enters this map unverified. Every other index in the watchtower
/// tolerates junk because a missing entry only ever produces a claim about an
/// absence, which is safe to be wrong about in the operator's favour. A
/// *present* forged retraction is the opposite: it would turn transactions the
/// operator honestly executed into provable misbehaviour. So the operator's
/// key is required to build one at all.
#[derive(Debug, Default)]
pub struct Withdrawals {
    by_withdrawn_hash: HashMap<[u8; LEN_HASH], SignedReceipt>,
}

impl Withdrawals {
    pub fn build(receipts: &[SignedReceipt], operator: &Operator) -> Self {
        let mut withdrawals = Self::default();
        for signed in receipts {
            if signed.receipt.mode != Mode::Retract || operator.accepts(signed).is_err() {
                continue;
            }
            withdrawals
                .by_withdrawn_hash
                .insert(signed.receipt.tx_hash, signed.clone());
        }
        withdrawals
    }

    /// The retraction that withdrew this receipt, if the operator issued one.
    pub fn of(&self, receipt: &SignedReceipt) -> Option<&SignedReceipt> {
        self.by_withdrawn_hash.get(&receipt.receipt_hash())
    }

    pub fn len(&self) -> usize {
        self.by_withdrawn_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_withdrawn_hash.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use mb_receipt::{Receipt, ZERO_HASH, ZERO_PUBKEY};

    const LOG_ID: [u8; 32] = [0x9c; 32];

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[0x07; 32])
    }

    fn stranger() -> SigningKey {
        SigningKey::from_bytes(&[0x09; 32])
    }

    fn operator() -> Operator {
        Operator::new(key().verifying_key(), LOG_ID)
    }

    fn promise(seq: u64) -> SignedReceipt {
        Receipt {
            log_id: LOG_ID,
            mode: Mode::Plain,
            seq,
            tx_sig: [(seq as u8) | 0x80; 64],
            tx_hash: [(seq as u8) | 0x40; 32],
            recent_blockhash: [0x33; 32],
            prev_receipt_hash: [0x44; 32],
            committer: ZERO_PUBKEY,
            ingress_slot: 1_000 + seq,
            t_ingress_micros: 1_700_000_000_000_000 + seq,
        }
        .sign(&key())
        .unwrap()
    }

    fn withdrawal_of(seq: u64, withdrawn: &SignedReceipt, key: &SigningKey) -> SignedReceipt {
        Receipt {
            log_id: LOG_ID,
            mode: Mode::Retract,
            seq,
            tx_sig: withdrawn.receipt.tx_sig,
            tx_hash: withdrawn.receipt_hash(),
            recent_blockhash: ZERO_HASH,
            prev_receipt_hash: [0x44; 32],
            committer: ZERO_PUBKEY,
            ingress_slot: 1_000 + seq,
            t_ingress_micros: 1_700_000_000_000_000 + seq,
        }
        .sign(key)
        .unwrap()
    }

    #[test]
    fn a_withdrawal_is_found_by_the_receipt_it_names() {
        let taken_back = promise(0);
        let standing = promise(1);
        let withdrawal = withdrawal_of(2, &taken_back, &key());

        let withdrawals = Withdrawals::build(
            &[taken_back.clone(), standing.clone(), withdrawal.clone()],
            &operator(),
        );

        assert_eq!(withdrawals.len(), 1);
        assert_eq!(withdrawals.of(&taken_back), Some(&withdrawal));
        assert_eq!(withdrawals.of(&standing), None);
    }

    /// Anyone can append bytes to a public log. If those bytes could withdraw a
    /// receipt, every transaction the operator honestly ran would become
    /// evidence against it.
    #[test]
    fn a_withdrawal_signed_by_a_stranger_is_not_indexed() {
        let standing = promise(0);
        let forged = withdrawal_of(2, &standing, &stranger());

        let withdrawals = Withdrawals::build(&[standing.clone(), forged], &operator());

        assert!(withdrawals.is_empty());
        assert_eq!(withdrawals.of(&standing), None);
    }

    /// A withdrawal issued in an earlier run of the log takes nothing back in
    /// this one, even though the operator genuinely signed it.
    #[test]
    fn a_withdrawal_from_another_log_is_not_indexed() {
        let standing = promise(0);
        let elsewhere = Receipt {
            log_id: [0x01; 32],
            ..withdrawal_of(2, &standing, &key()).receipt
        }
        .sign(&key())
        .unwrap();

        let withdrawals = Withdrawals::build(&[standing.clone(), elsewhere], &operator());

        assert!(withdrawals.is_empty());
        assert_eq!(withdrawals.of(&standing), None);
    }

    #[test]
    fn ordinary_receipts_withdraw_nothing() {
        let withdrawals = Withdrawals::build(&[promise(0), promise(1), promise(2)], &operator());

        assert!(withdrawals.is_empty());
    }
}
