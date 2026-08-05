use mb_receipt::ReceiptError;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StampError {
    #[error("receipt writer is no longer running")]
    WriterGone,
    #[error("refused to sign an invalid receipt: {0}")]
    Invalid(#[from] ReceiptError),
    #[error("could not persist the receipt: {0}")]
    Storage(String),
    #[error("the committer did not sign this commitment")]
    UnsignedCommitment,
    #[error("committer {0} has too many unrevealed commitments")]
    TooManyOutstanding(String),
}
