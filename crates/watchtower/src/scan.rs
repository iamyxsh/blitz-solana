use crate::{fault::Fault, undetermined::Undetermined, verdict::Verdict};

/// The result of examining a stretch of receipt log.
///
/// A scan that found nothing is not the same as a scan that could not look:
/// the two are kept apart so "we saw no misbehaviour" is only ever said about
/// the parts actually examined.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scan {
    pub faults: Vec<Fault>,
    pub undetermined: Vec<Undetermined>,
    examined: usize,
}

impl Scan {
    pub(crate) fn record(&mut self, verdict: Verdict) {
        self.examined += 1;
        match verdict {
            Verdict::Fault(fault) => self.faults.push(*fault),
            Verdict::CannotDetermine(reason) => self.undetermined.push(reason),
            Verdict::Clean => (),
        }
    }

    /// Every check ran and none of them found anything.
    pub fn is_clean(&self) -> bool {
        self.faults.is_empty() && self.undetermined.is_empty()
    }

    /// How many checks were performed, whatever their outcome.
    pub fn examined(&self) -> usize {
        self.examined
    }
}
