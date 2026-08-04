use std::sync::Arc;

use bytes::Bytes;
use ed25519_dalek::SigningKey;
use magicblock_ledger::Ledger;
use mb_receipt::{LEN_HASH, LEN_TX_SIG};

use crate::{slot_source::SlotSource, stamper::ReceiptStamper};

pub(crate) const TEST_SLOT: u64 = 4_242;

pub(crate) fn operator_key() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

pub(crate) fn fixed_slots() -> SlotSource {
    Box::new(|| TEST_SLOT)
}

pub(crate) fn ledger() -> Arc<Ledger> {
    let directory = tempfile::tempdir().expect("temp dir");
    Arc::new(Ledger::open(&directory.keep()).expect("ledger opens"))
}

pub(crate) fn stamper_with_ledger() -> (ReceiptStamper, Arc<Ledger>) {
    let ledger = ledger();
    let stamper =
        ReceiptStamper::spawn(ledger.clone(), operator_key(), fixed_slots());
    (stamper, ledger)
}

pub(crate) fn stamper() -> ReceiptStamper {
    stamper_with_ledger().0
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
