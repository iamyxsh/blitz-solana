//! Drives the sealed-bid path against a node: commit blind, then reveal.
//!
//! Usage: `sealed <url> <count> [--abandon]`
//!
//! With `--abandon` the contents are never produced, which is the shape of
//! speculation: hold several positions open, claim only the profitable one.

use base64::{Engine, prelude::BASE64_STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1:8899".to_owned());
    let count: usize = args
        .iter()
        .filter(|arg| !arg.starts_with("--"))
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let abandon = args.iter().any(|arg| arg == "--abandon");

    let committer = SigningKey::from_bytes(&[0x21; 32]);
    println!(
        "committer {}",
        bs58::encode(committer.verifying_key().to_bytes()).into_string()
    );

    let blockhash = latest_blockhash(&url);
    let destination = Pubkey::new_unique();

    for index in 0..count {
        let txn =
            solana_system_transaction::transfer(&Keypair::new(), &destination, 1_000, blockhash);
        let wire = bincode::serialize(&txn).expect("transaction encodes");
        let digest = mb_receipt::tx_hash(&wire);

        // 1. Commit: the operator sees a hash and nothing else.
        let ticket = post(
            &url,
            "commitTransaction",
            json!([
                BASE64_STANDARD.encode(digest),
                bs58::encode(committer.verifying_key().to_bytes()).into_string(),
                BASE64_STANDARD.encode(committer.sign(&digest).to_bytes()),
            ]),
        );
        let Some(result) = ticket.get("result") else {
            println!("{index}: commit refused — {}", ticket["error"]);
            continue;
        };
        let seq = result["seq"].as_u64().unwrap_or_default();
        println!("{index}: committed blind at seq {seq}");

        if abandon {
            println!("{index}: abandoning — contents never produced");
            continue;
        }

        // 2. Reveal: only now does the operator learn what it ordered.
        let revealed = post(
            &url,
            "revealTransaction",
            json!([bs58::encode(&wire).into_string(), {"skipPreflight": true}]),
        );
        match revealed.get("result") {
            Some(result) => println!(
                "{index}: revealed into seq {} · {}",
                result["seq"].as_u64().unwrap_or_default(),
                result["signature"].as_str().unwrap_or("?")
            ),
            None => println!("{index}: reveal refused — {}", revealed["error"]),
        }
    }
}

fn latest_blockhash(url: &str) -> solana_hash::Hash {
    let encoded = post(url, "getLatestBlockhash", json!([]))
        .pointer("/result/value/blockhash")
        .and_then(Value::as_str)
        .expect("node should report a blockhash")
        .to_owned();
    let bytes = bs58::decode(&encoded)
        .into_vec()
        .expect("blockhash should be base58");
    solana_hash::Hash::new_from_array(bytes.try_into().unwrap())
}

fn post(url: &str, method: &str, params: Value) -> Value {
    ureq::post(url)
        .send_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .expect("request should reach the node")
        .into_json()
        .expect("node should answer with JSON")
}
