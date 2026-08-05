use ed25519_dalek::VerifyingKey;
use mb_receipt::SignedReceipt;
use serde_json::{Value, json};

use crate::{
    observed_block::ObservedBlock,
    parse::{self, ParseError},
};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("node returned an error: {0}")]
    Rpc(String),
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("node identity {0} is not a valid ed25519 key")]
    BadIdentity(String),
}

/// A read-only JSON-RPC connection to a node under observation.
///
/// Deliberately polling rather than subscribing. The websocket stream is a
/// latency optimisation the node's own documentation calls best-effort, while
/// `getReceipts` is backed by the persisted log — so the source of truth is
/// also the simpler client.
pub struct Client {
    url: String,
}

impl Client {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    /// The key every receipt from this node must verify against.
    pub fn operator(&self) -> Result<VerifyingKey, ClientError> {
        let identity = self
            .call("getIdentity", json!([]))?
            .get("identity")
            .and_then(Value::as_str)
            .ok_or(ParseError::Missing("identity"))?
            .to_owned();

        let bytes: [u8; 32] = bs58::decode(&identity)
            .into_vec()
            .ok()
            .and_then(|raw| raw.try_into().ok())
            .ok_or_else(|| ClientError::BadIdentity(identity.clone()))?;

        VerifyingKey::from_bytes(&bytes).map_err(|_| ClientError::BadIdentity(identity))
    }

    pub fn slot(&self) -> Result<u64, ClientError> {
        self.call("getSlot", json!([]))?
            .as_u64()
            .ok_or_else(|| ParseError::Malformed("slot").into())
    }

    /// Receipts from `from_seq` onwards, in sequence order.
    pub fn receipts(&self, from_seq: u64, limit: u64) -> Result<Vec<SignedReceipt>, ClientError> {
        let result = self.call("getReceipts", json!([from_seq, limit]))?;
        result
            .as_array()
            .ok_or(ParseError::Malformed("receipts"))?
            .iter()
            .map(|entry| parse::receipt_entry(entry).map_err(Into::into))
            .collect()
    }

    /// One block, or `None` if the node has not produced that slot.
    pub fn block(&self, slot: u64) -> Result<Option<ObservedBlock>, ClientError> {
        let config = json!({
            "encoding": "base64",
            "transactionDetails": "full",
            "rewards": false,
            "maxSupportedTransactionVersion": 0,
        });
        let result = self.call("getBlock", json!([slot, config]))?;
        if result.is_null() {
            return Ok(None);
        }
        Ok(Some(parse::block(slot, &result)?))
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let response: Value = ureq::post(&self.url)
            .send_json(request)
            .map_err(|error| ClientError::Transport(error.to_string()))?
            .into_json()
            .map_err(|error| ClientError::Transport(error.to_string()))?;

        if let Some(error) = response.get("error") {
            return Err(ClientError::Rpc(error.to_string()));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| ParseError::Missing("result").into())
    }
}
