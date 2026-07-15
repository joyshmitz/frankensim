//! Affine, invocation-scoped resource accounting.
//!
//! `asupersync::Budget` is a copyable propagation envelope.  It is not a
//! spend ledger.  This module supplies the separate, non-`Clone` authority a
//! composed scientific invocation needs when sibling calls must not recreate
//! the ambient poll or cost allowance.

use crate::{CancelGate, Cx};
use fs_alloc::{LeaseCharge, OperationMemoryLease};
use fs_blake3::{ContentHash, hash_domain};

pub use asupersync::time::{TimeSource, VirtualClock, WallClock};
pub use asupersync::types::Time;

/// Version of the canonical invocation-accounting receipt.
pub const INVOCATION_RECEIPT_VERSION: u32 = 1;

const CHILD_ID_DOMAIN: &str = "frankensim.fs-exec.invocation-child.v1";
const CHILD_RECEIPT_DOMAIN: &str = "frankensim.fs-exec.invocation-child-receipt.v1";
const INVOCATION_RECEIPT_DOMAIN: &str = "frankensim.fs-exec.invocation-receipt.v1";

macro_rules! resource_unit {
    ($name:ident, $repr:ty, $docs:literal) => {
        #[doc = $docs]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name($repr);

        impl $name {
            /// Construct a typed quantity.
            #[must_use]
            pub const fn new(value: $repr) -> Self {
                Self(value)
            }

            /// Raw diagnostic value in this type's declared unit.
            #[must_use]
            pub const fn get(self) -> $repr {
                self.0
            }
        }
    };
}

resource_unit!(WorkUnits, u128, "Declared logical work units.");
resource_unit!(PollUnits, u32, "Cancellation/deadline poll opportunities.");
resource_unit!(CostUnits, u64, "Abstract monetary or energy cost units.");
resource_unit!(EvaluationUnits, u64, "Scientific evaluation count.");
resource_unit!(MemoryBytes, u64, "Concurrent live memory bytes.");
resource_unit!(OutputBytes, u64, "Retained publication capacity in bytes.");

/// Dimensioned affine capacities.  Deliberately no generic numeric indexing or
/// cross-kind conversion exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InvocationResources {
    work: WorkUnits,
    polls: PollUnits,
    cost: CostUnits,
    evaluations: EvaluationUnits,
    memory: MemoryBytes,
    output: OutputBytes,
}

impl InvocationResources {
    /// Construct one dimensionally explicit resource vector.
    #[must_use]
    pub const fn new(
        work: WorkUnits,
        polls: PollUnits,
        cost: CostUnits,
        evaluations: EvaluationUnits,
        memory: MemoryBytes,
        output: OutputBytes,
    ) -> Self {
        Self {
            work,
            polls,
            cost,
            evaluations,
            memory,
            output,
        }
    }

    /// Declared logical work.
    #[must_use]
    pub const fn work(self) -> WorkUnits {
        self.work
    }

    /// Poll opportunities.
    #[must_use]
    pub const fn polls(self) -> PollUnits {
        self.polls
    }

    /// Cost allowance.
    #[must_use]
    pub const fn cost(self) -> CostUnits {
        self.cost
    }

    /// Evaluation allowance.
    #[must_use]
    pub const fn evaluations(self) -> EvaluationUnits {
        self.evaluations
    }

    /// Concurrent-memory ceiling.
    #[must_use]
    pub const fn memory(self) -> MemoryBytes {
        self.memory
    }

    /// Retained-output capacity.
    #[must_use]
    pub const fn output(self) -> OutputBytes {
        self.output
    }

    /// Dimension-preserving checked subtraction.
    ///
    /// # Errors
    /// Refuses the first insufficient dimension in canonical resource order.
    pub fn checked_sub(self, requested: Self) -> Result<Self, InvocationError> {
        Ok(Self {
            work: WorkUnits(
                self.work
                    .0
                    .checked_sub(requested.work.0)
                    .ok_or_else(|| exceeded("work", requested.work.0, self.work.0))?,
            ),
            polls: PollUnits(self.polls.0.checked_sub(requested.polls.0).ok_or_else(|| {
                exceeded(
                    "polls",
                    u128::from(requested.polls.0),
                    u128::from(self.polls.0),
                )
            })?),
            cost: CostUnits(self.cost.0.checked_sub(requested.cost.0).ok_or_else(|| {
                exceeded(
                    "cost",
                    u128::from(requested.cost.0),
                    u128::from(self.cost.0),
                )
            })?),
            evaluations: EvaluationUnits(
                self.evaluations
                    .0
                    .checked_sub(requested.evaluations.0)
                    .ok_or_else(|| {
                        exceeded(
                            "evaluations",
                            u128::from(requested.evaluations.0),
                            u128::from(self.evaluations.0),
                        )
                    })?,
            ),
            memory: MemoryBytes(self.memory.0.checked_sub(requested.memory.0).ok_or_else(
                || {
                    exceeded(
                        "memory-bytes",
                        u128::from(requested.memory.0),
                        u128::from(self.memory.0),
                    )
                },
            )?),
            output: OutputBytes(self.output.0.checked_sub(requested.output.0).ok_or_else(
                || {
                    exceeded(
                        "output-bytes",
                        u128::from(requested.output.0),
                        u128::from(self.output.0),
                    )
                },
            )?),
        })
    }

    /// Dimension-preserving checked addition.
    ///
    /// # Errors
    /// Refuses representational overflow without changing either operand.
    pub fn checked_add(self, returned: Self) -> Result<Self, InvocationError> {
        Ok(Self {
            work: WorkUnits(
                self.work
                    .0
                    .checked_add(returned.work.0)
                    .ok_or(InvocationError::ArithmeticOverflow { resource: "work" })?,
            ),
            polls: PollUnits(
                self.polls
                    .0
                    .checked_add(returned.polls.0)
                    .ok_or(InvocationError::ArithmeticOverflow { resource: "polls" })?,
            ),
            cost: CostUnits(
                self.cost
                    .0
                    .checked_add(returned.cost.0)
                    .ok_or(InvocationError::ArithmeticOverflow { resource: "cost" })?,
            ),
            evaluations: EvaluationUnits(
                self.evaluations
                    .0
                    .checked_add(returned.evaluations.0)
                    .ok_or(InvocationError::ArithmeticOverflow {
                        resource: "evaluations",
                    })?,
            ),
            memory: MemoryBytes(self.memory.0.checked_add(returned.memory.0).ok_or(
                InvocationError::ArithmeticOverflow {
                    resource: "memory-bytes",
                },
            )?),
            output: OutputBytes(self.output.0.checked_add(returned.output.0).ok_or(
                InvocationError::ArithmeticOverflow {
                    resource: "output-bytes",
                },
            )?),
        })
    }
}

fn exceeded(resource: &'static str, requested: u128, available: u128) -> InvocationError {
    InvocationError::ResourceExceeded {
        resource,
        requested,
        available,
    }
}

/// Fixed admission envelope. Accuracy and capability are immutable identities,
/// not spendable counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationLimits {
    resources: InvocationResources,
    deadline: Option<Time>,
    accuracy_obligation: ContentHash,
    capability_scope: ContentHash,
}

impl InvocationLimits {
    /// Construct a complete invocation envelope.
    #[must_use]
    pub const fn new(
        resources: InvocationResources,
        deadline: Option<Time>,
        accuracy_obligation: ContentHash,
        capability_scope: ContentHash,
    ) -> Self {
        Self {
            resources,
            deadline,
            accuracy_obligation,
            capability_scope,
        }
    }

    /// Affine capacity dimensions.
    #[must_use]
    pub const fn resources(&self) -> InvocationResources {
        self.resources
    }

    /// Absolute logical deadline.
    #[must_use]
    pub const fn deadline(&self) -> Option<Time> {
        self.deadline
    }

    /// Immutable accuracy/tolerance obligation identity.
    #[must_use]
    pub const fn accuracy_obligation(&self) -> ContentHash {
        self.accuracy_obligation
    }

    /// Immutable capability-authority scope identity.
    #[must_use]
    pub const fn capability_scope(&self) -> ContentHash {
        self.capability_scope
    }
}

/// Opaque one-shot root-admission token.
///
/// The token is deliberately neither `Clone` nor `Copy`. Constructing it
/// validates the complete typed preflight against the caller's envelope;
/// [`Self::begin`] consumes it exactly once, so nested stages receive only
/// affine child leases and cannot reissue the admitted root authority.
#[derive(Debug)]
pub struct InvocationAdmission {
    invocation_id: ContentHash,
    limits: InvocationLimits,
    required: InvocationResources,
}

/// One-shot admission issuer for one scientific invocation.
///
/// The issuer is consumed when it seals a plan, so a coordinator must create
/// a distinct issuer for a distinct invocation and cannot remint the same
/// invocation from one authority object.
#[derive(Debug, Default)]
pub struct InvocationAdmitter {
    _private: (),
}

impl InvocationAdmitter {
    /// Create one unused invocation issuer.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Seal a complete preflight into a one-use admission token and consume
    /// this issuer.
    ///
    /// # Errors
    /// Refuses the first insufficient resource in canonical dimensional order.
    pub fn admit(
        self,
        invocation_id: ContentHash,
        limits: InvocationLimits,
        required: InvocationResources,
    ) -> Result<InvocationAdmission, InvocationError> {
        limits.resources.checked_sub(required)?;
        Ok(InvocationAdmission {
            invocation_id,
            limits,
            required,
        })
    }
}

impl InvocationAdmission {
    /// Consume this admission and mint the sole root spend authority.
    ///
    /// # Errors
    /// Refuses an already-reached absolute deadline.
    pub fn begin<'clock>(
        self,
        cx: &'clock Cx<'_>,
        clock: &'clock dyn TimeSource,
    ) -> Result<InvocationBudget<'clock>, InvocationError> {
        InvocationBudget::admit(self, cx.cancel_gate(), cx.lease(), clock)
    }
}

/// Terminal disposition retained by a child or invocation receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationDisposition {
    /// Authority closed without a latched error. Unused capacity is returned;
    /// a caller claiming an exact plan must separately verify exact spend.
    Completed,
    /// Cancellation was requested or observed and the operation drained.
    Cancelled,
    /// A typed admission/runtime fault refused publication.
    Refused,
}

/// Fail-closed affine-ledger refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationError {
    /// One typed capacity was insufficient.
    ResourceExceeded {
        /// Stable resource name.
        resource: &'static str,
        /// Requested units.
        requested: u128,
        /// Available units.
        available: u128,
    },
    /// Checked accounting overflowed.
    ArithmeticOverflow {
        /// Stable resource name.
        resource: &'static str,
    },
    /// The absolute deadline was reached or passed.
    DeadlineExpired {
        /// Stable observing phase.
        phase: &'static str,
        /// Absolute deadline.
        deadline_ns: u64,
        /// Clock observation.
        observed_ns: u64,
    },
    /// Cancellation was observed after spending one poll opportunity.
    Cancelled {
        /// Stable observing phase.
        phase: &'static str,
    },
    /// The backing operation-memory lease refused a reservation.
    MemoryRefused {
        /// Stable allocation site.
        what: &'static str,
        /// Requested bytes.
        requested: u64,
        /// Bytes live at refusal.
        used: u64,
        /// Enforced limit.
        limit: u64,
    },
    /// A scientific phase explicitly refused its domain result.
    ExplicitRefusal {
        /// Stable refusing phase.
        phase: &'static str,
        /// Content identity of the structured domain refusal.
        reason: ContentHash,
    },
    /// A child phase label must be non-empty before identity derivation.
    EmptyPhase,
    /// A lease was used after terminal disposition.
    InactiveChild,
    /// A child cannot close while a nested child remains live.
    LiveNestedChildren {
        /// Number of unfinished descendants immediately below it.
        count: u64,
    },
    /// A child cannot close while memory reservations remain live.
    LiveMemoryReservations {
        /// Bytes still held.
        bytes: u64,
    },
    /// Root finalization found an unfinished child.
    UnfinishedChild {
        /// Deterministic child identity.
        child: ContentHash,
    },
    /// The backing lease observed an impossible release invariant.
    MemoryReleaseInvariant,
}

impl core::fmt::Display for InvocationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ResourceExceeded {
                resource,
                requested,
                available,
            } => write!(
                formatter,
                "invocation resource `{resource}` requested {requested} units with {available} available"
            ),
            Self::ArithmeticOverflow { resource } => {
                write!(formatter, "invocation `{resource}` accounting overflowed")
            }
            Self::DeadlineExpired {
                phase,
                deadline_ns,
                observed_ns,
            } => write!(
                formatter,
                "invocation deadline {deadline_ns} ns expired during {phase} at {observed_ns} ns"
            ),
            Self::Cancelled { phase } => {
                write!(formatter, "invocation cancelled during {phase}")
            }
            Self::MemoryRefused {
                what,
                requested,
                used,
                limit,
            } => write!(
                formatter,
                "invocation memory refused {requested} B for `{what}` with {used}/{limit} B live"
            ),
            Self::ExplicitRefusal { phase, reason } => {
                write!(
                    formatter,
                    "invocation phase `{phase}` refused result {reason}"
                )
            }
            Self::EmptyPhase => formatter.write_str("invocation child phase must be non-empty"),
            Self::InactiveChild => formatter.write_str("invocation child is no longer active"),
            Self::LiveNestedChildren { count } => write!(
                formatter,
                "invocation child still owns {count} unfinished nested child lease(s)"
            ),
            Self::LiveMemoryReservations { bytes } => write!(
                formatter,
                "invocation child still owns {bytes} B of live memory reservations"
            ),
            Self::UnfinishedChild { child } => {
                write!(formatter, "invocation child {child} was not finalized")
            }
            Self::MemoryReleaseInvariant => {
                formatter.write_str("invocation backing memory lease violated release accounting")
            }
        }
    }
}

impl core::error::Error for InvocationError {}

fn error_disposition(error: &InvocationError) -> InvocationDisposition {
    match error {
        InvocationError::DeadlineExpired { .. } | InvocationError::Cancelled { .. } => {
            InvocationDisposition::Cancelled
        }
        _ => InvocationDisposition::Refused,
    }
}

/// First backing-lease refusal retained in the immutable invocation receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationMemoryRefusal {
    what: &'static str,
    requested: u64,
    used: u64,
    limit: u64,
}

impl InvocationMemoryRefusal {
    /// Stable allocation site.
    #[must_use]
    pub const fn what(&self) -> &'static str {
        self.what
    }

    /// Bytes requested by the refused reservation.
    #[must_use]
    pub const fn requested_bytes(&self) -> u64 {
        self.requested
    }

    /// Bytes live at refusal.
    #[must_use]
    pub const fn used_bytes(&self) -> u64 {
        self.used
    }

    /// Enforced backing limit.
    #[must_use]
    pub const fn limit_bytes(&self) -> u64 {
        self.limit
    }
}

/// Structured reason an immutable receipt failed semantic verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptSemanticError {
    /// The receipt schema is not understood by this verifier.
    UnsupportedVersion {
        /// Encountered schema version.
        found: u32,
    },
    /// The canonical root does not bind the retained fields.
    RootMismatch,
    /// A child receipt violated one named invariant.
    Child {
        /// Deterministic child ordinal.
        ordinal: u64,
        /// Stable invariant name.
        invariant: &'static str,
    },
    /// The invocation-level receipt violated one named invariant.
    Invocation {
        /// Stable invariant name.
        invariant: &'static str,
    },
}

impl core::fmt::Display for ReceiptSemanticError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => {
                write!(formatter, "unsupported invocation receipt version {found}")
            }
            Self::RootMismatch => formatter.write_str("invocation receipt root mismatch"),
            Self::Child { ordinal, invariant } => write!(
                formatter,
                "invocation child ordinal {ordinal} violated `{invariant}`"
            ),
            Self::Invocation { invariant } => {
                write!(formatter, "invocation receipt violated `{invariant}`")
            }
        }
    }
}

impl core::error::Error for ReceiptSemanticError {}

#[derive(Debug)]
struct ChildState {
    id: ContentHash,
    parent: Option<usize>,
    ordinal: u64,
    phase: &'static str,
    granted: InvocationResources,
    remaining: InvocationResources,
    direct_consumed: InvocationResources,
    memory_current: u64,
    subtree_memory_current: u64,
    direct_memory_peak: u64,
    memory_peak: u64,
    memory_requested: u64,
    memory_released: u64,
    output_retained: u64,
    live_children: u64,
    failure: Option<InvocationError>,
    disposition: Option<InvocationDisposition>,
}

/// Immutable accounting evidence for one child lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildReceipt {
    id: ContentHash,
    parent: Option<ContentHash>,
    ordinal: u64,
    phase: &'static str,
    granted: InvocationResources,
    consumed: InvocationResources,
    direct_consumed: InvocationResources,
    returned: InvocationResources,
    direct_memory_peak: u64,
    memory_peak: u64,
    memory_requested: u64,
    memory_released: u64,
    output_retained: u64,
    failure: Option<InvocationError>,
    disposition: InvocationDisposition,
    root: ContentHash,
}

impl ChildReceipt {
    /// Deterministic child identity.
    #[must_use]
    pub const fn id(&self) -> ContentHash {
        self.id
    }

    /// Parent child identity, or `None` for a root-level phase.
    #[must_use]
    pub const fn parent(&self) -> Option<ContentHash> {
        self.parent
    }

    /// Global deterministic issue ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// Stable phase name.
    #[must_use]
    pub const fn phase(&self) -> &'static str {
        self.phase
    }

    /// Original transferred allowance.
    #[must_use]
    pub const fn granted(&self) -> InvocationResources {
        self.granted
    }

    /// Permanently spent consumables and retained output.
    #[must_use]
    pub const fn consumed(&self) -> InvocationResources {
        self.consumed
    }

    /// Resources spent directly by this phase, excluding descendants.
    #[must_use]
    pub const fn direct_consumed(&self) -> InvocationResources {
        self.direct_consumed
    }

    /// Unused capacities returned exactly once.
    #[must_use]
    pub const fn returned(&self) -> InvocationResources {
        self.returned
    }

    /// Peak concurrent memory under this child.
    #[must_use]
    pub const fn memory_peak_bytes(&self) -> u64 {
        self.memory_peak
    }

    /// Peak bytes reserved directly by this phase, excluding descendants.
    #[must_use]
    pub const fn direct_memory_peak_bytes(&self) -> u64 {
        self.direct_memory_peak
    }

    /// Cumulative bytes directly reserved by this child.
    #[must_use]
    pub const fn memory_requested_bytes(&self) -> u64 {
        self.memory_requested
    }

    /// Cumulative direct reservations released by this child.
    #[must_use]
    pub const fn memory_released_bytes(&self) -> u64 {
        self.memory_released
    }

    /// Retained output bytes.
    #[must_use]
    pub const fn output_retained_bytes(&self) -> u64 {
        self.output_retained
    }

    /// First latched runtime refusal, when this child did not complete.
    #[must_use]
    pub const fn failure(&self) -> Option<&InvocationError> {
        self.failure.as_ref()
    }

    /// Terminal child disposition.
    #[must_use]
    pub const fn disposition(&self) -> InvocationDisposition {
        self.disposition
    }

    /// Canonical child-receipt root.
    #[must_use]
    pub const fn root(&self) -> ContentHash {
        self.root
    }
}

/// Immutable terminal receipt.  This value is cloneable evidence; it contains
/// no live resource authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationReceipt {
    version: u32,
    invocation_id: ContentHash,
    limits: InvocationLimits,
    required: InvocationResources,
    remaining: InvocationResources,
    children: Vec<ChildReceipt>,
    last_deadline_observation: Option<Time>,
    memory_peak: u64,
    memory_requested: u64,
    memory_released: u64,
    memory_refusals: u64,
    memory_first_refusal: Option<InvocationMemoryRefusal>,
    output_retained: u64,
    failure: Option<InvocationError>,
    disposition: InvocationDisposition,
    root: ContentHash,
}

impl InvocationReceipt {
    /// Receipt schema version.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Stable invocation identity.
    #[must_use]
    pub const fn invocation_id(&self) -> ContentHash {
        self.invocation_id
    }

    /// Admitted limits and immutable obligations.
    #[must_use]
    pub const fn limits(&self) -> &InvocationLimits {
        &self.limits
    }

    /// Preflight requirement.
    #[must_use]
    pub const fn required(&self) -> InvocationResources {
        self.required
    }

    /// Unspent capacity at terminal finalization.
    #[must_use]
    pub const fn remaining(&self) -> InvocationResources {
        self.remaining
    }

    /// Ordered child receipts.
    #[must_use]
    pub fn children(&self) -> &[ChildReceipt] {
        &self.children
    }

    /// Peak backing-memory live set.
    #[must_use]
    pub const fn memory_peak_bytes(&self) -> u64 {
        self.memory_peak
    }

    /// Cumulative bytes admitted by the backing memory lease.
    #[must_use]
    pub const fn memory_requested_bytes(&self) -> u64 {
        self.memory_requested
    }

    /// Cumulative bytes released by completed RAII reservations.
    #[must_use]
    pub const fn memory_released_bytes(&self) -> u64 {
        self.memory_released
    }

    /// Count of backing memory refusals retained by this transaction.
    #[must_use]
    pub const fn memory_refusals(&self) -> u64 {
        self.memory_refusals
    }

    /// First backing memory refusal, when any occurred.
    #[must_use]
    pub const fn memory_first_refusal(&self) -> Option<&InvocationMemoryRefusal> {
        self.memory_first_refusal.as_ref()
    }

    /// Last logical-clock observation made for deadline enforcement.
    #[must_use]
    pub const fn last_deadline_observation(&self) -> Option<Time> {
        self.last_deadline_observation
    }

    /// Retained output bytes.
    #[must_use]
    pub const fn output_retained_bytes(&self) -> u64 {
        self.output_retained
    }

    /// First latched transaction failure, when terminal disposition is not
    /// completed.
    #[must_use]
    pub const fn failure(&self) -> Option<&InvocationError> {
        self.failure.as_ref()
    }

    /// Terminal disposition.
    #[must_use]
    pub const fn disposition(&self) -> InvocationDisposition {
        self.disposition
    }

    /// Canonical accounting root.
    #[must_use]
    pub const fn root(&self) -> ContentHash {
        self.root
    }

    /// Recompute the canonical accounting root and all typed conservation,
    /// topology, memory, output, and disposition invariants.
    #[must_use]
    pub fn verifies_integrity(&self) -> bool {
        self.verify_semantics().is_ok()
    }

    /// Verify the canonical root and the complete affine receipt semantics.
    ///
    /// # Errors
    /// Returns the first invariant failure in deterministic schema order.
    pub fn verify_semantics(&self) -> Result<(), ReceiptSemanticError> {
        verify_receipt_semantics(self)
    }
}

/// Non-cloneable root invocation authority.
pub struct InvocationBudget<'clock> {
    invocation_id: ContentHash,
    limits: InvocationLimits,
    required: InvocationResources,
    remaining: InvocationResources,
    clock: &'clock dyn TimeSource,
    cancel_gate: &'clock CancelGate,
    last_deadline_observation: Option<Time>,
    _ambient_memory: Option<LeaseCharge>,
    backing_memory: OperationMemoryLease,
    children: Vec<ChildState>,
    next_ordinal: u64,
    failure: Option<InvocationError>,
}

impl core::fmt::Debug for InvocationBudget<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("InvocationBudget")
            .field("invocation_id", &self.invocation_id)
            .field("limits", &self.limits)
            .field("required", &self.required)
            .field("remaining", &self.remaining)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<'clock> InvocationBudget<'clock> {
    /// Admit a complete plan before any child can spend authority.
    ///
    /// # Errors
    /// Refuses the first resource in fixed work, poll, cost, evaluation,
    /// memory, output order, then an already-expired deadline.
    fn admit(
        admission: InvocationAdmission,
        cancel_gate: &'clock CancelGate,
        ambient_memory: Option<&OperationMemoryLease>,
        clock: &'clock dyn TimeSource,
    ) -> Result<Self, InvocationError> {
        let InvocationAdmission {
            invocation_id,
            limits,
            required,
        } = admission;
        let last_deadline_observation = if let Some(deadline) = limits.deadline {
            let now = clock.now();
            if now >= deadline {
                return Err(InvocationError::DeadlineExpired {
                    phase: "invocation-admission",
                    deadline_ns: deadline.as_nanos(),
                    observed_ns: now.as_nanos(),
                });
            }
            Some(now)
        } else {
            None
        };
        let ambient_memory = ambient_memory
            .map(|lease| lease.reserve("invocation-root-memory", required.memory.0))
            .transpose()
            .map_err(|refusal| InvocationError::MemoryRefused {
                what: refusal.what,
                requested: refusal.requested_bytes,
                used: refusal.used_bytes,
                limit: refusal.limit_bytes,
            })?;
        Ok(Self {
            invocation_id,
            limits,
            required,
            remaining: required,
            clock,
            cancel_gate,
            last_deadline_observation,
            _ambient_memory: ambient_memory,
            backing_memory: OperationMemoryLease::bounded(required.memory.0),
            children: Vec::new(),
            next_ordinal: 0,
            failure: None,
        })
    }

    /// Transfer an exact affine allowance to one sequential child.
    ///
    /// # Errors
    /// Refuses an empty phase, insufficient capacity, or ordinal overflow
    /// before mutation.
    pub fn split_child<'budget>(
        &'budget mut self,
        phase: &'static str,
        grant: InvocationResources,
    ) -> Result<ChildBudget<'budget, 'clock>, InvocationError> {
        let node = self.open_child(None, phase, grant)?;
        Ok(ChildBudget { owner: self, node })
    }

    fn open_child(
        &mut self,
        parent: Option<usize>,
        phase: &'static str,
        grant: InvocationResources,
    ) -> Result<usize, InvocationError> {
        if let Some(error) = parent
            .and_then(|index| self.children.get(index))
            .and_then(|state| state.failure.clone())
            .or_else(|| self.failure.clone())
        {
            return Err(error);
        }
        if phase.is_empty() {
            return Err(InvocationError::EmptyPhase);
        }
        let ordinal = self.next_ordinal;
        let next_ordinal = match ordinal.checked_add(1) {
            Some(next) => next,
            None => {
                let error = InvocationError::ArithmeticOverflow {
                    resource: "child-ordinal",
                };
                self.latch_failure(parent, error.clone());
                return Err(error);
            }
        };
        let available = match parent {
            Some(index) => {
                let state = self
                    .children
                    .get(index)
                    .ok_or(InvocationError::InactiveChild)?;
                if state.disposition.is_some() {
                    return Err(InvocationError::InactiveChild);
                }
                state.remaining
            }
            None => self.remaining,
        };
        if let Some(index) = parent {
            let direct_live = self.children[index].memory_current;
            let allocatable = available
                .memory
                .0
                .checked_sub(direct_live)
                .ok_or(InvocationError::MemoryReleaseInvariant)?;
            if grant.memory.0 > allocatable {
                let error = exceeded(
                    "memory-bytes",
                    u128::from(grant.memory.0),
                    u128::from(allocatable),
                );
                self.latch_failure(parent, error.clone());
                return Err(error);
            }
        }
        let remaining = match available.checked_sub(grant) {
            Ok(remaining) => remaining,
            Err(error) => {
                self.latch_failure(parent, error.clone());
                return Err(error);
            }
        };
        let live_children = match parent {
            Some(index) => match self.children[index].live_children.checked_add(1) {
                Some(count) => count,
                None => {
                    let error = InvocationError::ArithmeticOverflow {
                        resource: "live-children",
                    };
                    self.latch_failure(parent, error.clone());
                    return Err(error);
                }
            },
            None => 0,
        };
        let parent_id = parent.map(|index| self.children[index].id);
        let id = child_id(self.invocation_id, parent_id, ordinal, phase);
        match parent {
            Some(index) => {
                let parent_state = &mut self.children[index];
                parent_state.remaining = remaining;
                parent_state.live_children = live_children;
            }
            None => self.remaining = remaining,
        }
        self.next_ordinal = next_ordinal;
        let node = self.children.len();
        self.children.push(ChildState {
            id,
            parent,
            ordinal,
            phase,
            granted: grant,
            remaining: grant,
            direct_consumed: InvocationResources::default(),
            memory_current: 0,
            subtree_memory_current: 0,
            direct_memory_peak: 0,
            memory_peak: 0,
            memory_requested: 0,
            memory_released: 0,
            output_retained: 0,
            live_children: 0,
            failure: None,
            disposition: None,
        });
        Ok(node)
    }

    fn latch_failure(&mut self, mut node: Option<usize>, error: InvocationError) {
        while let Some(index) = node {
            let state = &mut self.children[index];
            if state.failure.is_none() {
                state.failure = Some(error.clone());
            }
            node = state.parent;
        }
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }

    fn close_child(&mut self, node: usize) -> Result<InvocationDisposition, InvocationError> {
        let (parent, returned, disposition) = {
            let state = self
                .children
                .get(node)
                .ok_or(InvocationError::InactiveChild)?;
            if state.disposition.is_some() {
                return Err(InvocationError::InactiveChild);
            }
            if state.live_children != 0 {
                return Err(InvocationError::LiveNestedChildren {
                    count: state.live_children,
                });
            }
            if state.memory_current != 0 {
                return Err(InvocationError::LiveMemoryReservations {
                    bytes: state.memory_current,
                });
            }
            if state.subtree_memory_current != 0 {
                return Err(InvocationError::MemoryReleaseInvariant);
            }
            (
                state.parent,
                state.remaining,
                state
                    .failure
                    .as_ref()
                    .map_or(InvocationDisposition::Completed, error_disposition),
            )
        };
        match parent {
            Some(index) => {
                let parent_state = &mut self.children[index];
                parent_state.remaining = parent_state.remaining.checked_add(returned)?;
                parent_state.live_children = parent_state.live_children.checked_sub(1).ok_or(
                    InvocationError::ArithmeticOverflow {
                        resource: "live-children",
                    },
                )?;
            }
            None => self.remaining = self.remaining.checked_add(returned)?,
        }
        self.children[node].disposition = Some(disposition);
        Ok(disposition)
    }

    fn current_failure(&self, node: Option<usize>) -> Option<InvocationError> {
        node.and_then(|index| self.children[index].failure.clone())
            .or_else(|| self.failure.clone())
    }

    fn observe_deadline(
        &mut self,
        node: Option<usize>,
        phase: &'static str,
    ) -> Result<(), InvocationError> {
        if let Some(error) = self.current_failure(node) {
            return Err(error);
        }
        let Some(deadline) = self.limits.deadline else {
            return Ok(());
        };
        let now = self.clock.now();
        self.last_deadline_observation = Some(now);
        if now < deadline {
            return Ok(());
        }
        let error = InvocationError::DeadlineExpired {
            phase,
            deadline_ns: deadline.as_nanos(),
            observed_ns: now.as_nanos(),
        };
        self.cancel_gate.request();
        self.latch_failure(node, error.clone());
        Err(error)
    }

    fn observe_cancellation(
        &mut self,
        node: Option<usize>,
        phase: &'static str,
    ) -> Result<(), InvocationError> {
        if let Some(error) = self.current_failure(node) {
            return Err(error);
        }
        if !self.cancel_gate.is_requested() {
            return Ok(());
        }
        let error = InvocationError::Cancelled { phase };
        self.latch_failure(node, error.clone());
        Err(error)
    }

    fn observe_terminal(
        &mut self,
        node: Option<usize>,
        phase: &'static str,
    ) -> Result<(), InvocationError> {
        self.observe_deadline(node, phase)?;
        self.observe_cancellation(node, phase)
    }

    /// Seal a terminal immutable receipt. No child authority survives.
    ///
    /// # Errors
    /// Refuses unfinished children or a backing-memory invariant violation.
    pub fn finish(mut self) -> Result<InvocationReceipt, InvocationError> {
        if let Some(state) = self
            .children
            .iter()
            .find(|state| state.disposition.is_none())
        {
            return Err(InvocationError::UnfinishedChild { child: state.id });
        }
        let _ = self.observe_terminal(None, "invocation-finalize");
        let memory = self.backing_memory.receipt();
        let (memory_requested, memory_released) =
            self.children
                .iter()
                .try_fold((0_u64, 0_u64), |(requested, released), state| {
                    Ok::<_, InvocationError>((
                        requested.checked_add(state.memory_requested).ok_or(
                            InvocationError::ArithmeticOverflow {
                                resource: "memory-requested",
                            },
                        )?,
                        released.checked_add(state.memory_released).ok_or(
                            InvocationError::ArithmeticOverflow {
                                resource: "memory-released",
                            },
                        )?,
                    ))
                })?;
        if memory.used_bytes != 0
            || memory.release_invariant_violations != 0
            || memory_requested != memory_released
            || memory_requested != memory.requested_bytes
        {
            return Err(InvocationError::MemoryReleaseInvariant);
        }
        let children = self
            .children
            .iter()
            .map(|state| child_receipt(&self.children, state))
            .collect::<Result<Vec<_>, _>>()?;
        let output_retained = children.iter().try_fold(0_u64, |sum, child| {
            sum.checked_add(child.output_retained)
                .ok_or(InvocationError::ArithmeticOverflow {
                    resource: "output-retained",
                })
        })?;
        let memory_first_refusal =
            memory
                .first_refusal
                .as_ref()
                .map(|refusal| InvocationMemoryRefusal {
                    what: refusal.what,
                    requested: refusal.requested_bytes,
                    used: refusal.used_bytes,
                    limit: refusal.limit_bytes,
                });
        let disposition = self
            .failure
            .as_ref()
            .map_or(InvocationDisposition::Completed, error_disposition);
        let mut receipt = InvocationReceipt {
            version: INVOCATION_RECEIPT_VERSION,
            invocation_id: self.invocation_id,
            limits: self.limits,
            required: self.required,
            remaining: self.remaining,
            children,
            last_deadline_observation: self.last_deadline_observation,
            memory_peak: memory.peak_bytes,
            memory_requested,
            memory_released,
            memory_refusals: memory.refusals,
            memory_first_refusal,
            output_retained,
            failure: self.failure,
            disposition,
            root: ContentHash([0; 32]),
        };
        receipt.root = invocation_receipt_root(&receipt);
        Ok(receipt)
    }
}

/// Non-cloneable affine child authority. `finish` consumes it, returning unused
/// capacities exactly once to its parent.
pub struct ChildBudget<'budget, 'clock> {
    owner: &'budget mut InvocationBudget<'clock>,
    node: usize,
}

impl core::fmt::Debug for ChildBudget<'_, '_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ChildBudget")
            .field("id", &self.owner.children[self.node].id)
            .field("phase", &self.owner.children[self.node].phase)
            .field("remaining", &self.owner.children[self.node].remaining)
            .finish_non_exhaustive()
    }
}

impl<'budget, 'clock> ChildBudget<'budget, 'clock> {
    /// Deterministic child identity.
    #[must_use]
    pub fn id(&self) -> ContentHash {
        self.owner.children[self.node].id
    }

    /// Remaining typed capacity.
    #[must_use]
    pub fn remaining(&self) -> InvocationResources {
        self.owner.children[self.node].remaining
    }

    /// Split a nested affine child from this child's remaining capacity.
    ///
    /// # Errors
    /// Refuses an empty phase or insufficient capacity before mutation.
    pub fn split_child<'child>(
        &'child mut self,
        phase: &'static str,
        grant: InvocationResources,
    ) -> Result<ChildBudget<'child, 'clock>, InvocationError> {
        let node = self.owner.open_child(Some(self.node), phase, grant)?;
        Ok(ChildBudget {
            owner: &mut *self.owner,
            node,
        })
    }

    fn ensure_active(&self) -> Result<(), InvocationError> {
        if self.owner.children[self.node].disposition.is_some() {
            Err(InvocationError::InactiveChild)
        } else if let Some(error) = self.owner.children[self.node].failure.clone() {
            Err(error)
        } else {
            Ok(())
        }
    }

    fn latch(&mut self, error: InvocationError) -> InvocationError {
        self.owner.latch_failure(Some(self.node), error.clone());
        error
    }

    /// Spend declared logical work.
    ///
    /// # Errors
    /// Refuses over-consumption or stale authority.
    pub fn charge_work(&mut self, amount: WorkUnits) -> Result<(), InvocationError> {
        self.ensure_active()?;
        let state = &self.owner.children[self.node];
        let remaining = match state.remaining.work.0.checked_sub(amount.0) {
            Some(remaining) => remaining,
            None => {
                let available = state.remaining.work.0;
                return Err(self.latch(exceeded("work", amount.0, available)));
            }
        };
        let direct = match state.direct_consumed.work.0.checked_add(amount.0) {
            Some(direct) => direct,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow { resource: "work" }));
            }
        };
        let state = &mut self.owner.children[self.node];
        state.remaining.work.0 = remaining;
        state.direct_consumed.work.0 = direct;
        Ok(())
    }

    /// Spend abstract cost.
    ///
    /// # Errors
    /// Refuses over-consumption or stale authority.
    pub fn charge_cost(&mut self, amount: CostUnits) -> Result<(), InvocationError> {
        self.ensure_active()?;
        let state = &self.owner.children[self.node];
        let remaining = match state.remaining.cost.0.checked_sub(amount.0) {
            Some(remaining) => remaining,
            None => {
                let available = state.remaining.cost.0;
                return Err(self.latch(exceeded(
                    "cost",
                    u128::from(amount.0),
                    u128::from(available),
                )));
            }
        };
        let direct = match state.direct_consumed.cost.0.checked_add(amount.0) {
            Some(direct) => direct,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow { resource: "cost" }));
            }
        };
        let state = &mut self.owner.children[self.node];
        state.remaining.cost.0 = remaining;
        state.direct_consumed.cost.0 = direct;
        Ok(())
    }

    /// Spend scientific evaluations.
    ///
    /// # Errors
    /// Refuses over-consumption or stale authority.
    pub fn charge_evaluations(&mut self, amount: EvaluationUnits) -> Result<(), InvocationError> {
        self.ensure_active()?;
        let state = &self.owner.children[self.node];
        let remaining = match state.remaining.evaluations.0.checked_sub(amount.0) {
            Some(remaining) => remaining,
            None => {
                let available = state.remaining.evaluations.0;
                return Err(self.latch(exceeded(
                    "evaluations",
                    u128::from(amount.0),
                    u128::from(available),
                )));
            }
        };
        let direct = match state.direct_consumed.evaluations.0.checked_add(amount.0) {
            Some(direct) => direct,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow {
                    resource: "evaluations",
                }));
            }
        };
        let state = &mut self.owner.children[self.node];
        state.remaining.evaluations.0 = remaining;
        state.direct_consumed.evaluations.0 = direct;
        Ok(())
    }

    /// Check deadline, spend one poll, then observe cancellation in that fixed
    /// order.
    ///
    /// # Errors
    /// Refuses expired deadline, exhausted poll allowance, or cancellation.
    pub fn poll(&mut self, phase: &'static str) -> Result<(), InvocationError> {
        self.ensure_active()?;
        self.owner.observe_deadline(Some(self.node), phase)?;
        let state = &self.owner.children[self.node];
        let Some(remaining) = state.remaining.polls.0.checked_sub(1) else {
            return Err(self.latch(exceeded("polls", 1, 0)));
        };
        let Some(direct) = state.direct_consumed.polls.0.checked_add(1) else {
            return Err(self.latch(InvocationError::ArithmeticOverflow { resource: "polls" }));
        };
        {
            let state = &mut self.owner.children[self.node];
            state.remaining.polls.0 = remaining;
            state.direct_consumed.polls.0 = direct;
        }
        if self.owner.cancel_gate.is_requested() {
            return Err(self.latch(InvocationError::Cancelled { phase }));
        }
        Ok(())
    }

    /// Reserve concurrent memory through both the child sub-cap and the root
    /// operation-memory lease. The returned guard releases on drop/unwind.
    ///
    /// # Errors
    /// Refuses a child-cap or backing-lease overrun before allocation.
    pub fn reserve_memory<'child>(
        &'child mut self,
        what: &'static str,
        bytes: MemoryBytes,
    ) -> Result<InvocationMemoryReservation<'child, 'budget, 'clock>, InvocationError> {
        self.ensure_active()?;
        let state = &self.owner.children[self.node];
        let next = match state.memory_current.checked_add(bytes.0) {
            Some(next) => next,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow {
                    resource: "memory-bytes",
                }));
            }
        };
        let next_requested = match state.memory_requested.checked_add(bytes.0) {
            Some(next) => next,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow {
                    resource: "memory-requested",
                }));
            }
        };
        if next > state.remaining.memory.0 {
            let available = state.remaining.memory.0;
            return Err(self.latch(exceeded(
                "memory-bytes",
                u128::from(next),
                u128::from(available),
            )));
        }
        let mut ancestor = Some(self.node);
        while let Some(index) = ancestor {
            let state = &self.owner.children[index];
            if state.subtree_memory_current.checked_add(bytes.0).is_none() {
                return Err(self.latch(InvocationError::ArithmeticOverflow {
                    resource: "subtree-memory-bytes",
                }));
            }
            ancestor = state.parent;
        }
        let charge = match self.owner.backing_memory.reserve(what, bytes.0) {
            Ok(charge) => charge,
            Err(refusal) => {
                let error = InvocationError::MemoryRefused {
                    what: refusal.what,
                    requested: refusal.requested_bytes,
                    used: refusal.used_bytes,
                    limit: refusal.limit_bytes,
                };
                return Err(self.latch(error));
            }
        };
        let mut ancestor = Some(self.node);
        while let Some(index) = ancestor {
            let state = &mut self.owner.children[index];
            state.subtree_memory_current = state
                .subtree_memory_current
                .checked_add(bytes.0)
                .expect("subtree memory was preflighted");
            state.memory_peak = state.memory_peak.max(state.subtree_memory_current);
            ancestor = state.parent;
        }
        let state = &mut self.owner.children[self.node];
        state.memory_current = next;
        state.direct_memory_peak = state.direct_memory_peak.max(next);
        state.memory_requested = next_requested;
        Ok(InvocationMemoryReservation {
            child: self,
            bytes: bytes.0,
            _charge: charge,
        })
    }

    /// Permanently retain publication capacity.
    ///
    /// # Errors
    /// Refuses output overrun or stale authority.
    pub fn publish_output(&mut self, bytes: OutputBytes) -> Result<(), InvocationError> {
        self.ensure_active()?;
        self.owner
            .observe_terminal(Some(self.node), "child-publication")?;
        let state = &self.owner.children[self.node];
        let remaining = match state.remaining.output.0.checked_sub(bytes.0) {
            Some(remaining) => remaining,
            None => {
                let available = state.remaining.output.0;
                return Err(self.latch(exceeded(
                    "output-bytes",
                    u128::from(bytes.0),
                    u128::from(available),
                )));
            }
        };
        let retained = match state.output_retained.checked_add(bytes.0) {
            Some(retained) => retained,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow {
                    resource: "output-retained",
                }));
            }
        };
        let direct = match state.direct_consumed.output.0.checked_add(bytes.0) {
            Some(direct) => direct,
            None => {
                return Err(self.latch(InvocationError::ArithmeticOverflow {
                    resource: "output-bytes",
                }));
            }
        };
        let state = &mut self.owner.children[self.node];
        state.remaining.output.0 = remaining;
        state.output_retained = retained;
        state.direct_consumed.output.0 = direct;
        Ok(())
    }

    /// Latch a structured scientific refusal so terminal receipts cannot
    /// misrepresent a domain error as successful completion.
    pub fn refuse(&mut self, phase: &'static str, reason: ContentHash) -> InvocationError {
        self.latch(InvocationError::ExplicitRefusal { phase, reason })
    }

    /// Return unused authority exactly once and retain terminal disposition.
    ///
    /// # Errors
    /// Refuses live nested children or live memory reservations.
    pub fn finish(self) -> Result<InvocationDisposition, InvocationError> {
        let _ = self
            .owner
            .observe_terminal(Some(self.node), "child-finalize");
        self.owner.close_child(self.node)
    }
}

/// Small object-safe poll seam for lower-layer progress engines.
pub trait InvocationPoll {
    /// Observe deadline/cancellation while consuming one affine poll.
    fn invocation_poll(&mut self, phase: &'static str) -> Result<(), InvocationError>;

    /// Remaining poll opportunities.
    fn invocation_polls_remaining(&self) -> PollUnits;
}

impl InvocationPoll for ChildBudget<'_, '_> {
    fn invocation_poll(&mut self, phase: &'static str) -> Result<(), InvocationError> {
        self.poll(phase)
    }

    fn invocation_polls_remaining(&self) -> PollUnits {
        self.remaining().polls()
    }
}

/// RAII memory reservation. Scientific code continues spending through
/// [`Self::budget`] while the allocation charge remains live.
pub struct InvocationMemoryReservation<'child, 'budget, 'clock> {
    child: &'child mut ChildBudget<'budget, 'clock>,
    bytes: u64,
    _charge: LeaseCharge,
}

impl<'budget, 'clock> InvocationMemoryReservation<'_, 'budget, 'clock> {
    /// Continue using the same child authority while this memory is live.
    pub fn budget(&mut self) -> &mut ChildBudget<'budget, 'clock> {
        self.child
    }

    /// Reserved bytes.
    #[must_use]
    pub const fn bytes(&self) -> MemoryBytes {
        MemoryBytes(self.bytes)
    }
}

impl Drop for InvocationMemoryReservation<'_, '_, '_> {
    fn drop(&mut self) {
        let node = self.child.node;
        let mut violation = false;
        {
            let state = &mut self.child.owner.children[node];
            match (
                state.memory_current.checked_sub(self.bytes),
                state.memory_released.checked_add(self.bytes),
            ) {
                (Some(current), Some(released)) => {
                    state.memory_current = current;
                    state.memory_released = released;
                }
                _ => {
                    state.memory_current = u64::MAX;
                    state.memory_released = u64::MAX;
                    violation = true;
                }
            }
        }
        let mut ancestor = Some(node);
        while let Some(index) = ancestor {
            let state = &mut self.child.owner.children[index];
            match state.subtree_memory_current.checked_sub(self.bytes) {
                Some(current) => state.subtree_memory_current = current,
                None => {
                    state.subtree_memory_current = u64::MAX;
                    violation = true;
                }
            }
            ancestor = state.parent;
        }
        if violation {
            self.child
                .owner
                .latch_failure(Some(node), InvocationError::MemoryReleaseInvariant);
        }
    }
}

fn child_receipt(
    states: &[ChildState],
    state: &ChildState,
) -> Result<ChildReceipt, InvocationError> {
    let consumed = state.granted.checked_sub(state.remaining)?;
    let parent = state.parent.map(|index| states[index].id);
    let mut receipt = ChildReceipt {
        id: state.id,
        parent,
        ordinal: state.ordinal,
        phase: state.phase,
        granted: state.granted,
        consumed,
        direct_consumed: state.direct_consumed,
        returned: state.remaining,
        direct_memory_peak: state.direct_memory_peak,
        memory_peak: state.memory_peak,
        memory_requested: state.memory_requested,
        memory_released: state.memory_released,
        output_retained: state.output_retained,
        failure: state.failure.clone(),
        disposition: state.disposition.ok_or(InvocationError::InactiveChild)?,
        root: ContentHash([0; 32]),
    };
    receipt.root = child_receipt_root(&receipt);
    Ok(receipt)
}

fn child_id(
    invocation: ContentHash,
    parent: Option<ContentHash>,
    ordinal: u64,
    phase: &str,
) -> ContentHash {
    let mut bytes = Vec::new();
    field(&mut bytes, "invocation", invocation.as_bytes());
    field(&mut bytes, "parent-present", &[u8::from(parent.is_some())]);
    if let Some(parent) = parent {
        field(&mut bytes, "parent", parent.as_bytes());
    }
    field(&mut bytes, "ordinal", &ordinal.to_le_bytes());
    field(&mut bytes, "phase", phase.as_bytes());
    hash_domain(CHILD_ID_DOMAIN, &bytes)
}

fn field(bytes: &mut Vec<u8>, label: &str, value: &[u8]) {
    bytes.extend_from_slice(&(label.len() as u64).to_le_bytes());
    bytes.extend_from_slice(label.as_bytes());
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value);
}

fn encode_resources(bytes: &mut Vec<u8>, prefix: &str, resources: InvocationResources) {
    field(
        bytes,
        &format!("{prefix}.work"),
        &resources.work.0.to_le_bytes(),
    );
    field(
        bytes,
        &format!("{prefix}.polls"),
        &resources.polls.0.to_le_bytes(),
    );
    field(
        bytes,
        &format!("{prefix}.cost"),
        &resources.cost.0.to_le_bytes(),
    );
    field(
        bytes,
        &format!("{prefix}.evaluations"),
        &resources.evaluations.0.to_le_bytes(),
    );
    field(
        bytes,
        &format!("{prefix}.memory"),
        &resources.memory.0.to_le_bytes(),
    );
    field(
        bytes,
        &format!("{prefix}.output"),
        &resources.output.0.to_le_bytes(),
    );
}

fn encode_disposition(disposition: InvocationDisposition) -> u8 {
    match disposition {
        InvocationDisposition::Completed => 0,
        InvocationDisposition::Cancelled => 1,
        InvocationDisposition::Refused => 2,
    }
}

#[allow(clippy::too_many_lines)]
fn encode_error(bytes: &mut Vec<u8>, prefix: &str, error: &InvocationError) {
    let tag = match error {
        InvocationError::ResourceExceeded { .. } => 0,
        InvocationError::ArithmeticOverflow { .. } => 1,
        InvocationError::DeadlineExpired { .. } => 2,
        InvocationError::Cancelled { .. } => 3,
        InvocationError::MemoryRefused { .. } => 4,
        InvocationError::ExplicitRefusal { .. } => 5,
        InvocationError::InactiveChild => 6,
        InvocationError::LiveNestedChildren { .. } => 7,
        InvocationError::LiveMemoryReservations { .. } => 8,
        InvocationError::UnfinishedChild { .. } => 9,
        InvocationError::MemoryReleaseInvariant => 10,
        InvocationError::EmptyPhase => 11,
    };
    field(bytes, &format!("{prefix}.tag"), &[tag]);
    match error {
        InvocationError::ResourceExceeded {
            resource,
            requested,
            available,
        } => {
            field(bytes, &format!("{prefix}.resource"), resource.as_bytes());
            field(
                bytes,
                &format!("{prefix}.requested"),
                &requested.to_le_bytes(),
            );
            field(
                bytes,
                &format!("{prefix}.available"),
                &available.to_le_bytes(),
            );
        }
        InvocationError::ArithmeticOverflow { resource } => {
            field(bytes, &format!("{prefix}.resource"), resource.as_bytes());
        }
        InvocationError::DeadlineExpired {
            phase,
            deadline_ns,
            observed_ns,
        } => {
            field(bytes, &format!("{prefix}.phase"), phase.as_bytes());
            field(
                bytes,
                &format!("{prefix}.deadline-nanos"),
                &deadline_ns.to_le_bytes(),
            );
            field(
                bytes,
                &format!("{prefix}.observed-nanos"),
                &observed_ns.to_le_bytes(),
            );
        }
        InvocationError::Cancelled { phase } => {
            field(bytes, &format!("{prefix}.phase"), phase.as_bytes());
        }
        InvocationError::MemoryRefused {
            what,
            requested,
            used,
            limit,
        } => {
            field(bytes, &format!("{prefix}.what"), what.as_bytes());
            field(
                bytes,
                &format!("{prefix}.requested"),
                &requested.to_le_bytes(),
            );
            field(bytes, &format!("{prefix}.used"), &used.to_le_bytes());
            field(bytes, &format!("{prefix}.limit"), &limit.to_le_bytes());
        }
        InvocationError::ExplicitRefusal { phase, reason } => {
            field(bytes, &format!("{prefix}.phase"), phase.as_bytes());
            field(bytes, &format!("{prefix}.reason"), reason.as_bytes());
        }
        InvocationError::LiveNestedChildren { count } => {
            field(bytes, &format!("{prefix}.count"), &count.to_le_bytes());
        }
        InvocationError::LiveMemoryReservations { bytes: live } => {
            field(bytes, &format!("{prefix}.bytes"), &live.to_le_bytes());
        }
        InvocationError::UnfinishedChild { child } => {
            field(bytes, &format!("{prefix}.child"), child.as_bytes());
        }
        InvocationError::EmptyPhase
        | InvocationError::InactiveChild
        | InvocationError::MemoryReleaseInvariant => {}
    }
}

fn child_receipt_root(receipt: &ChildReceipt) -> ContentHash {
    let mut bytes = Vec::new();
    field(&mut bytes, "id", receipt.id.as_bytes());
    field(
        &mut bytes,
        "parent-present",
        &[u8::from(receipt.parent.is_some())],
    );
    if let Some(parent) = receipt.parent {
        field(&mut bytes, "parent", parent.as_bytes());
    }
    field(&mut bytes, "ordinal", &receipt.ordinal.to_le_bytes());
    field(&mut bytes, "phase", receipt.phase.as_bytes());
    encode_resources(&mut bytes, "granted", receipt.granted);
    encode_resources(&mut bytes, "consumed", receipt.consumed);
    encode_resources(&mut bytes, "direct-consumed", receipt.direct_consumed);
    encode_resources(&mut bytes, "returned", receipt.returned);
    field(
        &mut bytes,
        "direct-memory-peak",
        &receipt.direct_memory_peak.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-peak",
        &receipt.memory_peak.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-requested",
        &receipt.memory_requested.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-released",
        &receipt.memory_released.to_le_bytes(),
    );
    field(
        &mut bytes,
        "output-retained",
        &receipt.output_retained.to_le_bytes(),
    );
    field(
        &mut bytes,
        "failure-present",
        &[u8::from(receipt.failure.is_some())],
    );
    if let Some(failure) = &receipt.failure {
        encode_error(&mut bytes, "failure", failure);
    }
    field(
        &mut bytes,
        "disposition",
        &[encode_disposition(receipt.disposition)],
    );
    hash_domain(CHILD_RECEIPT_DOMAIN, &bytes)
}

fn child_semantic_error(child: &ChildReceipt, invariant: &'static str) -> ReceiptSemanticError {
    ReceiptSemanticError::Child {
        ordinal: child.ordinal,
        invariant,
    }
}

fn invocation_semantic_error(invariant: &'static str) -> ReceiptSemanticError {
    ReceiptSemanticError::Invocation { invariant }
}

fn failure_evidence_is_valid(error: &InvocationError) -> bool {
    match error {
        InvocationError::ResourceExceeded {
            resource,
            requested,
            available,
        } => {
            matches!(
                *resource,
                "work" | "polls" | "cost" | "evaluations" | "memory-bytes" | "output-bytes"
            ) && requested > available
        }
        InvocationError::DeadlineExpired {
            deadline_ns,
            observed_ns,
            ..
        } => observed_ns >= deadline_ns,
        InvocationError::MemoryRefused {
            requested,
            used,
            limit,
            ..
        } => {
            *requested != 0
                && used <= limit
                && used
                    .checked_add(*requested)
                    .is_none_or(|total| total > *limit)
        }
        InvocationError::ArithmeticOverflow { resource } => matches!(
            *resource,
            "work"
                | "polls"
                | "cost"
                | "evaluations"
                | "memory-bytes"
                | "output-bytes"
                | "child-ordinal"
                | "live-children"
                | "memory-requested"
                | "memory-released"
                | "subtree-memory-bytes"
                | "output-retained"
        ),
        InvocationError::EmptyPhase
        | InvocationError::InactiveChild
        | InvocationError::LiveNestedChildren { .. }
        | InvocationError::LiveMemoryReservations { .. }
        | InvocationError::UnfinishedChild { .. }
        | InvocationError::MemoryReleaseInvariant => false,
        InvocationError::Cancelled { .. } | InvocationError::ExplicitRefusal { .. } => true,
    }
}

fn memory_refusal_matches_failure(
    refusal: &InvocationMemoryRefusal,
    failure: &InvocationError,
) -> bool {
    matches!(
        failure,
        InvocationError::MemoryRefused {
            what,
            requested,
            used,
            limit,
        } if *what == refusal.what
            && *requested == refusal.requested
            && *used == refusal.used
            && *limit == refusal.limit
    )
}

fn verify_deadline_semantics(receipt: &InvocationReceipt) -> Result<(), ReceiptSemanticError> {
    match (receipt.limits.deadline, receipt.last_deadline_observation) {
        (None, None) => {
            if matches!(
                &receipt.failure,
                Some(InvocationError::DeadlineExpired { .. })
            ) {
                return Err(invocation_semantic_error("deadline-without-limit"));
            }
        }
        (Some(deadline), Some(observed)) => {
            if let Some(InvocationError::DeadlineExpired {
                deadline_ns,
                observed_ns,
                ..
            }) = &receipt.failure
            {
                if *deadline_ns != deadline.as_nanos() || *observed_ns != observed.as_nanos() {
                    return Err(invocation_semantic_error("deadline-failure-observation"));
                }
            } else if observed >= deadline {
                return Err(invocation_semantic_error(
                    "nondeadline-observation-before-limit",
                ));
            }
        }
        _ => return Err(invocation_semantic_error("deadline-observation-presence")),
    }
    Ok(())
}

fn verify_failure_propagation(receipt: &InvocationReceipt) -> Result<(), ReceiptSemanticError> {
    if receipt
        .children
        .iter()
        .filter(|child| child.parent.is_none() && child.failure.is_some())
        .count()
        > 1
    {
        return Err(invocation_semantic_error("single-failure-origin"));
    }
    for child in &receipt.children {
        let Some(failure) = &child.failure else {
            continue;
        };
        if receipt.failure.as_ref() != Some(failure) {
            return Err(child_semantic_error(child, "failure-propagates-to-root"));
        }
        if let Some(parent) = child.parent {
            let parent = receipt
                .children
                .iter()
                .find(|candidate| candidate.id == parent)
                .ok_or_else(|| child_semantic_error(child, "failure-parent-exists"))?;
            if parent.failure.as_ref() != Some(failure) {
                return Err(child_semantic_error(child, "failure-propagates-to-parent"));
            }
        }
        if receipt
            .children
            .iter()
            .filter(|candidate| candidate.parent == Some(child.id) && candidate.failure.is_some())
            .count()
            > 1
        {
            return Err(child_semantic_error(child, "single-nested-failure-origin"));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn verify_receipt_semantics(receipt: &InvocationReceipt) -> Result<(), ReceiptSemanticError> {
    if receipt.version != INVOCATION_RECEIPT_VERSION {
        return Err(ReceiptSemanticError::UnsupportedVersion {
            found: receipt.version,
        });
    }
    if invocation_receipt_root(receipt) != receipt.root {
        return Err(ReceiptSemanticError::RootMismatch);
    }
    receipt
        .limits
        .resources
        .checked_sub(receipt.required)
        .map_err(|_| invocation_semantic_error("required-within-limits"))?;
    verify_deadline_semantics(receipt)?;

    for (index, child) in receipt.children.iter().enumerate() {
        let ordinal = u64::try_from(index)
            .map_err(|_| child_semantic_error(child, "ordinal-representable"))?;
        if child.ordinal != ordinal {
            return Err(child_semantic_error(child, "ordinal-order"));
        }
        if child.phase.is_empty() {
            return Err(child_semantic_error(child, "non-empty-phase"));
        }
        if receipt.children[..index]
            .iter()
            .any(|earlier| earlier.id == child.id)
        {
            return Err(child_semantic_error(child, "unique-id"));
        }
        let parent = match child.parent {
            Some(parent_id) => {
                if !receipt.children[..index]
                    .iter()
                    .any(|candidate| candidate.id == parent_id)
                {
                    return Err(child_semantic_error(child, "parent-precedes-child"));
                }
                Some(parent_id)
            }
            None => None,
        };
        if child_id(receipt.invocation_id, parent, child.ordinal, child.phase) != child.id {
            return Err(child_semantic_error(child, "derived-id"));
        }
        if child_receipt_root(child) != child.root {
            return Err(child_semantic_error(child, "receipt-root"));
        }
        let consumed = child
            .granted
            .checked_sub(child.returned)
            .map_err(|_| child_semantic_error(child, "granted-returned"))?;
        if consumed != child.consumed {
            return Err(child_semantic_error(child, "consumed-definition"));
        }
        let mut nested_consumed = InvocationResources::default();
        for nested in receipt
            .children
            .iter()
            .filter(|candidate| candidate.parent == Some(child.id))
        {
            nested_consumed = nested_consumed
                .checked_add(nested.consumed)
                .map_err(|_| child_semantic_error(child, "nested-consumption-sum"))?;
        }
        let expected_consumed = child
            .direct_consumed
            .checked_add(nested_consumed)
            .map_err(|_| child_semantic_error(child, "direct-plus-nested-consumption"))?;
        if expected_consumed != child.consumed {
            return Err(child_semantic_error(child, "subtree-conservation"));
        }
        let mut replay = child.granted;
        for nested in receipt
            .children
            .iter()
            .filter(|candidate| candidate.parent == Some(child.id))
        {
            replay = replay
                .checked_sub(nested.granted)
                .and_then(|available| available.checked_add(nested.returned))
                .map_err(|_| child_semantic_error(child, "nested-affine-transfer"))?;
        }
        replay = replay
            .checked_sub(child.direct_consumed)
            .map_err(|_| child_semantic_error(child, "direct-affine-spend"))?;
        if replay != child.returned {
            return Err(child_semantic_error(child, "returned-conservation"));
        }
        if child.direct_consumed.memory != MemoryBytes::new(0)
            || child.consumed.memory != MemoryBytes::new(0)
            || child.returned.memory != child.granted.memory
        {
            return Err(child_semantic_error(child, "memory-is-reusable-capacity"));
        }
        let descendant_memory_peak = receipt
            .children
            .iter()
            .filter(|candidate| candidate.parent == Some(child.id))
            .map(|candidate| candidate.memory_peak)
            .max()
            .unwrap_or(0);
        let minimum_memory_peak = child.direct_memory_peak.max(descendant_memory_peak);
        let maximum_memory_peak = child
            .direct_memory_peak
            .checked_add(descendant_memory_peak)
            .ok_or_else(|| child_semantic_error(child, "memory-peak-bound"))?;
        if child.memory_requested != child.memory_released
            || child.memory_peak < minimum_memory_peak
            || child.memory_peak > maximum_memory_peak
            || child.memory_peak > child.granted.memory.0
            || child.direct_memory_peak > child.memory_requested
            || (child.memory_requested == 0) != (child.direct_memory_peak == 0)
        {
            return Err(child_semantic_error(child, "memory-receipt"));
        }
        if child.direct_consumed.output.0 != child.output_retained {
            return Err(child_semantic_error(child, "direct-output-retention"));
        }
        let expected_disposition = child
            .failure
            .as_ref()
            .map_or(InvocationDisposition::Completed, error_disposition);
        if child.disposition != expected_disposition {
            return Err(child_semantic_error(child, "derived-disposition"));
        }
        if child
            .failure
            .as_ref()
            .is_some_and(|failure| !failure_evidence_is_valid(failure))
        {
            return Err(child_semantic_error(child, "failure-evidence"));
        }
    }

    let mut replay = receipt.required;
    for child in receipt
        .children
        .iter()
        .filter(|candidate| candidate.parent.is_none())
    {
        replay = replay
            .checked_sub(child.granted)
            .and_then(|available| available.checked_add(child.returned))
            .map_err(|_| invocation_semantic_error("root-affine-transfer"))?;
    }
    if replay != receipt.remaining {
        return Err(invocation_semantic_error("root-conservation"));
    }

    let (memory_requested, memory_released, output_retained) = receipt.children.iter().try_fold(
        (0_u64, 0_u64, 0_u64),
        |(requested, released, output), child| {
            Ok::<_, ReceiptSemanticError>((
                requested
                    .checked_add(child.memory_requested)
                    .ok_or_else(|| invocation_semantic_error("memory-requested-sum"))?,
                released
                    .checked_add(child.memory_released)
                    .ok_or_else(|| invocation_semantic_error("memory-released-sum"))?,
                output
                    .checked_add(child.output_retained)
                    .ok_or_else(|| invocation_semantic_error("output-retained-sum"))?,
            ))
        },
    )?;
    let memory_peak = receipt
        .children
        .iter()
        .filter(|child| child.parent.is_none())
        .map(|child| child.memory_peak)
        .max()
        .unwrap_or(0);
    if memory_requested != receipt.memory_requested
        || memory_released != receipt.memory_released
        || receipt.memory_requested != receipt.memory_released
        || memory_peak != receipt.memory_peak
        || receipt.memory_peak > receipt.required.memory.0
        || receipt.remaining.memory != receipt.required.memory
    {
        return Err(invocation_semantic_error("root-memory-receipt"));
    }
    if output_retained != receipt.output_retained
        || receipt
            .required
            .output
            .0
            .checked_sub(receipt.remaining.output.0)
            != Some(receipt.output_retained)
    {
        return Err(invocation_semantic_error("root-output-receipt"));
    }
    if receipt.memory_refusals > 1
        || (receipt.memory_refusals == 0) != receipt.memory_first_refusal.is_none()
    {
        return Err(invocation_semantic_error("memory-refusal-evidence"));
    }
    match (&receipt.memory_first_refusal, &receipt.failure) {
        (Some(refusal), Some(failure))
            if memory_refusal_matches_failure(refusal, failure)
                && refusal.limit == receipt.required.memory.0
                && refusal.used <= receipt.memory_peak
                && receipt
                    .children
                    .iter()
                    .any(|child| child.failure.as_ref() == Some(failure)) => {}
        (None, Some(InvocationError::MemoryRefused { .. })) | (Some(_), _) => {
            return Err(invocation_semantic_error("memory-refusal-first-fault"));
        }
        _ => {}
    }
    let expected_disposition = receipt
        .failure
        .as_ref()
        .map_or(InvocationDisposition::Completed, error_disposition);
    if receipt.disposition != expected_disposition {
        return Err(invocation_semantic_error("root-derived-disposition"));
    }
    if receipt
        .children
        .iter()
        .any(|child| child.disposition != InvocationDisposition::Completed)
        && receipt.failure.is_none()
    {
        return Err(invocation_semantic_error("child-failure-propagates"));
    }
    if receipt
        .failure
        .as_ref()
        .is_some_and(|failure| !failure_evidence_is_valid(failure))
    {
        return Err(invocation_semantic_error("root-failure-evidence"));
    }
    verify_failure_propagation(receipt)?;
    Ok(())
}

fn invocation_receipt_root(receipt: &InvocationReceipt) -> ContentHash {
    let mut bytes = Vec::new();
    field(&mut bytes, "version", &receipt.version.to_le_bytes());
    field(
        &mut bytes,
        "invocation-id",
        receipt.invocation_id.as_bytes(),
    );
    encode_resources(&mut bytes, "limits", receipt.limits.resources);
    field(
        &mut bytes,
        "deadline-present",
        &[u8::from(receipt.limits.deadline.is_some())],
    );
    if let Some(deadline) = receipt.limits.deadline {
        field(
            &mut bytes,
            "deadline-nanos",
            &deadline.as_nanos().to_le_bytes(),
        );
    }
    field(
        &mut bytes,
        "accuracy-obligation",
        receipt.limits.accuracy_obligation.as_bytes(),
    );
    field(
        &mut bytes,
        "capability-scope",
        receipt.limits.capability_scope.as_bytes(),
    );
    encode_resources(&mut bytes, "required", receipt.required);
    encode_resources(&mut bytes, "remaining", receipt.remaining);
    field(
        &mut bytes,
        "last-deadline-observation-present",
        &[u8::from(receipt.last_deadline_observation.is_some())],
    );
    if let Some(observed) = receipt.last_deadline_observation {
        field(
            &mut bytes,
            "last-deadline-observation-nanos",
            &observed.as_nanos().to_le_bytes(),
        );
    }
    field(
        &mut bytes,
        "memory-peak",
        &receipt.memory_peak.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-requested",
        &receipt.memory_requested.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-released",
        &receipt.memory_released.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-refusals",
        &receipt.memory_refusals.to_le_bytes(),
    );
    field(
        &mut bytes,
        "memory-first-refusal-present",
        &[u8::from(receipt.memory_first_refusal.is_some())],
    );
    if let Some(refusal) = &receipt.memory_first_refusal {
        field(
            &mut bytes,
            "memory-first-refusal.what",
            refusal.what.as_bytes(),
        );
        field(
            &mut bytes,
            "memory-first-refusal.requested",
            &refusal.requested.to_le_bytes(),
        );
        field(
            &mut bytes,
            "memory-first-refusal.used",
            &refusal.used.to_le_bytes(),
        );
        field(
            &mut bytes,
            "memory-first-refusal.limit",
            &refusal.limit.to_le_bytes(),
        );
    }
    field(
        &mut bytes,
        "output-retained",
        &receipt.output_retained.to_le_bytes(),
    );
    field(
        &mut bytes,
        "failure-present",
        &[u8::from(receipt.failure.is_some())],
    );
    if let Some(failure) = &receipt.failure {
        encode_error(&mut bytes, "failure", failure);
    }
    field(
        &mut bytes,
        "disposition",
        &[encode_disposition(receipt.disposition)],
    );
    field(
        &mut bytes,
        "child-count",
        &(receipt.children.len() as u64).to_le_bytes(),
    );
    for (index, child) in receipt.children.iter().enumerate() {
        field(
            &mut bytes,
            &format!("child.{index}.id"),
            child.id.as_bytes(),
        );
        field(
            &mut bytes,
            &format!("child.{index}.root"),
            child.root.as_bytes(),
        );
    }
    hash_domain(INVOCATION_RECEIPT_DOMAIN, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, CancelGate, ExecMode, StreamKey};
    use fs_alloc::{ArenaConfig, ArenaPool};

    fn resources(value: u64) -> InvocationResources {
        InvocationResources::new(
            WorkUnits::new(u128::from(value)),
            PollUnits::new(value as u32),
            CostUnits::new(value),
            EvaluationUnits::new(value),
            MemoryBytes::new(value),
            OutputBytes::new(value),
        )
    }

    fn resource_vector(values: [u64; 6]) -> InvocationResources {
        let [work, polls, cost, evaluations, memory, output] = values;
        InvocationResources::new(
            WorkUnits::new(u128::from(work)),
            PollUnits::new(u32::try_from(polls).expect("test poll value fits u32")),
            CostUnits::new(cost),
            EvaluationUnits::new(evaluations),
            MemoryBytes::new(memory),
            OutputBytes::new(output),
        )
    }

    fn identities() -> (ContentHash, ContentHash, ContentHash) {
        (
            hash_domain("test.invocation", b"id"),
            hash_domain("test.accuracy", b"obligation"),
            hash_domain("test.capability", b"scope"),
        )
    }

    fn with_cx<R>(f: impl FnOnce(&Cx<'_>) -> R) -> R {
        let gate = CancelGate::new_clock_free();
        let pool = ArenaPool::new(ArenaConfig::default());
        pool.scope(|arena| {
            let cx = Cx::new(
                &gate,
                arena,
                StreamKey {
                    seed: 1,
                    kernel_id: 2,
                    tile: 3,
                    iteration: 4,
                },
                Budget::INFINITE,
                ExecMode::Deterministic,
            );
            f(&cx)
        })
    }

    fn with_gate_cx<R>(f: impl FnOnce(&CancelGate, &Cx<'_>) -> R) -> R {
        let gate = CancelGate::new_clock_free();
        let pool = ArenaPool::new(ArenaConfig::default());
        pool.scope(|arena| {
            let cx = Cx::new(
                &gate,
                arena,
                StreamKey {
                    seed: 1,
                    kernel_id: 2,
                    tile: 3,
                    iteration: 4,
                },
                Budget::INFINITE,
                ExecMode::Deterministic,
            );
            f(&gate, &cx)
        })
    }

    fn with_leased_cx<R>(lease: &OperationMemoryLease, f: impl FnOnce(&Cx<'_>) -> R) -> R {
        let gate = CancelGate::new_clock_free();
        let refusals = crate::cx::RefusalSink::default();
        let pool = ArenaPool::new(ArenaConfig::default());
        pool.scope(|arena| {
            let cx = Cx::new_with_refusal_sink(
                &gate,
                arena,
                StreamKey {
                    seed: 1,
                    kernel_id: 2,
                    tile: 3,
                    iteration: 4,
                },
                Budget::INFINITE,
                ExecMode::Deterministic,
                &refusals,
                lease,
            );
            f(&cx)
        })
    }

    #[test]
    fn affine_children_conserve_each_dimension_and_memory_releases() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let limits = InvocationLimits::new(resources(20), None, accuracy, capability);
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(id, limits, resources(10))
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            {
                let mut child = root.split_child("phase-a", resources(10)).unwrap();
                child.charge_work(WorkUnits::new(7)).unwrap();
                child.charge_cost(CostUnits::new(5)).unwrap();
                child.charge_evaluations(EvaluationUnits::new(2)).unwrap();
                child.poll("phase-a.poll").unwrap();
                {
                    let mut memory = child
                        .reserve_memory("invocation-test", MemoryBytes::new(8))
                        .unwrap();
                    memory.budget().publish_output(OutputBytes::new(3)).unwrap();
                }
                assert_eq!(child.finish().unwrap(), InvocationDisposition::Completed);
            }
            root.finish().unwrap()
        });
        assert!(receipt.verifies_integrity());
        assert_eq!(receipt.children().len(), 1);
        assert_eq!(receipt.children()[0].consumed().work(), WorkUnits::new(7));
        assert_eq!(receipt.children()[0].memory_peak_bytes(), 8);
        assert_eq!(receipt.output_retained_bytes(), 3);
    }

    #[test]
    fn admission_and_deadline_refusals_are_exact_and_ordered() {
        let clock = VirtualClock::starting_at(Time::from_nanos(5));
        let (id, accuracy, capability) = identities();
        let limits = InvocationLimits::new(
            resources(9),
            Some(Time::from_nanos(10)),
            accuracy,
            capability,
        );
        assert!(matches!(
            InvocationAdmitter::new().admit(id, limits, resources(10)),
            Err(InvocationError::ResourceExceeded {
                resource: "work",
                requested: 10,
                available: 9
            })
        ));
        let expired = InvocationLimits::new(
            resources(10),
            Some(Time::from_nanos(5)),
            accuracy,
            capability,
        );
        with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(id, expired, resources(10))
                .unwrap();
            assert!(matches!(
                admission.begin(cx, &clock),
                Err(InvocationError::DeadlineExpired {
                    phase: "invocation-admission",
                    deadline_ns: 5,
                    observed_ns: 5
                })
            ));
        });
    }

    #[test]
    fn admission_refuses_one_below_in_every_resource_dimension() {
        let (id, accuracy, capability) = identities();
        let required = resources(10);
        let one_below = [
            (resource_vector([9, 10, 10, 10, 10, 10]), "work"),
            (resource_vector([10, 9, 10, 10, 10, 10]), "polls"),
            (resource_vector([10, 10, 9, 10, 10, 10]), "cost"),
            (resource_vector([10, 10, 10, 9, 10, 10]), "evaluations"),
            (resource_vector([10, 10, 10, 10, 9, 10]), "memory-bytes"),
            (resource_vector([10, 10, 10, 10, 10, 9]), "output-bytes"),
        ];
        for (available, resource) in one_below {
            let limits = InvocationLimits::new(available, None, accuracy, capability);
            assert!(matches!(
                InvocationAdmitter::new().admit(id, limits, required),
                Err(InvocationError::ResourceExceeded {
                    resource: observed,
                    requested: 10,
                    available: 9,
                }) if observed == resource
            ));
        }
    }

    #[test]
    fn child_runtime_refuses_overrun_in_every_resource_dimension() {
        for resource in [
            "work",
            "polls",
            "cost",
            "evaluations",
            "memory-bytes",
            "output-bytes",
        ] {
            let clock = VirtualClock::new();
            let (id, accuracy, capability) = identities();
            let receipt = with_cx(|cx| {
                let admission = InvocationAdmitter::new()
                    .admit(
                        id,
                        InvocationLimits::new(resources(4), None, accuracy, capability),
                        resources(4),
                    )
                    .unwrap();
                let mut root = admission.begin(cx, &clock).unwrap();
                let mut child = root.split_child("overrun", resources(4)).unwrap();
                let failure = match resource {
                    "work" => child.charge_work(WorkUnits::new(5)),
                    "polls" => {
                        let mut result = Ok(());
                        for _ in 0..5 {
                            result = child.poll("overrun.poll");
                            if result.is_err() {
                                break;
                            }
                        }
                        result
                    }
                    "cost" => child.charge_cost(CostUnits::new(5)),
                    "evaluations" => child.charge_evaluations(EvaluationUnits::new(5)),
                    "memory-bytes" => child
                        .reserve_memory("overrun-memory", MemoryBytes::new(5))
                        .map(drop),
                    "output-bytes" => child.publish_output(OutputBytes::new(5)),
                    _ => unreachable!(),
                };
                assert!(matches!(
                    failure,
                    Err(InvocationError::ResourceExceeded {
                        resource: observed,
                        requested,
                        available,
                    }) if observed == resource && requested > available
                ));
                assert_eq!(child.finish().unwrap(), InvocationDisposition::Refused);
                root.finish().unwrap()
            });
            assert_eq!(receipt.disposition(), InvocationDisposition::Refused);
            assert!(receipt.verifies_integrity());
        }
    }

    #[test]
    fn empty_child_phase_is_rejected_before_identity_or_ordinal_mutation() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(4), None, accuracy, capability),
                    resources(4),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            assert!(matches!(
                root.split_child("", resources(4)),
                Err(InvocationError::EmptyPhase)
            ));
            let child = root.split_child("valid", resources(4)).unwrap();
            assert_eq!(child.finish().unwrap(), InvocationDisposition::Completed);
            root.finish().unwrap()
        });
        assert_eq!(receipt.children().len(), 1);
        assert_eq!(receipt.children()[0].ordinal(), 0);
        assert_eq!(receipt.children()[0].phase(), "valid");
        assert!(receipt.verifies_integrity());
    }

    #[test]
    fn root_memory_is_reserved_once_against_the_ambient_operation_lease() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let occupied_lease = OperationMemoryLease::bounded(10);
        let occupied = occupied_lease.reserve("existing-operation", 4).unwrap();
        with_leased_cx(&occupied_lease, |cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(10), None, accuracy, capability),
                    resources(8),
                )
                .unwrap();
            assert!(matches!(
                admission.begin(cx, &clock),
                Err(InvocationError::MemoryRefused {
                    what: "invocation-root-memory",
                    requested: 8,
                    used: 4,
                    limit: 10,
                })
            ));
        });
        drop(occupied);

        let admitted_lease = OperationMemoryLease::bounded(10);
        with_leased_cx(&admitted_lease, |cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(10), None, accuracy, capability),
                    resources(8),
                )
                .unwrap();
            let root = admission.begin(cx, &clock).unwrap();
            assert_eq!(admitted_lease.receipt().used_bytes, 8);
            let receipt = root.finish().unwrap();
            assert!(receipt.verifies_integrity());
            assert_eq!(admitted_lease.receipt().used_bytes, 0);
        });
    }

    #[test]
    fn nested_child_ids_and_receipts_replay_deterministically() {
        fn run() -> InvocationReceipt {
            let clock = VirtualClock::new();
            let (id, accuracy, capability) = identities();
            let limits = InvocationLimits::new(resources(12), None, accuracy, capability);
            with_cx(|cx| {
                let admission = InvocationAdmitter::new()
                    .admit(id, limits, resources(12))
                    .unwrap();
                let mut root = admission.begin(cx, &clock).unwrap();
                {
                    let mut parent = root.split_child("parent", resources(12)).unwrap();
                    {
                        let nested = parent.split_child("nested", resources(4)).unwrap();
                        assert_eq!(nested.finish().unwrap(), InvocationDisposition::Completed);
                    }
                    assert_eq!(parent.finish().unwrap(), InvocationDisposition::Completed);
                }
                root.finish().unwrap()
            })
        }
        let first = run();
        let second = run();
        assert_eq!(first, second);
        assert!(first.verifies_integrity());
    }

    #[test]
    fn first_fault_latches_and_derives_refused_receipts() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(10), None, accuracy, capability),
                    resources(10),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            let mut child = root.split_child("overrun", resources(10)).unwrap();
            assert!(matches!(
                child.charge_work(WorkUnits::new(11)),
                Err(InvocationError::ResourceExceeded {
                    resource: "work",
                    requested: 11,
                    available: 10,
                })
            ));
            assert_eq!(child.finish().unwrap(), InvocationDisposition::Refused);
            root.finish().unwrap()
        });
        assert_eq!(receipt.disposition(), InvocationDisposition::Refused);
        assert!(receipt.failure().is_some());
        assert!(receipt.verifies_integrity());
    }

    #[test]
    fn cancellation_after_one_poll_drains_and_cannot_complete() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_gate_cx(|gate, cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(4), None, accuracy, capability),
                    resources(4),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            let mut child = root.split_child("cancelled", resources(4)).unwrap();
            gate.request();
            assert!(matches!(
                child.poll("cancelled.poll"),
                Err(InvocationError::Cancelled {
                    phase: "cancelled.poll"
                })
            ));
            assert_eq!(child.finish().unwrap(), InvocationDisposition::Cancelled);
            root.finish().unwrap()
        });
        assert_eq!(receipt.disposition(), InvocationDisposition::Cancelled);
        assert!(receipt.verifies_integrity());
    }

    #[test]
    fn nested_memory_peak_counts_concurrent_parent_and_child_reservations() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(8), None, accuracy, capability),
                    resources(8),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            {
                let mut parent = root.split_child("parent", resources(8)).unwrap();
                {
                    let mut parent_memory = parent
                        .reserve_memory("parent-memory", MemoryBytes::new(4))
                        .unwrap();
                    {
                        let mut nested = parent_memory
                            .budget()
                            .split_child("nested", resources(4))
                            .unwrap();
                        {
                            let _nested_memory = nested
                                .reserve_memory("nested-memory", MemoryBytes::new(4))
                                .unwrap();
                        }
                        assert_eq!(nested.finish().unwrap(), InvocationDisposition::Completed);
                    }
                }
                assert_eq!(parent.finish().unwrap(), InvocationDisposition::Completed);
            }
            root.finish().unwrap()
        });
        assert_eq!(receipt.memory_peak_bytes(), 8);
        assert_eq!(receipt.memory_requested_bytes(), 8);
        assert_eq!(receipt.memory_released_bytes(), 8);
        assert_eq!(receipt.children()[0].direct_memory_peak_bytes(), 4);
        assert_eq!(receipt.children()[0].memory_peak_bytes(), 8);
        assert!(receipt.verifies_integrity());
    }

    #[test]
    fn semantic_verifier_rejects_rehashed_descendant_memory_peak_underclaim() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(8), None, accuracy, capability),
                    resources(8),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            {
                let mut parent = root.split_child("parent", resources(8)).unwrap();
                {
                    let mut nested = parent.split_child("nested", resources(8)).unwrap();
                    {
                        let _memory = nested
                            .reserve_memory("nested-memory", MemoryBytes::new(4))
                            .unwrap();
                    }
                    assert_eq!(nested.finish().unwrap(), InvocationDisposition::Completed);
                }
                assert_eq!(parent.finish().unwrap(), InvocationDisposition::Completed);
            }
            root.finish().unwrap()
        });
        assert!(receipt.verifies_integrity());

        let mut forged = receipt;
        forged.children[0].memory_peak = 0;
        forged.children[0].root = child_receipt_root(&forged.children[0]);
        forged.memory_peak = 0;
        forged.root = invocation_receipt_root(&forged);
        assert!(matches!(
            forged.verify_semantics(),
            Err(ReceiptSemanticError::Child {
                invariant: "memory-receipt",
                ..
            })
        ));
    }

    #[test]
    fn semantic_verifier_rejects_rehashed_sibling_failure_origins() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(resources(8), None, accuracy, capability),
                    resources(8),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            for phase in ["first", "second"] {
                let child = root.split_child(phase, resources(4)).unwrap();
                assert_eq!(child.finish().unwrap(), InvocationDisposition::Completed);
            }
            root.finish().unwrap()
        });
        assert!(receipt.verifies_integrity());

        let failure = InvocationError::ExplicitRefusal {
            phase: "forged-siblings",
            reason: hash_domain("test.forged-siblings", b"same-failure"),
        };
        let mut forged = receipt;
        for child in &mut forged.children {
            child.failure = Some(failure.clone());
            child.disposition = InvocationDisposition::Refused;
            child.root = child_receipt_root(child);
        }
        forged.failure = Some(failure);
        forged.disposition = InvocationDisposition::Refused;
        forged.root = invocation_receipt_root(&forged);
        assert!(matches!(
            forged.verify_semantics(),
            Err(ReceiptSemanticError::Invocation {
                invariant: "single-failure-origin"
            })
        ));
    }

    #[test]
    fn semantic_verifier_rejects_rehashed_deadline_and_first_fault_forgery() {
        let clock = VirtualClock::new();
        let (id, accuracy, capability) = identities();
        let receipt = with_cx(|cx| {
            let admission = InvocationAdmitter::new()
                .admit(
                    id,
                    InvocationLimits::new(
                        resources(4),
                        Some(Time::from_nanos(10)),
                        accuracy,
                        capability,
                    ),
                    resources(4),
                )
                .unwrap();
            let mut root = admission.begin(cx, &clock).unwrap();
            let mut child = root.split_child("refusal", resources(4)).unwrap();
            assert!(matches!(
                child.charge_work(WorkUnits::new(5)),
                Err(InvocationError::ResourceExceeded { .. })
            ));
            assert_eq!(child.finish().unwrap(), InvocationDisposition::Refused);
            root.finish().unwrap()
        });
        assert!(receipt.verifies_integrity());

        let mut forged_deadline = receipt.clone();
        forged_deadline.last_deadline_observation = Some(Time::from_nanos(10));
        forged_deadline.root = invocation_receipt_root(&forged_deadline);
        assert!(matches!(
            forged_deadline.verify_semantics(),
            Err(ReceiptSemanticError::Invocation {
                invariant: "nondeadline-observation-before-limit"
            })
        ));

        let mut forged_impossible = receipt.clone();
        forged_impossible.failure = Some(InvocationError::ResourceExceeded {
            resource: "work",
            requested: 4,
            available: 4,
        });
        forged_impossible.children[0].failure = forged_impossible.failure.clone();
        forged_impossible.children[0].root = child_receipt_root(&forged_impossible.children[0]);
        forged_impossible.root = invocation_receipt_root(&forged_impossible);
        assert!(matches!(
            forged_impossible.verify_semantics(),
            Err(ReceiptSemanticError::Child {
                invariant: "failure-evidence",
                ..
            })
        ));

        let mut forged_resource_label = receipt.clone();
        forged_resource_label.failure = Some(InvocationError::ResourceExceeded {
            resource: "invented",
            requested: 5,
            available: 4,
        });
        forged_resource_label.children[0].failure = forged_resource_label.failure.clone();
        forged_resource_label.children[0].root =
            child_receipt_root(&forged_resource_label.children[0]);
        forged_resource_label.root = invocation_receipt_root(&forged_resource_label);
        assert!(matches!(
            forged_resource_label.verify_semantics(),
            Err(ReceiptSemanticError::Child {
                invariant: "failure-evidence",
                ..
            })
        ));

        let mut forged_overflow_label = receipt.clone();
        forged_overflow_label.failure = Some(InvocationError::ArithmeticOverflow {
            resource: "invented-overflow",
        });
        forged_overflow_label.children[0].failure = forged_overflow_label.failure.clone();
        forged_overflow_label.children[0].root =
            child_receipt_root(&forged_overflow_label.children[0]);
        forged_overflow_label.root = invocation_receipt_root(&forged_overflow_label);
        assert!(matches!(
            forged_overflow_label.verify_semantics(),
            Err(ReceiptSemanticError::Child {
                invariant: "failure-evidence",
                ..
            })
        ));

        let forged_memory_failure = InvocationError::MemoryRefused {
            what: "forged-memory",
            requested: 5,
            used: 0,
            limit: 4,
        };
        let mut forged_memory_count = receipt.clone();
        forged_memory_count.memory_refusals = 2;
        forged_memory_count.memory_first_refusal = Some(InvocationMemoryRefusal {
            what: "forged-memory",
            requested: 5,
            used: 0,
            limit: 4,
        });
        forged_memory_count.failure = Some(forged_memory_failure.clone());
        forged_memory_count.children[0].failure = Some(forged_memory_failure);
        forged_memory_count.children[0].root = child_receipt_root(&forged_memory_count.children[0]);
        forged_memory_count.root = invocation_receipt_root(&forged_memory_count);
        assert!(matches!(
            forged_memory_count.verify_semantics(),
            Err(ReceiptSemanticError::Invocation {
                invariant: "memory-refusal-evidence"
            })
        ));

        let mut forged_unsealable = receipt.clone();
        forged_unsealable.failure = Some(InvocationError::MemoryReleaseInvariant);
        forged_unsealable.children[0].failure = forged_unsealable.failure.clone();
        forged_unsealable.children[0].root = child_receipt_root(&forged_unsealable.children[0]);
        forged_unsealable.root = invocation_receipt_root(&forged_unsealable);
        assert!(matches!(
            forged_unsealable.verify_semantics(),
            Err(ReceiptSemanticError::Child {
                invariant: "failure-evidence",
                ..
            })
        ));

        let mut forged_fault = receipt;
        forged_fault.children[0].failure = Some(InvocationError::ExplicitRefusal {
            phase: "forged",
            reason: hash_domain("test.forged-fault", b"different"),
        });
        forged_fault.children[0].root = child_receipt_root(&forged_fault.children[0]);
        forged_fault.root = invocation_receipt_root(&forged_fault);
        assert!(matches!(
            forged_fault.verify_semantics(),
            Err(ReceiptSemanticError::Child {
                invariant: "failure-propagates-to-root",
                ..
            })
        ));
    }
}
