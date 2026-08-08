use mb_receipt::SignedReceipt;
use mb_slashing_program::instruction::Instruction as Slash;
use solana_sdk::{
    incinerator,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program, sysvar,
};

use crate::{addresses::Addresses, ed25519_instruction::verify_two};

/// The two instructions that convict an operator, in the order the program
/// expects to find them.
///
/// The precompile call has to sit immediately before the program call: the
/// program looks back exactly one instruction for what was verified, so a
/// third instruction wedged between them breaks the link on purpose rather
/// than by accident.
pub fn prove_equivocation(
    addresses: &Addresses,
    accuser: &Pubkey,
    signer: &Pubkey,
    a: &SignedReceipt,
    b: &SignedReceipt,
) -> Vec<Instruction> {
    let conviction = addresses.conviction(&a.message(), &b.message());

    vec![
        verify_two(signer, a, b),
        Instruction {
            program_id: addresses.program,
            accounts: vec![
                AccountMeta::new(*accuser, true),
                AccountMeta::new(addresses.operator, false),
                AccountMeta::new(conviction, false),
                AccountMeta::new(incinerator::ID, false),
                AccountMeta::new_readonly(sysvar::instructions::ID, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: Slash::ProveEquivocation.write(),
        },
    ]
}
