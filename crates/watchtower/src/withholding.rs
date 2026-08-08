use ed25519_dalek::VerifyingKey;
use mb_receipt::{Mode, SignedReceipt};

use crate::{
    blockhash_slots::BlockhashSlots, execution::ExecutionIndex, fault::Fault, scan::Scan,
    undetermined::Undetermined, verdict::Verdict, withdrawals::Withdrawals,
};

/// How patient the detector is before it calls delay misbehaviour.
#[derive(Debug, Clone, Copy)]
pub struct Patience {
    /// Slots a transaction may sit between being receipted and executing.
    /// Lock contention and just-in-time account cloning both cost real time,
    /// so this is generous by design.
    pub held_slots: u64,
    /// Slots a receipted transaction may fail to appear at all before its
    /// absence is treated as evidence rather than as lag.
    pub absent_slots: u64,
    /// Slots a promised position may go unclaimed before the promise is
    /// treated as one nobody ever meant to keep.
    pub reveal_slots: u64,
    /// The widest gap the node itself will accept between a transaction's
    /// block hash and its arrival: `slot + (400 / blocktime) * 150`, which is
    /// 1200 at the 50ms default. A receipt claiming ingress beyond this is
    /// claiming to have accepted something the node would have rejected.
    pub max_blockhash_age: u64,
}

impl Patience {
    /// Takes the reveal deadline from `MB_REVEAL_DEADLINE_SLOTS` so a
    /// watchtower and the node it watches can be told the same number.
    pub fn from_env() -> Self {
        let fallback = Self::default();
        Self {
            reveal_slots: std::env::var("MB_REVEAL_DEADLINE_SLOTS")
                .ok()
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(fallback.reveal_slots),
            ..fallback
        }
    }
}

impl Default for Patience {
    fn default() -> Self {
        Self {
            held_slots: 32,
            absent_slots: 64,
            reveal_slots: 150,
            max_blockhash_age: 1_200,
        }
    }
}

/// Looks for transactions the operator sequenced and then sat on.
///
/// `executed` maps a transaction signature to where it ran; `head` is the
/// newest slot the watchtower has seen.
pub fn scan_withholding(
    receipts: &[SignedReceipt],
    executed: &ExecutionIndex,
    blockhashes: &BlockhashSlots,
    head: u64,
    patience: &Patience,
    operator: &VerifyingKey,
) -> Scan {
    let mut scan = Scan::default();
    let withdrawn = Withdrawals::build(receipts, operator);

    for receipt in receipts {
        if receipt.verify(operator).is_err() {
            // Naming the forgery belongs to the receipt-log scan; refusing to
            // reason from it belongs here.
            continue;
        }
        // A withdrawal is a statement about a transaction, never one that runs
        // itself. Asking where it executed would report every honest
        // withdrawal as something the operator sat on.
        if receipt.receipt.mode == Mode::Retract {
            continue;
        }
        // A withdrawal retires the promise, not the statement. Whether the
        // transaction ever ran stops being this scan's question, but a receipt
        // whose own fields contradict each other said something false when it
        // was signed and taking the position back does not unsay it.
        scan.record(check_ingress_is_possible(receipt, blockhashes, patience));
        if withdrawn.of(receipt).is_some() {
            continue;
        }
        scan.record(check_delivery(receipt, executed, head, patience));
    }

    scan
}

/// A receipt cannot honestly claim to have arrived before the block hash it
/// names existed, nor after that hash was too old for the node to accept.
///
/// Both are statements the operator signed about itself, so either one is
/// provably false without reference to anything it did afterwards.
fn check_ingress_is_possible(
    receipt: &SignedReceipt,
    blockhashes: &BlockhashSlots,
    patience: &Patience,
) -> Verdict {
    // A commit ticket is signed before any transaction is seen, so it names
    // no block hash and there is nothing to place it against. Judging it as
    // if it did would accuse every commitment ever made.
    if receipt.receipt.mode == Mode::Commit {
        return Verdict::Clean;
    }

    let Some(blockhash_slot) = blockhashes.slot_of(&receipt.receipt.recent_blockhash) else {
        return Verdict::CannotDetermine(Undetermined::UnknownBlockhash {
            seq: receipt.receipt.seq,
        });
    };

    let ingress = receipt.receipt.ingress_slot;
    let impossible = ingress < blockhash_slot
        || ingress.saturating_sub(blockhash_slot) > patience.max_blockhash_age;

    if impossible {
        return Verdict::Fault(Box::new(Fault::ImpossibleIngress {
            receipt: receipt.clone(),
            blockhash_slot,
            max_blockhash_age: patience.max_blockhash_age,
        }));
    }
    Verdict::Clean
}

/// Either the transaction ran far later than it was received, or it has not
/// run at all long after it should have.
fn check_delivery(
    receipt: &SignedReceipt,
    executed: &ExecutionIndex,
    head: u64,
    patience: &Patience,
) -> Verdict {
    let ingress = receipt.receipt.ingress_slot;

    let Some(execution) = executed.find(receipt) else {
        let waited = head.saturating_sub(ingress);
        // A commit ticket that was never claimed is a different failure from
        // a known transaction the operator sat on: nobody ever produced the
        // contents, and who failed to is the whole question.
        if receipt.receipt.mode == Mode::Commit {
            if waited > patience.reveal_slots {
                return Verdict::Fault(Box::new(Fault::NotRevealed {
                    receipt: receipt.clone(),
                    head,
                    waited,
                }));
            }
            return Verdict::CannotDetermine(Undetermined::NotYetExecuted {
                seq: receipt.receipt.seq,
            });
        }
        if waited > patience.absent_slots {
            return Verdict::Fault(Box::new(Fault::Absent {
                receipt: receipt.clone(),
                head,
                waited,
            }));
        }
        // Still within its grace period, or the watchtower simply has not
        // caught up to the slot it landed in.
        return Verdict::CannotDetermine(Undetermined::NotYetExecuted {
            seq: receipt.receipt.seq,
        });
    };

    // A transaction may execute in the same slot it arrived, never earlier.
    let held = execution.slot.saturating_sub(ingress);
    let execution = &execution;
    if execution.slot >= ingress && held > patience.held_slots {
        return Verdict::Fault(Box::new(Fault::Withheld {
            receipt: receipt.clone(),
            execution: *execution,
            held,
        }));
    }
    Verdict::Clean
}
