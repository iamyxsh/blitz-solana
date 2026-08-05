/// Where a transaction actually ran.
///
/// Always carried as a pair. A bare index means nothing across slots, because
/// it restarts at zero every time the scheduler advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Execution {
    pub slot: u64,
    pub index: u32,
}

use std::collections::HashMap;

use mb_receipt::{Mode, SignedReceipt};

/// Where each transaction ran, reachable by signature or by content hash.
///
/// Both are needed because a commit ticket knows only the hash, and asking
/// "did this ever execute?" has to work for tickets as well as receipts.
#[derive(Debug, Default)]
pub struct ExecutionIndex {
    by_signature: HashMap<[u8; 64], Execution>,
    by_hash: HashMap<[u8; 32], Execution>,
}

impl ExecutionIndex {
    pub fn record(&mut self, signature: [u8; 64], tx_hash: [u8; 32], execution: Execution) {
        self.by_signature.insert(signature, execution);
        self.by_hash.insert(tx_hash, execution);
    }

    /// Where the transaction this receipt refers to actually ran.
    pub fn find(&self, receipt: &SignedReceipt) -> Option<Execution> {
        match receipt.receipt.mode {
            Mode::Commit => self.by_hash.get(&receipt.receipt.tx_hash).copied(),
            _ => self.by_signature.get(&receipt.receipt.tx_sig).copied(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_signature.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_signature.is_empty()
    }
}
