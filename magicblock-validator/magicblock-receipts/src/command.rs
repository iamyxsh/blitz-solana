use mb_receipt::{Outcome, LEN_HASH, LEN_TX_SIG};
use tokio::sync::oneshot;

use crate::pending::PendingCommit;

use crate::request::{CommitRequest, StampRequest};

/// Everything the writer accepts, on one channel.
///
/// Sharing a single inbox keeps sequencing and outcome updates on the same
/// task, so an outcome can never be applied before the receipt it belongs to
/// has been written.
pub(crate) enum WriterCommand {
    Stamp(StampRequest),
    Commit(CommitRequest),
    /// Produces the contents a position was promised to.
    Reveal {
        tx_hash: [u8; LEN_HASH],
        tx_sig: [u8; LEN_TX_SIG],
        reply: oneshot::Sender<Option<(u64, PendingCommit)>>,
    },
    RecordOutcome {
        seq: u64,
        outcome: Outcome,
    },
    /// Ages out promises nobody kept.
    Sweep,
}
