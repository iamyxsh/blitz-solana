use base64::{prelude::BASE64_STANDARD, Engine};
use json::Serialize;
use mb_receipt::SignedReceipt;
use solana_signature::Signature;

use crate::requests::params::SerdeSignature;

/// The `sendTransaction` result on a receipted validator.
///
/// The receipt travels as base64 of its 293 transport bytes rather than as a
/// per-field object: a verifier has to hash exactly what the operator signed,
/// and any structured re-encoding is a chance for the two to drift apart.
#[derive(Serialize)]
pub(crate) struct ReceiptedSignature {
    signature: SerdeSignature,
    receipt: String,
}

impl ReceiptedSignature {
    pub(crate) fn new(signature: Signature, receipt: &SignedReceipt) -> Self {
        Self {
            signature: SerdeSignature(signature),
            receipt: BASE64_STANDARD.encode(receipt.to_bytes()),
        }
    }
}
