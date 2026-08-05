use crate::{fault::Fault, undetermined::Undetermined};

/// The outcome of a single check.
///
/// Three variants, not two. A detector that can only say "fault" or "fine"
/// eventually reports a bug in its own ingestion as misbehaviour by the
/// operator; making the third case a variant the compiler insists on handling
/// is what stops that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Fault(Box<Fault>),
    Clean,
    CannotDetermine(Undetermined),
}

impl Verdict {
    pub fn is_fault(&self) -> bool {
        matches!(self, Verdict::Fault(_))
    }
}
