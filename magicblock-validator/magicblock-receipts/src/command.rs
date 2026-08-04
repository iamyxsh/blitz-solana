use mb_receipt::Outcome;

use crate::request::StampRequest;

/// Everything the writer accepts, on one channel.
///
/// Sharing a single inbox keeps sequencing and outcome updates on the same
/// task, so an outcome can never be applied before the receipt it belongs to
/// has been written.
pub(crate) enum WriterCommand {
    Stamp(StampRequest),
    RecordOutcome { seq: u64, outcome: Outcome },
}
