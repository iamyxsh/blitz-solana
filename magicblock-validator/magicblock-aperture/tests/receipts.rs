use std::time::{Duration, Instant};

use mb_receipt::{tx_hash, Mode, Outcome, GENESIS_PREV_HASH, ZERO_PUBKEY};
use setup::RpcTestEnv;

mod setup;

/// Outcome recording is fire-and-forget through the writer's channel, so the
/// HTTP response can land marginally before the row is updated.
async fn await_outcome(env: &RpcTestEnv, seq: u64) -> u8 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (outcome, _) = env
            .execution
            .ledger
            .read_receipt(seq)
            .unwrap()
            .expect("receipt must be persisted");
        if outcome != Outcome::Pending.as_u8() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "outcome never left Pending");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The load-bearing assertion of the whole design: the receipt must commit to
/// the bytes the client actually signed and sent.
///
/// If the node ever hashed a re-serialization of the decoded transaction
/// instead, this fails — and a client verifying its own receipt offline would
/// fail too, silently, with no way to tell an honest node from a lying one.
#[tokio::test]
async fn receipt_commits_to_the_bytes_the_client_sent() {
    let env = RpcTestEnv::new().await;
    let txn = env.build_transfer_txn();
    let wire = bincode::serialize(&txn).expect("transaction must encode");

    let (signature, signed) = env.send_transaction_ok(&txn).await;

    assert_eq!(signed.receipt.tx_hash, tx_hash(&wire));
    assert_eq!(signed.receipt.tx_sig, signature.as_ref());
    assert_eq!(
        signed.receipt.recent_blockhash,
        txn.message.recent_blockhash.to_bytes()
    );
}

#[tokio::test]
async fn a_receipt_verifies_against_the_node_identity() {
    let env = RpcTestEnv::new().await;
    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;

    assert!(signed.verify(&env.operator).is_ok());

    // A neighbouring key must not verify, or the check above proves nothing.
    let stranger = ed25519_dalek::SigningKey::from_bytes(&[0x09; 32]);
    assert!(signed.verify(&stranger.verifying_key()).is_err());
}

#[tokio::test]
async fn receipts_are_sequenced_from_zero_and_chain_together() {
    let env = RpcTestEnv::new().await;

    let mut receipts = Vec::new();
    for _ in 0..3 {
        let (_, signed) =
            env.send_transaction_ok(&env.build_transfer_txn()).await;
        receipts.push(signed);
    }

    assert_eq!(
        receipts.iter().map(|r| r.receipt.seq).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(receipts[0].receipt.prev_receipt_hash, GENESIS_PREV_HASH);
    for pair in receipts.windows(2) {
        assert_eq!(pair[1].receipt.prev_receipt_hash, pair[0].receipt_hash());
    }
}

/// A transaction refused at ingress must not consume a sequence number.
///
/// Stamping before validation would let anyone punch holes in the chain with
/// garbage, and a hole is exactly what the watchtower reads as a withheld
/// transaction.
#[tokio::test]
async fn a_refused_transaction_consumes_no_sequence_number() {
    let env = RpcTestEnv::new().await;

    let mut stale = env.build_transfer_txn();
    stale.message.recent_blockhash = Default::default();
    assert!(
        env.send_transaction(&stale, Default::default())
            .await
            .is_err(),
        "a transaction with an unknown blockhash must be rejected"
    );

    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;
    assert_eq!(signed.receipt.seq, 0);
    assert_eq!(signed.receipt.prev_receipt_hash, GENESIS_PREV_HASH);
}

#[tokio::test]
async fn a_receipt_records_the_live_slot_in_plain_mode() {
    let env = RpcTestEnv::new().await;
    let before = env.block.load().slot;

    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;

    assert_eq!(signed.receipt.mode, Mode::Plain);
    assert_eq!(signed.receipt.committer, ZERO_PUBKEY);
    assert!(
        signed.receipt.ingress_slot >= before,
        "ingress slot {} predates the slot observed before sending ({before})",
        signed.receipt.ingress_slot,
    );
    assert_ne!(signed.receipt.t_ingress_micros, 0);
}

#[tokio::test]
async fn a_receipt_is_durable_before_the_client_is_answered() {
    let env = RpcTestEnv::new().await;
    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;

    // No waiting: the row must already exist by the time we were answered.
    let (_, stored) = env
        .execution
        .ledger
        .read_receipt(signed.receipt.seq)
        .unwrap()
        .expect("receipt must be persisted before the response is sent");
    assert_eq!(stored, signed.to_bytes());
}

#[tokio::test]
async fn an_executed_transaction_is_recorded_as_accepted() {
    let env = RpcTestEnv::new().await;
    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;

    assert_eq!(
        await_outcome(&env, signed.receipt.seq).await,
        Outcome::Accepted.as_u8()
    );
}

/// The distinction that decides whether the outcome byte is safe.
///
/// With preflight on, a reverting transaction surfaces to the client as an
/// error — but it still occupied a position in a block, so the log must record
/// it as Accepted. Marking it Rejected would hand an operator a way to erase a
/// real execution from the watchtower's view.
#[tokio::test]
async fn a_reverted_transaction_is_still_accepted() {
    let env = RpcTestEnv::new().await;
    let doomed = env.build_failing_transfer_txn();
    let signature = doomed.signatures[0];

    let answered = env.send_transaction(&doomed, Default::default()).await;
    assert!(
        answered.is_err(),
        "preflight should surface the revert to the caller"
    );

    let (seq, _, _) = env
        .execution
        .ledger
        .read_receipt_by_signature(signature)
        .unwrap()
        .expect("a transaction that ran must still be receipted");
    assert_eq!(
        await_outcome(&env, seq).await,
        Outcome::Accepted.as_u8(),
        "a revert is an execution, not a rejection"
    );
}

/// The watchtower's read path, end to end: scan the node's persisted log and
/// verify the chain without asking the node anything.
#[tokio::test]
async fn the_persisted_log_replays_a_verifiable_chain() {
    let env = RpcTestEnv::new().await;
    for _ in 0..4 {
        env.send_transaction_ok(&env.build_transfer_txn()).await;
    }

    let replayed: Vec<mb_receipt::SignedReceipt> = env
        .execution
        .ledger
        .iter_receipts(0)
        .unwrap()
        .map(|(_, _, bytes)| {
            mb_receipt::SignedReceipt::from_bytes(&bytes).unwrap()
        })
        .collect();

    assert_eq!(replayed.len(), 4);
    assert_eq!(replayed[0].receipt.prev_receipt_hash, GENESIS_PREV_HASH);
    for (position, signed) in replayed.iter().enumerate() {
        assert_eq!(signed.receipt.seq, position as u64);
        assert!(signed.verify(&env.operator).is_ok());
    }
    for pair in replayed.windows(2) {
        assert_eq!(pair[1].receipt.prev_receipt_hash, pair[0].receipt_hash());
    }
}
