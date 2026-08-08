/// Something the detector declined to judge.
///
/// Never an accusation. A receipt log can be paged, backfilled, or joined
/// late, and every one of those leaves holes that look exactly like a
/// withheld transaction. Recording why a check was skipped keeps those holes
/// countable instead of letting them become convictions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undetermined {
    /// The log jumps from one sequence number to a later one, so the chain
    /// link across the gap cannot be checked.
    SequenceGap { after: u64, before: u64 },
    /// The log does not begin at sequence zero, so its origin is unverified.
    MissingOrigin { lowest: u64 },
    /// An entry carrying no valid operator signature. Anyone can write bytes
    /// and only the operator can sign them, so such an entry is attributable
    /// to nobody: it is set aside rather than held against the operator.
    UnverifiableReceipt { seq: u64 },
    /// Neither reading of the block reproduces its hash, so its execution
    /// order cannot be established at all.
    UnverifiableBlock { slot: u64 },
    /// A conflicting pair where at least one side has no receipt to compare.
    MissingReceipt { slot: u64 },
    /// A conflicting pair where at least one receipt has been withdrawn. The
    /// execution is reported on its own; ordering is not judged against a
    /// statement the operator has taken back.
    WithdrawnReceipt { slot: u64 },
    /// The block hash a receipt names is outside the window the watchtower
    /// still remembers, so no timing claim about it can be checked.
    UnknownBlockhash { seq: u64 },
    /// Receipted but not yet seen in a block, and still within its grace
    /// period. Absence this fresh is indistinguishable from lag.
    NotYetExecuted { seq: u64 },
    /// A conflicting pair involving operator-issued work. A just-in-time
    /// clone is created after the request it serves yet must execute before
    /// it, which is indistinguishable here from deliberate insertion.
    OperatorIssuedPair { slot: u64 },
}
