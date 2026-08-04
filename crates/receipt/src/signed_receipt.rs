use crate::error::ReceiptError;
use crate::receipt::Receipt;
use ed25519_dalek::{Signature, VerifyingKey};
use mb_constants::receipt::{LEN_HASH, LEN_TX_SIG, OFF_SIGNATURE, RECEIPT_LEN, SIGNED_RECEIPT_LEN};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedReceipt {
    pub receipt: Receipt,
    pub signature: [u8; LEN_TX_SIG],
}

impl SignedReceipt {
    pub fn message(&self) -> [u8; RECEIPT_LEN] {
        self.receipt.to_bytes()
    }

    pub fn receipt_hash(&self) -> [u8; LEN_HASH] {
        let mut hasher = Sha256::new();
        hasher.update(self.receipt.to_bytes());
        hasher.update(self.signature);
        hasher.finalize().into()
    }

    /// The transport encoding: the signed message followed by its signature.
    /// `receipt_hash` is the sha256 of exactly these bytes.
    pub fn to_bytes(&self) -> [u8; SIGNED_RECEIPT_LEN] {
        let mut out = [0u8; SIGNED_RECEIPT_LEN];
        out[..OFF_SIGNATURE].copy_from_slice(&self.receipt.to_bytes());
        out[OFF_SIGNATURE..].copy_from_slice(&self.signature);
        out
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, ReceiptError> {
        if buf.len() != SIGNED_RECEIPT_LEN {
            return Err(ReceiptError::WrongLength(buf.len()));
        }
        Ok(SignedReceipt {
            receipt: Receipt::from_bytes(&buf[..OFF_SIGNATURE])?,
            signature: buf[OFF_SIGNATURE..]
                .try_into()
                .expect("signature slice is LEN_SIGNATURE bytes"),
        })
    }

    pub fn verify(&self, key: &VerifyingKey) -> Result<(), ReceiptError> {
        self.receipt.validate()?;
        key.verify_strict(&self.message(), &Signature::from_bytes(&self.signature))
            .map_err(|_| ReceiptError::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use crate::hashing::tx_hash;

    #[test]
    fn an_honest_receipt_verifies() {
        let key = fixtures::operator_key();
        let signed = fixtures::plain().sign(&key).unwrap();
        assert!(signed.verify(&key.verifying_key()).is_ok());
    }

    #[test]
    fn a_different_key_does_not_verify() {
        let signed = fixtures::plain().sign(&fixtures::operator_key()).unwrap();
        let stranger = ed25519_dalek::SigningKey::from_bytes(&[0x09; 32]);
        assert_eq!(
            signed.verify(&stranger.verifying_key()).unwrap_err(),
            ReceiptError::BadSignature
        );
    }

    #[test]
    fn chain_link_covers_the_signature_not_just_the_message() {
        let signed = fixtures::plain().sign(&fixtures::operator_key()).unwrap();

        let mut tampered = signed.clone();
        tampered.signature[0] ^= 0x01;
        assert_ne!(signed.receipt_hash(), tampered.receipt_hash());
        assert_ne!(signed.receipt_hash(), tx_hash(&signed.message()));
    }

    #[test]
    fn verify_rejects_every_tampered_field() {
        let key = fixtures::operator_key();
        let signed = fixtures::plain().sign(&key).unwrap();
        let original = &signed.receipt;

        let forgeries = [
            Receipt {
                seq: original.seq + 1,
                ..original.clone()
            },
            Receipt {
                ingress_slot: original.ingress_slot - 1,
                ..original.clone()
            },
            Receipt {
                tx_hash: [0x00; 32],
                ..original.clone()
            },
            Receipt {
                prev_receipt_hash: [0xff; 32],
                ..original.clone()
            },
        ];

        for receipt in forgeries {
            let forged = SignedReceipt {
                receipt,
                signature: signed.signature,
            };
            assert_eq!(
                forged.verify(&key.verifying_key()).unwrap_err(),
                ReceiptError::BadSignature
            );
        }
    }

    #[test]
    fn round_trips_through_the_transport_encoding() {
        let signed = fixtures::plain().sign(&fixtures::operator_key()).unwrap();
        let bytes = signed.to_bytes();

        assert_eq!(bytes.len(), 293);
        assert_eq!(&bytes[..RECEIPT_LEN], &signed.message());
        assert_eq!(&bytes[RECEIPT_LEN..], &signed.signature);
        assert_eq!(SignedReceipt::from_bytes(&bytes).unwrap(), signed);
    }

    #[test]
    fn the_chain_link_is_the_digest_of_the_transport_encoding() {
        let signed = fixtures::plain().sign(&fixtures::operator_key()).unwrap();
        assert_eq!(signed.receipt_hash(), tx_hash(&signed.to_bytes()));
    }

    #[test]
    fn transport_decoding_rejects_wrong_lengths() {
        let signed = fixtures::plain().sign(&fixtures::operator_key()).unwrap();
        let bytes = signed.to_bytes();

        for truncated in [0, RECEIPT_LEN, SIGNED_RECEIPT_LEN - 1] {
            assert_eq!(
                SignedReceipt::from_bytes(&bytes[..truncated]).unwrap_err(),
                ReceiptError::WrongLength(truncated)
            );
        }
    }

    #[test]
    fn signing_is_deterministic() {
        let key = fixtures::operator_key();
        assert_eq!(
            fixtures::plain().sign(&key).unwrap(),
            fixtures::plain().sign(&key).unwrap()
        );
    }
}
