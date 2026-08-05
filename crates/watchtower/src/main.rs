//! Watches an ephemeral rollup and reports ordering faults.
//!
//! Reads only: `getIdentity`, `getSlot`, `getReceipts`, `getBlock`. It holds
//! no privileged position and shares no code with the validator, so anything
//! it reports can be reproduced by anyone who can reach the same endpoint.

use std::{collections::HashMap, thread::sleep, time::Duration};

use mb_watchtower::{
    BlockhashSlots, Execution, Fault, Order, Patience, Scan, Undetermined, client::Client,
    scan_block, scan_receipts, scan_withholding,
};

const RECEIPT_PAGE: u64 = 1_000;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1:8899".to_owned());
    let once = args.iter().any(|arg| arg == "--once");
    let client_receipts = args
        .iter()
        .position(|arg| arg == "--client-receipts")
        .and_then(|at| args.get(at + 1))
        .cloned();

    if let Err(error) = watch(&url, once, client_receipts.as_deref()) {
        eprintln!("watchtower stopped: {error}");
        std::process::exit(1);
    }
}

/// Receipts a client was handed, which the node's own log may contradict.
fn load_client_receipts(
    path: &str,
) -> Result<Vec<mb_receipt::SignedReceipt>, Box<dyn std::error::Error>> {
    use base64::{Engine, prelude::BASE64_STANDARD};

    let encoded: Vec<String> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let mut receipts = Vec::with_capacity(encoded.len());
    for entry in encoded {
        let bytes = BASE64_STANDARD.decode(entry)?;
        receipts.push(
            mb_receipt::SignedReceipt::from_bytes(&bytes).map_err(|error| error.to_string())?,
        );
    }
    Ok(receipts)
}

fn watch(
    url: &str,
    once: bool,
    client_receipts: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(url);
    let operator = client.operator()?;
    let identity = operator.to_bytes();

    println!("watching {url}");
    println!("operator {}", bs58::encode(identity).into_string());

    let held = match client_receipts {
        Some(path) => {
            let held = load_client_receipts(path)?;
            println!("holding {} client receipts from {path}", held.len());
            held
        }
        None => Vec::new(),
    };

    let mut next_slot = 1;
    let mut totals = Totals::default();
    let mut blockhashes = BlockhashSlots::default();
    let mut executed: HashMap<[u8; 64], Execution> = HashMap::new();
    let patience = Patience::default();

    loop {
        let receipts = client.receipts(0, RECEIPT_PAGE)?;
        let by_signature: HashMap<[u8; 64], _> = receipts
            .iter()
            .map(|signed| (signed.receipt.tx_sig, signed.clone()))
            .collect();

        // Scanned together: a contradiction between the published log and a
        // receipt the node handed a client is exactly two signed statements
        // about one position, which is what `scan_receipts` looks for.
        let mut combined = receipts.clone();
        combined.extend(held.iter().cloned());
        report(
            &mut totals,
            "receipt log",
            scan_receipts(&combined, &operator),
            &operator,
        );

        let head = client.slot()?;
        while next_slot < head {
            if let Some(block) = client.block(next_slot)? {
                blockhashes.record(block.blockhash, block.slot);
                if let Some(order) = block.executed_order() {
                    for (index, txn) in order.iter().enumerate() {
                        executed.insert(
                            txn.signature,
                            Execution {
                                slot: block.slot,
                                index: index as u32,
                            },
                        );
                    }
                }
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

        report(
            &mut totals,
            "delivery",
            scan_withholding(
                &receipts,
                &executed,
                &blockhashes,
                next_slot.saturating_sub(1),
                &patience,
                &operator,
            ),
            &operator,
        );

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
        Undetermined::UnknownBlockhash { .. } => "blockhash outside the window",
        Undetermined::NotYetExecuted { .. } => "not yet executed",
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
            "  equivocation at seq {seq}\n\
             \x20   both statements name transaction {}\n\
             \x20   but disagree on: {}\n\
             \x20   receipt hash A: {}\n\
             \x20   receipt hash B: {}\n\
             \n  The operator signed two different receipts for one position.\n\
             \x20 Whoever holds the other copy can prove it made both.",
            sig(&a.receipt.tx_sig),
            disagreements(a, b).join(", "),
            bs58::encode(a.receipt_hash()).into_string(),
            bs58::encode(b.receipt_hash()).into_string(),
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
        Fault::Withheld {
            receipt,
            execution,
            held,
        } => format!(
            "  withholding at seq {}\n\
             \x20   {}\n\
             \x20   receipted at slot {}, executed at slot {} — held {held} slots\n\
             \n  The operator signed for when it received this and then sat\n\
             \x20 on it. Both numbers are its own.",
            receipt.receipt.seq,
            sig(&receipt.receipt.tx_sig),
            receipt.receipt.ingress_slot,
            execution.slot,
        ),
        Fault::Absent {
            receipt,
            head,
            waited,
        } => format!(
            "  receipted but never executed, seq {}\n\
             \x20   {}\n\
             \x20   receipted at slot {}, still absent at slot {head} after {waited} slots\n\
             \n  The operator promised this transaction a position and never\n\
             \x20 gave it one.",
            receipt.receipt.seq,
            sig(&receipt.receipt.tx_sig),
            receipt.receipt.ingress_slot,
        ),
        Fault::ImpossibleIngress {
            receipt,
            blockhash_slot,
            ..
        } => format!(
            "  impossible ingress at seq {}\n\
             \x20   claims arrival at slot {}, but its block hash is from slot {blockhash_slot}\n\
             \n  A receipt cannot arrive before the block hash it names\n\
             \x20 existed, nor long after that hash would be refused.",
            receipt.receipt.seq, receipt.receipt.ingress_slot,
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

/// Which fields two receipts for one position disagree on.
///
/// Named explicitly because the difference can be a single field, and a
/// report that only says "these differ" reads like a bug in the reporter.
fn disagreements(
    a: &mb_receipt::SignedReceipt,
    b: &mb_receipt::SignedReceipt,
) -> Vec<&'static str> {
    let (a, b) = (&a.receipt, &b.receipt);
    let mut fields = Vec::new();
    if a.tx_sig != b.tx_sig {
        fields.push("tx_sig");
    }
    if a.tx_hash != b.tx_hash {
        fields.push("tx_hash");
    }
    if a.recent_blockhash != b.recent_blockhash {
        fields.push("recent_blockhash");
    }
    if a.prev_receipt_hash != b.prev_receipt_hash {
        fields.push("prev_receipt_hash");
    }
    if a.ingress_slot != b.ingress_slot {
        fields.push("ingress_slot");
    }
    if a.t_ingress_micros != b.t_ingress_micros {
        fields.push("t_ingress_micros");
    }
    if a.committer != b.committer {
        fields.push("committer");
    }
    if fields.is_empty() {
        fields.push("nothing (identical)");
    }
    fields
}
