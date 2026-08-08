use crate::receipt::Receipt;
use ed25519_dalek::SigningKey;
use mb_constants::mode::Mode;
use mb_constants::receipt::{GENESIS_PREV_HASH, ZERO_HASH, ZERO_PUBKEY, ZERO_SIG};

pub fn operator_key() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

pub const LOG_ID: [u8; 32] = [0x9c; 32];

pub fn plain() -> Receipt {
    Receipt {
        log_id: LOG_ID,
        mode: Mode::Plain,
        seq: 0,
        tx_sig: [0xa1; 64],
        tx_hash: [0xb2; 32],
        recent_blockhash: [0xc3; 32],
        prev_receipt_hash: GENESIS_PREV_HASH,
        committer: ZERO_PUBKEY,
        ingress_slot: 6_150_402,
        t_ingress_micros: 1_754_300_123_456_789,
    }
}

pub fn commit() -> Receipt {
    Receipt {
        mode: Mode::Commit,
        seq: 1,
        tx_sig: ZERO_SIG,
        committer: [0xe5; 32],
        ..plain()
    }
}

pub fn retract() -> Receipt {
    Receipt {
        mode: Mode::Retract,
        seq: 2,
        tx_hash: [0xf6; 32],
        recent_blockhash: ZERO_HASH,
        ..plain()
    }
}
