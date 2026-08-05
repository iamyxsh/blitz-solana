use json::Serialize;
use magicblock_receipts::Outcome;
use mb_receipt::tx_hash;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_transaction_error::TransactionError;
use solana_transaction_status::UiTransactionEncoding;
use tracing::*;

use super::prelude::*;

/// The answer to a reveal: which promised position these contents took.
#[derive(Serialize)]
pub(crate) struct RevealedSignature {
    signature: SerdeSignature,
    seq: u64,
}

impl HttpDispatcher {
    /// Handles the `revealTransaction` RPC request.
    ///
    /// Produces the transaction a position was promised to. The operator
    /// matches it against the commitment by hash, then executes it — having
    /// already fixed where it would go, before it could see what it was.
    ///
    /// No new receipt is issued. The commit ticket already binds these
    /// contents through `tx_hash`, and a second statement about one position
    /// is exactly the shape of equivocation.
    #[instrument(skip_all)]
    pub(crate) async fn reveal_transaction(
        &self,
        request: &mut JsonRequest,
    ) -> HandlerResult {
        self.require_primary_rpc_method("revealTransaction")?;
        let (transaction_str, config) =
            parse_params!(request.params()?, String, RpcSendTransactionConfig);

        let transaction_str: String = some_or_err!(transaction_str);
        let config = config.unwrap_or_default();
        let encoding = config.encoding.unwrap_or(UiTransactionEncoding::Base58);

        let transaction =
            self.prepare_transaction(&transaction_str, encoding, true, false)?;
        let signature = *transaction.txn.signature();

        if self.transactions.contains(&signature)
            || !self.transactions.push(signature, None)
        {
            return Err(TransactionError::AlreadyProcessed.into());
        }

        // Only contents that hash to something already promised may claim a
        // position. Anything else is a transaction trying to take a place
        // nobody reserved for it.
        let digest = tx_hash(&transaction.encoded);
        let Some((seq, _commitment)) =
            self.receipts.reveal(digest, signature.into()).await
        else {
            return Err(RpcError::invalid_request(
                "no outstanding commitment matches these contents",
            ));
        };

        let fetch_context = Self::send_transaction_context(
            signature,
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        if let Err(err) = self
            .ensure_transaction_accounts(&transaction.txn, fetch_context)
            .await
        {
            self.receipts.record_outcome(seq, Outcome::Rejected).await;
            return Err(err);
        }

        // Already ticketed at commit time, so it must not be stamped again.
        let scheduled = if config.skip_preflight {
            self.transactions_scheduler
                .schedule_receipted(transaction)
                .await
        } else {
            self.transactions_scheduler
                .execute_receipted(transaction)
                .await
        };

        let outcome = match &scheduled {
            Err(TransactionError::ClusterMaintenance) => Outcome::Rejected,
            _ => Outcome::Accepted,
        };
        self.receipts.record_outcome(seq, outcome).await;
        scheduled?;

        let result = RevealedSignature {
            signature: SerdeSignature(signature),
            seq,
        };
        Ok(ResponsePayload::encode_no_context(&request.id, result))
    }
}
