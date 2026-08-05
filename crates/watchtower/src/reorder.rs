use ed25519_dalek::VerifyingKey;

use crate::{
    conflict::conflicts, fault::Fault, observed_block::ObservedBlock,
    observed_transaction::ObservedTransaction, receipt_index::ReceiptIndex,
    reordered_transaction::ReorderedTransaction, scan::Scan, undetermined::Undetermined,
    verdict::Verdict,
};

/// Checks one block against the receipts the operator issued for it.
///
/// `receipts` maps a transaction signature to its receipt; `identity` is the
/// validator's own public key, used to recognise transactions the operator
/// issued rather than relayed.
pub fn scan_block(
    block: &ObservedBlock,
    receipts: &ReceiptIndex,
    operator: &VerifyingKey,
    identity: &[u8; 32],
) -> Scan {
    let mut scan = Scan::default();

    let Some(executed) = block.executed_order() else {
        scan.record(Verdict::CannotDetermine(Undetermined::UnverifiableBlock {
            slot: block.slot,
        }));
        return scan;
    };

    for (index, txn) in executed.iter().enumerate() {
        scan.record(check_ticketed(block, index as u32, txn, receipts));
    }

    for (earlier, first) in executed.iter().enumerate() {
        for (later, second) in executed.iter().enumerate().skip(earlier + 1) {
            scan.record(check_pair(
                block,
                (earlier as u32, first),
                (later as u32, second),
                receipts,
                operator,
                identity,
            ));
        }
    }

    scan
}

fn check_ticketed(
    block: &ObservedBlock,
    index: u32,
    txn: &ObservedTransaction,
    receipts: &ReceiptIndex,
) -> Verdict {
    if receipts.find(txn).is_some() {
        return Verdict::Clean;
    }
    Verdict::Fault(Box::new(Fault::Unticketed {
        slot: block.slot,
        index,
        signature: txn.signature,
        wire_bytes: txn.wire_bytes.clone(),
    }))
}

/// `first` executed before `second`. On an honest validator that means the
/// operator sequenced `first` first, unless the two never interacted.
fn check_pair(
    block: &ObservedBlock,
    (first_index, first): (u32, &ObservedTransaction),
    (second_index, second): (u32, &ObservedTransaction),
    receipts: &ReceiptIndex,
    operator: &VerifyingKey,
    identity: &[u8; 32],
) -> Verdict {
    // Transactions sharing no written account cannot have front-run each
    // other, and parallel execution reorders them constantly.
    if !conflicts(first, second) {
        return Verdict::Clean;
    }

    let (Some(first_receipt), Some(second_receipt)) = (receipts.find(first), receipts.find(second))
    else {
        // Already reported as unticketed; ordering cannot be judged without
        // both receipts.
        return Verdict::CannotDetermine(Undetermined::MissingReceipt { slot: block.slot });
    };

    // Never build an ordering accusation on a receipt that does not verify.
    // Naming the forgery is `scan_receipts`' job; refusing to reason from it
    // is this one's.
    if first_receipt.verify(operator).is_err() || second_receipt.verify(operator).is_err() {
        return Verdict::CannotDetermine(Undetermined::MissingReceipt { slot: block.slot });
    }

    if first_receipt.receipt.seq < second_receipt.receipt.seq {
        return Verdict::Clean;
    }

    // The operator's own transactions legitimately run ahead of the client
    // transaction that caused them: a just-in-time account clone is created
    // after the request it serves and must execute before it. This detector
    // cannot tell that apart from an operator inserting its own work, so it
    // declines rather than guessing in either direction.
    if first.is_issued_by(identity) || second.is_issued_by(identity) {
        return Verdict::CannotDetermine(Undetermined::OperatorIssuedPair { slot: block.slot });
    }

    Verdict::Fault(Box::new(Fault::Reorder {
        slot: block.slot,
        previous_blockhash: block.previous_blockhash,
        blockhash: block.blockhash,
        executed: block
            .executed_order()
            .expect("order already derived")
            .iter()
            .map(|txn| txn.signature)
            .collect(),
        jumped: ReorderedTransaction {
            receipt: first_receipt.clone(),
            wire_bytes: first.wire_bytes.clone(),
            index: first_index,
        },
        delayed: ReorderedTransaction {
            receipt: second_receipt.clone(),
            wire_bytes: second.wire_bytes.clone(),
            index: second_index,
        },
    }))
}
