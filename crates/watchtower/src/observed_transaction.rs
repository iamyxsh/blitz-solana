/// One transaction as a watchtower sees it in a block.
///
/// Account sets arrive already extracted: parsing Solana messages belongs to
/// the ingestion layer, so the detection core stays testable without building
/// real transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedTransaction {
    pub signature: [u8; 64],
    /// `sha256(wire_bytes)` — what a commit ticket binds instead of a
    /// signature it could not have known.
    pub tx_hash: [u8; 32],
    /// Identifies operator-issued work: the validator signs its own
    /// transactions with its identity.
    pub fee_payer: [u8; 32],
    pub writable: Vec<[u8; 32]>,
    pub readonly: Vec<[u8; 32]>,
    /// Exactly the bytes the receipt's `tx_hash` commits to.
    pub wire_bytes: Vec<u8>,
}

impl ObservedTransaction {
    pub fn is_issued_by(&self, identity: &[u8; 32]) -> bool {
        &self.fee_payer == identity
    }
}
