use magicblock_ledger::Ledger;
use solana_signature::Signature;
use tempfile::tempdir;

const PENDING: u8 = 0x01;
const ACCEPTED: u8 = 0x02;
const REJECTED: u8 = 0x03;

/// Stand-in for a 293-byte signed receipt, distinct per sequence number.
fn receipt_bytes(seq: u64) -> Vec<u8> {
    let mut bytes = vec![0u8; 293];
    bytes[..8].copy_from_slice(&seq.to_le_bytes());
    bytes[292] = 0xAB;
    bytes
}

fn setup() -> Ledger {
    let directory = tempdir().unwrap();
    Ledger::open(&directory.keep()).unwrap()
}

fn signature(n: u8) -> Signature {
    Signature::from([n | 0x80; 64])
}

#[test]
fn receipts_round_trip_through_both_indexes() {
    let ledger = setup();
    let bytes = receipt_bytes(7);

    ledger
        .write_receipt(7, signature(7), PENDING, &bytes)
        .unwrap();

    assert_eq!(
        ledger.read_receipt(7).unwrap(),
        Some((PENDING, bytes.clone()))
    );
    assert_eq!(
        ledger.read_receipt_by_signature(signature(7)).unwrap(),
        Some((7, PENDING, bytes))
    );
}

/// Big-endian keys are the only reason a scan comes back in numeric order.
/// Little-endian would sort 256 before 255, silently reordering the chain the
/// watchtower walks — so the fixture straddles two byte boundaries.
#[test]
fn receipts_scan_in_numeric_order_across_byte_boundaries() {
    let ledger = setup();
    let written = [254u64, 255, 256, 257, 65_535, 65_536, 65_537];

    // Insert out of order so the scan cannot accidentally reflect write order.
    for seq in [65_536u64, 255, 65_537, 257, 254, 65_535, 256] {
        ledger
            .write_receipt(
                seq,
                signature(seq as u8),
                PENDING,
                &receipt_bytes(seq),
            )
            .unwrap();
    }

    let scanned: Vec<u64> = ledger
        .iter_receipts(0)
        .unwrap()
        .map(|(seq, _, _)| seq)
        .collect();
    assert_eq!(scanned, written);

    let from_middle: Vec<u64> = ledger
        .iter_receipts(257)
        .unwrap()
        .map(|(seq, _, _)| seq)
        .collect();
    assert_eq!(from_middle, [257, 65_535, 65_536, 65_537]);
}

#[test]
fn an_outcome_update_preserves_the_receipt_bytes() {
    let ledger = setup();
    let bytes = receipt_bytes(3);
    ledger
        .write_receipt(3, signature(3), PENDING, &bytes)
        .unwrap();

    assert!(ledger.set_receipt_outcome(3, REJECTED).unwrap());

    let (outcome, stored) = ledger.read_receipt(3).unwrap().unwrap();
    assert_eq!(outcome, REJECTED);
    assert_eq!(stored, bytes, "the signed receipt must survive verbatim");
}

#[test]
fn updating_an_unknown_sequence_reports_false_rather_than_failing() {
    let ledger = setup();
    assert!(!ledger.set_receipt_outcome(42, ACCEPTED).unwrap());
}

/// A gap must be reportable as absence. The watchtower's third verdict is
/// "cannot determine", and it can only reach it if a missing row is not an
/// error.
#[test]
fn a_missing_sequence_reads_as_none() {
    let ledger = setup();
    ledger
        .write_receipt(0, signature(0), PENDING, &receipt_bytes(0))
        .unwrap();

    assert_eq!(ledger.read_receipt(1).unwrap(), None);
    assert_eq!(
        ledger.read_receipt_by_signature(signature(9)).unwrap(),
        None
    );
}

/// The whole point of the slice. Without this the tests only prove a memtable
/// works.
#[test]
fn receipts_survive_reopening_the_ledger() {
    let directory = tempdir().unwrap();
    let path = directory.keep();

    {
        let ledger = Ledger::open(&path).unwrap();
        for seq in 0..4u64 {
            ledger
                .write_receipt(
                    seq,
                    signature(seq as u8),
                    PENDING,
                    &receipt_bytes(seq),
                )
                .unwrap();
        }
        ledger.set_receipt_outcome(2, ACCEPTED).unwrap();
        ledger.flush().unwrap();
    }

    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.count_receipts().unwrap(), 4);
    assert_eq!(
        ledger.read_receipt(2).unwrap(),
        Some((ACCEPTED, receipt_bytes(2)))
    );
    assert_eq!(
        ledger.read_receipt_by_signature(signature(3)).unwrap(),
        Some((3, PENDING, receipt_bytes(3)))
    );
}
