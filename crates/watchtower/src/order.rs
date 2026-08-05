use crate::observed_block::ObservedBlock;

/// Which reading of a reported block reproduces its hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    AsReported,
    Reversed,
    /// Neither direction reproduces the hash, so nothing about this block's
    /// order can be asserted.
    Unverifiable,
}

impl Order {
    pub fn derive(block: &ObservedBlock) -> Order {
        let forward: Vec<&[u8; 64]> = block
            .transactions
            .iter()
            .map(|txn| &txn.signature)
            .collect();

        if fold(&block.previous_blockhash, forward.iter().copied()) == block.blockhash {
            return Order::AsReported;
        }
        if fold(&block.previous_blockhash, forward.iter().rev().copied()) == block.blockhash {
            return Order::Reversed;
        }
        Order::Unverifiable
    }
}

/// The scheduler's streaming block hash: the previous hash, then every
/// dispatched signature in order.
pub fn fold<'a>(previous: &[u8; 32], signatures: impl Iterator<Item = &'a [u8; 64]>) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(previous);
    for signature in signatures {
        hasher.update(signature);
    }
    *hasher.finalize().as_bytes()
}
