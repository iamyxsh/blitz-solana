/// How long a promised position may go unclaimed, and how many a single
/// committer may hold at once.
///
/// Both numbers bound the same thing: positions handed out blind that nobody
/// has produced contents for. Left unbounded, an operator could pre-commit a
/// menu of transactions, reveal only the profitable one, and abandon the rest
/// for free — which is the standard weakness of any commit-reveal scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevealDeadline {
    /// Slots a commitment may remain unrevealed. Generous on purpose:
    /// honest networks hiccup, and a false expiry is an accusation.
    pub slots: u64,
    /// Positions one committer may hold open at once.
    pub max_outstanding: usize,
}

impl RevealDeadline {
    /// Reads `MB_REVEAL_DEADLINE_SLOTS` and `MB_MAX_OUTSTANDING_COMMITS`.
    ///
    /// A knob rather than a constant because the right deadline depends on
    /// the block time and on how far the committing service sits from the
    /// operator — and because demonstrating an expiry should not require
    /// eight seconds of silence.
    pub fn from_env() -> Self {
        let fallback = Self::default();
        Self {
            slots: read("MB_REVEAL_DEADLINE_SLOTS").unwrap_or(fallback.slots),
            max_outstanding: read("MB_MAX_OUTSTANDING_COMMITS")
                .map(|value| value as usize)
                .unwrap_or(fallback.max_outstanding),
        }
    }
}

fn read(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

impl Default for RevealDeadline {
    fn default() -> Self {
        Self {
            slots: 150,
            max_outstanding: 8,
        }
    }
}
