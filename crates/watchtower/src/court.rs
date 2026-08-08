use std::str::FromStr;

use mb_court::{Addresses, prove_equivocation};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use mb_watchtower::{Fault, Operator};

/// Where proven faults are taken to be settled.
///
/// Off by default. A watchtower that submits by accident spends real lamports
/// on whatever its detector believed, so this exists only when the operator to
/// accuse and the program to accuse them in were both named on the command
/// line.
pub struct Court {
    rpc: RpcClient,
    payer: Keypair,
    addresses: Addresses,
}

impl Court {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let value = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .and_then(|at| args.get(at + 1))
        };
        let program = Pubkey::from_str(value("--slash")?).ok()?;
        let authority = Pubkey::from_str(value("--operator")?).ok()?;
        let url = value("--court-url")
            .cloned()
            .unwrap_or_else(|| "https://api.devnet.solana.com".to_owned());

        let payer = load_payer()?;
        println!("submitting evidence to {program} as {}", payer.pubkey());
        Some(Self {
            rpc: RpcClient::new_with_commitment(url, CommitmentConfig::confirmed()),
            addresses: Addresses::new(program, &authority),
            payer,
        })
    }

    /// Takes one fault to the program. Only equivocation is settled on chain
    /// so far; the rest are reported and left for a verifier to act on.
    pub fn convict(&self, fault: &Fault, operator: &Operator) {
        let Fault::Equivocation { a, b, .. } = fault else {
            return;
        };
        let signer = Pubkey::new_from_array(operator.key.to_bytes());

        match self.send(&prove_equivocation(
            &self.addresses,
            &self.payer.pubkey(),
            &signer,
            a,
            b,
        )) {
            Ok(signature) => println!("  [convicted: {signature}]"),
            // A conviction that already exists is the normal outcome of
            // rescanning a log, not a failure worth stopping for.
            Err(error) => println!("  [not convicted: {error}]"),
        }
    }

    fn send(
        &self,
        instructions: &[solana_sdk::instruction::Instruction],
    ) -> Result<solana_sdk::signature::Signature, Box<dyn std::error::Error>> {
        let blockhash = self.rpc.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            instructions,
            Some(&self.payer.pubkey()),
            &[&self.payer],
            blockhash,
        );
        Ok(self.rpc.send_and_confirm_transaction(&transaction)?)
    }
}

fn load_payer() -> Option<Keypair> {
    let path = std::env::var("SOLANA_KEYPAIR").unwrap_or_else(|_| {
        format!(
            "{}/.config/solana/id.json",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let bytes: Vec<u8> = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    Keypair::try_from(bytes.as_slice()).ok()
}
