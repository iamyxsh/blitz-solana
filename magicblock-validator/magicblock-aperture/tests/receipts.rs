use mb_receipt::{tx_hash, Mode, GENESIS_PREV_HASH, ZERO_PUBKEY};
use setup::RpcTestEnv;

mod setup;

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
