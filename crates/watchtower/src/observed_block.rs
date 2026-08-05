use crate::{observed_transaction::ObservedTransaction, order::Order};

/// One block as reported by a node, in whatever order the source returned it.
///
/// The reported order is deliberately untrusted. `getBlock` on this validator
/// hands transactions back newest-index-first, so anything reading execution
/// order straight from it sees an honest slot fully reversed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedBlock {
    pub slot: u64,
    pub previous_blockhash: [u8; 32],
    pub blockhash: [u8; 32],
    pub transactions: Vec<ObservedTransaction>,
}

impl ObservedBlock {
    /// Recovers execution order by recomputing the block hash both ways and
    /// keeping the direction that reproduces it.
    ///
    /// This is not defensive: the wrong direction is the one `getBlock`
    /// returns by default, so a watchtower that trusted the reported order
    /// would read every honest slot as maximally reordered.
    pub fn executed_order(&self) -> Option<Vec<&ObservedTransaction>> {
        let forward: Vec<&ObservedTransaction> = self.transactions.iter().collect();
        match Order::derive(self) {
            Order::AsReported => Some(forward),
            Order::Reversed => Some(forward.into_iter().rev().collect()),
            Order::Unverifiable => None,
        }
    }
}
