use mb_constants::receipt::RECEIPT_LEN;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReceiptError {
    #[error("wrong length: expected {RECEIPT_LEN}, got {0}")]
    WrongLength(usize),
    #[error("bad domain tag")]
    BadDomainTag,
    #[error("invalid mode byte: {0:#04x}")]
    InvalidMode(u8),
    #[error("mode REVEAL is reserved and has no defined invariants yet")]
    ModeReserved,
    #[error("tx_sig must be zero in COMMIT mode")]
    TxSigNotZeroed,
    #[error("tx_sig must name a transaction in PLAIN and RETRACT modes")]
    TxSigZeroed,
    #[error("tx_hash must name the receipt being withdrawn in RETRACT mode")]
    TxHashZeroed,
    #[error("recent_blockhash must be zero in RETRACT mode")]
    RecentBlockhashNotZeroed,
    #[error("committer must be zero outside COMMIT mode")]
    CommitterNotZeroed,
    #[error("committer must be non-zero in COMMIT mode")]
    CommitterZeroed,
    #[error("signature verification failed")]
    BadSignature,
}
