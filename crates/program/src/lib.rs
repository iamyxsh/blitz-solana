pub mod conviction_account;
pub mod ed25519;
pub mod error;
pub mod evidence;
pub mod instruction;
pub mod operator_account;
pub mod position_account;
pub mod processor;

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(entry);

#[cfg(not(feature = "no-entrypoint"))]
fn entry(
    program_id: &solana_program::pubkey::Pubkey,
    accounts: &[solana_program::account_info::AccountInfo],
    data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    processor::process(program_id, accounts, data)
}
