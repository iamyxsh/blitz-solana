use std::collections::HashMap;

use mb_receipt::{LEN_HASH, LEN_TX_SIG, Mode, SignedReceipt};

use crate::observed_transaction::ObservedTransaction;

/// Finds the receipt belonging to a transaction, whichever way it was issued.
///
/// A plain receipt names its transaction's signature. A commit ticket cannot:
/// it was signed while the operator held only a content hash, and the
/// signature lives inside bytes it had not seen. So a ticket is matched by
/// what it *could* commit to — the hash of the contents — and producing bytes
/// that hash to it is the proof that this is the transaction that position
/// was reserved for.
#[derive(Debug, Default)]
pub struct ReceiptIndex {
    by_signature: HashMap<[u8; LEN_TX_SIG], SignedReceipt>,
    by_hash: HashMap<[u8; LEN_HASH], SignedReceipt>,
}

impl ReceiptIndex {
    pub fn build(receipts: &[SignedReceipt]) -> Self {
        let mut index = Self::default();
        for signed in receipts {
            match signed.receipt.mode {
                Mode::Commit => {
                    index.by_hash.insert(signed.receipt.tx_hash, signed.clone());
                }
                _ => {
                    index
                        .by_signature
                        .insert(signed.receipt.tx_sig, signed.clone());
                }
            }
        }
        index
    }

    pub fn find(&self, txn: &ObservedTransaction) -> Option<&SignedReceipt> {
        self.by_signature
            .get(&txn.signature)
            .or_else(|| self.by_hash.get(&txn.tx_hash))
    }

    pub fn is_empty(&self) -> bool {
        self.by_signature.is_empty() && self.by_hash.is_empty()
    }
}
