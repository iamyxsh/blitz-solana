use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::SigningKey;
use magicblock_ledger::Ledger;
use mb_receipt::{
    tx_hash, Mode, Outcome, Receipt, SignedReceipt, GENESIS_PREV_HASH,
    LEN_HASH, LEN_TX_SIG, ZERO_PUBKEY,
};
use solana_signature::Signature;
use tokio::sync::{broadcast, mpsc};
use tracing::error;

use crate::{
    command::WriterCommand, error::StampError, request::StampRequest,
    slot_source::SlotSource,
};

/// The only task permitted to assign a sequence number, advance the chain, or
/// sign. Every field below is owned here and reachable from nowhere else, so
/// the single-writer property is enforced by ownership rather than by
/// discipline at each call site.
pub(crate) struct ReceiptWriter {
    inbox: mpsc::Receiver<WriterCommand>,
    events: broadcast::Sender<SignedReceipt>,
    ledger: Arc<Ledger>,
    key: SigningKey,
    slots: SlotSource,
    seq: u64,
    prev_receipt_hash: [u8; LEN_HASH],
}

impl ReceiptWriter {
    pub(crate) fn new(
        inbox: mpsc::Receiver<WriterCommand>,
        events: broadcast::Sender<SignedReceipt>,
        ledger: Arc<Ledger>,
        key: SigningKey,
        slots: SlotSource,
    ) -> Self {
        Self {
            inbox,
            events,
            ledger,
            key,
            slots,
            seq: 0,
            prev_receipt_hash: GENESIS_PREV_HASH,
        }
    }

    pub(crate) async fn run(mut self) {
        while let Some(command) = self.inbox.recv().await {
            match command {
                WriterCommand::Stamp(request) => self.handle_stamp(request),
                WriterCommand::RecordOutcome { seq, outcome } => {
                    self.handle_outcome(seq, outcome)
                }
            }
        }
    }

    fn handle_stamp(&mut self, request: StampRequest) {
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

    fn handle_outcome(&self, seq: u64, outcome: Outcome) {
        match self.ledger.set_receipt_outcome(seq, outcome.as_u8()) {
            Ok(true) => (),
            Ok(false) => {
                error!(seq, "no stored receipt to record an outcome against")
            }
            Err(error) => error!(%error, seq, "failed to record outcome"),
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
        let signed = receipt.sign(&self.key)?;

        // Persisted before the caller ever sees it. A receipt handed out but
        // not recorded would leave the client holding a signed statement this
        // node has no memory of, which is indistinguishable from equivocation.
        self.ledger
            .write_receipt(
                self.seq,
                Signature::from(tx_sig),
                Outcome::Pending.as_u8(),
                &signed.to_bytes(),
            )
            .map_err(|error| StampError::Storage(error.to_string()))?;

        // Advancing only after both the signature and the write succeed keeps
        // the log dense: a refused receipt must not consume a seq.
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
