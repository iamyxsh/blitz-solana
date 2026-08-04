pub mod error;
pub mod hashing;
pub mod receipt;
pub mod signed_receipt;

#[cfg(test)]
mod fixtures;

pub use error::ReceiptError;
pub use hashing::tx_hash;
pub use mb_constants::mode::{MODE_COMMIT, MODE_PLAIN, MODE_REVEAL, Mode};
pub use mb_constants::outcome::{OUTCOME_ACCEPTED, OUTCOME_PENDING, OUTCOME_REJECTED, Outcome};
pub use mb_constants::receipt::*;
pub use receipt::Receipt;
pub use signed_receipt::SignedReceipt;
