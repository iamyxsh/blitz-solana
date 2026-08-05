use base64::{prelude::BASE64_STANDARD, Engine};
use ed25519_dalek::{Signature as CommitterSignature, Verifier, VerifyingKey};
use json::Serialize;
use mb_receipt::{SignedReceipt, LEN_HASH};

use super::prelude::*;
use crate::RpcResult;

/// The answer to a commitment: a position, assigned blind.
#[derive(Serialize)]
pub(crate) struct CommitTicket {
    seq: u64,
    ticket: String,
}

impl CommitTicket {
    fn new(receipt: &SignedReceipt) -> Self {
        Self {
            seq: receipt.receipt.seq,
            ticket: BASE64_STANDARD.encode(receipt.to_bytes()),
        }
    }
}

impl HttpDispatcher {
    /// Handles the `commitTransaction` RPC request.
    ///
    /// Takes `sha256(wire_bytes)` and returns a signed ticket naming the
    /// position that content will occupy. The operator sees no transaction,
    /// so it cannot order by contents — the position is fixed before the
    /// contents are knowable.
    ///
    /// Params: `[tx_hash_base64, committer_base58, signature_base64]`, where
    /// the signature is the committer's over the 32-byte hash. Without it the
    /// `committer` field would be unauthenticated, and an unrevealed
    /// commitment could not be attributed to anyone.
    pub(crate) async fn commit_transaction(
        &self,
        request: &mut JsonRequest,
    ) -> HandlerResult {
        self.require_primary_rpc_method("commitTransaction")?;

        let (hash, committer, signature) =
            parse_params!(request.params()?, String, String, String);
        let hash: String = some_or_err!(hash);
        let committer: String = some_or_err!(committer);
        let signature: String = some_or_err!(signature);

        let tx_hash = decode_hash(&hash)?;
        let committer = decode_pubkey(&committer)?;
        verify_commitment(&tx_hash, &committer, &signature)?;

        let ticket = self
            .receipts
            .commit(tx_hash, committer)
            .await
            .map_err(RpcError::internal)?;

        Ok(ResponsePayload::encode_no_context(
            &request.id,
            CommitTicket::new(&ticket),
        ))
    }
}

fn decode_hash(encoded: &str) -> RpcResult<[u8; LEN_HASH]> {
    BASE64_STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            RpcError::invalid_params("tx_hash must be 32 base64 bytes")
        })
}

fn decode_pubkey(encoded: &str) -> RpcResult<[u8; 32]> {
    bs58::decode(encoded)
        .into_vec()
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            RpcError::invalid_params("committer must be a base58 pubkey")
        })
}

/// The committer must prove it meant this hash, or Rule 3 cannot say whose
/// unrevealed commitment it was looking at.
fn verify_commitment(
    tx_hash: &[u8; LEN_HASH],
    committer: &[u8; 32],
    signature: &str,
) -> RpcResult<()> {
    let raw: [u8; 64] = BASE64_STANDARD
        .decode(signature)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            RpcError::invalid_params("signature must be 64 base64 bytes")
        })?;

    let key = VerifyingKey::from_bytes(committer).map_err(|_| {
        RpcError::invalid_params("committer is not a valid ed25519 key")
    })?;

    key.verify(tx_hash, &CommitterSignature::from_bytes(&raw))
        .map_err(|_| {
            RpcError::transaction_verification(
                "commitment is not signed by the committer",
            )
        })
}
