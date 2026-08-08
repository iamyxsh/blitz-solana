use mb_slashing_program::{
    conviction_account::CONVICTION_SEED, operator_account::OPERATOR_SEED,
    position_account::POSITION_SEED,
};
use solana_sdk::{hash::hash, pubkey::Pubkey};

/// Where every account of one operator lives.
///
/// Derived here rather than passed around so a client and the program cannot
/// disagree about an address — the program recomputes each of these and
/// refuses anything else.
#[derive(Debug, Clone, Copy)]
pub struct Addresses {
    pub program: Pubkey,
    pub operator: Pubkey,
}

impl Addresses {
    pub fn new(program: Pubkey, authority: &Pubkey) -> Self {
        let (operator, _) =
            Pubkey::find_program_address(&[OPERATOR_SEED, authority.as_ref()], &program);
        Self { program, operator }
    }

    pub fn position(&self, owner: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[POSITION_SEED, self.operator.as_ref(), owner.as_ref()],
            &self.program,
        )
        .0
    }

    /// Addressed by the contradiction itself, under the same canonical
    /// ordering the program applies, so the pair presented either way round
    /// names one account.
    pub fn conviction(&self, a: &[u8], b: &[u8]) -> Pubkey {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        Pubkey::find_program_address(
            &[
                CONVICTION_SEED,
                self.operator.as_ref(),
                hash(low).as_ref(),
                hash(high).as_ref(),
            ],
            &self.program,
        )
        .0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addresses() -> Addresses {
        Addresses::new(
            Pubkey::new_from_array([0x01; 32]),
            &Pubkey::new_from_array([0x02; 32]),
        )
    }

    #[test]
    fn the_conviction_address_ignores_the_order_the_pair_is_presented_in() {
        let (a, b) = ([0xaa; 261].as_slice(), [0xbb; 261].as_slice());
        assert_eq!(
            addresses().conviction(a, b),
            addresses().conviction(b, a),
            "presenting the pair backwards must not mint a second conviction"
        );
    }

    #[test]
    fn different_contradictions_get_different_addresses() {
        let (a, b, c) = (
            [0xaa; 261].as_slice(),
            [0xbb; 261].as_slice(),
            [0xcc; 261].as_slice(),
        );
        assert_ne!(addresses().conviction(a, b), addresses().conviction(a, c));
    }

    #[test]
    fn every_staker_gets_their_own_position() {
        let addresses = addresses();
        assert_ne!(
            addresses.position(&Pubkey::new_from_array([0x03; 32])),
            addresses.position(&Pubkey::new_from_array([0x04; 32]))
        );
    }
}
