use ed25519_dalek::VerifyingKey;
use mb_receipt::{GENESIS_PREV_HASH, SignedReceipt};

use crate::{fault::Fault, scan::Scan, undetermined::Undetermined, verdict::Verdict};

/// Examines a stretch of receipt log for contradictions the operator signed.
///
/// Needs nothing but the receipts and the operator's public key: no blocks, no
/// transactions, no live node. Everything it can find is provable cold.
///
/// The input may arrive in any order and may contain duplicates; neither
/// carries meaning, because a log can be tailed and backfilled at once.
pub fn scan_receipts(receipts: &[SignedReceipt], operator: &VerifyingKey) -> Scan {
    let mut scan = Scan::default();

    let mut ordered: Vec<&SignedReceipt> = receipts.iter().collect();
    ordered.sort_by_key(|signed| signed.receipt.seq);

    for signed in &ordered {
        scan.record(check_signature(signed, operator));
    }

    // Collapse byte-identical re-deliveries, then look for genuine
    // contradictions among what remains at each sequence number.
    let mut position = 0;
    let mut distinct: Vec<&SignedReceipt> = Vec::new();
    while position < ordered.len() {
        let seq = ordered[position].receipt.seq;
        let end = ordered[position..]
            .iter()
            .position(|signed| signed.receipt.seq != seq)
            .map_or(ordered.len(), |offset| position + offset);

        let group = &ordered[position..end];
        scan.record(check_one_statement_per_sequence(seq, group));
        distinct.push(group[0]);
        position = end;
    }

    if let Some(first) = distinct.first() {
        scan.record(check_origin(first));
    }
    for pair in distinct.windows(2) {
        scan.record(check_link(pair[0], pair[1]));
    }

    scan
}

fn check_signature(signed: &SignedReceipt, operator: &VerifyingKey) -> Verdict {
    if signed.verify(operator).is_err() {
        return Verdict::Fault(Box::new(Fault::Unverifiable {
            seq: signed.receipt.seq,
            receipt: (*signed).clone(),
        }));
    }
    Verdict::Clean
}

/// One sequence number may carry only one statement. Repeats of the very same
/// statement are re-delivery, not equivocation.
fn check_one_statement_per_sequence(seq: u64, group: &[&SignedReceipt]) -> Verdict {
    let first = group[0];
    match group.iter().find(|signed| **signed != first) {
        Some(other) => Verdict::Fault(Box::new(Fault::Equivocation {
            seq,
            a: first.clone(),
            b: (*other).clone(),
        })),
        None => Verdict::Clean,
    }
}

fn check_origin(first: &SignedReceipt) -> Verdict {
    if first.receipt.seq != 0 {
        return Verdict::CannotDetermine(Undetermined::MissingOrigin {
            lowest: first.receipt.seq,
        });
    }
    if first.receipt.prev_receipt_hash != GENESIS_PREV_HASH {
        return Verdict::Fault(Box::new(Fault::BadOrigin {
            receipt: first.clone(),
        }));
    }
    Verdict::Clean
}

fn check_link(predecessor: &SignedReceipt, receipt: &SignedReceipt) -> Verdict {
    if predecessor.receipt.seq + 1 != receipt.receipt.seq {
        return Verdict::CannotDetermine(Undetermined::SequenceGap {
            after: predecessor.receipt.seq,
            before: receipt.receipt.seq,
        });
    }
    if receipt.receipt.prev_receipt_hash != predecessor.receipt_hash() {
        return Verdict::Fault(Box::new(Fault::BrokenChain {
            seq: receipt.receipt.seq,
            receipt: receipt.clone(),
            predecessor: predecessor.clone(),
        }));
    }
    Verdict::Clean
}
