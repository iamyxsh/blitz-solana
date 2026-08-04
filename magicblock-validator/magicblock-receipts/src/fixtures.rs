use bytes::Bytes;
use ed25519_dalek::SigningKey;
use mb_receipt::{LEN_HASH, LEN_TX_SIG};

use crate::{slot_source::SlotSource, stamper::ReceiptStamper};

pub(crate) const TEST_SLOT: u64 = 4_242;

pub(crate) fn operator_key() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

pub(crate) fn fixed_slots() -> SlotSource {
    Box::new(|| TEST_SLOT)
}

pub(crate) fn stamper() -> ReceiptStamper {
    ReceiptStamper::spawn(operator_key(), fixed_slots())
}

/// Distinct, deterministic transaction material per index. The high bit keeps
/// `tx_sig` non-zero, which PLAIN mode requires.
pub(crate) fn wire_bytes(n: u8) -> Bytes {
    Bytes::from(vec![n; 96])
}

pub(crate) fn tx_sig(n: u8) -> [u8; LEN_TX_SIG] {
    [n | 0x80; LEN_TX_SIG]
}

pub(crate) fn blockhash(n: u8) -> [u8; LEN_HASH] {
    [n | 0x40; LEN_HASH]
}
