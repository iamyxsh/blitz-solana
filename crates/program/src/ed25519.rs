use crate::error::SlashError;

/// Layout of one entry in the ed25519 program's instruction data.
const OFFSETS_LEN: usize = 14;
const HEADER_LEN: usize = 2;
/// The ed25519 program's way of saying "this same instruction".
const THIS_INSTRUCTION: u16 = u16::MAX;

/// What the ed25519 program actually verified, read back out of its own
/// instruction data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified<'a> {
    pub key: &'a [u8; 32],
    pub message: &'a [u8],
}

/// Recovers the (key, message) pairs an ed25519 instruction proves.
///
/// The precompile verifies signatures and says nothing about *whose* or *over
/// what* — those live at byte offsets in its own data, and a program that only
/// checks such an instruction is present has checked nothing at all. So every
/// entry is required to point inside this instruction: an offset naming a
/// different one would let an attacker put the real signature somewhere
/// harmless and the receipt bytes here, and the two would never meet.
pub fn verified_pairs(data: &[u8], index: u16) -> Result<Vec<Verified<'_>>, SlashError> {
    if data.len() < HEADER_LEN {
        return Err(SlashError::NoEd25519Instruction);
    }
    let count = data[0] as usize;

    let mut pairs = Vec::with_capacity(count);
    for entry in 0..count {
        let at = HEADER_LEN + entry * OFFSETS_LEN;
        let offsets: &[u8] = data
            .get(at..at + OFFSETS_LEN)
            .ok_or(SlashError::NoEd25519Instruction)?;

        let field = |n: usize| u16::from_le_bytes([offsets[n * 2], offsets[n * 2 + 1]]);
        let (signature_at, key_at, message_at, message_len) =
            (field(0), field(2), field(4), field(5) as usize);

        for instruction_index in [field(1), field(3), field(6)] {
            if instruction_index != THIS_INSTRUCTION && instruction_index != index {
                return Err(SlashError::OffsetsNotSelfContained);
            }
        }
        // The signature itself is never read here, but a run of 64 bytes has
        // to exist where the precompile was told to look, or the offsets
        // describe a different instruction than the one that was verified.
        data.get(signature_at as usize..signature_at as usize + 64)
            .ok_or(SlashError::OffsetsNotSelfContained)?;

        let key: &[u8; 32] = data
            .get(key_at as usize..key_at as usize + 32)
            .ok_or(SlashError::OffsetsNotSelfContained)?
            .try_into()
            .map_err(|_| SlashError::OffsetsNotSelfContained)?;
        let message = data
            .get(message_at as usize..message_at as usize + message_len)
            .ok_or(SlashError::OffsetsNotSelfContained)?;

        pairs.push(Verified { key, message });
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IX_INDEX: u16 = 3;

    /// Builds ed25519 instruction data the way the precompile's own helper
    /// does: header, one offsets entry per signature, then the payloads.
    fn instruction_data(messages: &[&[u8]], key: [u8; 32]) -> Vec<u8> {
        let count = messages.len();
        let mut data = vec![count as u8, 0];
        let mut payload = Vec::new();
        let payload_start = HEADER_LEN + count * OFFSETS_LEN;

        for message in messages {
            let signature_at = payload_start + payload.len();
            payload.extend_from_slice(&[0x11; 64]);
            let key_at = payload_start + payload.len();
            payload.extend_from_slice(&key);
            let message_at = payload_start + payload.len();
            payload.extend_from_slice(message);

            for value in [
                signature_at as u16,
                IX_INDEX,
                key_at as u16,
                IX_INDEX,
                message_at as u16,
                message.len() as u16,
                IX_INDEX,
            ] {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        data.extend_from_slice(&payload);
        data
    }

    #[test]
    fn reads_back_every_key_and_message_the_precompile_was_given() {
        let key = [0x9c; 32];
        let data = instruction_data(&[b"first message", b"second"], key);

        let pairs = verified_pairs(&data, IX_INDEX).unwrap();

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].key, &key);
        assert_eq!(pairs[0].message, b"first message");
        assert_eq!(pairs[1].message, b"second");
    }

    /// The attack this function exists to stop. The precompile is pointed at a
    /// harmless message in another instruction and genuinely verifies it, while
    /// the bytes sitting here — the ones a careless program would read and
    /// adjudicate — were never signed by anybody.
    #[test]
    fn an_entry_pointing_at_another_instruction_is_refused() {
        let key = [0x9c; 32];
        let mut data = instruction_data(&[b"planted bytes"], key);
        // message_instruction_index is the seventh u16 of the entry.
        let at = HEADER_LEN + 12;
        data[at..at + 2].copy_from_slice(&(IX_INDEX + 1).to_le_bytes());

        assert_eq!(
            verified_pairs(&data, IX_INDEX),
            Err(SlashError::OffsetsNotSelfContained)
        );
    }

    #[test]
    fn the_sentinel_for_this_instruction_is_accepted() {
        let key = [0x9c; 32];
        let mut data = instruction_data(&[b"message"], key);
        for field in [1usize, 3, 6] {
            let at = HEADER_LEN + field * 2;
            data[at..at + 2].copy_from_slice(&THIS_INSTRUCTION.to_le_bytes());
        }

        let pairs = verified_pairs(&data, IX_INDEX).unwrap();
        assert_eq!(pairs[0].message, b"message");
    }

    #[test]
    fn an_offset_running_past_the_end_is_refused() {
        let key = [0x9c; 32];
        let mut data = instruction_data(&[b"message"], key);
        let at = HEADER_LEN + 8; // message_data_offset
        data[at..at + 2].copy_from_slice(&60_000u16.to_le_bytes());

        assert_eq!(
            verified_pairs(&data, IX_INDEX),
            Err(SlashError::OffsetsNotSelfContained)
        );
    }

    /// Claiming more signatures than were verified reads the payload back as
    /// offsets. Whichever check catches that, nothing may be returned: the
    /// caller counts the pairs and would otherwise adjudicate bytes the
    /// precompile never saw.
    #[test]
    fn a_count_larger_than_the_entries_present_is_refused() {
        let key = [0x9c; 32];
        let mut data = instruction_data(&[b"message"], key);
        data[0] = 4;

        assert!(verified_pairs(&data, IX_INDEX).is_err());
    }

    #[test]
    fn empty_data_is_refused() {
        assert_eq!(
            verified_pairs(&[], IX_INDEX),
            Err(SlashError::NoEd25519Instruction)
        );
    }
}
