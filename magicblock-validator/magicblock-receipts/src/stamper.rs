use std::sync::Arc;

use bytes::Bytes;
use ed25519_dalek::{SigningKey, VerifyingKey};
use magicblock_ledger::Ledger;
use mb_receipt::{Outcome, SignedReceipt, LEN_HASH, LEN_PUBKEY, LEN_TX_SIG};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::error;

use crate::{
    command::WriterCommand,
    error::StampError,
    pending::PendingCommit,
    request::{CommitRequest, StampRequest},
    slot_source::SlotSource,
    writer::ReceiptWriter,
};

/// Bounds how many callers may queue before `stamp` applies backpressure.
const REQUEST_QUEUE_CAPACITY: usize = 1024;
/// Bounds how far a slow subscriber may lag before it starts missing receipts.
const EVENT_QUEUE_CAPACITY: usize = 4096;
/// How often unrevealed commitments are aged out.
const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// A cheap, cloneable handle to the receipt writer.
///
/// Holding one grants the ability to request a receipt and to subscribe to the
/// stream. It grants no access whatsoever to the sequence number or the chain.
#[derive(Clone)]
pub struct ReceiptStamper {
    outbox: mpsc::Sender<WriterCommand>,
    events: broadcast::Sender<SignedReceipt>,
    operator: VerifyingKey,
}

impl ReceiptStamper {
    pub fn spawn(
        ledger: Arc<Ledger>,
        key: SigningKey,
        slots: SlotSource,
    ) -> Self {
        let operator = key.verifying_key();
        let (outbox, inbox) = mpsc::channel(REQUEST_QUEUE_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_QUEUE_CAPACITY);
        tokio::spawn(
            ReceiptWriter::new(inbox, events.clone(), ledger, key, slots).run(),
        );
        tokio::spawn(sweep_periodically(outbox.clone()));
        Self {
            outbox,
            events,
            operator,
        }
    }

    /// The key every receipt from this node verifies against.
    pub fn operator(&self) -> VerifyingKey {
        self.operator
    }

    /// Sequences and signs a receipt for one transaction.
    ///
    /// `wire_bytes` must be the bytes exactly as the client sent them: the
    /// receipt commits to their digest, and a client verifying its own receipt
    /// will hash the same bytes.
    pub async fn stamp(
        &self,
        tx_sig: [u8; LEN_TX_SIG],
        wire_bytes: Bytes,
        recent_blockhash: [u8; LEN_HASH],
    ) -> Result<SignedReceipt, StampError> {
        let (reply, answer) = oneshot::channel();
        let request = StampRequest {
            tx_sig,
            wire_bytes,
            recent_blockhash,
            reply,
        };
        self.outbox
            .send(WriterCommand::Stamp(request))
            .await
            .map_err(|_| StampError::WriterGone)?;
        answer.await.map_err(|_| StampError::WriterGone)?
    }

    /// Assigns a position to a transaction the operator has not seen.
    ///
    /// The caller supplies only `sha256(wire_bytes)`. Because the operator
    /// holds no content when it chooses the position, it cannot have chosen
    /// the position because of the content — which is the whole difference
    /// between detecting unfair ordering and preventing it.
    pub async fn commit(
        &self,
        tx_hash: [u8; LEN_HASH],
        committer: [u8; LEN_PUBKEY],
    ) -> Result<SignedReceipt, StampError> {
        let (reply, answer) = oneshot::channel();
        let request = CommitRequest {
            tx_hash,
            committer,
            reply,
        };
        self.outbox
            .send(WriterCommand::Commit(request))
            .await
            .map_err(|_| StampError::WriterGone)?;
        answer.await.map_err(|_| StampError::WriterGone)?
    }

    /// Claims the position promised for these contents.
    ///
    /// Returns `None` when nothing was committed for them, which is the only
    /// answer that keeps a position from being taken by content nobody
    /// promised.
    pub async fn reveal(
        &self,
        tx_hash: [u8; LEN_HASH],
        tx_sig: [u8; LEN_TX_SIG],
    ) -> Option<(u64, PendingCommit)> {
        let (reply, answer) = oneshot::channel();
        self.outbox
            .send(WriterCommand::Reveal {
                tx_hash,
                tx_sig,
                reply,
            })
            .await
            .ok()?;
        answer.await.ok().flatten()
    }

    /// Records what became of an already-sequenced transaction.
    ///
    /// Ordered behind the stamp that created `seq`, because both travel the
    /// same channel and callers await their receipt first. Failure leaves the
    /// receipt `Pending`, which reads as "unknown" rather than as a fault.
    pub async fn record_outcome(&self, seq: u64, outcome: Outcome) {
        let command = WriterCommand::RecordOutcome { seq, outcome };
        if self.outbox.send(command).await.is_err() {
            error!(seq, "receipt writer gone; outcome stays pending");
        }
    }

    /// Tails every receipt in sequence order, from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<SignedReceipt> {
        self.events.subscribe()
    }
}

/// Nudges the writer to age out unrevealed commitments.
///
/// A separate task rather than lazy expiry on the next request: a quiet node
/// is exactly when an abandoned commitment would otherwise sit unrecorded
/// forever.
async fn sweep_periodically(outbox: mpsc::Sender<WriterCommand>) {
    let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
    loop {
        ticker.tick().await;
        if outbox.send(WriterCommand::Sweep).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use mb_receipt::{
        tx_hash, Mode, ReceiptError, GENESIS_PREV_HASH, ZERO_SIG,
    };
    use solana_signature::Signature;
    use tokio::task::JoinSet;

    use super::*;
    use crate::fixtures;

    const SHA256_OF_ABC: &str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// Stamps `n` receipts concurrently and returns them ordered by seq.
    async fn stamp_concurrently(n: u8) -> Vec<SignedReceipt> {
        let stamper = fixtures::stamper();
        let mut jobs = JoinSet::new();
        for i in 0..n {
            let stamper = stamper.clone();
            jobs.spawn(async move {
                stamper
                    .stamp(
                        fixtures::tx_sig(i),
                        fixtures::wire_bytes(i),
                        fixtures::blockhash(i),
                    )
                    .await
                    .expect("stamping must succeed")
            });
        }
        let mut receipts = Vec::with_capacity(n as usize);
        while let Some(joined) = jobs.join_next().await {
            receipts.push(joined.expect("task must not panic"));
        }
        receipts.sort_by_key(|signed| signed.receipt.seq);
        receipts
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn seq_is_dense_and_gapless_under_concurrency() {
        const N: u8 = 64;
        let seqs: Vec<u64> = stamp_concurrently(N)
            .await
            .iter()
            .map(|signed| signed.receipt.seq)
            .collect();

        assert_eq!(seqs, (0..N as u64).collect::<Vec<_>>());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn chain_links_form_an_unbroken_sequence() {
        let receipts = stamp_concurrently(32).await;

        assert_eq!(receipts[0].receipt.prev_receipt_hash, GENESIS_PREV_HASH);
        for pair in receipts.windows(2) {
            assert_eq!(
                pair[1].receipt.prev_receipt_hash,
                pair[0].receipt_hash(),
                "seq {} does not link to seq {}",
                pair[1].receipt.seq,
                pair[0].receipt.seq,
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn every_receipt_verifies_against_the_operator_key() {
        let operator = fixtures::operator_key().verifying_key();
        for signed in stamp_concurrently(16).await {
            assert!(signed.verify(&operator).is_ok());
        }
    }

    #[tokio::test]
    async fn tx_hash_is_over_raw_wire_bytes() {
        let signed = fixtures::stamper()
            .stamp(
                fixtures::tx_sig(0),
                Bytes::from_static(b"abc"),
                fixtures::blockhash(0),
            )
            .await
            .unwrap();

        assert_eq!(hex::encode(signed.receipt.tx_hash), SHA256_OF_ABC);
        assert_ne!(
            signed.receipt.tx_hash,
            tx_hash(bs58::encode(b"abc").into_string().as_bytes()),
        );
    }

    #[tokio::test]
    async fn a_refused_receipt_consumes_no_seq_and_leaves_the_chain_intact() {
        let stamper = fixtures::stamper();

        let refused = stamper
            .stamp(ZERO_SIG, fixtures::wire_bytes(0), fixtures::blockhash(0))
            .await;
        assert_eq!(
            refused.unwrap_err(),
            StampError::Invalid(ReceiptError::TxSigZeroed),
        );

        let accepted = stamper
            .stamp(
                fixtures::tx_sig(1),
                fixtures::wire_bytes(1),
                fixtures::blockhash(1),
            )
            .await
            .unwrap();

        assert_eq!(accepted.receipt.seq, 0);
        assert_eq!(accepted.receipt.prev_receipt_hash, GENESIS_PREV_HASH);
    }

    #[tokio::test]
    async fn the_writer_supplies_the_slot_and_the_clock() {
        let signed = fixtures::stamper()
            .stamp(
                fixtures::tx_sig(0),
                fixtures::wire_bytes(0),
                fixtures::blockhash(0),
            )
            .await
            .unwrap();

        assert_eq!(signed.receipt.ingress_slot, fixtures::TEST_SLOT);
        assert_ne!(signed.receipt.t_ingress_micros, 0);
        assert_eq!(signed.receipt.mode, Mode::Plain);
    }

    #[tokio::test]
    async fn subscribers_observe_every_receipt_in_seq_order() {
        const N: u8 = 8;
        let stamper = fixtures::stamper();
        let mut events = stamper.subscribe();

        for i in 0..N {
            stamper
                .stamp(
                    fixtures::tx_sig(i),
                    fixtures::wire_bytes(i),
                    fixtures::blockhash(i),
                )
                .await
                .unwrap();
        }

        for expected in 0..N as u64 {
            let observed = events.recv().await.expect("stream must stay open");
            assert_eq!(observed.receipt.seq, expected);
        }
    }

    #[tokio::test]
    async fn a_dead_writer_surfaces_as_an_error_rather_than_a_hang() {
        let stamper = {
            let (outbox, inbox) = mpsc::channel::<WriterCommand>(1);
            let (events, _) = broadcast::channel(1);
            drop(inbox);
            ReceiptStamper {
                outbox,
                events,
                operator: fixtures::operator_key().verifying_key(),
            }
        };

        let result = stamper
            .stamp(
                fixtures::tx_sig(0),
                fixtures::wire_bytes(0),
                fixtures::blockhash(0),
            )
            .await;

        assert_eq!(result.unwrap_err(), StampError::WriterGone);
    }

    /// Forces every command queued before this point to have been handled.
    ///
    /// The inbox is FIFO with a single consumer, so once a later stamp has
    /// replied, everything sent earlier — including fire-and-forget outcome
    /// updates — has already been applied. No sleeping, no polling.
    async fn barrier(stamper: &ReceiptStamper) {
        stamper
            .stamp(
                fixtures::tx_sig(0xFE),
                fixtures::wire_bytes(0xFE),
                fixtures::blockhash(0xFE),
            )
            .await
            .expect("barrier stamp must succeed");
    }

    #[tokio::test]
    async fn a_receipt_is_persisted_before_the_caller_ever_sees_it() {
        let (stamper, ledger) = fixtures::stamper_with_ledger();

        let signed = stamper
            .stamp(
                fixtures::tx_sig(1),
                fixtures::wire_bytes(1),
                fixtures::blockhash(1),
            )
            .await
            .unwrap();

        // Read immediately: no barrier, because the write must already have
        // happened by the time the reply arrived.
        let (outcome, stored) = ledger.read_receipt(0).unwrap().unwrap();
        assert_eq!(outcome, Outcome::Pending.as_u8());
        assert_eq!(SignedReceipt::from_bytes(&stored).unwrap(), signed);

        let by_sig = ledger
            .read_receipt_by_signature(Signature::from(fixtures::tx_sig(1)))
            .unwrap()
            .unwrap();
        assert_eq!(by_sig.0, 0);
        assert_eq!(by_sig.2, stored);
    }

    #[tokio::test]
    async fn recording_an_outcome_leaves_the_signed_receipt_untouched() {
        let (stamper, ledger) = fixtures::stamper_with_ledger();
        let signed = stamper
            .stamp(
                fixtures::tx_sig(1),
                fixtures::wire_bytes(1),
                fixtures::blockhash(1),
            )
            .await
            .unwrap();

        stamper.record_outcome(0, Outcome::Rejected).await;
        barrier(&stamper).await;

        let (outcome, stored) = ledger.read_receipt(0).unwrap().unwrap();
        assert_eq!(outcome, Outcome::Rejected.as_u8());
        assert_eq!(SignedReceipt::from_bytes(&stored).unwrap(), signed);
    }

    /// The watchtower's actual read path: scan the persisted log, decode each
    /// row, and check the chain end to end without ever talking to the node.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_persisted_log_replays_a_verifiable_chain() {
        let (stamper, ledger) = fixtures::stamper_with_ledger();
        let operator = fixtures::operator_key().verifying_key();

        let mut jobs = JoinSet::new();
        for i in 0..16u8 {
            let stamper = stamper.clone();
            jobs.spawn(async move {
                stamper
                    .stamp(
                        fixtures::tx_sig(i),
                        fixtures::wire_bytes(i),
                        fixtures::blockhash(i),
                    )
                    .await
                    .unwrap()
            });
        }
        while jobs.join_next().await.is_some() {}

        let replayed: Vec<SignedReceipt> = ledger
            .iter_receipts(0)
            .unwrap()
            .map(|(_, _, bytes)| SignedReceipt::from_bytes(&bytes).unwrap())
            .collect();

        assert_eq!(replayed.len(), 16);
        assert_eq!(replayed[0].receipt.prev_receipt_hash, GENESIS_PREV_HASH);
        for (position, signed) in replayed.iter().enumerate() {
            assert_eq!(signed.receipt.seq, position as u64);
            assert!(signed.verify(&operator).is_ok());
        }
        for pair in replayed.windows(2) {
            assert_eq!(
                pair[1].receipt.prev_receipt_hash,
                pair[0].receipt_hash()
            );
        }
    }

    #[tokio::test]
    async fn an_outcome_for_an_unknown_sequence_is_survivable() {
        let stamper = fixtures::stamper();
        stamper.record_outcome(999, Outcome::Accepted).await;

        // The writer must still be alive and sequencing from zero.
        let signed = stamper
            .stamp(
                fixtures::tx_sig(1),
                fixtures::wire_bytes(1),
                fixtures::blockhash(1),
            )
            .await
            .unwrap();
        assert_eq!(signed.receipt.seq, 0);
    }
}
