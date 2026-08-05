use ed25519_dalek::SigningKey;
use mb_receipt::{Mode, Receipt, SignedReceipt, ZERO_PUBKEY};
use mb_watchtower::{Fault, Undetermined, equivocation::scan_receipts};

fn operator() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

fn stranger() -> SigningKey {
    SigningKey::from_bytes(&[0x09; 32])
}

fn receipt(index: u8, previous: [u8; 32]) -> Receipt {
    Receipt {
        mode: Mode::Plain,
        seq: index as u64,
        tx_sig: [index | 0x80; 64],
        tx_hash: [index | 0x40; 32],
        recent_blockhash: [index | 0x20; 32],
        prev_receipt_hash: previous,
        committer: ZERO_PUBKEY,
        ingress_slot: 1_000 + index as u64,
        t_ingress_micros: 1_700_000_000_000_000 + index as u64,
    }
}

fn honest_log(count: u8) -> Vec<SignedReceipt> {
    let key = operator();
    let mut previous = [0u8; 32];
    (0..count)
        .map(|index| {
            let signed = receipt(index, previous).sign(&key).unwrap();
            previous = signed.receipt_hash();
            signed
        })
        .collect()
}

// --- False-positive guards ---
//
// These matter more than the detection tests. A detector that cries wolf on
// ordinary stream behaviour is worse than no detector, because the one real
// fault it eventually finds carries no weight.

/// A reconnect overlapping a backfill delivers the same receipt twice. That is
/// re-delivery, not two contradictory statements.
#[test]
fn an_identical_receipt_delivered_twice_is_not_a_fault() {
    let log = honest_log(4);
    let mut doubled = log.clone();
    doubled.extend(log.iter().cloned());

    let scan = scan_receipts(&doubled, &operator().verifying_key());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert!(scan.is_clean());
}

/// Paging, a lagged stream and a late subscriber all leave holes. A hole is
/// something the detector could not see, never something the operator did.
#[test]
fn a_sequence_gap_is_undetermined_rather_than_a_fault() {
    let log = honest_log(6);
    let punctured: Vec<SignedReceipt> = log
        .iter()
        .filter(|signed| signed.receipt.seq != 3)
        .cloned()
        .collect();

    let scan = scan_receipts(&punctured, &operator().verifying_key());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::SequenceGap {
            after: 2,
            before: 4
        },]
    );
}

/// Arrival order carries no meaning: the stream and the backfill interleave.
#[test]
fn receipts_arriving_out_of_order_are_not_a_fault() {
    let log = honest_log(5);
    let shuffled = vec![
        log[3].clone(),
        log[0].clone(),
        log[4].clone(),
        log[1].clone(),
        log[2].clone(),
    ];

    let scan = scan_receipts(&shuffled, &operator().verifying_key());

    assert!(scan.is_clean(), "{scan:?}");
}

#[test]
fn a_log_that_starts_above_zero_is_undetermined_at_its_origin() {
    let log = honest_log(5);

    let scan = scan_receipts(&log[2..], &operator().verifying_key());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::MissingOrigin { lowest: 2 }]
    );
}

#[test]
fn an_honest_log_is_clean() {
    let scan = scan_receipts(&honest_log(16), &operator().verifying_key());
    assert!(scan.is_clean(), "{scan:?}");
    assert!(scan.examined() > 16);
}

#[test]
fn an_empty_log_is_clean() {
    let scan = scan_receipts(&[], &operator().verifying_key());
    assert!(scan.is_clean());
}

// --- Detection ---

/// The demo's opening beat: two signed slips claiming the same position.
#[test]
fn two_different_receipts_at_one_sequence_are_equivocation() {
    let key = operator();
    let mut log = honest_log(3);

    // A second, different statement about position 1.
    let forked = Receipt {
        tx_sig: [0xAB; 64],
        ..receipt(1, log[0].receipt_hash())
    }
    .sign(&key)
    .unwrap();
    log.push(forked.clone());

    let scan = scan_receipts(&log, &key.verifying_key());

    assert_eq!(scan.faults.len(), 1, "{:?}", scan.faults);
    let Fault::Equivocation { seq, a, b } = &scan.faults[0] else {
        panic!("expected equivocation, got {:?}", scan.faults[0]);
    };
    assert_eq!(*seq, 1);
    assert_ne!(a, b);
    assert!(*a == forked || *b == forked);
}

/// Rewriting one entry breaks its own link *and* every link after it: the
/// tampered receipt hashes differently, so its successor no longer points at
/// it. Tamper-evidence propagating forward is what the chain buys.
#[test]
fn rewriting_one_entry_breaks_that_link_and_the_next() {
    let key = operator();
    let mut log = honest_log(4);

    log[2] = Receipt {
        prev_receipt_hash: [0xFF; 32],
        ..log[2].receipt.clone()
    }
    .sign(&key)
    .unwrap();

    let scan = scan_receipts(&log, &key.verifying_key());

    let broken: Vec<u64> = scan
        .faults
        .iter()
        .filter(|fault| matches!(fault, Fault::BrokenChain { .. }))
        .map(|fault| fault.seq())
        .collect();
    assert_eq!(broken, vec![2, 3], "{:?}", scan.faults);
}

/// Tampering with the last entry breaks exactly one link, which confirms the
/// pair above is propagation rather than double-counting.
#[test]
fn rewriting_the_final_entry_breaks_only_its_own_link() {
    let key = operator();
    let mut log = honest_log(4);

    log[3] = Receipt {
        prev_receipt_hash: [0xFF; 32],
        ..log[3].receipt.clone()
    }
    .sign(&key)
    .unwrap();

    let scan = scan_receipts(&log, &key.verifying_key());

    let broken: Vec<u64> = scan
        .faults
        .iter()
        .filter(|fault| matches!(fault, Fault::BrokenChain { .. }))
        .map(|fault| fault.seq())
        .collect();
    assert_eq!(broken, vec![3], "{:?}", scan.faults);
}

#[test]
fn a_receipt_signed_by_a_stranger_does_not_verify() {
    let mut log = honest_log(3);
    log[1] = log[1].receipt.clone().sign(&stranger()).unwrap();

    let scan = scan_receipts(&log, &operator().verifying_key());

    assert!(
        scan.faults
            .iter()
            .any(|fault| matches!(fault, Fault::Unverifiable { seq: 1, .. }))
    );
}

// --- Evidence quality ---

/// The property that makes a fault worth publishing: someone holding only the
/// object and the operator's public key reaches the same conclusion.
#[test]
fn every_fault_verifies_standalone() {
    let key = operator();
    let operator_key = key.verifying_key();
    let mut log = honest_log(4);

    log.push(
        Receipt {
            tx_sig: [0xAB; 64],
            ..receipt(1, log[0].receipt_hash())
        }
        .sign(&key)
        .unwrap(),
    );
    log[3] = Receipt {
        prev_receipt_hash: [0xFF; 32],
        ..log[3].receipt.clone()
    }
    .sign(&key)
    .unwrap();

    let scan = scan_receipts(&log, &operator_key);
    assert!(scan.faults.len() >= 2, "{:?}", scan.faults);

    for fault in &scan.faults {
        assert_eq!(
            fault.verify(&operator_key),
            Ok(()),
            "fault did not re-derive from its own evidence: {fault:?}"
        );
    }
}

/// A fault object assembled against the wrong key must refuse to verify,
/// or `verify` proves nothing.
#[test]
fn a_fault_does_not_verify_under_a_foreign_key() {
    let key = operator();
    let mut log = honest_log(2);
    log.push(
        Receipt {
            tx_sig: [0xAB; 64],
            ..receipt(1, log[0].receipt_hash())
        }
        .sign(&key)
        .unwrap(),
    );

    let scan = scan_receipts(&log, &key.verifying_key());
    assert_eq!(scan.faults.len(), 1);
    assert!(scan.faults[0].verify(&stranger().verifying_key()).is_err());
}
