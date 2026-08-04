use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use mb_receipt::{
    tx_hash, Mode, Receipt, SignedReceipt, GENESIS_PREV_HASH, LEN_HASH,
    LEN_TX_SIG, ZERO_PUBKEY,
};
use tokio::sync::{broadcast, mpsc};

use crate::{
    error::StampError, request::StampRequest, slot_source::SlotSource,
};

/// The only task permitted to assign a sequence number, advance the chain, or
/// sign. Every field below is owned here and reachable from nowhere else, so
/// the single-writer property is enforced by ownership rather than by
/// discipline at each call site.
pub(crate) struct ReceiptWriter {
    inbox: mpsc::Receiver<StampRequest>,
    events: broadcast::Sender<SignedReceipt>,
    key: SigningKey,
    slots: SlotSource,
    seq: u64,
    prev_receipt_hash: [u8; LEN_HASH],
}

impl ReceiptWriter {
    pub(crate) fn new(
        inbox: mpsc::Receiver<StampRequest>,
        events: broadcast::Sender<SignedReceipt>,
        key: SigningKey,
        slots: SlotSource,
    ) -> Self {
        Self {
            inbox,
            events,
            key,
            slots,
            seq: 0,
            prev_receipt_hash: GENESIS_PREV_HASH,
        }
    }

    pub(crate) async fn run(mut self) {
        while let Some(request) = self.inbox.recv().await {
            let StampRequest {
                tx_sig,
                wire_bytes,
                recent_blockhash,
                reply,
            } = request;

            let result = self.stamp(tx_sig, &wire_bytes, recent_blockhash);
            if let Ok(receipt) = &result {
                let _ = self.events.send(receipt.clone());
            }
            let _ = reply.send(result);
        }
    }

    fn stamp(
        &mut self,
        tx_sig: [u8; LEN_TX_SIG],
        wire_bytes: &[u8],
        recent_blockhash: [u8; LEN_HASH],
    ) -> Result<SignedReceipt, StampError> {
        let receipt = Receipt {
            mode: Mode::Plain,
            seq: self.seq,
            tx_sig,
            tx_hash: tx_hash(wire_bytes),
            recent_blockhash,
            prev_receipt_hash: self.prev_receipt_hash,
            committer: ZERO_PUBKEY,
            ingress_slot: (self.slots)(),
            t_ingress_micros: now_micros(),
        };

        // Advancing only after a successful signature keeps the log dense: a
        // refused receipt must not consume a seq or break the chain.
        let signed = receipt.sign(&self.key)?;
        self.seq += 1;
        self.prev_receipt_hash = signed.receipt_hash();
        Ok(signed)
    }
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_micros() as u64)
        .unwrap_or_default()
}
