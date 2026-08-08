use crate::{
    conflict::conflicts, execution::Execution, fault::Fault, observed_block::ObservedBlock,
    observed_transaction::ObservedTransaction, operator::Operator, receipt_index::ReceiptIndex,
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
    operator: &Operator,
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
        scan.record(check_ticket_is_live(
            block,
            index as u32,
            txn,
            &executed,
            receipts,
        ));
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

/// A position in a block must be backed by a receipt that is still standing.
///
/// A receipt the operator has withdrawn is not a weaker ticket than a missing
/// one, it is a stronger accusation: the operator signed a statement that this
/// transaction would not run, and then ran it.
fn check_ticket_is_live(
    block: &ObservedBlock,
    index: u32,
    txn: &ObservedTransaction,
    executed: &[&ObservedTransaction],
    receipts: &ReceiptIndex,
) -> Verdict {
    let Some(receipt) = receipts.find(txn) else {
        return Verdict::Fault(Box::new(Fault::Unticketed {
            slot: block.slot,
            index,
            signature: txn.signature,
            wire_bytes: txn.wire_bytes.clone(),
        }));
    };

    let Some(withdrawal) = receipts.withdrawal(receipt) else {
        return Verdict::Clean;
    };

    Verdict::Fault(Box::new(Fault::WithdrawnButExecuted {
        receipt: receipt.clone(),
        withdrawal: withdrawal.clone(),
        execution: Execution {
            slot: block.slot,
            index,
        },
        previous_blockhash: block.previous_blockhash,
        blockhash: block.blockhash,
        executed: executed.iter().map(|txn| txn.signature).collect(),
    }))
}

/// `first` executed before `second`. On an honest validator that means the
/// operator sequenced `first` first, unless the two never interacted.
fn check_pair(
    block: &ObservedBlock,
    (first_index, first): (u32, &ObservedTransaction),
    (second_index, second): (u32, &ObservedTransaction),
    receipts: &ReceiptIndex,
    operator: &Operator,
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

    // The index already filters, so this cannot fire today. It stays because
    // the rule belongs at the accusation site: whatever the index does later,
    // an ordering fault is never built from a receipt this operator did not
    // sign about this log.
    if operator.accepts(first_receipt).is_err() || operator.accepts(second_receipt).is_err() {
        return Verdict::CannotDetermine(Undetermined::MissingReceipt { slot: block.slot });
    }

    // A withdrawn receipt is a statement the operator has disowned, so it is
    // no basis for an ordering accusation either. Running the transaction at
    // all is already reported, and by a fault that does not rest on it.
    if receipts.withdrawal(first_receipt).is_some() || receipts.withdrawal(second_receipt).is_some()
    {
        return Verdict::CannotDetermine(Undetermined::WithdrawnReceipt { slot: block.slot });
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
