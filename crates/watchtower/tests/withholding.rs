use std::collections::HashMap;

use ed25519_dalek::SigningKey;
use mb_receipt::{Mode, Receipt, SignedReceipt, ZERO_PUBKEY};
use mb_watchtower::{BlockhashSlots, Execution, Fault, Patience, Undetermined, scan_withholding};

const BLOCKHASH: [u8; 32] = [0x33; 32];
const BLOCKHASH_SLOT: u64 = 1_000;

fn operator() -> SigningKey {
    SigningKey::from_bytes(&[0x07; 32])
}

fn receipt(seq: u64, ingress_slot: u64) -> SignedReceipt {
    Receipt {
        mode: Mode::Plain,
        seq,
        tx_sig: [(seq as u8) | 0x80; 64],
        tx_hash: [(seq as u8) | 0x40; 32],
        recent_blockhash: BLOCKHASH,
        prev_receipt_hash: [0x44; 32],
        committer: ZERO_PUBKEY,
        ingress_slot,
        t_ingress_micros: 1_700_000_000_000_000 + seq,
    }
    .sign(&operator())
    .unwrap()
}

fn blockhashes() -> BlockhashSlots {
    let mut slots = BlockhashSlots::default();
    slots.record(BLOCKHASH, BLOCKHASH_SLOT);
    slots
}

fn ran_at(receipt: &SignedReceipt, slot: u64) -> HashMap<[u8; 64], Execution> {
    HashMap::from([(receipt.receipt.tx_sig, Execution { slot, index: 0 })])
}

fn scan(
    receipts: &[SignedReceipt],
    executed: &HashMap<[u8; 64], Execution>,
    head: u64,
) -> mb_watchtower::Scan {
    scan_withholding(
        receipts,
        executed,
        &blockhashes(),
        head,
        &Patience::default(),
        &operator().verifying_key(),
    )
}

// --- False-positive guards ---

#[test]
fn a_transaction_that_ran_promptly_is_clean() {
    let receipt = receipt(0, BLOCKHASH_SLOT + 5);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 6);

    let scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 10);

    assert!(scan.is_clean(), "{scan:?}");
}

/// Delay within the grace period is ordinary: lock contention and cold-account
/// cloning both cost real slots on an honest node.
#[test]
fn delay_inside_the_grace_period_is_clean() {
    let receipt = receipt(0, BLOCKHASH_SLOT);
    let patience = Patience::default();
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + patience.held_slots);

    let scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 500);

    assert!(scan.is_clean(), "{scan:?}");
}

/// A receipt whose transaction has not appeared yet is the normal state of
/// anything recent. Calling that withholding would fire constantly.
#[test]
fn a_recent_receipt_with_no_execution_yet_is_undetermined() {
    let receipt = receipt(0, BLOCKHASH_SLOT);

    let scan = scan(&[receipt], &HashMap::new(), BLOCKHASH_SLOT + 3);

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::NotYetExecuted { seq: 0 }]
    );
}

/// Once a block hash has aged out of the ring, nothing about its timing can
/// be asserted — so nothing is.
#[test]
fn a_forgotten_blockhash_is_undetermined_rather_than_a_fault() {
    let receipt = receipt(0, BLOCKHASH_SLOT);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 1);

    let scan = scan_withholding(
        &[receipt],
        &executed,
        &BlockhashSlots::default(), // empty: hash never recorded
        BLOCKHASH_SLOT + 10,
        &Patience::default(),
        &operator().verifying_key(),
    );

    assert!(scan.faults.is_empty(), "{:?}", scan.faults);
    assert_eq!(
        scan.undetermined,
        vec![Undetermined::UnknownBlockhash { seq: 0 }]
    );
}

// --- Detection ---

#[test]
fn a_transaction_held_far_past_its_receipt_is_withheld() {
    let receipt = receipt(0, BLOCKHASH_SLOT);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 500);

    let scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 600);

    assert_eq!(scan.faults.len(), 1, "{:?}", scan.faults);
    let Fault::Withheld { held, .. } = &scan.faults[0] else {
        panic!("expected withholding, got {:?}", scan.faults[0]);
    };
    assert_eq!(*held, 500);
}

#[test]
fn a_receipted_transaction_that_never_runs_is_absent() {
    let receipt = receipt(0, BLOCKHASH_SLOT);

    let scan = scan(&[receipt], &HashMap::new(), BLOCKHASH_SLOT + 5_000);

    assert_eq!(scan.faults.len(), 1, "{:?}", scan.faults);
    let Fault::Absent { waited, .. } = &scan.faults[0] else {
        panic!("expected absence, got {:?}", scan.faults[0]);
    };
    assert_eq!(*waited, 5_000);
}

/// A receipt cannot have arrived before the block hash it names existed.
/// The operator signed both numbers, so this needs nothing else to disprove.
#[test]
fn ingress_before_its_own_blockhash_is_impossible() {
    let receipt = receipt(0, BLOCKHASH_SLOT - 1);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 1);

    let scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 10);

    assert!(
        scan.faults
            .iter()
            .any(|fault| matches!(fault, Fault::ImpossibleIngress { .. }))
    );
}

/// Backdating ingress to hide a delay runs into the other wall: the node
/// would never have accepted a block hash that old.
#[test]
fn ingress_beyond_the_blockhash_window_is_impossible() {
    let patience = Patience::default();
    let receipt = receipt(0, BLOCKHASH_SLOT + patience.max_blockhash_age + 1);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 2_000);

    let scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 2_100);

    assert!(
        scan.faults
            .iter()
            .any(|fault| matches!(fault, Fault::ImpossibleIngress { .. }))
    );
}

// --- Evidence quality ---

#[test]
fn withholding_evidence_verifies_standalone() {
    let receipt = receipt(0, BLOCKHASH_SLOT);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 500);
    let scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 600);

    for fault in &scan.faults {
        assert_eq!(fault.verify(&operator().verifying_key()), Ok(()));
    }
}

/// A doctored delay must not survive re-derivation, or `verify` proves
/// nothing about the number it reports.
#[test]
fn an_overstated_delay_does_not_verify() {
    let receipt = receipt(0, BLOCKHASH_SLOT);
    let executed = ran_at(&receipt, BLOCKHASH_SLOT + 500);
    let mut scan = scan(&[receipt], &executed, BLOCKHASH_SLOT + 600);

    let Fault::Withheld { held, .. } = &mut scan.faults[0] else {
        unreachable!()
    };
    *held = 9_999;

    assert!(scan.faults[0].verify(&operator().verifying_key()).is_err());
}

#[test]
fn a_forged_receipt_is_never_reasoned_from() {
    let stranger = SigningKey::from_bytes(&[0x09; 32]);
    let forged = receipt(0, BLOCKHASH_SLOT).receipt.sign(&stranger).unwrap();
    let executed = ran_at(&forged, BLOCKHASH_SLOT + 500);

    let scan = scan(&[forged], &executed, BLOCKHASH_SLOT + 600);

    assert!(scan.is_clean(), "{scan:?}");
}

// --- The ring itself ---

#[test]
fn the_ring_forgets_oldest_first_and_stays_bounded() {
    let mut slots = BlockhashSlots::new(3);
    for index in 0..5u8 {
        slots.record([index; 32], index as u64);
    }

    assert_eq!(slots.len(), 3);
    assert_eq!(slots.slot_of(&[0; 32]), None);
    assert_eq!(slots.slot_of(&[1; 32]), None);
    assert_eq!(slots.slot_of(&[4; 32]), Some(4));
}

/// Re-recording a hash must not consume a fresh slot in the ring, or a node
/// replaying blocks would evict everything it still needs.
#[test]
fn recording_the_same_blockhash_twice_does_not_grow_the_ring() {
    let mut slots = BlockhashSlots::new(3);
    for _ in 0..10 {
        slots.record([1; 32], 1);
    }

    assert_eq!(slots.len(), 1);
    assert_eq!(slots.slot_of(&[1; 32]), Some(1));
}
