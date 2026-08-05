use ed25519_dalek::VerifyingKey;
use mb_receipt::{SignedReceipt, tx_hash};

use crate::fault::FaultError;

/// One side of a reorder: the receipt the operator signed, the transaction it
/// commits to, and where that transaction actually ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderedTransaction {
    pub receipt: SignedReceipt,
    /// Bound to the receipt by `tx_hash`, so a verifier can re-derive the
    /// account sets rather than trust an assertion about them.
    pub wire_bytes: Vec<u8>,
    pub index: u32,
}

impl ReorderedTransaction {
    pub fn verify(&self, operator: &VerifyingKey) -> Result<(), FaultError> {
        self.receipt
            .verify(operator)
            .map_err(|_| FaultError::NotSigned(self.receipt.receipt.seq))?;
        if tx_hash(&self.wire_bytes) != self.receipt.receipt.tx_hash {
            return Err(FaultError::ReceiptDoesNotBindTransaction);
        }
        Ok(())
    }
}
