use base64::{Engine, prelude::BASE64_STANDARD};
use mb_receipt::SignedReceipt;
use solana_message::VersionedMessage;
use solana_transaction::versioned::VersionedTransaction;

use crate::{observed_block::ObservedBlock, observed_transaction::ObservedTransaction};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("missing field: {0}")]
    Missing(&'static str),
    #[error("field {0} is not the expected shape")]
    Malformed(&'static str),
    #[error("base58 value did not decode: {0}")]
    BadBase58(&'static str),
    #[error("base64 value did not decode: {0}")]
    BadBase64(&'static str),
    #[error("transaction bytes did not deserialize")]
    BadTransaction,
    #[error("receipt bytes did not parse")]
    BadReceipt,
}

/// Turns one `getReceipts` entry into a receipt.
pub fn receipt_entry(value: &serde_json::Value) -> Result<SignedReceipt, ParseError> {
    let encoded = value
        .get("receipt")
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::Missing("receipt"))?;
    let bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| ParseError::BadBase64("receipt"))?;
    SignedReceipt::from_bytes(&bytes).map_err(|_| ParseError::BadReceipt)
}

/// Turns a `getBlock` result into an observed block.
///
/// Requires `encoding: "base64"`: the node's own bytes are deserialized here
/// rather than trusting its JSON view of account keys, because writability
/// follows from the message header and getting that wrong would silently
/// break the conflict test in the direction that accuses honest validators.
pub fn block(slot: u64, value: &serde_json::Value) -> Result<ObservedBlock, ParseError> {
    let previous_blockhash = hash_field(value, "previousBlockhash")?;
    let blockhash = hash_field(value, "blockhash")?;

    let entries = value
        .get("transactions")
        .and_then(serde_json::Value::as_array)
        .ok_or(ParseError::Missing("transactions"))?;

    let transactions = entries
        .iter()
        .map(transaction)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ObservedBlock {
        slot,
        previous_blockhash,
        blockhash,
        transactions,
    })
}

fn transaction(value: &serde_json::Value) -> Result<ObservedTransaction, ParseError> {
    // `[base64, "base64"]` is how the RPC encodes a transaction body.
    let encoded = value
        .pointer("/transaction/0")
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::Missing("transaction"))?;
    let wire_bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| ParseError::BadBase64("transaction"))?;

    observe(wire_bytes)
}

/// Derives everything the detectors need from a transaction's own bytes.
pub fn observe(wire_bytes: Vec<u8>) -> Result<ObservedTransaction, ParseError> {
    let txn: VersionedTransaction =
        bincode::deserialize(&wire_bytes).map_err(|_| ParseError::BadTransaction)?;

    let signature = txn
        .signatures
        .first()
        .ok_or(ParseError::Malformed("signatures"))?
        .as_ref()
        .try_into()
        .map_err(|_| ParseError::Malformed("signatures"))?;

    let keys = txn.message.static_account_keys();
    let fee_payer = keys
        .first()
        .ok_or(ParseError::Malformed("accountKeys"))?
        .to_bytes();

    let (mut writable, mut readonly) = (Vec::new(), Vec::new());
    for (index, key) in keys.iter().enumerate() {
        if is_writable(&txn.message, index) {
            writable.push(key.to_bytes());
        } else {
            readonly.push(key.to_bytes());
        }
    }

    Ok(ObservedTransaction {
        signature,
        tx_hash: mb_receipt::tx_hash(&wire_bytes),
        fee_payer,
        writable,
        readonly,
        wire_bytes,
    })
}

/// Writability as the runtime sees it: header roles, minus reserved accounts,
/// minus program ids demoted to read-only.
///
/// Delegated to `solana-message` rather than re-derived, because getting it
/// wrong understates the conflict set — and understating conflicts is the
/// direction that accuses honest validators.
///
/// Address lookup tables are rejected at ingress by this validator, so every
/// account a transaction touches is present in the static keys.
fn is_writable(message: &VersionedMessage, index: usize) -> bool {
    message.is_maybe_writable(index, None)
}

fn hash_field(value: &serde_json::Value, field: &'static str) -> Result<[u8; 32], ParseError> {
    let encoded = value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::Missing(field))?;
    let bytes = bs58::decode(encoded)
        .into_vec()
        .map_err(|_| ParseError::BadBase58(field))?;
    bytes.try_into().map_err(|_| ParseError::Malformed(field))
}
