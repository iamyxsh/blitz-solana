use ed25519_dalek::SigningKey;
use mb_receipt::{Mode, Receipt, SignedReceipt, ZERO_HASH, ZERO_PUBKEY};
use mb_watchtower::{Fault, Operator, Undetermined, equivocation::scan_receipts};

fn operator() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

const LOG_ID: [u8; 32] = [0x9c; 32];

/// The node this watchtower follows: whose key, and which run of its log.
fn watched() -> Operator {
    Operator::new(operator().verifying_key(), LOG_ID)
}

fn stranger() -> SigningKey {
    SigningKey::from_bytes(&[0x09; 32])
}

fn receipt(index: u8, previous: [u8; 32]) -> Receipt {
    Receipt {
        log_id: LOG_ID,
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

fn log_in(log_id: [u8; 32], count: u8) -> Vec<SignedReceipt> {
    let key = operator();
    let mut previous = [0u8; 32];
    (0..count)
        .map(|index| {
            let signed = Receipt {
                log_id,
                ..receipt(index, previous)
            }
            .sign(&key)
            .unwrap();
            previous = signed.receipt_hash();
            signed
        })
        .collect()
}

fn honest_log(count: u8) -> Vec<SignedReceipt> {
    log_in(LOG_ID, count)
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

    let scan = scan_receipts(&doubled, &watched());

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

    let scan = scan_receipts(&punctured, &watched());

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

    let scan = scan_receipts(&shuffled, &watched());

    assert!(scan.is_clean(), "{scan:?}");
}

#[test]
fn a_log_that_starts_above_zero_is_undetermined_at_its_origin() {
    let log = honest_log(5);

    let scan = scan_receipts(&log[2..], &watched());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::MissingOrigin { lowest: 2 }]
    );
}

/// A withdrawal takes its own position rather than the one it voids, so the
/// log still holds exactly one statement per sequence number and every link
/// still points at the entry before it. Without this the operator's own
/// documented way of undoing a position reads as a contradiction.
#[test]
fn a_log_containing_a_retraction_is_clean() {
    let key = operator();
    let mut log = honest_log(3);
    let retraction = Receipt {
        log_id: LOG_ID,
        mode: Mode::Retract,
        seq: 3,
        tx_sig: log[1].receipt.tx_sig,
        tx_hash: log[1].receipt_hash(),
        recent_blockhash: ZERO_HASH,
        prev_receipt_hash: log[2].receipt_hash(),
        committer: ZERO_PUBKEY,
        ingress_slot: 1_003,
        t_ingress_micros: 1_700_000_000_000_003,
    }
    .sign(&key)
    .unwrap();
    log.push(retraction);

    let scan = scan_receipts(&log, &watched());

    assert!(scan.is_clean(), "{scan:?}");
}

/// A restart is not misbehaviour. The sequence counter starts again at zero
/// while the signing key does not, so without the log id every entry of the
/// new run contradicts the old run's entry at the same position — all of them
/// genuinely signed, all of them verifying standalone.
#[test]
fn two_incarnations_of_one_log_are_not_equivocation() {
    let mut receipts = honest_log(4);
    receipts.extend(log_in([0x01; 32], 4));

    let scan = scan_receipts(&receipts, &watched());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![
            Undetermined::ForeignLog { seq: 0 },
            Undetermined::ForeignLog { seq: 1 },
            Undetermined::ForeignLog { seq: 2 },
            Undetermined::ForeignLog { seq: 3 },
        ]
    );
}

#[test]
fn an_honest_log_is_clean() {
    let scan = scan_receipts(&honest_log(16), &watched());
    assert!(scan.is_clean(), "{scan:?}");
    assert!(scan.examined() > 16);
}

#[test]
fn an_empty_log_is_clean() {
    let scan = scan_receipts(&[], &watched());
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

    let scan = scan_receipts(&log, &watched());

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

    let scan = scan_receipts(&log, &watched());

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

    let scan = scan_receipts(&log, &watched());

    let broken: Vec<u64> = scan
        .faults
        .iter()
        .filter(|fault| matches!(fault, Fault::BrokenChain { .. }))
        .map(|fault| fault.seq())
        .collect();
    assert_eq!(broken, vec![3], "{:?}", scan.faults);
}

/// Replacing an entry with one the operator never signed removes it from the
/// evidence rather than convicting anyone with it. What is left is a hole,
/// which is exactly what a hole should look like.
#[test]
fn a_receipt_signed_by_a_stranger_is_set_aside() {
    let mut log = honest_log(3);
    log[1] = log[1].receipt.clone().sign(&stranger()).unwrap();

    let scan = scan_receipts(&log, &watched());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![
            Undetermined::UnverifiableReceipt { seq: 1 },
            Undetermined::SequenceGap {
                after: 0,
                before: 2
            },
        ]
    );
}

/// A stranger holding no key must not be able to make this detector accuse
/// anybody. Appending junk at a sequence the honest log already occupies is
/// the cheapest attempt available, and it costs nothing to try.
#[test]
fn a_forged_receipt_cannot_manufacture_an_equivocation() {
    let mut log = honest_log(3);
    log.push(
        Receipt {
            tx_sig: [0xAB; 64],
            ..receipt(1, log[0].receipt_hash())
        }
        .sign(&stranger())
        .unwrap(),
    );

    let scan = scan_receipts(&log, &watched());

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::UnverifiableReceipt { seq: 1 }]
    );
}

// --- Evidence quality ---

/// The property that makes a fault worth publishing: someone holding only the
/// object and the operator's public key reaches the same conclusion.
#[test]
fn every_fault_verifies_standalone() {
    let key = operator();
    let operator_key = watched().key;
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

    let scan = scan_receipts(&log, &watched());
    assert!(scan.faults.len() >= 2, "{:?}", scan.faults);

    for fault in &scan.faults {
        assert_eq!(
            fault.verify(&operator_key),
            Ok(()),
            "fault did not re-derive from its own evidence: {fault:?}"
        );
    }
}

/// Evidence must be sound out of context, because `verify` is the predicate
/// the on-chain program runs and it has no log to configure itself against.
///
/// Two honest runs under one key: entry 2 of the first and entry 3 of the
/// second are adjacent by sequence number, and of course the second's chain
/// link does not point at the first's entry — it points at its own. Assembled
/// as a broken chain that is an honest operator convicted of rewriting its log,
/// on receipts it genuinely signed, by an accuser who wrote nothing.
#[test]
fn broken_chain_evidence_from_two_runs_does_not_verify() {
    let first = log_in(LOG_ID, 4);
    let second = log_in([0x01; 32], 4);

    let framed = Fault::BrokenChain {
        seq: 3,
        receipt: second[3].clone(),
        predecessor: first[2].clone(),
    };

    assert_eq!(
        framed.verify(&watched().key),
        Err(mb_watchtower::FaultError::MixedLogs)
    );
}

/// The same hole in the variant the slashing program implements first.
#[test]
fn equivocation_evidence_from_two_runs_does_not_verify() {
    let first = log_in(LOG_ID, 4);
    let second = log_in([0x01; 32], 4);

    let framed = Fault::Equivocation {
        seq: 2,
        a: first[2].clone(),
        b: second[2].clone(),
    };

    assert_eq!(
        framed.verify(&watched().key),
        Err(mb_watchtower::FaultError::MixedLogs)
    );
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

    let scan = scan_receipts(&log, &watched());
    assert_eq!(scan.faults.len(), 1);
    assert!(scan.faults[0].verify(&stranger().verifying_key()).is_err());
}
