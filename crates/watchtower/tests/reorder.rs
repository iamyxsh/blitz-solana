use ed25519_dalek::SigningKey;
use mb_receipt::{Mode, Receipt, SignedReceipt, ZERO_PUBKEY, tx_hash};
use mb_watchtower::{
    Fault, ObservedBlock, ObservedTransaction, Order, ReceiptIndex, Undetermined, order::fold,
    scan_block,
};

const IDENTITY: [u8; 32] = [0xEE; 32];
const ACCOUNT_A: [u8; 32] = [0xA1; 32];
const ACCOUNT_B: [u8; 32] = [0xB2; 32];
const SLOT: u64 = 42;
const PREVIOUS: [u8; 32] = [0x11; 32];

fn operator() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

/// A client transaction writing `writable` and reading `readonly`.
fn txn(tag: u8, writable: &[[u8; 32]], readonly: &[[u8; 32]]) -> ObservedTransaction {
    ObservedTransaction {
        signature: [tag; 64],
        tx_hash: mb_receipt::tx_hash(&[tag; 96]),
        fee_payer: [tag.wrapping_add(1); 32],
        writable: writable.to_vec(),
        readonly: readonly.to_vec(),
        wire_bytes: vec![tag; 96],
    }
}

fn operator_txn(tag: u8, writable: &[[u8; 32]]) -> ObservedTransaction {
    ObservedTransaction {
        fee_payer: IDENTITY,
        ..txn(tag, writable, &[])
    }
}

/// Builds a block whose hash is the genuine fold over `executed`, then reports
/// the transactions in `reported` order.
fn block(executed: &[&ObservedTransaction], reported: Vec<ObservedTransaction>) -> ObservedBlock {
    let blockhash = fold(&PREVIOUS, executed.iter().map(|txn| &txn.signature));
    ObservedBlock {
        slot: SLOT,
        previous_blockhash: PREVIOUS,
        blockhash,
        transactions: reported,
    }
}

/// A receipt at `seq` committing to `txn`'s bytes.
fn receipt_for(seq: u64, txn: &ObservedTransaction) -> SignedReceipt {
    Receipt {
        mode: Mode::Plain,
        seq,
        tx_sig: txn.signature,
        tx_hash: tx_hash(&txn.wire_bytes),
        recent_blockhash: [0x33; 32],
        prev_receipt_hash: [0x44; 32],
        committer: ZERO_PUBKEY,
        ingress_slot: SLOT,
        t_ingress_micros: 1_700_000_000_000_000 + seq,
    }
    .sign(&operator())
    .unwrap()
}

fn log(entries: &[(u64, &ObservedTransaction)]) -> ReceiptIndex {
    let receipts: Vec<SignedReceipt> = entries
        .iter()
        .map(|(seq, txn)| receipt_for(*seq, txn))
        .collect();
    ReceiptIndex::build(&receipts)
}

fn scan(block: &ObservedBlock, receipts: &ReceiptIndex) -> mb_watchtower::Scan {
    scan_block(block, receipts, &operator().verifying_key(), &IDENTITY)
}

// --- Order derivation ---

/// getBlock hands transactions back newest-index-first. A watchtower that
/// trusted the reported order would read this honest block as fully reversed.
#[test]
fn a_block_reported_backwards_is_still_read_in_execution_order() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_A], &[]));
    let reported = vec![b.clone(), a.clone()]; // as getBlock returns them
    let block = block(&[&a, &b], reported);

    assert_eq!(Order::derive(&block), Order::Reversed);
    let executed: Vec<[u8; 64]> = block
        .executed_order()
        .unwrap()
        .iter()
        .map(|txn| txn.signature)
        .collect();
    assert_eq!(executed, vec![a.signature, b.signature]);

    assert!(scan(&block, &log(&[(0, &a), (1, &b)])).is_clean());
}

#[test]
fn a_block_that_reproduces_neither_direction_is_undetermined() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_A], &[]));
    let mut tampered = block(&[&a, &b], vec![a.clone(), b.clone()]);
    tampered.blockhash = [0xFF; 32];

    let scan = scan(&tampered, &log(&[(0, &a), (1, &b)]));

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::UnverifiableBlock { slot: SLOT }]
    );
}

// --- False-positive guards ---

/// Parallel execution reorders unrelated transactions constantly. Without the
/// conflict test the detector would fire on every busy honest slot.
#[test]
fn transactions_sharing_no_account_are_never_a_reorder() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_B], &[]));
    // b executed first despite arriving second.
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    let scan = scan(&block, &log(&[(0, &a), (1, &b)]));

    assert!(scan.is_clean(), "{scan:?}");
}

/// Two readers of one account cannot front-run each other.
#[test]
fn read_only_sharing_is_not_a_conflict() {
    let (a, b) = (txn(1, &[], &[ACCOUNT_A]), txn(2, &[], &[ACCOUNT_A]));
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    assert!(scan(&block, &log(&[(0, &a), (1, &b)])).is_clean());
}

/// The clone inversion. A just-in-time account clone is sequenced after the
/// request that triggered it, yet must execute before it. Honest, and
/// indistinguishable here from deliberate insertion — so neither accused nor
/// cleared.
#[test]
fn operator_issued_work_executing_early_is_undetermined() {
    let client = txn(1, &[ACCOUNT_A], &[]);
    let clone = operator_txn(2, &[ACCOUNT_A]);
    // The clone runs first but was sequenced second.
    let block = block(&[&clone, &client], vec![clone.clone(), client.clone()]);

    let scan = scan(&block, &log(&[(0, &client), (1, &clone)]));

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::OperatorIssuedPair { slot: SLOT }]
    );
}

#[test]
fn an_honest_fifo_block_is_clean() {
    let (a, b, c) = (
        txn(1, &[ACCOUNT_A], &[]),
        txn(2, &[ACCOUNT_A], &[]),
        txn(3, &[ACCOUNT_B], &[]),
    );
    let block = block(&[&a, &b, &c], vec![a.clone(), b.clone(), c.clone()]);

    let scan = scan(&block, &log(&[(0, &a), (1, &b), (2, &c)]));

    assert!(scan.is_clean(), "{scan:?}");
    assert!(scan.examined() >= 6);
}

#[test]
fn an_empty_block_is_clean() {
    let block = block(&[], vec![]);
    assert!(scan(&block, &ReceiptIndex::default()).is_clean());
}

// --- Detection ---

#[test]
fn a_swapped_pair_of_conflicting_transactions_is_a_reorder() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_A], &[]));
    // b arrived second (seq 1) but executed first.
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    let scan = scan(&block, &log(&[(0, &a), (1, &b)]));

    assert_eq!(scan.faults.len(), 1, "{:?}", scan.faults);
    let Fault::Reorder {
        jumped, delayed, ..
    } = &scan.faults[0]
    else {
        panic!("expected a reorder, got {:?}", scan.faults[0]);
    };
    assert_eq!(jumped.receipt.receipt.seq, 1);
    assert_eq!(jumped.index, 0);
    assert_eq!(delayed.receipt.receipt.seq, 0);
    assert_eq!(delayed.index, 1);
}

/// A conflict through one shared account is enough, even when most accounts
/// differ.
#[test]
fn a_single_shared_written_account_is_enough_to_order_two_transactions() {
    let a = txn(1, &[ACCOUNT_A], &[]);
    let b = txn(2, &[ACCOUNT_B], &[ACCOUNT_A]);
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    let scan = scan(&block, &log(&[(0, &a), (1, &b)]));

    assert_eq!(scan.faults.len(), 1, "{:?}", scan.faults);
}

#[test]
fn a_transaction_with_no_receipt_is_unticketed() {
    let (a, injected) = (txn(1, &[ACCOUNT_A], &[]), txn(9, &[ACCOUNT_A], &[]));
    let block = block(&[&injected, &a], vec![injected.clone(), a.clone()]);

    let scan = scan(&block, &log(&[(0, &a)]));

    let unticketed: Vec<&Fault> = scan
        .faults
        .iter()
        .filter(|fault| matches!(fault, Fault::Unticketed { .. }))
        .collect();
    assert_eq!(unticketed.len(), 1, "{:?}", scan.faults);
    let Fault::Unticketed {
        index, signature, ..
    } = unticketed[0]
    else {
        unreachable!()
    };
    assert_eq!(*index, 0);
    assert_eq!(*signature, injected.signature);
}

// --- Evidence quality ---

#[test]
fn reorder_evidence_verifies_standalone() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_A], &[]));
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    let scan = scan(&block, &log(&[(0, &a), (1, &b)]));

    for fault in &scan.faults {
        assert_eq!(
            fault.verify(&operator().verifying_key()),
            Ok(()),
            "{fault:?}"
        );
    }
}

/// The evidence must fall apart if its transaction bytes are swapped for
/// something the receipt does not commit to, or the binding proves nothing.
#[test]
fn reorder_evidence_rejects_transaction_bytes_the_receipt_does_not_bind() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_A], &[]));
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    let mut scan = scan(&block, &log(&[(0, &a), (1, &b)]));
    let Fault::Reorder { jumped, .. } = &mut scan.faults[0] else {
        unreachable!()
    };
    jumped.wire_bytes = vec![0xDE; 96];

    assert!(scan.faults[0].verify(&operator().verifying_key()).is_err());
}

/// Likewise if the claimed execution order does not reproduce the block hash.
#[test]
fn reorder_evidence_rejects_a_doctored_execution_order() {
    let (a, b) = (txn(1, &[ACCOUNT_A], &[]), txn(2, &[ACCOUNT_A], &[]));
    let block = block(&[&b, &a], vec![b.clone(), a.clone()]);

    let mut scan = scan(&block, &log(&[(0, &a), (1, &b)]));
    let Fault::Reorder { executed, .. } = &mut scan.faults[0] else {
        unreachable!()
    };
    executed.reverse();

    assert!(scan.faults[0].verify(&operator().verifying_key()).is_err());
}
