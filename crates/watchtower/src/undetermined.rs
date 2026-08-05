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
    /// Neither reading of the block reproduces its hash, so its execution
    /// order cannot be established at all.
    UnverifiableBlock { slot: u64 },
    /// A conflicting pair where at least one side has no receipt to compare.
    MissingReceipt { slot: u64 },
    /// A conflicting pair involving operator-issued work. A just-in-time
    /// clone is created after the request it serves yet must execute before
    /// it, which is indistinguishable here from deliberate insertion.
    OperatorIssuedPair { slot: u64 },
}
