use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::SigningKey;
use magicblock_ledger::Ledger;
use mb_receipt::{
    tx_hash, Mode, Outcome, Receipt, SignedReceipt, GENESIS_PREV_HASH,
    LEN_HASH, LEN_PUBKEY, LEN_TX_SIG, ZERO_PUBKEY, ZERO_SIG,
};
use solana_signature::Signature;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, warn};

use crate::{
    command::WriterCommand,
    deadline::RevealDeadline,
    equivocation::Equivocation,
    error::StampError,
    pending::PendingCommits,
    request::{CommitRequest, StampRequest},
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
    log_id: [u8; LEN_HASH],
    seq: u64,
    prev_receipt_hash: [u8; LEN_HASH],
    equivocation: Equivocation,
    /// The copy that went to storage, awaiting broadcast.
    published: Option<SignedReceipt>,
    /// Positions handed out blind, waiting for their transaction.
    pending: PendingCommits,
    deadline: RevealDeadline,
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
            log_id: mint_log_id(&key),
            key,
            slots,
            seq: 0,
            prev_receipt_hash: GENESIS_PREV_HASH,
            equivocation: Equivocation::from_env(),
            published: None,
            pending: PendingCommits::default(),
            deadline: RevealDeadline::from_env(),
        }
    }

    pub(crate) async fn run(mut self) {
        while let Some(command) = self.inbox.recv().await {
            match command {
                WriterCommand::Stamp(request) => self.handle_stamp(request),
                WriterCommand::Commit(request) => self.handle_commit(request),
                WriterCommand::Reveal {
                    tx_hash,
                    tx_sig,
                    reply,
                } => {
                    let _ = reply.send(self.handle_reveal(tx_hash, tx_sig));
                }
                WriterCommand::RecordOutcome { seq, outcome } => {
                    self.handle_outcome(seq, outcome)
                }
                WriterCommand::Sweep => self.handle_sweep(),
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
        // Subscribers see the published copy, which is what the log holds.
        if let Some(published) = self.published.take() {
            let _ = self.events.send(published);
        }
        let _ = reply.send(result);
    }

    fn handle_commit(&mut self, request: CommitRequest) {
        let CommitRequest {
            tx_hash,
            committer,
            reply,
        } = request;

        let result = self.commit(tx_hash, committer);
        if let Some(published) = self.published.take() {
            let _ = self.events.send(published);
        }
        let _ = reply.send(result);
    }

    /// Assigns a position to a transaction the operator has not seen.
    ///
    /// `tx_sig` is zeroed because it cannot be known: the signature lives
    /// inside wire bytes that have not been revealed. That absence is the
    /// entire guarantee — a position assigned to content the operator cannot
    /// read cannot have been chosen because of it.
    fn commit(
        &mut self,
        tx_hash: [u8; LEN_HASH],
        committer: [u8; LEN_PUBKEY],
    ) -> Result<SignedReceipt, StampError> {
        if self.pending.outstanding_for(&committer)
            >= self.deadline.max_outstanding
        {
            return Err(StampError::TooManyOutstanding(bs58_committer(
                &committer,
            )));
        }

        let ingress_slot = (self.slots)();
        let ticket = Receipt {
            log_id: self.log_id,
            mode: Mode::Commit,
            seq: self.seq,
            tx_sig: ZERO_SIG,
            tx_hash,
            recent_blockhash: [0u8; LEN_HASH],
            prev_receipt_hash: self.prev_receipt_hash,
            committer,
            ingress_slot,
            t_ingress_micros: now_micros(),
        };
        let signed = ticket.sign(&self.key)?;

        // A commit ticket has no transaction signature to index by, so it is
        // reachable by sequence until the reveal arrives.
        self.ledger
            .write_receipt(
                self.seq,
                None,
                Outcome::Pending.as_u8(),
                &signed.to_bytes(),
            )
            .map_err(|error| StampError::Storage(error.to_string()))?;

        self.pending
            .record(self.seq, tx_hash, committer, ingress_slot);
        self.seq += 1;
        self.prev_receipt_hash = signed.receipt_hash();
        self.published = Some(signed.clone());
        Ok(signed)
    }

    /// Matches revealed contents to the position promised for them.
    ///
    /// The match is on the hash alone: producing bytes that hash to what was
    /// committed is the only way to prove this is the transaction that
    /// position was reserved for. Claiming removes it, so a position cannot
    /// be spent twice.
    fn handle_reveal(
        &mut self,
        tx_hash: [u8; LEN_HASH],
        tx_sig: [u8; LEN_TX_SIG],
    ) -> Option<(u64, crate::pending::PendingCommit)> {
        let claimed = self.pending.claim(&tx_hash)?;

        // Now that the transaction is known, the ticket becomes reachable by
        // signature like any other receipt.
        if let Err(error) = self
            .ledger
            .index_receipt_signature(claimed.0, Signature::from(tx_sig))
        {
            error!(%error, seq = claimed.0, "failed to index revealed ticket");
        }
        Some(claimed)
    }

    /// Records every promise whose deadline has passed.
    ///
    /// Nothing is deleted from the log: the ticket stays, and its outcome
    /// becomes `Expired`. A watchtower does not need this to reach the same
    /// conclusion — it can see the commitment and the absence of its contents
    /// — but the operator saying so itself is cheap and it bounds memory.
    fn handle_sweep(&mut self) {
        let now = (self.slots)();
        let cutoff = now.saturating_sub(self.deadline.slots);
        for (seq, commit) in self.pending.expire(cutoff) {
            warn!(
                seq,
                committer = %bs58_committer(&commit.committer),
                "commitment expired unrevealed"
            );
            if let Err(error) = self
                .ledger
                .set_receipt_outcome(seq, Outcome::Expired.as_u8())
            {
                error!(%error, seq, "failed to record expiry");
            }
        }
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
            log_id: self.log_id,
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
        let signed = receipt.clone().sign(&self.key)?;

        // Under the equivocation attack the node publishes a *different*
        // statement about this same position. Only `tx_hash` differs, so the
        // log stays internally perfect — signatures still map to receipts and
        // the chain still links — and the sole contradiction is between what
        // the client was handed and what anyone else can read.
        let published = if self.equivocation.is_enabled() {
            let mut tampered = receipt;
            tampered.tx_hash[0] ^= 0xFF;
            tampered.sign(&self.key)?
        } else {
            signed.clone()
        };

        // Persisted before the caller ever sees it. A receipt handed out but
        // not recorded would leave the client holding a signed statement this
        // node has no memory of, which is indistinguishable from equivocation.
        self.ledger
            .write_receipt(
                self.seq,
                Some(Signature::from(tx_sig)),
                Outcome::Pending.as_u8(),
                &published.to_bytes(),
            )
            .map_err(|error| StampError::Storage(error.to_string()))?;

        // Advancing only after both the signature and the write succeed keeps
        // the log dense: a refused receipt must not consume a seq.
        self.seq += 1;
        self.prev_receipt_hash = published.receipt_hash();
        self.published = Some(published);
        Ok(signed)
    }
}

/// Names this run of the log.
///
/// The sequence counter starts again at zero every time a writer is built, so
/// a fresh identifier is minted at the same moment. Without it the new run's
/// entries occupy the same positions as the previous run's under the same
/// signing key, which reads as the operator contradicting itself.
fn mint_log_id(key: &SigningKey) -> [u8; LEN_HASH] {
    static MINTED: AtomicU64 = AtomicU64::new(0);

    // The clock separates runs of the process and the counter separates
    // writers inside one, because two writers can be built inside a single
    // clock tick and a repeated log id would undo the whole point of having
    // one.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or_default();

    let mut seed = [0u8; 48];
    seed[..32].copy_from_slice(key.verifying_key().as_bytes());
    seed[32..40].copy_from_slice(&nanos.to_le_bytes());
    seed[40..].copy_from_slice(
        &MINTED.fetch_add(1, Ordering::Relaxed).to_le_bytes(),
    );
    tx_hash(&seed)
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_micros() as u64)
        .unwrap_or_default()
}

fn bs58_committer(committer: &[u8; LEN_PUBKEY]) -> String {
    bs58::encode(committer).into_string()
}
