//! Experiment 1: does `getBlock` return a slot's transactions in the order
//! they executed?
//!
//! `get_block` seeks from `(slot, u32::MAX)` with `IteratorDirection::Reverse`
//! and pushes signatures in iteration order without re-sorting, while its
//! sibling `get_transaction_signatures_for_slot` seeks forward from
//! `(slot, 0)` and documents ascending as canonical.
//!
//! This matters well beyond cosmetics: anything deriving executed order from
//! `getBlock` sees a correctly ordered slot as fully reversed.

use magicblock_ledger::Ledger;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::sanitized::SanitizedTransaction;
use solana_transaction_status::TransactionStatusMeta;
use tempfile::tempdir;

fn setup() -> Ledger {
    let directory = tempdir().unwrap();
    Ledger::open(&directory.keep()).unwrap()
}

const SLOT: u64 = 10;
const TRANSACTIONS: u32 = 4;

/// Writes one transaction keyed by its own signature, the way the executor
/// does at `executor/processing.rs:304`. The shared `write_dummy_transaction`
/// helper keys rows by an unrelated random signature, which would make an
/// ordering comparison meaningless.
fn write_transaction_at(ledger: &Ledger, index: u32) -> Signature {
    let txn = solana_system_transaction::transfer(
        &Keypair::new(),
        &Pubkey::new_unique(),
        99,
        solana_hash::Hash::new_unique(),
    );
    let signature = txn.signatures[0];
    let sanitized = SanitizedTransaction::from_transaction_for_tests(txn);
    let locks = sanitized.get_account_locks_unchecked();
    let encoded =
        bincode::serialize(&sanitized.to_versioned_transaction()).unwrap();

    ledger
        .write_transaction(
            signature,
            SLOT,
            index,
            locks.writable,
            locks.readonly,
            &encoded,
            TransactionStatusMeta::default(),
        )
        .expect("failed to write transaction");
    signature
}

/// Writes `TRANSACTIONS` transactions at ascending indices and returns their
/// signatures in index order.
fn write_slot(ledger: &Ledger) -> Vec<String> {
    (0..TRANSACTIONS)
        .map(|index| write_transaction_at(ledger, index).to_string())
        .collect()
}

#[test]
fn get_block_returns_transactions_in_descending_index_order() {
    let ledger = setup();
    let by_index = write_slot(&ledger);
    ledger
        .write_block(magicblock_core::link::blocks::LatestBlockInner::new(
            SLOT,
            solana_hash::Hash::new_unique(),
            1,
        ))
        .unwrap();

    let block = ledger
        .get_block(SLOT)
        .unwrap()
        .expect("block should be readable");
    let returned: Vec<String> = block
        .transactions
        .iter()
        .map(|txn| txn.transaction.signatures[0].to_string())
        .collect();

    let mut reversed = by_index.clone();
    reversed.reverse();

    println!("wrote (index order): {by_index:#?}");
    println!("getBlock returned:   {returned:#?}");

    assert_eq!(
        returned, reversed,
        "getBlock returns a slot's transactions newest-index-first"
    );
    assert_ne!(
        returned, by_index,
        "if this now passes, getBlock has been fixed upstream"
    );
}

/// The sibling reader is the one that agrees with execution order, which is
/// what makes the inconsistency a bug rather than a convention.
#[test]
fn get_transaction_signatures_for_slot_returns_ascending_index_order() {
    let ledger = setup();
    let by_index = write_slot(&ledger);

    let returned: Vec<String> = ledger
        .get_transaction_signatures_for_slot(SLOT)
        .unwrap()
        .iter()
        .map(|signature| signature.to_string())
        .collect();

    assert_eq!(returned, by_index);
}
