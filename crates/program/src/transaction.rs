use crate::error::SlashError;

/// The account that pays for a transaction, read straight from its wire bytes.
///
/// Only ever called on bytes already bound to a receipt by hash, so this does
/// not authenticate anything — it answers "who sent the transaction the log
/// lied to" from bytes that are already known to be that transaction.
pub fn fee_payer(wire: &[u8]) -> Result<[u8; 32], SlashError> {
    let (signatures, after_count) = compact_u16(wire, 0)?;
    let message_at = after_count + signatures * 64;

    let header_at = match wire.get(message_at) {
        // A versioned message starts with the version byte, high bit set.
        Some(byte) if byte & 0x80 != 0 => message_at + 1,
        Some(_) => message_at,
        None => return Err(SlashError::MalformedReceipt),
    };

    // Three header bytes, then the account key count, then the keys.
    let (_keys, after_keys) = compact_u16(wire, header_at + 3)?;
    wire.get(after_keys..after_keys + 32)
        .ok_or(SlashError::MalformedReceipt)?
        .try_into()
        .map_err(|_| SlashError::MalformedReceipt)
}

/// Solana's compact-u16: seven bits per byte, low group first, at most three
/// bytes. Returns the value and the offset just past it.
fn compact_u16(wire: &[u8], at: usize) -> Result<(usize, usize), SlashError> {
    let mut value = 0usize;
    for step in 0..3 {
        let byte = *wire.get(at + step).ok_or(SlashError::MalformedReceipt)?;
        value |= ((byte & 0x7f) as usize) << (step * 7);
        if byte & 0x80 == 0 {
            return Ok((value, at + step + 1));
        }
    }
    Err(SlashError::MalformedReceipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYER: [u8; 32] = [0x22; 32];

    /// A legacy transfer, byte-exact: one signature, three account keys.
    fn legacy() -> Vec<u8> {
        let mut wire = vec![0x01];
        wire.extend_from_slice(&[0x11; 64]);
        wire.extend_from_slice(&[0x01, 0x00, 0x01]); // header
        wire.push(0x03); // account key count
        wire.extend_from_slice(&PAYER);
        wire.extend_from_slice(&[0x33; 32]);
        wire.extend_from_slice(&[0x00; 32]);
        wire.extend_from_slice(&[0x44; 32]); // blockhash
        wire
    }

    #[test]
    fn the_first_account_key_of_a_legacy_transaction_pays() {
        assert_eq!(fee_payer(&legacy()).unwrap(), PAYER);
    }

    /// A v0 message carries a version byte before the header. Reading it as a
    /// header byte would shift every field and hand the payout to whoever the
    /// misaligned bytes happened to name.
    #[test]
    fn a_versioned_message_skips_its_version_byte() {
        let mut wire = vec![0x01];
        wire.extend_from_slice(&[0x11; 64]);
        wire.push(0x80); // v0
        wire.extend_from_slice(&[0x01, 0x00, 0x01]);
        wire.push(0x03);
        wire.extend_from_slice(&PAYER);
        wire.extend_from_slice(&[0x33; 32]);

        assert_eq!(fee_payer(&wire).unwrap(), PAYER);
    }

    #[test]
    fn several_signatures_are_stepped_over() {
        let mut wire = vec![0x03];
        wire.extend_from_slice(&[0x11; 192]);
        wire.extend_from_slice(&[0x03, 0x00, 0x01]);
        wire.push(0x04);
        wire.extend_from_slice(&PAYER);

        assert_eq!(fee_payer(&wire).unwrap(), PAYER);
    }

    #[test]
    fn a_multi_byte_key_count_is_decoded() {
        let mut wire = vec![0x01];
        wire.extend_from_slice(&[0x11; 64]);
        wire.extend_from_slice(&[0x01, 0x00, 0x01]);
        wire.extend_from_slice(&[0x80, 0x01]); // 128 keys
        wire.extend_from_slice(&PAYER);

        assert_eq!(fee_payer(&wire).unwrap(), PAYER);
    }

    /// The fee payer ends at byte 101 of this fixture; anything shorter must
    /// be refused rather than reading a key out of whatever follows.
    #[test]
    fn truncated_bytes_are_refused_rather_than_read_short() {
        let full = legacy();
        assert!(fee_payer(&full[..101]).is_ok());
        for length in 0..101 {
            assert!(
                fee_payer(&full[..length]).is_err(),
                "accepted {length} bytes"
            );
        }
    }

    #[test]
    fn a_compact_u16_that_never_terminates_is_refused() {
        assert_eq!(
            compact_u16(&[0x80, 0x80, 0x80, 0x00], 0),
            Err(SlashError::MalformedReceipt)
        );
    }
}
