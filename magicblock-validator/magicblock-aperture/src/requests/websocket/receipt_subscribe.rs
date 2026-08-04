use super::prelude::*;

impl WsDispatcher {
    /// Handles the `receiptSubscribe` WebSocket RPC request.
    ///
    /// Registers the connection to receive every ingress receipt this node
    /// issues, from now on. The stream is live-only: a subscriber that joins
    /// late, or reconnects, must backfill the sequence numbers it missed from
    /// the persisted log rather than assume the gap means anything.
    pub(crate) fn receipt_subscribe(&mut self) -> RpcResult<SubResult> {
        let handle =
            self.subscriptions.subscribe_to_receipts(self.chan.clone());
        let result = SubResult::SubId(handle.id);
        self.register_unsub(handle);

        Ok(result)
    }
}
