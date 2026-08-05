//! Watches an ephemeral rollup and reports ordering faults.
//!
//! Reads only: `getIdentity`, `getSlot`, `getReceipts`, `getBlock`. It holds
//! no privileged position and shares no code with the validator, so anything
//! it reports can be reproduced by anyone who can reach the same endpoint.

use std::{collections::HashMap, thread::sleep, time::Duration};

use mb_watchtower::{Fault, Order, Scan, Undetermined, client::Client, scan_block, scan_receipts};

const RECEIPT_PAGE: u64 = 1_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:8899".to_owned());
    let once = args.any(|arg| arg == "--once");

    if let Err(error) = watch(&url, once) {
        eprintln!("watchtower stopped: {error}");
        std::process::exit(1);
    }
}

fn watch(url: &str, once: bool) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(url);
    let operator = client.operator()?;
    let identity = operator.to_bytes();

    println!("watching {url}");
    println!("operator {}", bs58::encode(identity).into_string());

    let mut next_slot = 1;
    let mut totals = Totals::default();

    loop {
        let receipts = client.receipts(0, RECEIPT_PAGE)?;
        let by_signature: HashMap<[u8; 64], _> = receipts
            .iter()
            .map(|signed| (signed.receipt.tx_sig, signed.clone()))
            .collect();

        report(
            &mut totals,
            "receipt log",
            scan_receipts(&receipts, &operator),
            &operator,
        );

        let head = client.slot()?;
        while next_slot < head {
            if let Some(block) = client.block(next_slot)? {
                if !block.transactions.is_empty() {
                    totals.blocks_with_transactions += 1;
                    totals.transactions += block.transactions.len();
                    *totals
                        .order
                        .entry(match Order::derive(&block) {
                            Order::AsReported => "as reported",
                            Order::Reversed => "reversed",
                            Order::Unverifiable => "unverifiable",
                        })
                        .or_default() += 1;
                }
                let scan = scan_block(&block, &by_signature, &operator, &identity);
                report(&mut totals, &format!("slot {next_slot}"), scan, &operator);
            }
            next_slot += 1;
        }

        println!(
            "· {} receipts · {} transactions in {} blocks · {} slots scanned",
            receipts.len(),
            totals.transactions,
            totals.blocks_with_transactions,
            next_slot.saturating_sub(1),
        );
        println!(
            "· {} faults, {} undetermined",
            totals.faults,
            totals.undetermined_count()
        );
        for (reason, count) in &totals.undetermined {
            println!("    {count} × {reason}");
        }
        for (direction, count) in &totals.order {
            println!("· execution order recovered {direction} in {count} blocks");
        }

        if once {
            return Ok(());
        }
        sleep(Duration::from_millis(500));
    }
}

#[derive(Default)]
struct Totals {
    faults: usize,
    undetermined: HashMap<&'static str, usize>,
    blocks_with_transactions: usize,
    transactions: usize,
    order: HashMap<&'static str, usize>,
}

impl Totals {
    fn undetermined_count(&self) -> usize {
        self.undetermined.values().sum()
    }
}

fn reason(undetermined: &Undetermined) -> &'static str {
    match undetermined {
        Undetermined::SequenceGap { .. } => "sequence gap",
        Undetermined::MissingOrigin { .. } => "log does not start at zero",
        Undetermined::UnverifiableBlock { .. } => "block hash not reproduced",
        Undetermined::MissingReceipt { .. } => "receipt missing for a pair",
        Undetermined::OperatorIssuedPair { .. } => "operator-issued pair",
    }
}

/// Faults are printed. Undetermined checks are counted and stay silent: a
/// detector that narrates everything it could not judge trains its reader to
/// ignore it.
fn report(totals: &mut Totals, context: &str, scan: Scan, operator: &ed25519_dalek::VerifyingKey) {
    for undetermined in &scan.undetermined {
        *totals.undetermined.entry(reason(undetermined)).or_default() += 1;
    }
    for fault in &scan.faults {
        totals.faults += 1;
        // Re-derived from the evidence before it is shown, so what gets
        // printed is something a third party can reproduce rather than
        // something this process merely asserts.
        let proof = match fault.verify(operator) {
            Ok(()) => "verified against the operator key".to_owned(),
            Err(error) => format!("EVIDENCE DID NOT VERIFY: {error}"),
        };
        println!("\nFAULT in {context}\n{}\n\n  [{proof}]", describe(fault));
    }
}

fn describe(fault: &Fault) -> String {
    let sig = |bytes: &[u8; 64]| bs58::encode(bytes).into_string();
    match fault {
        Fault::Equivocation { seq, a, b } => format!(
            "  equivocation at seq {seq}\n    statement A: {}\n    statement B: {}\n\
             \n  The operator signed two different receipts for one position.",
            sig(&a.receipt.tx_sig),
            sig(&b.receipt.tx_sig),
        ),
        Fault::BrokenChain { seq, .. } => format!(
            "  broken chain link at seq {seq}\n\
             \n  This receipt does not follow from the one before it, so the\n\
             \x20 log was rewritten after the fact."
        ),
        Fault::BadOrigin { receipt } => format!(
            "  bad origin at seq {}\n\
             \n  The log does not begin from a genesis link.",
            receipt.receipt.seq
        ),
        Fault::Unverifiable { seq, .. } => format!(
            "  unverifiable receipt at seq {seq}\n\
             \n  This receipt is not signed by the node's identity."
        ),
        Fault::Unticketed {
            slot,
            index,
            signature,
            ..
        } => format!(
            "  unticketed transaction in slot {slot} at index {index}\n    {}\n\
             \n  It holds a position in a block with no receipt behind it.",
            sig(signature)
        ),
        Fault::Reorder {
            slot,
            jumped,
            delayed,
            ..
        } => format!(
            "  reorder in slot {slot}\n\
             \x20   {} arrived at seq {} and ran at index {}\n\
             \x20   {} arrived at seq {} and ran at index {}\n\
             \n  These two transactions touch the same account, so the order\n\
             \x20 between them was the operator's to keep, and it did not.",
            sig(&jumped.receipt.receipt.tx_sig),
            jumped.receipt.receipt.seq,
            jumped.index,
            sig(&delayed.receipt.receipt.tx_sig),
            delayed.receipt.receipt.seq,
            delayed.index,
        ),
    }
}
