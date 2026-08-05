use std::sync::{atomic::AtomicU64, Arc};

use magicblock_metrics::metrics::{
    TRANSACTION_PROCESSING_TIME, TRANSACTION_SKIP_PREFLIGHT,
};
use magicblock_receipts::Outcome;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_transaction_error::TransactionError;
use solana_transaction_status::UiTransactionEncoding;
use tracing::*;

use super::{prelude::*, receipted_signature::ReceiptedSignature};

impl HttpDispatcher {
    /// Handles the `sendTransaction` RPC request.
    ///
    /// Submits a new transaction to the validator's processing pipeline.
    /// The handler decodes and sanitizes the transaction, performs a robust
    /// replay-protection check, stamps an ingress-order receipt, and then
    /// forwards it directly to the execution queue.
    ///
    /// The response carries the receipt alongside the signature, so this
    /// method is deliberately not wire-compatible with stock Solana.
    #[instrument(skip_all)]
    pub(crate) async fn send_transaction(
        &self,
        request: &mut JsonRequest,
        remote_account_claims: Arc<AtomicU64>,
    ) -> HandlerResult {
        self.require_primary_rpc_method("sendTransaction")?;
        let _timer = TRANSACTION_PROCESSING_TIME.start_timer();
        let (transaction_str, config) =
            parse_params!(request.params()?, String, RpcSendTransactionConfig);

        let transaction_str: String = some_or_err!(transaction_str);
        let config = config.unwrap_or_default();
        let encoding = config.encoding.unwrap_or(UiTransactionEncoding::Base58);

        let transaction = self
            .prepare_transaction(&transaction_str, encoding, true, false)
            .inspect_err(
                |err| debug!(error = ?err, "Failed to prepare transaction"),
            )?;
        let signature = *transaction.txn.signature();

        // Perform a replay check and reserve the signature in the cache
        if self.transactions.contains(&signature)
            || !self.transactions.push(signature, None)
        {
            return Err(TransactionError::AlreadyProcessed.into());
        }

        // Stamped here, before account resolution: everything past this point
        // is latency the operator controls, and an ordering claim made after
        // an operator-controlled delay proves nothing about arrival order.
        let receipt = self
            .receipts
            .stamp(
                signature.into(),
                transaction.encoded.clone(),
                transaction.txn.message().recent_blockhash().to_bytes(),
            )
            .await
            .inspect_err(|err| error!(error = ?err, "Failed to stamp receipt"))
            .map_err(RpcError::internal)?;

        let seq = receipt.receipt.seq;

        let fetch_context =
            Self::send_transaction_context(signature, remote_account_claims);
        if let Err(err) = self
            .ensure_transaction_accounts(&transaction.txn, fetch_context)
            .await
        {
            self.receipts.record_outcome(seq, Outcome::Rejected).await;
            return Err(err);
        }

        // Based on the preflight flag, either execute and await the result,
        // or schedule (fire-and-forget) for background processing.
        //
        // The attack rig sits between the receipt and the scheduler, the only
        // place a reorder can be staged without disturbing the log itself.
        let mut scheduled = Ok(());
        for transaction in self.attack.intercept(transaction) {
            scheduled = if config.skip_preflight {
                TRANSACTION_SKIP_PREFLIGHT.inc();
                self.transactions_scheduler
                    .schedule_receipted(transaction)
                    .await
            } else {
                self.transactions_scheduler
                    .execute_receipted(transaction)
                    .await
            };
        }

        // A transaction the scheduler took holds a position in a block even
        // if execution then reverted. Only failing to reach the scheduler
        // means no position was ever assigned, so only that is a rejection.
        let outcome = match &scheduled {
            Err(TransactionError::ClusterMaintenance) => Outcome::Rejected,
            _ => Outcome::Accepted,
        };
        self.receipts.record_outcome(seq, outcome).await;
        scheduled?;

        let result = ReceiptedSignature::new(signature, &receipt);
        Ok(ResponsePayload::encode_no_context(&request.id, result))
    }
}
