use bytes::Bytes;
use mb_receipt::{SignedReceipt, LEN_HASH, LEN_PUBKEY, LEN_TX_SIG};
use tokio::sync::oneshot;

use crate::error::StampError;

/// One unit of work handed to the writer, carrying its own return address.
///
/// The raw wire bytes travel rather than a precomputed digest: hashing belongs
/// to the writer so no call site can hash an encoded form by mistake.
pub(crate) struct StampRequest {
    pub(crate) tx_sig: [u8; LEN_TX_SIG],
    pub(crate) wire_bytes: Bytes,
    pub(crate) recent_blockhash: [u8; LEN_HASH],
    pub(crate) reply: oneshot::Sender<Result<SignedReceipt, StampError>>,
}

/// A request for a position, made while the operator can see only a hash.
///
/// The committer signs the hash so an unrevealed commit can be attributed:
/// a user failing to reveal is spam, an operator failing to reveal is
/// speculation, and the two carry very different consequences.
pub(crate) struct CommitRequest {
    pub(crate) tx_hash: [u8; LEN_HASH],
    pub(crate) committer: [u8; LEN_PUBKEY],
    pub(crate) reply: oneshot::Sender<Result<SignedReceipt, StampError>>,
}
