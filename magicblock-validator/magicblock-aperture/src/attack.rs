use std::sync::Mutex;

use magicblock_core::link::transactions::WithEncoded;
use solana_transaction::sanitized::SanitizedTransaction;
use tracing::warn;

type Forwardable = WithEncoded<SanitizedTransaction>;

/// Deliberate misbehaviour, for demonstrating detection.
///
/// Enabled only by the `MB_ATTACK` environment variable and announced loudly
/// at startup. Nothing here is reachable in a default build's normal path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attack {
    None,
    /// Hold one transaction back and let the next overtake it.
    ///
    /// The receipt log stays perfectly honest: sequence numbers are still
    /// issued in arrival order and the chain still links. Only execution is
    /// reordered — which is precisely the divergence receipts exist to catch,
    /// and the one nothing in the vanilla validator can express.
    ReorderSwap,
}

impl Attack {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "reorder-swap" => Some(Attack::ReorderSwap),
            _ => None,
        }
    }
}

/// Holds at most one transaction back so the following one can overtake it.
pub struct AttackRig {
    mode: Attack,
    held: Mutex<Option<Forwardable>>,
}

impl AttackRig {
    pub fn disabled() -> Self {
        Self {
            mode: Attack::None,
            held: Mutex::new(None),
        }
    }

    pub fn from_env() -> Self {
        let Ok(raw) = std::env::var("MB_ATTACK") else {
            return Self::disabled();
        };
        let Some(mode) = Attack::parse(&raw) else {
            warn!(%raw, "unrecognised MB_ATTACK value; running honestly");
            return Self::disabled();
        };
        warn!(
            ?mode,
            "ATTACK MODE ENABLED — this validator will deliberately \
             misbehave. Never run this anywhere real."
        );
        Self {
            mode,
            held: Mutex::new(None),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.mode != Attack::None
    }

    /// Decides what actually gets forwarded, and in what order.
    ///
    /// Returns the transactions to hand the scheduler. Under `ReorderSwap`
    /// every other transaction is held, and released *after* its successor,
    /// so the pair executes in the opposite order to the one receipted.
    pub fn intercept(&self, transaction: Forwardable) -> Vec<Forwardable> {
        match self.mode {
            Attack::None => vec![transaction],
            Attack::ReorderSwap => {
                let mut held = self.held.lock().expect("attack rig poisoned");
                match held.take() {
                    // Its predecessor is waiting: this one jumps the queue.
                    Some(earlier) => vec![transaction, earlier],
                    None => {
                        *held = Some(transaction);
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Releases anything still held, so a run with an odd number of
    /// transactions does not leave one permanently unexecuted.
    pub fn drain(&self) -> Option<Forwardable> {
        self.held.lock().expect("attack rig poisoned").take()
    }
}
