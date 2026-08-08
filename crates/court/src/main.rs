use std::str::FromStr;

use ed25519_dalek::SigningKey;
use mb_court::{Addresses, prove_equivocation};
use mb_receipt::{GENESIS_PREV_HASH, Mode, Receipt, SignedReceipt, ZERO_PUBKEY, tx_hash};
use mb_slashing_program::{
    conviction_account::ConvictionAccount, instruction::Instruction as Slash,
    operator_account::OperatorAccount, position_account::PositionAccount,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};

const BOND: u64 = LAMPORTS_PER_SOL / 10;
const STAKE: u64 = LAMPORTS_PER_SOL / 20;
const LOG_ID: [u8; 32] = [0x9c; 32];
const SEQ: u64 = 7;

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn main() -> Fallible {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let program = Pubkey::from_str(
        args.first()
            .map(String::as_str)
            .unwrap_or("8VMsFLGQEF4x3wrFUfoipjjyzYFNe8DhNGAjXeDTSey7"),
    )?;
    let url = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "https://api.devnet.solana.com".to_owned());

    run(
        &RpcClient::new_with_commitment(url, CommitmentConfig::confirmed()),
        program,
    )
}

fn run(rpc: &RpcClient, program: Pubkey) -> Fallible {
    let payer = load_payer()?;
    let victim = Keypair::new();
    let operator_key = SigningKey::from_bytes(&[0x07; 32]);
    let signing_key = Pubkey::new_from_array(operator_key.verifying_key().to_bytes());
    let addresses = Addresses::new(program, &payer.pubkey());

    println!("program   {program}");
    println!("authority {}", payer.pubkey());
    println!("operator  {}", addresses.operator);
    println!("signer    {signing_key}\n");

    // A transaction the operator will promise a position to. Its bytes are
    // what the victim must later produce to collect, so they are built first
    // and the receipts commit to their hash.
    let wronged = victim_transaction(&victim.pubkey());

    step("register the operator and post its bond", || {
        send(
            rpc,
            &payer,
            &[Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(addresses.operator, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: Slash::Register {
                    signing_key: signing_key.to_bytes(),
                    bond: BOND,
                }
                .write(),
            }],
        )
    })?;

    step("stake coverage against it", || {
        send(
            rpc,
            &payer,
            &[Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(addresses.operator, false),
                    AccountMeta::new(addresses.position(&payer.pubkey()), false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: Slash::Stake { amount: STAKE }.write(),
            }],
        )
    })?;

    report(rpc, &addresses, &payer.pubkey())?;

    // The fault: one position, two different signed statements about it.
    let (a, b) = contradiction(&operator_key, &wronged);
    println!("\noperator signed two receipts at seq {SEQ}:");
    println!("  a {}", bs58(&a.receipt_hash()));
    println!("  b {}", bs58(&b.receipt_hash()));

    let conviction = addresses.conviction(&a.message(), &b.message());
    step("prove the equivocation", || {
        send(
            rpc,
            &payer,
            &prove_equivocation(&addresses, &payer.pubkey(), &signing_key, &a, &b),
        )
    })?;
    println!("  conviction {conviction}");

    report(rpc, &addresses, &payer.pubkey())?;

    step("claim the pool reward", || {
        send(
            rpc,
            &payer,
            &[Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(addresses.operator, false),
                    AccountMeta::new(addresses.position(&payer.pubkey()), false),
                ],
                data: Slash::Claim.write(),
            }],
        )
    })?;

    // The victim needs lamports of its own only to sign; the payout follows.
    let before = rpc.get_balance(&victim.pubkey())?;
    fund(rpc, &payer, &victim.pubkey(), LAMPORTS_PER_SOL / 200)?;
    step("victim produces the transaction and collects", || {
        send(
            rpc,
            &victim,
            &[Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(victim.pubkey(), true),
                    AccountMeta::new(conviction, false),
                ],
                data: Slash::ClaimVictim {
                    wire_bytes: wronged.clone(),
                }
                .write(),
            }],
        )
    })?;
    let after = rpc.get_balance(&victim.pubkey())?;
    println!("  victim {} -> {} lamports", before, after);

    if let Some(record) = read::<ConvictionAccount>(rpc, &conviction, ConvictionAccount::read)? {
        println!(
            "\nconviction: slashed {} lamports at slot {}, {} still owed",
            record.slashed, record.slot, record.owed_to_victim
        );
    }
    Ok(())
}

/// Two receipts at one sequence number, in one run of the log, differing only
/// in which transaction they promise the position to.
fn contradiction(key: &SigningKey, wronged: &[u8]) -> (SignedReceipt, SignedReceipt) {
    let base = Receipt {
        log_id: LOG_ID,
        mode: Mode::Plain,
        seq: SEQ,
        tx_sig: first_signature(wronged),
        tx_hash: tx_hash(wronged),
        recent_blockhash: [0xc3; 32],
        prev_receipt_hash: GENESIS_PREV_HASH,
        committer: ZERO_PUBKEY,
        ingress_slot: 1_000,
        t_ingress_micros: 1_700_000_000_000_000,
    };
    let displaced = Receipt {
        tx_sig: [0xbb; 64],
        tx_hash: [0xbb; 32],
        ..base.clone()
    };
    (
        base.sign(key).expect("valid receipt"),
        displaced.sign(key).expect("valid receipt"),
    )
}

/// A legacy transfer, byte-exact, whose fee payer is the victim. Only its
/// bytes matter here: the program never executes it, it identifies who was
/// wronged.
fn victim_transaction(payer: &Pubkey) -> Vec<u8> {
    let mut wire = vec![0x01];
    wire.extend_from_slice(&[0x5a; 64]);
    wire.extend_from_slice(&[0x01, 0x00, 0x01]);
    wire.push(0x03);
    wire.extend_from_slice(payer.as_ref());
    wire.extend_from_slice(&[0x33; 32]);
    wire.extend_from_slice(&[0x00; 32]);
    wire.extend_from_slice(&[0x44; 32]);
    wire
}

fn first_signature(wire: &[u8]) -> [u8; 64] {
    wire[1..65].try_into().expect("one signature present")
}

fn report(rpc: &RpcClient, addresses: &Addresses, staker: &Pubkey) -> Fallible {
    if let Some(operator) = read(rpc, &addresses.operator, OperatorAccount::read)? {
        println!(
            "  bond {:>12}   staked {:>12}   index {}",
            operator.bond, operator.pool_staked, operator.reward_index
        );
        if let Some(position) = read(rpc, &addresses.position(staker), PositionAccount::read)? {
            println!(
                "  position {:>8}   earned {:>12}",
                position.staked,
                position.reward + operator.pool().claim(&mut position.position())
            );
        }
    }
    Ok(())
}

fn read<T>(
    rpc: &RpcClient,
    at: &Pubkey,
    parse: fn(&[u8]) -> Result<T, mb_slashing_program::error::SlashError>,
) -> Result<Option<T>, Box<dyn std::error::Error>> {
    match rpc
        .get_account_with_commitment(at, CommitmentConfig::confirmed())?
        .value
    {
        Some(account) => Ok(parse(&account.data).ok()),
        None => Ok(None),
    }
}

fn step(what: &str, run: impl FnOnce() -> Fallible) -> Fallible {
    println!("\n{what}");
    run()
}

fn send(rpc: &RpcClient, payer: &Keypair, instructions: &[Instruction]) -> Fallible {
    let blockhash = rpc.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &[payer],
        blockhash,
    );
    let signature = rpc.send_and_confirm_transaction(&transaction)?;
    println!("  {signature}");
    Ok(())
}

fn fund(rpc: &RpcClient, payer: &Keypair, to: &Pubkey, lamports: u64) -> Fallible {
    send(
        rpc,
        payer,
        &[solana_sdk::system_instruction::transfer(
            &payer.pubkey(),
            to,
            lamports,
        )],
    )
}

fn load_payer() -> Result<Keypair, Box<dyn std::error::Error>> {
    let path = std::env::var("SOLANA_KEYPAIR").unwrap_or_else(|_| {
        format!(
            "{}/.config/solana/id.json",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let bytes: Vec<u8> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    Ok(Keypair::try_from(bytes.as_slice())?)
}

fn bs58(bytes: &[u8]) -> String {
    solana_sdk::bs58::encode(bytes).into_string()
}
