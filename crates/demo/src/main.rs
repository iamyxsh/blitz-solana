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
    let flag = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };
    let program = Pubkey::from_str(
        args.first()
            .filter(|arg| !arg.starts_with("--"))
            .map(String::as_str)
            .unwrap_or("8VMsFLGQEF4x3wrFUfoipjjyzYFNe8DhNGAjXeDTSey7"),
    )?;

    // Receipts the attack rig actually produced, rather than a contradiction
    // this binary signed for itself. The operator is registered on its behalf,
    // which is the demo taking a shortcut a real operator would not need.
    if let (Some(path), Some(signer)) = (flag("--receipts"), flag("--signer")) {
        let url = flag("--url").unwrap_or_else(|| "https://api.devnet.solana.com".to_owned());
        return convict_captured(
            &RpcClient::new_with_commitment(url, CommitmentConfig::confirmed()),
            program,
            &path,
            &Pubkey::from_str(&signer)?,
        );
    }
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
    let funder = load_payer()?;
    let victim = Keypair::new();
    // A fresh authority every run, so the whole thing can be filmed twice.
    let payer = Keypair::new();
    let operator_key = SigningKey::from_bytes(&[0x07; 32]);
    let signing_key = Pubkey::new_from_array(operator_key.verifying_key().to_bytes());
    let addresses = Addresses::new(program, &payer.pubkey());

    fund(
        rpc,
        &funder,
        &payer.pubkey(),
        BOND + STAKE + LAMPORTS_PER_SOL / 50,
    )?;

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

    // The operator asks for its bond back, then tries to take it. The delay is
    // what stops misbehaving and withdrawing in the same breath.
    step("operator asks for its bond back", || {
        send(
            rpc,
            &payer,
            &[authority_only(
                program,
                &payer.pubkey(),
                &addresses,
                Slash::BeginUnbond,
            )],
        )
    })?;
    println!("\noperator tries to withdraw immediately");
    match send(
        rpc,
        &payer,
        &[authority_only(
            program,
            &payer.pubkey(),
            &addresses,
            Slash::WithdrawBond,
        )],
    ) {
        Ok(()) => return Err("the bond came out before its timelock ran".into()),
        Err(_) => println!("  refused: the bond stays slashable until the delay runs"),
    }

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

/// Convicts on a contradiction found in a captured receipt log.
///
/// The pair is not chosen here: the watchtower's own scan picks it, so what
/// reaches the program is exactly what the detector would have submitted.
fn convict_captured(rpc: &RpcClient, program: Pubkey, path: &str, signer: &Pubkey) -> Fallible {
    let payer = load_payer()?;
    let addresses = Addresses::new(program, &payer.pubkey());
    let receipts = load_receipts(path)?;

    let watched = mb_watchtower::Operator::new(
        ed25519_dalek::VerifyingKey::from_bytes(&signer.to_bytes())?,
        receipts
            .first()
            .ok_or("no receipts in the capture")?
            .receipt
            .log_id,
    );
    let scan = mb_watchtower::scan_receipts(&receipts, &watched);
    let Some(mb_watchtower::Fault::Equivocation { seq, a, b }) = scan
        .faults
        .iter()
        .find(|fault| matches!(fault, mb_watchtower::Fault::Equivocation { .. }))
    else {
        return Err(format!(
            "no equivocation in {path}: {} faults, {} undetermined",
            scan.faults.len(),
            scan.undetermined.len()
        )
        .into());
    };

    println!("program   {program}");
    println!("operator  {}", addresses.operator);
    println!("signer    {signer}");
    println!("\ncaptured contradiction at seq {seq}:");
    println!("  a {}", bs58(&a.receipt_hash()));
    println!("  b {}", bs58(&b.receipt_hash()));

    step("register the operator and post its bond", || {
        send(
            rpc,
            &payer,
            &[register(program, &payer.pubkey(), &addresses, signer)],
        )
    })?;
    step("stake coverage against it", || {
        send(rpc, &payer, &[stake(program, &payer.pubkey(), &addresses)])
    })?;
    report(rpc, &addresses, &payer.pubkey())?;

    let conviction = addresses.conviction(&a.message(), &b.message());
    step("prove the equivocation", || {
        send(
            rpc,
            &payer,
            &prove_equivocation(&addresses, &payer.pubkey(), signer, a, b),
        )
    })?;
    println!("  conviction {conviction}");
    report(rpc, &addresses, &payer.pubkey())?;

    if let Some(record) = read::<ConvictionAccount>(rpc, &conviction, ConvictionAccount::read)? {
        println!(
            "\nconviction: slashed {} lamports at slot {}, {} owed to the \
             sender of transaction {}",
            record.slashed,
            record.slot,
            record.owed_to_victim,
            bs58(&record.wronged)
        );
    }
    Ok(())
}

fn load_receipts(path: &str) -> Result<Vec<SignedReceipt>, Box<dyn std::error::Error>> {
    use solana_sdk::bs58 as _;
    let encoded: Vec<String> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let mut receipts = Vec::with_capacity(encoded.len());
    for entry in encoded {
        let bytes = base64_decode(&entry)?;
        receipts.push(SignedReceipt::from_bytes(&bytes).map_err(|error| error.to_string())?);
    }
    Ok(receipts)
}

/// Base64 without pulling in another dependency for six lines.
fn base64_decode(text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        let value = ALPHABET
            .iter()
            .position(|candidate| *candidate == byte)
            .ok_or("not base64")? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Ok(out)
}

fn register(
    program: Pubkey,
    payer: &Pubkey,
    addresses: &Addresses,
    signer: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(addresses.operator, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: Slash::Register {
            signing_key: signer.to_bytes(),
            bond: BOND,
        }
        .write(),
    }
}

fn authority_only(
    program: Pubkey,
    authority: &Pubkey,
    addresses: &Addresses,
    data: Slash,
) -> Instruction {
    Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(addresses.operator, false),
        ],
        data: data.write(),
    }
}

fn stake(program: Pubkey, payer: &Pubkey, addresses: &Addresses) -> Instruction {
    Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(addresses.operator, false),
            AccountMeta::new(addresses.position(payer), false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: Slash::Stake { amount: STAKE }.write(),
    }
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
