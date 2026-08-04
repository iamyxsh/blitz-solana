use mb_constants::receipt::LEN_HASH;
use sha2::{Digest, Sha256};

pub fn tx_hash(wire_bytes: &[u8]) -> [u8; LEN_HASH] {
    Sha256::digest(wire_bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn matches_a_known_sha256_digest() {
        assert_eq!(hex::encode(tx_hash(b"")), EMPTY_SHA256);
        assert_eq!(
            hex::encode(tx_hash(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn differs_from_hashing_the_encoded_forms() {
        let raw = [0xde, 0xad, 0xbe, 0xef];
        let digest = tx_hash(&raw);
        assert_ne!(digest, tx_hash(bs58::encode(raw).into_string().as_bytes()));
        assert_ne!(digest, tx_hash(hex::encode(raw).as_bytes()));
    }

    #[test]
    fn one_flipped_bit_changes_the_digest() {
        let mut raw = [0x01, 0x02, 0x03, 0x04];
        let before = tx_hash(&raw);
        raw[3] ^= 0x01;
        assert_ne!(before, tx_hash(&raw));
    }
}
