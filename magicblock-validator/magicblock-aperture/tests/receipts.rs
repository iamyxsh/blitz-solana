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

#[tokio::test]
async fn subscribers_receive_receipts_in_sequence_order() {
    let env = RpcTestEnv::new().await;
    let mut stream = env.receipt_stream().await;

    let mut sent = Vec::new();
    for _ in 0..3 {
        let (_, signed) =
            env.send_transaction_ok(&env.build_transfer_txn()).await;
        sent.push(signed);
    }

    for expected in sent {
        let observed = stream.next_receipt().await;
        assert_eq!(observed, expected);
        assert!(observed.verify(&env.operator).is_ok());
    }
}

/// The stream is live-only, and that is the whole reason a watchtower needs a
/// backfill path. A subscriber that joins late sees nothing before it arrived,
/// and the gap is indistinguishable from censorship unless it reads the log.
#[tokio::test]
async fn a_late_subscriber_sees_only_receipts_issued_after_it_joined() {
    let env = RpcTestEnv::new().await;

    let (_, missed) = env.send_transaction_ok(&env.build_transfer_txn()).await;
    assert_eq!(missed.receipt.seq, 0);

    let mut stream = env.receipt_stream().await;
    let (_, observed) =
        env.send_transaction_ok(&env.build_transfer_txn()).await;
    assert_eq!(observed.receipt.seq, 1);

    assert_eq!(stream.next_receipt().await, observed);

    // The missed receipt is only reachable from the persisted log.
    let (_, replayed) = env
        .execution
        .ledger
        .read_receipt(0)
        .unwrap()
        .expect("seq 0 must still be on disk");
    assert_eq!(
        mb_receipt::SignedReceipt::from_bytes(&replayed).unwrap(),
        missed
    );
}

#[tokio::test]
async fn a_dropped_subscriber_stops_receiving() {
    let env = RpcTestEnv::new().await;
    let mut stream = env.receipt_stream().await;

    env.send_transaction_ok(&env.build_transfer_txn()).await;
    stream.next_receipt().await;

    drop(stream);

    // A second subscriber proves the stream still works for everyone else.
    let mut survivor = env.receipt_stream().await;
    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;
    assert_eq!(survivor.next_receipt().await, signed);
}

#[tokio::test]
async fn the_stream_stays_quiet_when_nothing_is_sent() {
    let env = RpcTestEnv::new().await;
    let mut stream = env.receipt_stream().await;

    assert!(
        stream.is_idle().await,
        "an idle node must not emit receipt notifications"
    );
}

#[tokio::test]
async fn unsubscribing_stops_delivery() {
    let env = RpcTestEnv::new().await;
    let mut stream = env.receipt_stream().await;

    env.send_transaction_ok(&env.build_transfer_txn()).await;
    stream.next_receipt().await;

    stream.unsubscribe().await;

    env.send_transaction_ok(&env.build_transfer_txn()).await;
    assert!(
        stream.is_idle().await,
        "no receipts should arrive after unsubscribing"
    );
}

/// Decodes a `getReceipts` entry into the receipt it carries.
fn decode_entry(entry: &serde_json::Value) -> mb_receipt::SignedReceipt {
    use base64::{prelude::BASE64_STANDARD, Engine};

    let bytes = BASE64_STANDARD
        .decode(
            entry["receipt"]
                .as_str()
                .expect("receipt should be a string"),
        )
        .expect("receipt should be base64");
    mb_receipt::SignedReceipt::from_bytes(&bytes).expect("receipt decodes")
}

/// The reason this method exists: a watchtower that was not connected when a
/// receipt was issued must still be able to obtain it, or the gap in its view
/// is indistinguishable from a withheld transaction.
#[tokio::test]
async fn get_receipts_backfills_what_the_stream_missed() {
    let env = RpcTestEnv::new().await;

    let mut issued = Vec::new();
    for _ in 0..3 {
        let (_, signed) =
            env.send_transaction_ok(&env.build_transfer_txn()).await;
        issued.push(signed);
    }

    // Subscribing only now would miss all three.
    let result = env.call("getReceipts", serde_json::json!([0])).await;
    let entries = result.as_array().expect("result should be an array");

    assert_eq!(entries.len(), 3);
    for (position, entry) in entries.iter().enumerate() {
        assert_eq!(entry["seq"], position as u64);
        assert_eq!(decode_entry(entry), issued[position]);
        assert!(decode_entry(entry).verify(&env.operator).is_ok());
    }
}

#[tokio::test]
async fn get_receipts_pages_from_a_sequence_number() {
    let env = RpcTestEnv::new().await;
    for _ in 0..5 {
        env.send_transaction_ok(&env.build_transfer_txn()).await;
    }

    let page = env.call("getReceipts", serde_json::json!([2, 2])).await;
    let entries = page.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["seq"], 2);
    assert_eq!(entries[1]["seq"], 3);

    let tail = env.call("getReceipts", serde_json::json!([4])).await;
    assert_eq!(tail.as_array().unwrap().len(), 1);

    let past_the_end = env.call("getReceipts", serde_json::json!([99])).await;
    assert!(
        past_the_end.as_array().unwrap().is_empty(),
        "a sequence number beyond the log is empty, not an error"
    );
}

/// The chain must survive the round trip through JSON, or the backfill path
/// hands a watchtower something it cannot verify.
#[tokio::test]
async fn a_backfilled_page_replays_the_same_chain() {
    let env = RpcTestEnv::new().await;
    for _ in 0..4 {
        env.send_transaction_ok(&env.build_transfer_txn()).await;
    }

    let result = env.call("getReceipts", serde_json::json!([0])).await;
    let replayed: Vec<mb_receipt::SignedReceipt> = result
        .as_array()
        .unwrap()
        .iter()
        .map(decode_entry)
        .collect();

    assert_eq!(replayed[0].receipt.prev_receipt_hash, GENESIS_PREV_HASH);
    for pair in replayed.windows(2) {
        assert_eq!(pair[1].receipt.prev_receipt_hash, pair[0].receipt_hash());
    }
}

#[tokio::test]
async fn get_receipts_reports_the_outcome_alongside_each_receipt() {
    let env = RpcTestEnv::new().await;
    let (_, signed) = env.send_transaction_ok(&env.build_transfer_txn()).await;
    await_outcome(&env, signed.receipt.seq).await;

    let result = env.call("getReceipts", serde_json::json!([0])).await;
    assert_eq!(result[0]["outcome"], "accepted");
}

#[tokio::test]
async fn get_receipt_finds_a_receipt_by_transaction_signature() {
    let env = RpcTestEnv::new().await;
    let txn = env.build_transfer_txn();
    let (signature, signed) = env.send_transaction_ok(&txn).await;

    let found = env
        .call("getReceipt", serde_json::json!([signature.to_string()]))
        .await;
    assert_eq!(found["seq"], 0);
    assert_eq!(decode_entry(&found), signed);

    let missing = env
        .call(
            "getReceipt",
            serde_json::json!([
                solana_signature::Signature::default().to_string()
            ]),
        )
        .await;
    assert!(
        missing.is_null(),
        "an unknown signature is null, not an error"
    );
}

/// The rule that turns "no receipt" into evidence.
///
/// Internal producers — the account cloner, task scheduler, undelegation and
/// committor services — reach the scheduler directly rather than through the
/// RPC. If they were exempt, an operator could inject its own transaction and
/// its absence from the log would mean nothing.
#[tokio::test]
async fn a_transaction_that_bypasses_the_rpc_is_still_receipted() {
    let env = RpcTestEnv::new().await;
    let txn = env.build_transfer_txn();
    let signature = txn.signatures[0];

    env.execution
        .execute_transaction(txn)
        .await
        .expect("internal execution should succeed");

    let (seq, _, bytes) = env
        .execution
        .ledger
        .read_receipt_by_signature(signature)
        .unwrap()
        .expect("a transaction scheduled internally must still be receipted");

    assert_eq!(seq, 0);
    let signed = mb_receipt::SignedReceipt::from_bytes(&bytes).unwrap();
    assert!(signed.verify(&env.operator).is_ok());
    assert_eq!(signed.receipt.tx_sig, signature.as_ref());
}

/// The RPC path stamps at ingress and must then opt out of stamping again on
/// the way to the scheduler. A second receipt would break the one-to-one
/// mapping between sequence numbers and transactions.
#[tokio::test]
async fn the_rpc_path_issues_exactly_one_receipt_per_transaction() {
    let env = RpcTestEnv::new().await;
    for _ in 0..3 {
        env.send_transaction_ok(&env.build_transfer_txn()).await;
    }

    assert_eq!(env.execution.ledger.count_receipts().unwrap(), 3);
}

/// Client and operator transactions share one sequence and one chain, which
/// is what lets a watchtower reason about their relative order at all.
#[tokio::test]
async fn internal_and_client_transactions_share_one_chain() {
    let env = RpcTestEnv::new().await;

    let (_, from_rpc) =
        env.send_transaction_ok(&env.build_transfer_txn()).await;
    env.execution
        .execute_transaction(env.build_transfer_txn())
        .await
        .expect("internal execution should succeed");

    let log: Vec<mb_receipt::SignedReceipt> = env
        .execution
        .ledger
        .iter_receipts(0)
        .unwrap()
        .map(|(_, _, bytes)| {
            mb_receipt::SignedReceipt::from_bytes(&bytes).unwrap()
        })
        .collect();

    assert_eq!(log.len(), 2);
    assert_eq!(log[0], from_rpc);
    assert_eq!(log[1].receipt.seq, 1);
    assert_eq!(log[1].receipt.prev_receipt_hash, log[0].receipt_hash());
    assert!(log[1].verify(&env.operator).is_ok());
}
