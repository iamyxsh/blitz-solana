use crate::observed_transaction::ObservedTransaction;

/// Whether two transactions could have influenced each other.
///
/// They conflict when their account sets intersect on at least one account
/// that either of them writes. Two transactions sharing only reads cannot
/// front-run one another, and treating them as ordered would accuse an honest
/// validator every time it executed in parallel.
pub fn conflicts(a: &ObservedTransaction, b: &ObservedTransaction) -> bool {
    touches(a, &b.writable) || touches(b, &a.writable)
}

fn touches(txn: &ObservedTransaction, written: &[[u8; 32]]) -> bool {
    written
        .iter()
        .any(|key| txn.writable.contains(key) || txn.readonly.contains(key))
}
