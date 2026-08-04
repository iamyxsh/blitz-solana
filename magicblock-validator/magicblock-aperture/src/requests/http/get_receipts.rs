use super::{prelude::*, receipt_entry::ReceiptEntry};

/// Bounds one page of the receipt log. Each entry is roughly 400 bytes of
/// base64, so this caps a response at a few hundred kilobytes.
pub(crate) const MAX_RECEIPTS_LIMIT: u64 = 1_000;

impl HttpDispatcher {
    /// Handles the `getReceipts` RPC request.
    ///
    /// Returns up to `limit` receipts in ascending sequence order, starting at
    /// `from_seq`. This is the backfill path: the websocket stream is
    /// live-only, so a watchtower that reconnects closes its gap from here
    /// rather than treating missing sequence numbers as evidence.
    pub(crate) fn get_receipts(
        &self,
        request: &mut JsonRequest,
    ) -> HandlerResult {
        let (from_seq, limit) = parse_params!(request.params()?, u64, u64);
        let from_seq: u64 = some_or_err!(from_seq);
        let limit = limit.unwrap_or(MAX_RECEIPTS_LIMIT).min(MAX_RECEIPTS_LIMIT);

        let entries = self
            .ledger
            .iter_receipts(from_seq)
            .map_err(RpcError::internal)?
            .take(limit as usize)
            .map(|(seq, outcome, receipt)| {
                ReceiptEntry::new(seq, outcome, &receipt)
            })
            .collect::<Vec<_>>();

        Ok(ResponsePayload::encode_no_context(&request.id, entries))
    }

    /// Handles the `getReceipt` RPC request.
    ///
    /// Looks up the receipt issued for one transaction signature, so a client
    /// can re-fetch its own receipt without having kept the response.
    pub(crate) fn get_receipt(
        &self,
        request: &mut JsonRequest,
    ) -> HandlerResult {
        let signature = parse_params!(request.params()?, SerdeSignature);
        let signature: SerdeSignature = some_or_err!(signature);

        let entry = self
            .ledger
            .read_receipt_by_signature(signature.0)
            .map_err(RpcError::internal)?
            .map(|(seq, outcome, receipt)| {
                ReceiptEntry::new(seq, outcome, &receipt)
            });

        Ok(ResponsePayload::encode_no_context(&request.id, entry))
    }
}
