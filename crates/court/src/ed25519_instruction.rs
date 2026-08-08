use mb_receipt::SignedReceipt;
use solana_sdk::{ed25519_program, instruction::Instruction, pubkey::Pubkey};

const HEADER_LEN: usize = 2;
const OFFSETS_LEN: usize = 14;
/// Tells the precompile the data it needs is in this same instruction.
const THIS_INSTRUCTION: u16 = u16::MAX;

/// Asks the ed25519 precompile to verify both receipts under one key.
///
/// The layout is the precompile's, and the program reads it back out with
/// `verified_pairs`. The two live in different crates and a disagreement
/// between them would be silent — the program would either reject valid
/// evidence or, worse, adjudicate the wrong bytes — so a test in this file
/// puts them back to back.
pub fn verify_two(signer: &Pubkey, a: &SignedReceipt, b: &SignedReceipt) -> Instruction {
    let mut data = vec![2u8, 0];
    let mut payload = Vec::new();
    let payload_start = HEADER_LEN + 2 * OFFSETS_LEN;

    for receipt in [a, b] {
        let signature_at = payload_start + payload.len();
        payload.extend_from_slice(&receipt.signature);
        let key_at = payload_start + payload.len();
        payload.extend_from_slice(signer.as_ref());
        let message_at = payload_start + payload.len();
        let message = receipt.message();
        payload.extend_from_slice(&message);

        for value in [
            signature_at as u16,
            THIS_INSTRUCTION,
            key_at as u16,
            THIS_INSTRUCTION,
            message_at as u16,
            message.len() as u16,
            THIS_INSTRUCTION,
        ] {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    data.extend_from_slice(&payload);

    Instruction {
        program_id: ed25519_program::ID,
        accounts: vec![],
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use mb_receipt::{Mode, Receipt, ZERO_PUBKEY};
    use mb_slashing_program::{ed25519::verified_pairs, evidence::Equivocation};

    const LOG_ID: [u8; 32] = [0x9c; 32];

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[0x07; 32])
    }

    fn receipt(seq: u64, tx_sig: u8) -> SignedReceipt {
        Receipt {
            log_id: LOG_ID,
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
        .sign(&key())
        .unwrap()
    }

    /// The builder and the on-chain reader are in different crates and must
    /// agree byte for byte, or valid evidence is rejected and invalid evidence
    /// is adjudicated against the wrong bytes.
    #[test]
    fn what_this_builds_is_what_the_program_reads_back() {
        let (a, b) = (receipt(7, 0xaa), receipt(7, 0xbb));
        let signer = Pubkey::new_from_array(key().verifying_key().to_bytes());

        let instruction = verify_two(&signer, &a, &b);
        let pairs = verified_pairs(&instruction.data, 0).unwrap();

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].key, &signer.to_bytes());
        assert_eq!(pairs[0].message, a.message().as_slice());
        assert_eq!(pairs[1].message, b.message().as_slice());

        let evidence = Equivocation::check(pairs[0].message, pairs[1].message).unwrap();
        assert_eq!(evidence.seq, 7);
    }

    /// The signatures the precompile will check have to be the real ones, or
    /// the transaction fails on chain with nothing to show for it.
    #[test]
    fn the_signatures_carried_are_the_ones_the_operator_made() {
        let (a, b) = (receipt(7, 0xaa), receipt(7, 0xbb));
        let signer = Pubkey::new_from_array(key().verifying_key().to_bytes());
        let instruction = verify_two(&signer, &a, &b);

        for (receipt, offset_entry) in [(&a, 0usize), (&b, 1)] {
            let at = HEADER_LEN + offset_entry * OFFSETS_LEN;
            let signature_at =
                u16::from_le_bytes([instruction.data[at], instruction.data[at + 1]]) as usize;
            assert_eq!(
                &instruction.data[signature_at..signature_at + 64],
                &receipt.signature
            );
            assert_eq!(key().sign(&receipt.message()).to_bytes(), receipt.signature);
        }
    }
}
