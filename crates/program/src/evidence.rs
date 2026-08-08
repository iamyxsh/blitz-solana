use mb_constants::receipt::{
    OFF_LOG_ID, OFF_RECENT_BLOCKHASH, OFF_SEQ, OFF_TX_HASH, OFF_TX_SIG, RECEIPT_LEN,
};

use crate::error::SlashError;

/// Two contradictory statements the operator signed about one position.
///
/// The receipts arrive as raw bytes because that is what the ed25519 program
/// verified — parsing them into a struct first and adjudicating the struct
/// would leave a gap where a field could be read differently from the way it
/// was signed. Everything here is read at the offsets the spec freezes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equivocation<'a> {
    pub seq: u64,
    pub log_id: &'a [u8; 32],
    pub a: &'a [u8],
    pub b: &'a [u8],
}

impl<'a> Equivocation<'a> {
    pub fn check(a: &'a [u8], b: &'a [u8]) -> Result<Self, SlashError> {
        let field = |receipt: &'a [u8], at: usize, len: usize| -> Result<&'a [u8], SlashError> {
            receipt
                .get(at..at + len)
                .ok_or(SlashError::MalformedReceipt)
        };

        for receipt in [a, b] {
            if receipt.len() != RECEIPT_LEN {
                return Err(SlashError::MalformedReceipt);
            }
            if &receipt[..OFF_LOG_ID] != mb_constants::receipt::DOMAIN_TAG {
                return Err(SlashError::MalformedReceipt);
            }
        }

        let log_id: &[u8; 32] = field(a, OFF_LOG_ID, 32)?
            .try_into()
            .map_err(|_| SlashError::MalformedReceipt)?;
        if field(b, OFF_LOG_ID, 32)? != log_id.as_slice() {
            return Err(SlashError::MixedLogs);
        }

        let seq_of = |receipt: &'a [u8]| -> Result<u64, SlashError> {
            Ok(u64::from_le_bytes(
                field(receipt, OFF_SEQ, 8)?
                    .try_into()
                    .map_err(|_| SlashError::MalformedReceipt)?,
            ))
        };
        let seq = seq_of(a)?;
        if seq_of(b)? != seq {
            return Err(SlashError::DifferentSequence);
        }

        if a == b {
            return Err(SlashError::NotContradictory);
        }

        Ok(Self { seq, log_id, a, b })
    }

    /// Canonical ordering for the conviction address.
    ///
    /// Whichever way round an accuser presents the pair it must name the same
    /// account, or the same contradiction could be convicted twice and the
    /// bond paid out twice for one offence.
    pub fn ordered(&self) -> (&'a [u8], &'a [u8]) {
        if self.a <= self.b {
            (self.a, self.b)
        } else {
            (self.b, self.a)
        }
    }

    /// The transaction the log lied to, taken from the earlier-ordered
    /// receipt. Cold evidence cannot tell which of the two was betrayed — the
    /// operator promised one position to both — so the choice is made by the
    /// same canonical ordering that names the conviction, and the payout is
    /// escrowed rather than sent, because a signature is not an address.
    pub fn wronged_signature(&self) -> [u8; 64] {
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&self.ordered().0[OFF_TX_SIG..OFF_TX_SIG + 64]);
        signature
    }

    /// What that transaction's wire bytes must hash to, so the escrow can be
    /// claimed only by producing the transaction itself.
    pub fn wronged_tx_hash(&self) -> [u8; 32] {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&self.ordered().0[OFF_TX_HASH..OFF_RECENT_BLOCKHASH]);
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_receipt::{Mode, Receipt, ZERO_PUBKEY};

    const LOG_ID: [u8; 32] = [0x9c; 32];

    fn receipt(log_id: [u8; 32], seq: u64, tx_sig: u8) -> Vec<u8> {
        Receipt {
            log_id,
            mode: Mode::Plain,
            seq,
            tx_sig: [tx_sig; 64],
            tx_hash: [0xb2; 32],
            recent_blockhash: [0xc3; 32],
            prev_receipt_hash: [0xd4; 32],
            committer: ZERO_PUBKEY,
            ingress_slot: 1_000,
            t_ingress_micros: 1_700_000_000_000_000,
        }
        .to_bytes()
        .to_vec()
    }

    #[test]
    fn two_different_receipts_at_one_position_contradict() {
        let (a, b) = (receipt(LOG_ID, 7, 0xaa), receipt(LOG_ID, 7, 0xbb));

        let evidence = Equivocation::check(&a, &b).unwrap();

        assert_eq!(evidence.seq, 7);
        assert_eq!(evidence.log_id, &LOG_ID);
    }

    /// A restart is not misbehaviour. Without this the program slashes any
    /// operator whose node rebooted, on receipts it genuinely signed.
    #[test]
    fn receipts_from_two_runs_of_the_log_do_not_contradict() {
        let (a, b) = (receipt(LOG_ID, 7, 0xaa), receipt([0x01; 32], 7, 0xbb));

        assert_eq!(Equivocation::check(&a, &b), Err(SlashError::MixedLogs));
    }

    #[test]
    fn receipts_at_different_positions_do_not_contradict() {
        let (a, b) = (receipt(LOG_ID, 7, 0xaa), receipt(LOG_ID, 8, 0xbb));

        assert_eq!(
            Equivocation::check(&a, &b),
            Err(SlashError::DifferentSequence)
        );
    }

    /// A reconnect overlapping a backfill delivers the same receipt twice.
    #[test]
    fn the_same_receipt_twice_does_not_contradict() {
        let a = receipt(LOG_ID, 7, 0xaa);

        assert_eq!(
            Equivocation::check(&a, &a.clone()),
            Err(SlashError::NotContradictory)
        );
    }

    /// The conviction address is derived from this ordering, so presenting the
    /// pair backwards must not mint a second conviction for one offence.
    #[test]
    fn the_ordering_is_the_same_whichever_way_the_pair_is_presented() {
        let (a, b) = (receipt(LOG_ID, 7, 0xaa), receipt(LOG_ID, 7, 0xbb));

        let forward = Equivocation::check(&a, &b).unwrap().ordered();
        let backward = Equivocation::check(&b, &a).unwrap().ordered();

        assert_eq!(forward, backward);
    }

    #[test]
    fn anything_that_is_not_a_receipt_is_refused() {
        let a = receipt(LOG_ID, 7, 0xaa);
        let mut wrong_tag = a.clone();
        wrong_tag[0] = b'X';

        assert_eq!(
            Equivocation::check(&a, &wrong_tag),
            Err(SlashError::MalformedReceipt)
        );
        assert_eq!(
            Equivocation::check(&a, &a[..RECEIPT_LEN - 1]),
            Err(SlashError::MalformedReceipt)
        );
    }
}
