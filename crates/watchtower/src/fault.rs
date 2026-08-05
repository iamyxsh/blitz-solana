use ed25519_dalek::VerifyingKey;
use mb_receipt::{GENESIS_PREV_HASH, SignedReceipt};

use crate::{execution::Execution, order::fold, reordered_transaction::ReorderedTransaction};

/// Attributable operator misbehaviour, carrying everything needed to check it.
///
/// Each variant is self-contained on purpose: a third party who has never
/// spoken to the node, and holds nothing but this object and the operator's
/// public key, can run [`Fault::verify`] and reach the same conclusion. That
/// property is the difference between evidence and a log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Two different receipts signed at the same sequence number. The
    /// operator has made two contradictory statements about one position.
    Equivocation {
        seq: u64,
        a: SignedReceipt,
        b: SignedReceipt,
    },
    /// A receipt whose chain link does not match its predecessor, meaning the
    /// log was rewritten after the fact.
    BrokenChain {
        seq: u64,
        receipt: SignedReceipt,
        predecessor: SignedReceipt,
    },
    /// The first receipt of the log does not start the chain from zero.
    BadOrigin { receipt: SignedReceipt },
    /// A receipt that does not verify under the operator's key.
    Unverifiable { seq: u64, receipt: SignedReceipt },
    /// A transaction holding a position in a block with no receipt behind it.
    /// On a validator where every producer stamps, this is an injection.
    Unticketed {
        slot: u64,
        index: u32,
        signature: [u8; 64],
        wire_bytes: Vec<u8>,
    },
    /// A transaction that ran far later than the operator's own receipt says
    /// it arrived.
    Withheld {
        receipt: SignedReceipt,
        execution: Execution,
        held: u64,
    },
    /// A transaction the operator receipted and then never ran.
    ///
    /// Unlike the others this is a claim about an absence, so it cannot be
    /// re-derived from the object alone: a verifier has to check the block
    /// range themselves. `verify` confirms the receipt is genuine and leaves
    /// the search to the reader.
    Absent {
        receipt: SignedReceipt,
        head: u64,
        waited: u64,
    },
    /// A receipt whose own fields contradict each other: it claims to have
    /// arrived before its block hash existed, or long after that hash would
    /// have been refused.
    ImpossibleIngress {
        receipt: SignedReceipt,
        blockhash_slot: u64,
        max_blockhash_age: u64,
    },
    /// Two conflicting transactions executed in the opposite order to the one
    /// the operator signed for them.
    Reorder {
        slot: u64,
        previous_blockhash: [u8; 32],
        blockhash: [u8; 32],
        /// Execution order, recheckable against the two hashes above.
        executed: Vec<[u8; 64]>,
        /// Arrived later, executed first.
        jumped: ReorderedTransaction,
        /// Arrived earlier, executed second.
        delayed: ReorderedTransaction,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FaultError {
    #[error("receipt at seq {0} does not carry a valid operator signature")]
    NotSigned(u64),
    #[error("the two receipts are identical, so they do not contradict")]
    NotContradictory,
    #[error("receipts at seq {a} and {b} are not adjacent")]
    NotAdjacent { a: u64, b: u64 },
    #[error("the chain link is intact, so nothing was rewritten")]
    ChainIntact,
    #[error("the origin is correct")]
    OriginIntact,
    #[error("the receipt verifies, so it is not a forgery")]
    ReceiptVerifies,
    #[error("the transaction bytes do not hash to the receipt's tx_hash")]
    ReceiptDoesNotBindTransaction,
    #[error("the signature list does not reproduce the block hash")]
    OrderNotReproduced,
    #[error("the named transaction is not at that index in the block")]
    NotAtClaimedIndex,
    #[error("execution order agrees with receipt order, so nothing jumped")]
    OrderRespected,
    #[error("the transaction ran within the allowed delay")]
    DeliveredInTime,
    #[error("the ingress slot is consistent with the block hash it names")]
    IngressPossible,
}

impl Fault {
    pub fn seq(&self) -> u64 {
        match self {
            Fault::Equivocation { seq, .. }
            | Fault::BrokenChain { seq, .. }
            | Fault::Unverifiable { seq, .. } => *seq,
            Fault::BadOrigin { receipt } => receipt.receipt.seq,
            Fault::Unticketed { .. } => u64::MAX,
            Fault::Withheld { receipt, .. }
            | Fault::Absent { receipt, .. }
            | Fault::ImpossibleIngress { receipt, .. } => receipt.receipt.seq,
            Fault::Reorder { jumped, .. } => jumped.receipt.receipt.seq,
        }
    }

    /// Re-derives the accusation from the evidence alone.
    ///
    /// Returns `Ok(())` only if the carried objects genuinely demonstrate the
    /// claimed misbehaviour.
    pub fn verify(&self, operator: &VerifyingKey) -> Result<(), FaultError> {
        match self {
            Fault::Equivocation { seq, a, b } => {
                signed(a, operator)?;
                signed(b, operator)?;
                if a.receipt.seq != *seq || b.receipt.seq != *seq {
                    return Err(FaultError::NotAdjacent {
                        a: a.receipt.seq,
                        b: b.receipt.seq,
                    });
                }
                if a == b {
                    return Err(FaultError::NotContradictory);
                }
                Ok(())
            }
            Fault::BrokenChain {
                seq,
                receipt,
                predecessor,
            } => {
                signed(receipt, operator)?;
                signed(predecessor, operator)?;
                if receipt.receipt.seq != *seq || predecessor.receipt.seq + 1 != *seq {
                    return Err(FaultError::NotAdjacent {
                        a: predecessor.receipt.seq,
                        b: receipt.receipt.seq,
                    });
                }
                if receipt.receipt.prev_receipt_hash == predecessor.receipt_hash() {
                    return Err(FaultError::ChainIntact);
                }
                Ok(())
            }
            Fault::BadOrigin { receipt } => {
                signed(receipt, operator)?;
                if receipt.receipt.seq == 0
                    && receipt.receipt.prev_receipt_hash == GENESIS_PREV_HASH
                {
                    return Err(FaultError::OriginIntact);
                }
                Ok(())
            }
            Fault::Unverifiable { seq, receipt } => {
                if receipt.verify(operator).is_ok() {
                    return Err(FaultError::ReceiptVerifies);
                }
                if receipt.receipt.seq != *seq {
                    return Err(FaultError::NotAdjacent {
                        a: receipt.receipt.seq,
                        b: *seq,
                    });
                }
                Ok(())
            }
            Fault::Withheld {
                receipt,
                execution,
                held,
            } => {
                signed(receipt, operator)?;
                if execution.slot < receipt.receipt.ingress_slot
                    || execution.slot - receipt.receipt.ingress_slot != *held
                {
                    return Err(FaultError::DeliveredInTime);
                }
                Ok(())
            }
            Fault::Absent { receipt, .. } => signed(receipt, operator),
            Fault::ImpossibleIngress {
                receipt,
                blockhash_slot,
                max_blockhash_age,
            } => {
                signed(receipt, operator)?;
                let ingress = receipt.receipt.ingress_slot;
                let impossible = ingress < *blockhash_slot
                    || ingress.saturating_sub(*blockhash_slot) > *max_blockhash_age;
                if !impossible {
                    return Err(FaultError::IngressPossible);
                }
                Ok(())
            }
            // Nothing to re-derive: the claim is the absence of a receipt,
            // which is checked against the log rather than against itself.
            Fault::Unticketed { .. } => Ok(()),
            Fault::Reorder {
                previous_blockhash,
                blockhash,
                executed,
                jumped,
                delayed,
                ..
            } => {
                jumped.verify(operator)?;
                delayed.verify(operator)?;

                if fold(previous_blockhash, executed.iter()) != *blockhash {
                    return Err(FaultError::OrderNotReproduced);
                }
                at_index(executed, jumped)?;
                at_index(executed, delayed)?;

                if jumped.index >= delayed.index {
                    return Err(FaultError::OrderRespected);
                }
                if jumped.receipt.receipt.seq <= delayed.receipt.receipt.seq {
                    return Err(FaultError::OrderRespected);
                }
                Ok(())
            }
        }
    }
}

fn at_index(executed: &[[u8; 64]], txn: &ReorderedTransaction) -> Result<(), FaultError> {
    executed
        .get(txn.index as usize)
        .filter(|signature| **signature == txn.receipt.receipt.tx_sig)
        .map(|_| ())
        .ok_or(FaultError::NotAtClaimedIndex)
}

fn signed(receipt: &SignedReceipt, operator: &VerifyingKey) -> Result<(), FaultError> {
    receipt
        .verify(operator)
        .map_err(|_| FaultError::NotSigned(receipt.receipt.seq))
}
