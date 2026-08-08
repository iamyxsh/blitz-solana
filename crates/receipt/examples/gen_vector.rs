use ed25519_dalek::SigningKey;
use mb_receipt::*;
use serde_json::json;

const OPERATOR_SEED: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];

/// Legacy Solana transfer, byte-exact. The signature is a fixed placeholder, not a real
/// ed25519 signature: nothing in this crate verifies it, and using a real one would make
/// the fixture depend on solana-sdk.
fn fixture_tx(sig_byte: u8, from_byte: u8, blockhash_byte: u8, lamports: u64) -> Vec<u8> {
    let mut tx = Vec::new();
    tx.push(0x01); // signature count
    tx.extend_from_slice(&[sig_byte; 64]);
    tx.extend_from_slice(&[0x01, 0x00, 0x01]); // header: 1 signer, 0 ro-signed, 1 ro-unsigned
    tx.push(0x03); // account key count
    tx.extend_from_slice(&[from_byte; 32]);
    tx.extend_from_slice(&[0x33; 32]); // destination
    tx.extend_from_slice(&[0x00; 32]); // system program
    tx.extend_from_slice(&[blockhash_byte; 32]);
    tx.push(0x01); // instruction count
    tx.push(0x02); // program id index -> system program
    tx.push(0x02); // account index count
    tx.extend_from_slice(&[0x00, 0x01]);
    tx.push(0x0c); // data length
    tx.extend_from_slice(&2u32.to_le_bytes()); // SystemInstruction::Transfer
    tx.extend_from_slice(&lamports.to_le_bytes());
    tx
}

fn main() {
    let key = SigningKey::from_bytes(&OPERATOR_SEED);
    let pubkey = key.verifying_key();

    let tx_a = fixture_tx(0x11, 0x22, 0x44, 1_000);
    let tx_b = fixture_tx(0x77, 0x66, 0x88, 2_000);

    let plain = Receipt {
        mode: Mode::Plain,
        seq: 0,
        tx_sig: [0x11; 64],
        tx_hash: tx_hash(&tx_a),
        recent_blockhash: [0x44; 32],
        prev_receipt_hash: GENESIS_PREV_HASH,
        committer: ZERO_PUBKEY,
        ingress_slot: 6_150_402,
        t_ingress_micros: 1_754_300_123_456_789,
    }
    .sign(&key)
    .expect("plain receipt must be valid");

    let commit = Receipt {
        mode: Mode::Commit,
        seq: 1,
        tx_sig: ZERO_SIG,
        tx_hash: tx_hash(&tx_b),
        recent_blockhash: [0x88; 32],
        prev_receipt_hash: plain.receipt_hash(),
        committer: [0x55; 32],
        ingress_slot: 6_150_403,
        t_ingress_micros: 1_754_300_123_506_789,
    }
    .sign(&key)
    .expect("commit receipt must be valid");

    // Withdraws the plain receipt above: `tx_sig` names the transaction and
    // `tx_hash` carries that receipt's hash, which appears in this same file
    // as case 0's `receipt_hash_hex`.
    let retract = Receipt {
        mode: Mode::Retract,
        seq: 2,
        tx_sig: [0x11; 64],
        tx_hash: plain.receipt_hash(),
        recent_blockhash: ZERO_HASH,
        prev_receipt_hash: commit.receipt_hash(),
        committer: ZERO_PUBKEY,
        ingress_slot: 6_150_404,
        t_ingress_micros: 1_754_300_123_556_789,
    }
    .sign(&key)
    .expect("retract receipt must be valid");

    let case = |name: &str, tx: Option<&[u8]>, s: &SignedReceipt| {
        let mut case = json!({
            "name": name,
            "tx_hash_hex": hex::encode(s.receipt.tx_hash),
            "mode": s.receipt.mode as u8,
            "seq": s.receipt.seq,
            "tx_sig_hex": hex::encode(s.receipt.tx_sig),
            "recent_blockhash_hex": hex::encode(s.receipt.recent_blockhash),
            "prev_receipt_hash_hex": hex::encode(s.receipt.prev_receipt_hash),
            "committer_hex": hex::encode(s.receipt.committer),
            "ingress_slot": s.receipt.ingress_slot,
            "t_ingress_micros": s.receipt.t_ingress_micros,
            "message_hex": hex::encode(s.message()),
            "signature_hex": hex::encode(s.signature),
            "receipt_hash_hex": hex::encode(s.receipt_hash()),
        });
        // A retraction binds a receipt hash rather than transaction bytes, so
        // it carries no wire bytes to reproduce.
        if let Some(tx) = tx {
            case["wire_bytes_hex"] = json!(hex::encode(tx));
            case["wire_bytes_base58"] = json!(bs58::encode(tx).into_string());
        }
        case
    };

    let vector = json!({
        "spec": "MBRECEIPT_V1",
        "receipt_len": RECEIPT_LEN,
        "operator_seed_hex": hex::encode(OPERATOR_SEED),
        "operator_pubkey_hex": hex::encode(pubkey.to_bytes()),
        "offsets": {
            "domain_tag": OFF_DOMAIN_TAG,
            "mode": OFF_MODE,
            "seq": OFF_SEQ,
            "tx_sig": OFF_TX_SIG,
            "tx_hash": OFF_TX_HASH,
            "recent_blockhash": OFF_RECENT_BLOCKHASH,
            "prev_receipt_hash": OFF_PREV_RECEIPT_HASH,
            "committer": OFF_COMMITTER,
            "ingress_slot": OFF_INGRESS_SLOT,
            "t_ingress_micros": OFF_T_INGRESS_MICROS,
        },
        "cases": [
            case("plain_genesis", Some(&tx_a), &plain),
            case("commit_chained", Some(&tx_b), &commit),
            case("retract_of_plain_genesis", None, &retract),
        ],
    });

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/vectors/receipt_v1.json");
    std::fs::write(path, format!("{:#}\n", vector)).expect("write vector");
    println!("wrote {path}");
}
