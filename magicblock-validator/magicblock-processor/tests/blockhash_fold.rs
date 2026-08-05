//! Experiment 4: is a block hash literally a blake3 fold over the previous
//! block hash followed by the slot's transaction signatures, in order?
//!
//! `scheduler/mod.rs:554` stirs each signature into a running hasher at
//! dispatch, `:596` finalises it, and `update_sysvars` reseeds the hasher with
//! the finished hash. If that reading is right then
//!
//!   blockhash(n) == blake3(blockhash(n-1) ‖ sig₀ ‖ … ‖ sigₖ)
//!
//! which makes the executed side of any ordering evidence self-checking: hand
//! a verifier the previous hash and the ordered signature list and it can
//! recompute the block hash rather than take the operator's word for the
//! order.

use std::{str::FromStr, time::Duration};

use guinea::GuineaInstruction;
use solana_program::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    native_token::LAMPORTS_PER_SOL,
};
use solana_signature::Signature;
use solana_transaction::Transaction;
use test_kit::{ExecutionTestEnv, Signer};

/// The first slot a freshly started validator finalises has no predecessor to
/// seed its hasher, so it is excluded from the reproduction claim.
const BOOT_SLOT: u64 = 1;

fn transfer(env: &ExecutionTestEnv) -> Transaction {
    let from = env.create_account(LAMPORTS_PER_SOL * 10).pubkey();
    let to = env.create_account(LAMPORTS_PER_SOL * 10).pubkey();
    let ix = Instruction::new_with_bincode(
        guinea::ID,
        &GuineaInstruction::Transfer(1_000),
        vec![AccountMeta::new(from, false), AccountMeta::new(to, false)],
    );
    env.build_transaction(&[ix])
}

/// Recomputes a slot's hash the way the scheduler builds it.
fn fold(previous: &Hash, signatures: &[Signature]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(previous.as_ref());
    for signature in signatures {
        hasher.update(signature.as_ref());
    }
    Hash::new_from_array(*hasher.finalize().as_bytes())
}

/// Executes transactions in batches spread across several slots.
async fn run_across_slots(env: &ExecutionTestEnv) {
    for _ in 0..6 {
        for _ in 0..4 {
            let txn = transfer(env);
            let _ = env.execute_transaction(txn).await;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
}

#[tokio::test]
async fn a_blockhash_is_a_blake3_fold_over_its_ordered_signatures() {
    let env = ExecutionTestEnv::new();
    run_across_slots(&env).await;

    let latest = env.ledger.latest_block().load().slot;
    let (mut populated, mut empty) = (0, 0);

    for slot in 1..=latest {
        let Some(block) = env.ledger.get_block(slot).unwrap() else {
            continue;
        };
        // Ascending order, deliberately not `block.transactions`: getBlock
        // hands those back newest-index-first.
        let signatures = env
            .ledger
            .get_transaction_signatures_for_slot(slot)
            .unwrap();
        let previous = Hash::from_str(&block.previous_blockhash).unwrap();
        let reproduces =
            fold(&previous, &signatures).to_string() == block.blockhash;

        println!(
            "slot {slot:>3}  txs {:>2}  reproduces {reproduces}",
            signatures.len()
        );

        if slot == BOOT_SLOT {
            continue;
        }
        assert!(
            reproduces,
            "slot {slot} with {} signature(s) did not reproduce",
            signatures.len()
        );
        if signatures.is_empty() {
            empty += 1;
        } else {
            populated += 1;
        }
    }

    println!("reproduced {populated} populated and {empty} empty blocks");
    assert!(
        populated >= 3,
        "only {populated} block(s) held transactions; the fold was barely \
         exercised"
    );
    assert!(empty >= 1, "no empty block was checked");
}

/// Reversing the signature list must break the reproduction, or the check
/// above would pass on a hash that ignores order — which is the entire
/// property the evidence relies on.
#[tokio::test]
async fn the_fold_is_order_sensitive() {
    let env = ExecutionTestEnv::new();
    run_across_slots(&env).await;

    let latest = env.ledger.latest_block().load().slot;
    let mut proven = 0;

    for slot in 2..=latest {
        let Some(block) = env.ledger.get_block(slot).unwrap() else {
            continue;
        };
        let mut signatures = env
            .ledger
            .get_transaction_signatures_for_slot(slot)
            .unwrap();
        if signatures.len() < 2 {
            continue;
        }

        let previous = Hash::from_str(&block.previous_blockhash).unwrap();
        signatures.reverse();
        assert_ne!(
            fold(&previous, &signatures).to_string(),
            block.blockhash,
            "slot {slot} reproduced from a reversed signature list"
        );
        proven += 1;
    }

    assert!(proven >= 3, "only {proven} slot(s) held two transactions");
}
