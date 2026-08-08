use ed25519_dalek::SigningKey;
use mb_receipt::*;
use serde_json::Value;

const VECTOR_JSON: &str = include_str!("../vectors/receipt_v1.json");

fn vector() -> Value {
    serde_json::from_str(VECTOR_JSON).expect("vector is valid json")
}

fn h32(s: &str) -> [u8; 32] {
    hex::decode(s).unwrap().try_into().unwrap()
}

fn h64(s: &str) -> [u8; 64] {
    hex::decode(s).unwrap().try_into().unwrap()
}

fn operator_key(v: &Value) -> SigningKey {
    SigningKey::from_bytes(&h32(v["operator_seed_hex"].as_str().unwrap()))
}

fn offset(v: &Value, field: &str) -> usize {
    v["offsets"][field].as_u64().unwrap() as usize
}

fn receipt_from_case(c: &Value) -> Receipt {
    Receipt {
        log_id: h32(c["log_id_hex"].as_str().unwrap()),
        mode: Mode::from_u8(c["mode"].as_u64().unwrap() as u8).unwrap(),
        seq: c["seq"].as_u64().unwrap(),
        tx_sig: h64(c["tx_sig_hex"].as_str().unwrap()),
        tx_hash: h32(c["tx_hash_hex"].as_str().unwrap()),
        recent_blockhash: h32(c["recent_blockhash_hex"].as_str().unwrap()),
        prev_receipt_hash: h32(c["prev_receipt_hash_hex"].as_str().unwrap()),
        committer: h32(c["committer_hex"].as_str().unwrap()),
        ingress_slot: c["ingress_slot"].as_u64().unwrap(),
        t_ingress_micros: c["t_ingress_micros"].as_u64().unwrap(),
    }
}

#[test]
fn vector_describes_the_layout_this_build_produces() {
    let v = vector();
    assert_eq!(v["spec"], "MBRECEIPT_V1");
    assert_eq!(v["receipt_len"].as_u64().unwrap() as usize, RECEIPT_LEN);

    assert_eq!(offset(&v, "domain_tag"), OFF_DOMAIN_TAG);
    assert_eq!(offset(&v, "log_id"), OFF_LOG_ID);
    assert_eq!(offset(&v, "mode"), OFF_MODE);
    assert_eq!(offset(&v, "seq"), OFF_SEQ);
    assert_eq!(offset(&v, "tx_sig"), OFF_TX_SIG);
    assert_eq!(offset(&v, "tx_hash"), OFF_TX_HASH);
    assert_eq!(offset(&v, "recent_blockhash"), OFF_RECENT_BLOCKHASH);
    assert_eq!(offset(&v, "prev_receipt_hash"), OFF_PREV_RECEIPT_HASH);
    assert_eq!(offset(&v, "committer"), OFF_COMMITTER);
    assert_eq!(offset(&v, "ingress_slot"), OFF_INGRESS_SLOT);
    assert_eq!(offset(&v, "t_ingress_micros"), OFF_T_INGRESS_MICROS);
}

#[test]
fn reproduces_the_frozen_vector() {
    let v = vector();
    let key = operator_key(&v);
    assert_eq!(
        hex::encode(key.verifying_key().to_bytes()),
        v["operator_pubkey_hex"].as_str().unwrap()
    );

    for c in v["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let signed = receipt_from_case(c).sign(&key).unwrap();
        assert_eq!(
            hex::encode(signed.message()),
            c["message_hex"].as_str().unwrap(),
            "{name} message"
        );
        assert_eq!(
            hex::encode(signed.signature),
            c["signature_hex"].as_str().unwrap(),
            "{name} signature"
        );
        assert_eq!(
            hex::encode(signed.receipt_hash()),
            c["receipt_hash_hex"].as_str().unwrap(),
            "{name} receipt_hash"
        );
        signed.verify(&key.verifying_key()).unwrap();
    }
}

#[test]
fn tx_hash_covers_raw_wire_bytes_not_the_encoded_string() {
    let v = vector();
    // A retraction's tx_hash is a receipt hash, so it has no wire bytes to
    // reproduce and is checked by the test below instead.
    let cases: Vec<&Value> = v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["wire_bytes_hex"].is_string())
        .collect();
    assert_eq!(cases.len(), 2);

    for c in cases {
        let raw = hex::decode(c["wire_bytes_hex"].as_str().unwrap()).unwrap();
        let base58 = c["wire_bytes_base58"].as_str().unwrap();
        let expected = h32(c["tx_hash_hex"].as_str().unwrap());

        assert_eq!(tx_hash(&raw), expected);
        assert_ne!(tx_hash(base58.as_bytes()), expected);
        assert_ne!(tx_hash(hex::encode(&raw).as_bytes()), expected);
    }
}

/// The one place `tx_hash` means something other than "hash of a transaction".
/// A reimplementation that misses this reproduces every byte of the layout and
/// still binds retractions to nothing.
#[test]
fn the_retraction_names_the_receipt_it_withdraws() {
    let v = vector();
    let key = operator_key(&v);
    let plain = receipt_from_case(&v["cases"][0]).sign(&key).unwrap();
    let retraction = receipt_from_case(&v["cases"][2]);

    assert_eq!(retraction.mode, Mode::Retract);
    assert_eq!(retraction.tx_hash, plain.receipt_hash());
    assert_eq!(retraction.tx_sig, plain.receipt.tx_sig);
    assert_eq!(retraction.recent_blockhash, ZERO_HASH);
}

#[test]
fn chain_links_the_second_receipt_to_the_first() {
    let v = vector();
    let key = operator_key(&v);
    let first = receipt_from_case(&v["cases"][0]).sign(&key).unwrap();
    let second = receipt_from_case(&v["cases"][1]);

    assert_eq!(first.receipt.prev_receipt_hash, GENESIS_PREV_HASH);
    assert_eq!(second.prev_receipt_hash, first.receipt_hash());
}
