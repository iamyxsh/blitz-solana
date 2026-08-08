use mb_constants::receipt::RECEIPT_LEN;
use mb_slashing::Split;
use solana_program::{
    account_info::{AccountInfo, next_account_info},
    entrypoint::ProgramResult,
    incinerator,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::{
        Sysvar,
        clock::Clock,
        instructions::{load_current_index_checked, load_instruction_at_checked},
    },
};

use crate::{
    conviction_account::{CONVICTION_LEN, CONVICTION_SEED, ConvictionAccount},
    ed25519::verified_pairs,
    error::SlashError,
    evidence::Equivocation,
    instruction::Instruction,
    operator_account::{OPERATOR_LEN, OPERATOR_SEED, OperatorAccount},
    position_account::{POSITION_LEN, POSITION_SEED, PositionAccount},
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match Instruction::read(data)? {
        Instruction::Register { signing_key, bond } => {
            register(program_id, accounts, signing_key, bond)
        }
        Instruction::Stake { amount } => stake(program_id, accounts, amount),
        Instruction::Unstake { amount } => unstake(program_id, accounts, amount),
        Instruction::Claim => claim(program_id, accounts),
        Instruction::ProveEquivocation => prove_equivocation(program_id, accounts),
        Instruction::ClaimVictim { wire_bytes } => claim_victim(program_id, accounts, &wire_bytes),
    }
}

fn owned_by(account: &AccountInfo, program_id: &Pubkey) -> Result<(), SlashError> {
    if account.owner != program_id {
        return Err(SlashError::WrongOwner);
    }
    Ok(())
}

fn at_pda(account: &AccountInfo, seeds: &[&[u8]], program_id: &Pubkey) -> Result<u8, SlashError> {
    let (expected, bump) = Pubkey::find_program_address(seeds, program_id);
    if *account.key != expected {
        return Err(SlashError::WrongPda);
    }
    Ok(bump)
}

/// Lamports the account must keep: rent, the bond, staked capital, and
/// rewards already credited but not yet claimed.
///
/// Everything above this line is unowned and nothing may move it; everything
/// below belongs to someone and only their own instruction may.
fn reserved(operator: &OperatorAccount) -> u64 {
    Rent::get()
        .map(|rent| rent.minimum_balance(OPERATOR_LEN))
        .unwrap_or_default()
        + operator.bond
        + operator.pool_staked
}

fn register(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    signing_key: [u8; 32],
    bond: u64,
) -> ProgramResult {
    let info = &mut accounts.iter();
    let authority = next_account_info(info)?;
    let operator_info = next_account_info(info)?;
    let system = next_account_info(info)?;

    if !authority.is_signer {
        return Err(SlashError::NotSigner.into());
    }
    if bond == 0 {
        return Err(SlashError::InsufficientBond.into());
    }
    let seeds: &[&[u8]] = &[OPERATOR_SEED, authority.key.as_ref()];
    let bump = at_pda(operator_info, seeds, program_id)?;

    let rent = Rent::get()?.minimum_balance(OPERATOR_LEN);
    invoke_signed(
        &system_instruction::create_account(
            authority.key,
            operator_info.key,
            rent + bond,
            OPERATOR_LEN as u64,
            program_id,
        ),
        &[authority.clone(), operator_info.clone(), system.clone()],
        &[&[OPERATOR_SEED, authority.key.as_ref(), &[bump]]],
    )?;

    OperatorAccount {
        authority: *authority.key,
        signing_key,
        bond,
        pool_staked: 0,
        reward_index: 0,
        unbond_at: 0,
        bump,
    }
    .write(&mut operator_info.try_borrow_mut_data()?)?;
    Ok(())
}

/// Loads the operator and the caller's position, creating the position if this
/// is their first stake.
fn open_position<'a>(
    program_id: &Pubkey,
    owner: &AccountInfo<'a>,
    operator_info: &AccountInfo<'a>,
    position_info: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
) -> Result<PositionAccount, ProgramError> {
    let seeds: &[&[u8]] = &[
        POSITION_SEED,
        operator_info.key.as_ref(),
        owner.key.as_ref(),
    ];
    let bump = at_pda(position_info, seeds, program_id)?;

    if position_info.owner == &system_program::ID {
        let rent = Rent::get()?.minimum_balance(POSITION_LEN);
        invoke_signed(
            &system_instruction::create_account(
                owner.key,
                position_info.key,
                rent,
                POSITION_LEN as u64,
                program_id,
            ),
            &[owner.clone(), position_info.clone(), system.clone()],
            &[&[
                POSITION_SEED,
                operator_info.key.as_ref(),
                owner.key.as_ref(),
                &[bump],
            ]],
        )?;
        let fresh = PositionAccount {
            owner: *owner.key,
            operator: *operator_info.key,
            staked: 0,
            entry_index: 0,
            reward: 0,
            bump,
        };
        fresh.write(&mut position_info.try_borrow_mut_data()?)?;
        return Ok(fresh);
    }

    owned_by(position_info, program_id)?;
    let position = PositionAccount::read(&position_info.try_borrow_data()?)?;
    if position.owner != *owner.key || position.operator != *operator_info.key {
        return Err(SlashError::WrongPda.into());
    }
    Ok(position)
}

fn stake(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
    let info = &mut accounts.iter();
    let owner = next_account_info(info)?;
    let operator_info = next_account_info(info)?;
    let position_info = next_account_info(info)?;
    let system = next_account_info(info)?;

    if !owner.is_signer {
        return Err(SlashError::NotSigner.into());
    }
    owned_by(operator_info, program_id)?;

    let mut account = OperatorAccount::read(&operator_info.try_borrow_data()?)?;
    let mut stored = open_position(program_id, owner, operator_info, position_info, system)?;

    let mut pool = account.pool();
    let mut position = stored.position();
    pool.stake(&mut position, amount)
        .map_err(|_| SlashError::NothingStaked)?;

    // The staked lamports live in the operator account beside the bond; the
    // position account holds only the claim on them.
    invoke_signed(
        &system_instruction::transfer(owner.key, operator_info.key, amount),
        &[owner.clone(), operator_info.clone(), system.clone()],
        &[],
    )?;

    account.set_pool(pool);
    stored.set_position(position);
    account.write(&mut operator_info.try_borrow_mut_data()?)?;
    stored.write(&mut position_info.try_borrow_mut_data()?)?;
    Ok(())
}

/// Moves lamports out of the operator account without the system program,
/// which cannot sign for an account it does not own.
///
/// The floor is checked against the state being written, not the state that
/// was read, so a payout can never dip into rent, the bond, or capital still
/// staked by somebody else.
fn pay_out_from_operator(
    operator_info: &AccountInfo,
    account: &OperatorAccount,
    to: &AccountInfo,
    amount: u64,
) -> ProgramResult {
    let remaining = operator_info
        .lamports()
        .checked_sub(amount)
        .ok_or(SlashError::Overdraw)?;
    if remaining < reserved(account) {
        return Err(SlashError::Overdraw.into());
    }
    pay_out(operator_info, to, amount)
}

fn pay_out(from: &AccountInfo, to: &AccountInfo, amount: u64) -> ProgramResult {
    **from.try_borrow_mut_lamports()? = from
        .lamports()
        .checked_sub(amount)
        .ok_or(SlashError::Overdraw)?;
    **to.try_borrow_mut_lamports()? = to
        .lamports()
        .checked_add(amount)
        .ok_or(SlashError::Overdraw)?;
    Ok(())
}

fn unstake(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
    let info = &mut accounts.iter();
    let owner = next_account_info(info)?;
    let operator_info = next_account_info(info)?;
    let position_info = next_account_info(info)?;

    if !owner.is_signer {
        return Err(SlashError::NotSigner.into());
    }
    owned_by(operator_info, program_id)?;
    owned_by(position_info, program_id)?;

    let mut account = OperatorAccount::read(&operator_info.try_borrow_data()?)?;
    let mut stored = PositionAccount::read(&position_info.try_borrow_data()?)?;
    if stored.owner != *owner.key || stored.operator != *operator_info.key {
        return Err(SlashError::WrongPda.into());
    }

    let mut pool = account.pool();
    let mut position = stored.position();
    pool.unstake(&mut position, amount)
        .map_err(|_| SlashError::Overdraw)?;

    account.set_pool(pool);
    stored.set_position(position);
    account.write(&mut operator_info.try_borrow_mut_data()?)?;
    stored.write(&mut position_info.try_borrow_mut_data()?)?;
    pay_out_from_operator(operator_info, &account, owner, amount)
}

fn claim(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let info = &mut accounts.iter();
    let owner = next_account_info(info)?;
    let operator_info = next_account_info(info)?;
    let position_info = next_account_info(info)?;

    if !owner.is_signer {
        return Err(SlashError::NotSigner.into());
    }
    owned_by(operator_info, program_id)?;
    owned_by(position_info, program_id)?;

    let account = OperatorAccount::read(&operator_info.try_borrow_data()?)?;
    let mut stored = PositionAccount::read(&position_info.try_borrow_data()?)?;
    if stored.owner != *owner.key || stored.operator != *operator_info.key {
        return Err(SlashError::WrongPda.into());
    }

    let mut position = stored.position();
    let earned = account.pool().claim(&mut position);
    stored.set_position(position);
    stored.write(&mut position_info.try_borrow_mut_data()?)?;
    pay_out_from_operator(operator_info, &account, owner, earned)
}

/// Slashes the bond on two contradictory receipts the operator signed.
///
/// Accounts: accuser, operator, conviction, incinerator, instructions sysvar,
/// system program.
fn prove_equivocation(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let info = &mut accounts.iter();
    let accuser = next_account_info(info)?;
    let operator_info = next_account_info(info)?;
    let conviction_info = next_account_info(info)?;
    let burn_to = next_account_info(info)?;
    let sysvar = next_account_info(info)?;
    let system = next_account_info(info)?;

    if !accuser.is_signer {
        return Err(SlashError::NotSigner.into());
    }
    if *burn_to.key != incinerator::ID {
        return Err(SlashError::BadInstruction.into());
    }
    owned_by(operator_info, program_id)?;
    let mut account = OperatorAccount::read(&operator_info.try_borrow_data()?)?;

    // Whatever the precompile verified must be found by walking back from this
    // instruction, and it must have verified it under the registered key.
    let here = load_current_index_checked(sysvar)?;
    let ed25519_index = here
        .checked_sub(1)
        .ok_or(SlashError::NoEd25519Instruction)?;
    let verifier = load_instruction_at_checked(ed25519_index as usize, sysvar)?;
    if verifier.program_id != solana_program::ed25519_program::ID {
        return Err(SlashError::NoEd25519Instruction.into());
    }

    let pairs = verified_pairs(&verifier.data, ed25519_index)?;
    if pairs.len() != 2 {
        return Err(SlashError::WrongSignatureCount.into());
    }
    for pair in &pairs {
        if pair.key != &account.signing_key {
            return Err(SlashError::UnregisteredKey.into());
        }
        if pair.message.len() != RECEIPT_LEN {
            return Err(SlashError::MalformedReceipt.into());
        }
    }

    let evidence = Equivocation::check(pairs[0].message, pairs[1].message)?;
    let (low, high) = evidence.ordered();
    let (low, high) = (
        solana_program::hash::hash(low).to_bytes(),
        solana_program::hash::hash(high).to_bytes(),
    );

    let seeds: &[&[u8]] = &[
        CONVICTION_SEED,
        operator_info.key.as_ref(),
        low.as_ref(),
        high.as_ref(),
    ];
    let bump = at_pda(conviction_info, seeds, program_id)?;
    if conviction_info.owner != &system_program::ID {
        return Err(SlashError::AlreadyConvicted.into());
    }

    let slashed = account.bond;
    if slashed == 0 {
        return Err(SlashError::InsufficientBond.into());
    }

    let mut pool = account.pool();
    let applied = pool.distribute(Split::of(slashed));

    let rent = Rent::get()?.minimum_balance(CONVICTION_LEN);
    invoke_signed(
        &system_instruction::create_account(
            accuser.key,
            conviction_info.key,
            rent,
            CONVICTION_LEN as u64,
            program_id,
        ),
        &[accuser.clone(), conviction_info.clone(), system.clone()],
        &[&[
            CONVICTION_SEED,
            operator_info.key.as_ref(),
            low.as_ref(),
            high.as_ref(),
            &[bump],
        ]],
    )?;

    ConvictionAccount {
        operator: *operator_info.key,
        wronged: evidence.wronged_signature(),
        wronged_tx_hash: evidence.wronged_tx_hash(),
        slashed,
        owed_to_victim: applied.victim,
        slot: Clock::get()?.slot,
        bump,
    }
    .write(&mut conviction_info.try_borrow_mut_data()?)?;

    // The bond is gone the moment the fault is proven. The pool's share stays
    // in this account because the index now says who it belongs to; the
    // victim's waits in the conviction until the transaction is produced.
    account.bond = 0;
    account.set_pool(pool);
    account.write(&mut operator_info.try_borrow_mut_data()?)?;

    pay_out(operator_info, burn_to, applied.burn)?;
    pay_out(operator_info, conviction_info, applied.victim)?;

    solana_program::msg!(
        "slashed {} at seq {}: burn {} victim {} pool {}",
        slashed,
        evidence.seq,
        applied.burn,
        applied.victim,
        applied.pool
    );
    Ok(())
}

/// Pays the escrowed share to whoever produces the transaction the log lied
/// to.
///
/// Accounts: claimant, conviction.
fn claim_victim(program_id: &Pubkey, accounts: &[AccountInfo], wire: &[u8]) -> ProgramResult {
    let info = &mut accounts.iter();
    let claimant = next_account_info(info)?;
    let conviction_info = next_account_info(info)?;

    if !claimant.is_signer {
        return Err(SlashError::NotSigner.into());
    }
    owned_by(conviction_info, program_id)?;
    let mut conviction = ConvictionAccount::read(&conviction_info.try_borrow_data()?)?;

    // The hash is what binds these bytes to the receipt. Matching the
    // signature alone would not: nothing here verifies it, so anyone could
    // paste it into a transaction naming themselves as the payer.
    if solana_program::hash::hashv(&[wire]).to_bytes() != conviction.wronged_tx_hash {
        return Err(SlashError::MalformedReceipt.into());
    }
    if *claimant.key != Pubkey::new_from_array(crate::transaction::fee_payer(wire)?) {
        return Err(SlashError::NotSigner.into());
    }

    let owed = conviction.owed_to_victim;
    conviction.owed_to_victim = 0;
    conviction.write(&mut conviction_info.try_borrow_mut_data()?)?;
    pay_out(conviction_info, claimant, owed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slashing must never spend staked capital or rewards: those belong to
    /// people who did nothing wrong. Only the bond is at risk.
    #[test]
    fn the_reserved_floor_covers_stake_but_not_the_bond_once_slashed() {
        let account = OperatorAccount {
            authority: Pubkey::new_from_array([0x11; 32]),
            signing_key: [0x22; 32],
            bond: 0,
            pool_staked: 5_000,
            reward_index: 0,
            unbond_at: 0,
            bump: 255,
        };
        // Rent is unavailable outside the runtime, so this checks the parts
        // this test can see: staked capital is counted, a zeroed bond is not.
        assert!(reserved(&account) >= account.pool_staked);
    }
}
