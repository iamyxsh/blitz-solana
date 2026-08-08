use crate::error::ReceiptError;
use crate::signed_receipt::SignedReceipt;
use ed25519_dalek::{Signer, SigningKey};
use mb_constants::mode::Mode;
use mb_constants::receipt::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Which run of the log this belongs to. Sequence numbers restart from
    /// zero whenever the node does; the signing key does not, so without this
    /// every entry of a fresh log contradicts the previous incarnation's.
    pub log_id: [u8; LEN_LOG_ID],
    pub mode: Mode,
    pub seq: u64,
    pub tx_sig: [u8; LEN_TX_SIG],
    pub tx_hash: [u8; LEN_HASH],
    pub recent_blockhash: [u8; LEN_HASH],
    pub prev_receipt_hash: [u8; LEN_HASH],
    pub committer: [u8; LEN_PUBKEY],
    pub ingress_slot: u64,
    pub t_ingress_micros: u64,
}

fn arr<const N: usize>(buf: &[u8], off: usize) -> [u8; N] {
    buf[off..off + N]
        .try_into()
        .expect("offset table disagrees with RECEIPT_LEN")
}

impl Receipt {
    pub fn to_bytes(&self) -> [u8; RECEIPT_LEN] {
        let mut out = [0u8; RECEIPT_LEN];
        out[OFF_DOMAIN_TAG..OFF_LOG_ID].copy_from_slice(DOMAIN_TAG);
        out[OFF_LOG_ID..OFF_MODE].copy_from_slice(&self.log_id);
        out[OFF_MODE] = self.mode.as_u8();
        out[OFF_SEQ..OFF_TX_SIG].copy_from_slice(&self.seq.to_le_bytes());
        out[OFF_TX_SIG..OFF_TX_HASH].copy_from_slice(&self.tx_sig);
        out[OFF_TX_HASH..OFF_RECENT_BLOCKHASH].copy_from_slice(&self.tx_hash);
        out[OFF_RECENT_BLOCKHASH..OFF_PREV_RECEIPT_HASH].copy_from_slice(&self.recent_blockhash);
        out[OFF_PREV_RECEIPT_HASH..OFF_COMMITTER].copy_from_slice(&self.prev_receipt_hash);
        out[OFF_COMMITTER..OFF_INGRESS_SLOT].copy_from_slice(&self.committer);
        out[OFF_INGRESS_SLOT..OFF_T_INGRESS_MICROS]
            .copy_from_slice(&self.ingress_slot.to_le_bytes());
        out[OFF_T_INGRESS_MICROS..RECEIPT_LEN]
            .copy_from_slice(&self.t_ingress_micros.to_le_bytes());
        out
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, ReceiptError> {
        if buf.len() != RECEIPT_LEN {
            return Err(ReceiptError::WrongLength(buf.len()));
        }
        if &buf[OFF_DOMAIN_TAG..OFF_LOG_ID] != DOMAIN_TAG {
            return Err(ReceiptError::BadDomainTag);
        }
        let mode_byte = buf[OFF_MODE];
        let receipt = Receipt {
            log_id: arr(buf, OFF_LOG_ID),
            mode: Mode::from_u8(mode_byte).ok_or(ReceiptError::InvalidMode(mode_byte))?,
            seq: u64::from_le_bytes(arr(buf, OFF_SEQ)),
            tx_sig: arr(buf, OFF_TX_SIG),
            tx_hash: arr(buf, OFF_TX_HASH),
            recent_blockhash: arr(buf, OFF_RECENT_BLOCKHASH),
            prev_receipt_hash: arr(buf, OFF_PREV_RECEIPT_HASH),
            committer: arr(buf, OFF_COMMITTER),
            ingress_slot: u64::from_le_bytes(arr(buf, OFF_INGRESS_SLOT)),
            t_ingress_micros: u64::from_le_bytes(arr(buf, OFF_T_INGRESS_MICROS)),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), ReceiptError> {
        match self.mode {
            Mode::Plain => {
                if self.tx_sig == ZERO_SIG {
                    return Err(ReceiptError::TxSigZeroed);
                }
                if self.committer != ZERO_PUBKEY {
                    return Err(ReceiptError::CommitterNotZeroed);
                }
            }
            Mode::Commit => {
                if self.tx_sig != ZERO_SIG {
                    return Err(ReceiptError::TxSigNotZeroed);
                }
                if self.committer == ZERO_PUBKEY {
                    return Err(ReceiptError::CommitterZeroed);
                }
            }
            // `tx_hash` holds the withdrawn receipt's hash here, not a
            // transaction hash: the mode byte is what tells the two apart.
            Mode::Retract => {
                if self.tx_sig == ZERO_SIG {
                    return Err(ReceiptError::TxSigZeroed);
                }
                if self.tx_hash == ZERO_HASH {
                    return Err(ReceiptError::TxHashZeroed);
                }
                if self.recent_blockhash != ZERO_HASH {
                    return Err(ReceiptError::RecentBlockhashNotZeroed);
                }
                if self.committer != ZERO_PUBKEY {
                    return Err(ReceiptError::CommitterNotZeroed);
                }
            }
            Mode::Reveal => return Err(ReceiptError::ModeReserved),
        }
        Ok(())
    }

    pub fn sign(&self, key: &SigningKey) -> Result<SignedReceipt, ReceiptError> {
        self.validate()?;
        Ok(SignedReceipt {
            receipt: self.clone(),
            signature: key.sign(&self.to_bytes()).to_bytes(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use mb_constants::mode::{MODE_COMMIT, MODE_PLAIN, MODE_RETRACT};

    #[test]
    fn every_field_lands_at_its_documented_offset() {
        let receipt = Receipt {
            seq: 0x0102_0304_0506_0708,
            tx_sig: [0xa1; 64],
            tx_hash: [0xb2; 32],
            recent_blockhash: [0xc3; 32],
            prev_receipt_hash: [0xd4; 32],
            ingress_slot: 0x1112_1314_1516_1718,
            t_ingress_micros: 0x2122_2324_2526_2728,
            ..fixtures::plain()
        };
        let bytes = receipt.to_bytes();

        assert_eq!(bytes.len(), RECEIPT_LEN);
        assert_eq!(&bytes[OFF_DOMAIN_TAG..OFF_LOG_ID], b"MBRECEIPT_V1");
        assert_eq!(&bytes[OFF_LOG_ID..OFF_MODE], &fixtures::LOG_ID);
        assert_eq!(bytes[OFF_MODE], MODE_PLAIN);
        assert_eq!(&bytes[OFF_TX_SIG..OFF_TX_HASH], &[0xa1; 64]);
        assert_eq!(&bytes[OFF_TX_HASH..OFF_RECENT_BLOCKHASH], &[0xb2; 32]);
        assert_eq!(
            &bytes[OFF_RECENT_BLOCKHASH..OFF_PREV_RECEIPT_HASH],
            &[0xc3; 32]
        );
        assert_eq!(&bytes[OFF_PREV_RECEIPT_HASH..OFF_COMMITTER], &[0xd4; 32]);
        assert_eq!(&bytes[OFF_COMMITTER..OFF_INGRESS_SLOT], &ZERO_PUBKEY);
    }

    /// The whole point of the field. The same statement about two runs of the
    /// log is two different signed messages, so it can never be read as one
    /// operator contradicting itself at one position.
    #[test]
    fn the_log_id_separates_otherwise_identical_receipts() {
        let key = fixtures::operator_key();
        let first = fixtures::plain();
        let second = Receipt {
            log_id: [0x01; 32],
            ..fixtures::plain()
        };

        assert_eq!(first.seq, second.seq);
        assert_ne!(first.to_bytes(), second.to_bytes());
        assert_ne!(
            first.sign(&key).unwrap().signature,
            second.sign(&key).unwrap().signature
        );
    }

    #[test]
    fn every_integer_is_little_endian() {
        let receipt = Receipt {
            seq: 0x0102_0304_0506_0708,
            ingress_slot: 0x1112_1314_1516_1718,
            t_ingress_micros: 0x2122_2324_2526_2728,
            ..fixtures::plain()
        };
        let bytes = receipt.to_bytes();

        assert_eq!(
            &bytes[OFF_SEQ..OFF_TX_SIG],
            &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
        assert_eq!(
            &bytes[OFF_INGRESS_SLOT..OFF_T_INGRESS_MICROS],
            &[0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11]
        );
        assert_eq!(
            &bytes[OFF_T_INGRESS_MICROS..RECEIPT_LEN],
            &[0x28, 0x27, 0x26, 0x25, 0x24, 0x23, 0x22, 0x21]
        );
    }

    #[test]
    fn commit_mode_zeroes_tx_sig_and_carries_the_committer() {
        let bytes = fixtures::commit().to_bytes();
        assert_eq!(bytes[OFF_MODE], MODE_COMMIT);
        assert_eq!(&bytes[OFF_TX_SIG..OFF_TX_HASH], &ZERO_SIG);
        assert_eq!(&bytes[OFF_COMMITTER..OFF_INGRESS_SLOT], &[0xe5; 32]);
    }

    /// A retraction names two things: the transaction being withdrawn, so a
    /// verifier knows what to search blocks for, and the receipt that promised
    /// it, so the withdrawal is bound to one signed statement rather than to a
    /// position in general.
    #[test]
    fn retract_mode_names_a_transaction_and_the_receipt_it_withdraws() {
        let bytes = fixtures::retract().to_bytes();
        assert_eq!(bytes[OFF_MODE], MODE_RETRACT);
        assert_ne!(&bytes[OFF_TX_SIG..OFF_TX_HASH], &ZERO_SIG);
        assert_eq!(&bytes[OFF_TX_HASH..OFF_RECENT_BLOCKHASH], &[0xf6; 32]);
        assert_eq!(
            &bytes[OFF_RECENT_BLOCKHASH..OFF_PREV_RECEIPT_HASH],
            &ZERO_HASH
        );
        assert_eq!(&bytes[OFF_COMMITTER..OFF_INGRESS_SLOT], &ZERO_PUBKEY);
    }

    #[test]
    fn round_trips_through_bytes() {
        for receipt in [fixtures::plain(), fixtures::commit(), fixtures::retract()] {
            assert_eq!(Receipt::from_bytes(&receipt.to_bytes()).unwrap(), receipt);
        }
    }

    #[track_caller]
    fn assert_rejected_everywhere(receipt: &Receipt, expected: ReceiptError) {
        assert_eq!(receipt.validate().unwrap_err(), expected);
        assert_eq!(
            Receipt::from_bytes(&receipt.to_bytes()).unwrap_err(),
            expected
        );
        assert_eq!(
            receipt.sign(&fixtures::operator_key()).unwrap_err(),
            expected
        );
    }

    #[test]
    fn plain_mode_rejects_a_committer() {
        let receipt = Receipt {
            committer: [0x55; 32],
            ..fixtures::plain()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::CommitterNotZeroed);
    }

    #[test]
    fn plain_mode_rejects_a_zeroed_tx_sig() {
        let receipt = Receipt {
            tx_sig: ZERO_SIG,
            ..fixtures::plain()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::TxSigZeroed);
    }

    #[test]
    fn commit_mode_rejects_a_populated_tx_sig() {
        let receipt = Receipt {
            tx_sig: [0x11; 64],
            ..fixtures::commit()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::TxSigNotZeroed);
    }

    #[test]
    fn commit_mode_rejects_a_zeroed_committer() {
        let receipt = Receipt {
            committer: ZERO_PUBKEY,
            ..fixtures::commit()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::CommitterZeroed);
    }

    #[test]
    fn retract_mode_rejects_a_zeroed_tx_sig() {
        let receipt = Receipt {
            tx_sig: ZERO_SIG,
            ..fixtures::retract()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::TxSigZeroed);
    }

    /// Without a bound receipt hash a retraction voids a position in general
    /// rather than one statement in particular, which is an excuse nobody can
    /// check rather than a withdrawal anybody can.
    #[test]
    fn retract_mode_rejects_a_zeroed_tx_hash() {
        let receipt = Receipt {
            tx_hash: ZERO_HASH,
            ..fixtures::retract()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::TxHashZeroed);
    }

    #[test]
    fn retract_mode_rejects_a_recent_blockhash() {
        let receipt = Receipt {
            recent_blockhash: [0xc3; 32],
            ..fixtures::retract()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::RecentBlockhashNotZeroed);
    }

    #[test]
    fn retract_mode_rejects_a_committer() {
        let receipt = Receipt {
            committer: [0x55; 32],
            ..fixtures::retract()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::CommitterNotZeroed);
    }

    #[test]
    fn reveal_mode_is_reserved() {
        let receipt = Receipt {
            mode: Mode::Reveal,
            ..fixtures::plain()
        };
        assert_rejected_everywhere(&receipt, ReceiptError::ModeReserved);
    }

    #[test]
    fn rejects_unknown_mode_bytes() {
        let mut bytes = fixtures::plain().to_bytes();
        for byte in [0x00, 0x05, 0xff] {
            bytes[OFF_MODE] = byte;
            assert_eq!(
                Receipt::from_bytes(&bytes).unwrap_err(),
                ReceiptError::InvalidMode(byte)
            );
        }
    }

    #[test]
    fn rejects_a_foreign_domain_tag() {
        let mut bytes = fixtures::plain().to_bytes();
        bytes[0] = b'X';
        assert_eq!(
            Receipt::from_bytes(&bytes).unwrap_err(),
            ReceiptError::BadDomainTag
        );
    }

    #[test]
    fn rejects_wrong_lengths() {
        let bytes = fixtures::plain().to_bytes();
        assert_eq!(
            Receipt::from_bytes(&bytes[..RECEIPT_LEN - 1]).unwrap_err(),
            ReceiptError::WrongLength(RECEIPT_LEN - 1)
        );
        assert_eq!(
            Receipt::from_bytes(&[]).unwrap_err(),
            ReceiptError::WrongLength(0)
        );
    }
}
