use mb_receipt::SignedReceipt;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::state::{subscriptions::SubscriptionsDb, SharedState};

/// Relays freshly sequenced receipts from the stamper to websocket
/// subscribers.
///
/// Deliberately a single task rather than a field on `EventProcessor`: the
/// node may run several event processors, and each would deliver its own copy
/// of every receipt to the same subscribers.
pub(crate) struct ReceiptFanout {
    subscriptions: SubscriptionsDb,
    receipts: broadcast::Receiver<SignedReceipt>,
}

impl ReceiptFanout {
    pub(crate) fn new(state: &SharedState) -> Self {
        Self {
            subscriptions: state.subscriptions.clone(),
            // Subscribed here rather than inside `run` so receipts issued
            // before the task is first polled are not lost.
            receipts: state.receipts.subscribe(),
        }
    }

    pub(crate) async fn run(mut self, cancel: CancellationToken) {
        info!("Receipt fanout started");
        loop {
            tokio::select! {
                result = self.receipts.recv() => match result {
                    Ok(receipt) => self.subscriptions.send_receipt(&receipt),
                    // The stream fell behind and receipts were dropped before
                    // reaching any subscriber. Invisible to clients, so it is
                    // logged loudly: a watchtower has to close the gap from
                    // the persisted log, which is the source of truth.
                    Err(broadcast::error::RecvError::Lagged(missed)) => error!(
                        missed,
                        "receipt stream lagged; subscribers must backfill \
                         from the persisted log"
                    ),
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = cancel.cancelled() => break,
            }
        }
        info!("Receipt fanout terminated");
    }
}
