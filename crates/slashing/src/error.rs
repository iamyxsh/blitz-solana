#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PoolError {
    #[error("cannot stake nothing")]
    StakeIsZero,
    #[error("withdrawing {requested} from a position holding {held}")]
    Overdraw { requested: u64, held: u64 },
    #[error("this position belongs to a different pool")]
    ForeignPosition,
}
