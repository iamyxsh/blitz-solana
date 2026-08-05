/// Where a transaction actually ran.
///
/// Always carried as a pair. A bare index means nothing across slots, because
/// it restarts at zero every time the scheduler advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Execution {
    pub slot: u64,
    pub index: u32,
}
