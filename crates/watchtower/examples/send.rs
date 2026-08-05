//! Generates traffic against a node so the watchtower has something to read.
//!
//! The transfers fail for want of funds — the fee payers do not exist. That is
//! irrelevant here: a transaction that reaches the scheduler is receipted at
//! ingress, folded into the block hash at dispatch, and recorded with an
//! index, which is everything the watchtower reads.

use serde_json::{Value, json};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;

fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8899".to_owned());
    let count: usize = std::env::args()
        .nth(2)
        .and_then(|n| n.parse().ok())
        .unwrap_or(6);

    let blockhash = call(&url, "getLatestBlockhash", json!([]))
        .pointer("/value/blockhash")
        .and_then(Value::as_str)
        .expect("node should report a blockhash")
        .to_owned();
    let blockhash = bs58::decode(&blockhash)
        .into_vec()
        .expect("blockhash should be base58");
    let blockhash = solana_hash::Hash::new_from_array(blockhash.try_into().unwrap());

    // One shared destination so consecutive transfers conflict on an account,
    // which is what makes their relative order meaningful.
    let destination = Pubkey::new_unique();

    // The client keeps every receipt it is handed. Equivocation is only
    // visible when someone can hold the node's promise up against the node's
    // published log, and this file is that half of the evidence.
    let mut held: Vec<String> = Vec::new();

    for index in 0..count {
        let payer = Keypair::new();
        let txn = solana_system_transaction::transfer(&payer, &destination, 1_000, blockhash);
        let encoded = bs58::encode(bincode::serialize(&txn).expect("transaction should encode"))
            .into_string();

        let response = post(
            &url,
            "sendTransaction",
            json!([encoded, {"skipPreflight": true}]),
        );
        if let Some(receipt) = response.pointer("/result/receipt").and_then(Value::as_str) {
            held.push(receipt.to_owned());
        }
        match response.get("result") {
            Some(result) => println!(
                "{index}: seq {} · {}",
                seq_of(result),
                result["signature"].as_str().unwrap_or("?")
            ),
            None => println!("{index}: {}", response["error"]),
        }
    }

    let path = "client-receipts.json";
    std::fs::write(path, serde_json::to_string_pretty(&held).unwrap())
        .expect("client receipts should be writable");
    println!("kept {} receipts in {path}", held.len());
}

/// Reads the sequence number back out of the receipt the node returned.
fn seq_of(result: &Value) -> String {
    use base64::{Engine, prelude::BASE64_STANDARD};

    let Some(encoded) = result.get("receipt").and_then(Value::as_str) else {
        return "none".to_owned();
    };
    let Ok(bytes) = BASE64_STANDARD.decode(encoded) else {
        return "unreadable".to_owned();
    };
    match mb_receipt::SignedReceipt::from_bytes(&bytes) {
        Ok(signed) => signed.receipt.seq.to_string(),
        Err(_) => "unparseable".to_owned(),
    }
}

fn call(url: &str, method: &str, params: Value) -> Value {
    post(url, method, params)
        .get("result")
        .cloned()
        .unwrap_or(Value::Null)
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
