use mb_receipt::ReceiptError;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StampError {
    #[error("receipt writer is no longer running")]
    WriterGone,
    #[error("refused to sign an invalid receipt: {0}")]
    Invalid(#[from] ReceiptError),
}
