use std::collections::BTreeMap;

use mb_receipt::{LEN_HASH, LEN_PUBKEY};

/// One position handed out before its transaction was revealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingCommit {
    pub tx_hash: [u8; LEN_HASH],
    pub committer: [u8; LEN_PUBKEY],
    pub committed_at_slot: u64,
}

/// Positions the operator has promised but cannot yet fill.
///
/// Keyed by content hash because that is all a reveal can be matched on: the
/// committer proves it meant *this* transaction by producing bytes that hash
/// to what it committed to.
#[derive(Debug, Default)]
pub struct PendingCommits {
    by_hash: BTreeMap<[u8; LEN_HASH], (u64, PendingCommit)>,
}

impl PendingCommits {
    pub fn record(
        &mut self,
        seq: u64,
        tx_hash: [u8; LEN_HASH],
        committer: [u8; LEN_PUBKEY],
        committed_at_slot: u64,
    ) {
        self.by_hash.insert(
            tx_hash,
            (
                seq,
                PendingCommit {
                    tx_hash,
                    committer,
                    committed_at_slot,
                },
            ),
        );
    }

    /// Claims the position promised for these contents, if one was.
    pub fn claim(
        &mut self,
        tx_hash: &[u8; LEN_HASH],
    ) -> Option<(u64, PendingCommit)> {
        self.by_hash.remove(tx_hash)
    }

    /// Removes every commitment made before `cutoff_slot`, returning them so
    /// the log can record that they went unfulfilled.
    pub fn expire(&mut self, cutoff_slot: u64) -> Vec<(u64, PendingCommit)> {
        let stale: Vec<[u8; LEN_HASH]> = self
            .by_hash
            .iter()
            .filter(|(_, (_, commit))| commit.committed_at_slot < cutoff_slot)
            .map(|(hash, _)| *hash)
            .collect();

        stale
            .into_iter()
            .filter_map(|hash| self.by_hash.remove(&hash))
            .collect()
    }

    /// Commits still outstanding, oldest first.
    pub fn outstanding(&self) -> impl Iterator<Item = (u64, &PendingCommit)> {
        self.by_hash.values().map(|(seq, commit)| (*seq, commit))
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    /// How many positions this committer is holding open.
    pub fn outstanding_for(&self, committer: &[u8; LEN_PUBKEY]) -> usize {
        self.by_hash
            .values()
            .filter(|(_, commit)| &commit.committer == committer)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTER: [u8; LEN_PUBKEY] = [0x21; LEN_PUBKEY];
    const OTHER: [u8; LEN_PUBKEY] = [0x22; LEN_PUBKEY];

    fn hash(n: u8) -> [u8; LEN_HASH] {
        [n; LEN_HASH]
    }

    fn table() -> PendingCommits {
        let mut pending = PendingCommits::default();
        pending.record(0, hash(1), COMMITTER, 100);
        pending.record(1, hash(2), COMMITTER, 200);
        pending.record(2, hash(3), OTHER, 300);
        pending
    }

    #[test]
    fn a_commitment_is_claimed_by_producing_its_contents() {
        let mut pending = table();

        let (seq, claimed) = pending.claim(&hash(2)).unwrap();
        assert_eq!(seq, 1);
        assert_eq!(claimed.committer, COMMITTER);
        assert_eq!(pending.len(), 2);
    }

    /// One promise, one position. A claimed commitment is gone.
    #[test]
    fn a_commitment_cannot_be_claimed_twice() {
        let mut pending = table();

        assert!(pending.claim(&hash(1)).is_some());
        assert!(pending.claim(&hash(1)).is_none());
    }

    #[test]
    fn contents_that_were_never_committed_claim_nothing() {
        let mut pending = table();
        assert!(pending.claim(&hash(9)).is_none());
        assert_eq!(pending.len(), 3);
    }

    /// The cutoff is exclusive: a commitment made exactly at the boundary is
    /// still inside its deadline. Expiring it would accuse someone who was
    /// on time.
    #[test]
    fn expiry_takes_only_commitments_older_than_the_cutoff() {
        let mut pending = table();

        let expired = pending.expire(200);

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, 0);
        assert_eq!(pending.len(), 2, "the boundary commitment survives");
    }

    #[test]
    fn expiry_reports_nothing_when_every_commitment_is_fresh() {
        let mut pending = table();
        assert!(pending.expire(50).is_empty());
        assert_eq!(pending.len(), 3);
    }

    /// The allowance is per committer, so one greedy key cannot throttle
    /// everybody else.
    #[test]
    fn outstanding_is_counted_per_committer() {
        let pending = table();
        assert_eq!(pending.outstanding_for(&COMMITTER), 2);
        assert_eq!(pending.outstanding_for(&OTHER), 1);
        assert_eq!(pending.outstanding_for(&[0x99; LEN_PUBKEY]), 0);
    }
}
