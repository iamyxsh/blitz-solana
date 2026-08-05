use std::collections::{HashMap, VecDeque};

/// Remembers which slot each block hash belongs to.
///
/// A receipt names the block hash its transaction was built against, but not
/// the slot that hash came from — so every timing claim needs this lookup.
/// Bounded, because a watchtower that never forgets is a watchtower that
/// eventually stops running.
pub struct BlockhashSlots {
    slots: HashMap<[u8; 32], u64>,
    order: VecDeque<[u8; 32]>,
    capacity: usize,
}

impl BlockhashSlots {
    /// Comfortably wider than the ~1200-slot window a client's block hash can
    /// legitimately be accepted within, so an honest transaction's hash is
    /// still known by the time it executes.
    pub const DEFAULT_CAPACITY: usize = 2_048;

    pub fn new(capacity: usize) -> Self {
        Self {
            slots: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn record(&mut self, blockhash: [u8; 32], slot: u64) {
        if self.slots.insert(blockhash, slot).is_some() {
            return;
        }
        self.order.push_back(blockhash);
        while self.order.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.slots.remove(&evicted);
            }
        }
    }

    /// The slot a block hash came from, if it is still remembered.
    pub fn slot_of(&self, blockhash: &[u8; 32]) -> Option<u64> {
        self.slots.get(blockhash).copied()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

impl Default for BlockhashSlots {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}
