use tracing::warn;

/// Whether the writer publishes a different receipt from the one it hands
/// back to the client.
///
/// Enabled only by `MB_ATTACK=equivocate` and announced loudly at startup.
/// Nothing here is reachable in a default build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equivocation {
    enabled: bool,
}

impl Equivocation {
    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    pub fn from_env() -> Self {
        let enabled = std::env::var("MB_ATTACK")
            .map(|raw| raw.trim() == "equivocate")
            .unwrap_or(false);
        if enabled {
            warn!(
                "ATTACK MODE ENABLED — this validator will publish receipts \
                 that differ from the ones it returns. Never run this \
                 anywhere real."
            );
        }
        Self { enabled }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}
