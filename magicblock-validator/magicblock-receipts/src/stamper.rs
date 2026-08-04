use bytes::Bytes;
use ed25519_dalek::{SigningKey, VerifyingKey};
use mb_receipt::{SignedReceipt, LEN_HASH, LEN_TX_SIG};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{
    error::StampError, request::StampRequest, slot_source::SlotSource,
    writer::ReceiptWriter,
};

/// Bounds how many callers may queue before `stamp` applies backpressure.
const REQUEST_QUEUE_CAPACITY: usize = 1024;
/// Bounds how far a slow subscriber may lag before it starts missing receipts.
const EVENT_QUEUE_CAPACITY: usize = 4096;

/// A cheap, cloneable handle to the receipt writer.
///
/// Holding one grants the ability to request a receipt and to subscribe to the
/// stream. It grants no access whatsoever to the sequence number or the chain.
#[derive(Clone)]
pub struct ReceiptStamper {
    outbox: mpsc::Sender<StampRequest>,
    events: broadcast::Sender<SignedReceipt>,
    operator: VerifyingKey,
}

impl ReceiptStamper {
    pub fn spawn(key: SigningKey, slots: SlotSource) -> Self {
        let operator = key.verifying_key();
        let (outbox, inbox) = mpsc::channel(REQUEST_QUEUE_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_QUEUE_CAPACITY);
        tokio::spawn(
            ReceiptWriter::new(inbox, events.clone(), key, slots).run(),
        );
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
            .send(request)
            .await
            .map_err(|_| StampError::WriterGone)?;
        answer.await.map_err(|_| StampError::WriterGone)?
    }

    /// Tails every receipt in sequence order, from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<SignedReceipt> {
        self.events.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use mb_receipt::{
        tx_hash, Mode, ReceiptError, GENESIS_PREV_HASH, ZERO_SIG,
    };
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
            let (outbox, inbox) = mpsc::channel(1);
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
}
