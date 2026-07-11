//! Capability tokens: the EXPLICIT grant every IR program executes under
//! — operators (globs), core-seconds, resident memory, wall time, ledger
//! scope. Admission checks the token statically (fs-ir admission consumes
//! the bridge type); the governor meters against it continuously.

use fs_ir::admission::SessionCapability;

use crate::SessionError;

/// Maximum byte length of one canonical ledger scope.
pub const MAX_LEDGER_SCOPE_BYTES: usize = 128;
const LEDGER_SCOPE_REQUIREMENT: &str =
    "must contain 1..=128 ASCII graphic bytes (0x21..=0x7e), with no whitespace or controls";

/// A session identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub u64);

/// The capability token: explicit, bounded, ledger-scoped.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityToken {
    /// The session this token is bound to.
    pub session: SessionId,
    /// Granted operator globs (`flux.*`, `ascent.optimize`, …).
    pub ops: Vec<String>,
    /// Core-seconds grant (CPU time across all cores).
    pub core_s: f64,
    /// Resident-memory grant in bytes.
    pub mem_bytes: u64,
    /// Wall-clock grant in seconds.
    pub wall_s: f64,
    /// Cores the session may occupy at once.
    pub cores: u64,
    /// Canonical ledger scope (exact branch/namespace this session may write).
    /// [`crate::Governor::open_session`] validates it before registration.
    pub ledger_scope: String,
}

impl CapabilityToken {
    /// Validate the exact ledger namespace carried as session authority.
    ///
    /// Restricting scopes to bounded ASCII graphic bytes makes byte equality
    /// the canonical identity: there are no whitespace, control-character, or
    /// Unicode-normalization aliases for the same apparent namespace.
    pub fn validate_ledger_scope(scope: &str) -> Result<(), SessionError> {
        if scope.is_empty()
            || scope.len() > MAX_LEDGER_SCOPE_BYTES
            || !scope.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(SessionError::InvalidLedgerScope {
                scope: scope.to_string(),
                requirement: LEDGER_SCOPE_REQUIREMENT,
            });
        }
        Ok(())
    }

    /// The static-admission view of this token. Memory remains an exact `u64`
    /// through this bridge, so static admission and the enforcing governor make
    /// the same byte-for-byte authority decision.
    #[must_use]
    pub fn to_admission(&self) -> SessionCapability {
        SessionCapability {
            ops: self.ops.clone(),
            cores: self.cores,
            mem_bytes: self.mem_bytes,
            wall_s: self.wall_s,
        }
    }

    /// Does the token grant an operator?
    #[must_use]
    pub fn grants_op(&self, verb: &str) -> bool {
        self.ops.iter().any(|p| {
            p.strip_suffix('*')
                .map_or(p == verb, |prefix| verb.starts_with(prefix))
        })
    }
}
