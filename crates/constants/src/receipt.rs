pub const LEN_DOMAIN_TAG: usize = 12;
pub const LEN_MODE: usize = 1;
pub const LEN_SEQ: usize = 8;
pub const LEN_TX_SIG: usize = 64;
pub const LEN_HASH: usize = 32;
pub const LEN_PUBKEY: usize = 32;
pub const LEN_SLOT: usize = 8;
pub const LEN_MICROS: usize = 8;

pub const DOMAIN_TAG: &[u8; LEN_DOMAIN_TAG] = b"MBRECEIPT_V1";

pub const OFF_DOMAIN_TAG: usize = 0;
pub const OFF_MODE: usize = OFF_DOMAIN_TAG + LEN_DOMAIN_TAG;
pub const OFF_SEQ: usize = OFF_MODE + LEN_MODE;
pub const OFF_TX_SIG: usize = OFF_SEQ + LEN_SEQ;
pub const OFF_TX_HASH: usize = OFF_TX_SIG + LEN_TX_SIG;
pub const OFF_RECENT_BLOCKHASH: usize = OFF_TX_HASH + LEN_HASH;
pub const OFF_PREV_RECEIPT_HASH: usize = OFF_RECENT_BLOCKHASH + LEN_HASH;
pub const OFF_COMMITTER: usize = OFF_PREV_RECEIPT_HASH + LEN_HASH;
pub const OFF_INGRESS_SLOT: usize = OFF_COMMITTER + LEN_PUBKEY;
pub const OFF_T_INGRESS_MICROS: usize = OFF_INGRESS_SLOT + LEN_SLOT;

pub const RECEIPT_LEN: usize = OFF_T_INGRESS_MICROS + LEN_MICROS;

pub const LEN_SIGNATURE: usize = 64;
pub const OFF_SIGNATURE: usize = RECEIPT_LEN;
pub const SIGNED_RECEIPT_LEN: usize = OFF_SIGNATURE + LEN_SIGNATURE;

pub const ZERO_SIG: [u8; LEN_TX_SIG] = [0u8; LEN_TX_SIG];
pub const ZERO_PUBKEY: [u8; LEN_PUBKEY] = [0u8; LEN_PUBKEY];
pub const GENESIS_PREV_HASH: [u8; LEN_HASH] = [0u8; LEN_HASH];

const _: () = assert!(RECEIPT_LEN == 229);
const _: () = assert!(SIGNED_RECEIPT_LEN == 293);

#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: [(usize, usize); 10] = [
        (OFF_DOMAIN_TAG, LEN_DOMAIN_TAG),
        (OFF_MODE, LEN_MODE),
        (OFF_SEQ, LEN_SEQ),
        (OFF_TX_SIG, LEN_TX_SIG),
        (OFF_TX_HASH, LEN_HASH),
        (OFF_RECENT_BLOCKHASH, LEN_HASH),
        (OFF_PREV_RECEIPT_HASH, LEN_HASH),
        (OFF_COMMITTER, LEN_PUBKEY),
        (OFF_INGRESS_SLOT, LEN_SLOT),
        (OFF_T_INGRESS_MICROS, LEN_MICROS),
    ];

    #[test]
    fn offsets_match_the_frozen_spec() {
        assert_eq!(
            FIELDS.map(|(off, _)| off),
            [0, 12, 13, 21, 85, 117, 149, 181, 213, 221]
        );
    }

    #[test]
    fn total_length_is_frozen() {
        assert_eq!(RECEIPT_LEN, 229);
        assert_eq!(DOMAIN_TAG, b"MBRECEIPT_V1");
    }

    #[test]
    fn fields_tile_the_buffer_with_no_gap_or_overlap() {
        let mut cursor = 0;
        for (off, len) in FIELDS {
            assert_eq!(
                off, cursor,
                "field at {off} does not follow the previous one"
            );
            cursor += len;
        }
        assert_eq!(cursor, RECEIPT_LEN);
    }
}
