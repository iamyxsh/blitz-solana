use ed25519_dalek::SigningKey;
use solana_keypair::Keypair;

/// Reuses the validator identity as the receipt signing key.
///
/// A receipt is then attributable to the same pubkey `getIdentity` already
/// serves and the domain registry already knows, so there is no second key to
/// distribute or discover.
pub fn operator_signing_key(identity: &Keypair) -> SigningKey {
    SigningKey::from_bytes(identity.secret_bytes())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::Signer as _;
    use solana_signer::Signer as _;

    use super::*;

    const MESSAGE: &[u8] = b"MBRECEIPT_V1 conversion probe";

    fn identity() -> Keypair {
        Keypair::new_from_array([0x11; Keypair::SECRET_KEY_LENGTH])
    }

    #[test]
    fn the_converted_key_signs_identically_to_the_validator_identity() {
        let identity = identity();
        let converted = operator_signing_key(&identity);

        assert_eq!(
            converted.sign(MESSAGE).to_bytes().as_slice(),
            identity.sign_message(MESSAGE).as_ref(),
        );
    }

    #[test]
    fn the_converted_key_carries_the_same_public_key() {
        let identity = identity();
        assert_eq!(
            operator_signing_key(&identity).verifying_key().to_bytes(),
            identity.pubkey().to_bytes(),
        );
    }
}
