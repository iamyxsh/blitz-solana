use bytes::Bytes;
use magicblock_core::link::transactions::IngressStamper;
use mb_receipt::{LEN_HASH, LEN_TX_SIG};

use crate::stamper::ReceiptStamper;

/// Lets the scheduler handle reach the stamper without `magicblock-core`
/// depending on this crate.
#[async_trait::async_trait]
impl IngressStamper for ReceiptStamper {
    async fn stamp(
        &self,
        tx_sig: [u8; LEN_TX_SIG],
        wire_bytes: Bytes,
        recent_blockhash: [u8; LEN_HASH],
    ) -> Result<u64, String> {
        ReceiptStamper::stamp(self, tx_sig, wire_bytes, recent_blockhash)
            .await
            .map(|signed| signed.receipt.seq)
            .map_err(|error| error.to_string())
    }
}
