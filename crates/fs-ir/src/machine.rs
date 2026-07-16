//! Durable Machine-IR entity identity and topology-lineage kernel.
//!
//! This is PR-1 of the Machine-IR program. It deliberately does not define the
//! machine graph, joints, port schemas, boundary conditions, controllers, or
//! scenario lowering yet. It establishes the identity law those later schemas must use:
//! array positions are never identity, entity roles are nominally distinct,
//! and a topology change may rebind an attachment only when its source has one
//! unambiguous target. One-to-many changes with live attachments return a
//! typed invalidation receipt instead of guessing.

use core::fmt;
use core::hash::{Hash, Hasher};

use fs_blake3::identity::{
    CanonicalEncoder, CanonicalError, CanonicalLimits, CanonicalSchema, EntityId, Field, FieldSpec,
    IdentityReceipt, NeverCancel, SemanticId, StrongIdentity, WireType,
};

/// Version of every durable Machine-IR entity-key schema in this module.
pub const MACHINE_ENTITY_ID_SCHEMA_VERSION_V1: u32 = 1;
/// Version of the canonical lineage-record and invalidation schemas.
pub const MACHINE_LINEAGE_SCHEMA_VERSION_V1: u32 = 1;
/// Maximum canonical bytes in one human-auditable entity key.
pub const MAX_MACHINE_ENTITY_KEY_BYTES: usize = 128;
/// Maximum source relations in one synchronous lineage record.
pub const MAX_LINEAGE_RELATIONS: usize = 4_096;
/// Maximum targets declared for one source entity.
pub const MAX_LINEAGE_TARGETS_PER_SOURCE: usize = 4_096;
/// Maximum total source-to-target endpoints in one lineage record.
pub const MAX_LINEAGE_ENDPOINTS: usize = 8_192;
/// Maximum dependent attachments considered by one lineage admission.
pub const MAX_LINEAGE_DEPENDENTS: usize = 4_096;

const MACHINE_IDENTITY_LIMITS: CanonicalLimits = CanonicalLimits::new(4_096, 128, 1, 1, 256);
const LINEAGE_IDENTITY_LIMITS: CanonicalLimits =
    CanonicalLimits::new(4 * 1_024 * 1_024, 1_024 * 1_024, 5, 16_384, 4_096);

/// Nominal role of a durable Machine-IR entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MachineElementKind {
    /// Physical or logical body occurrence.
    Body = 1,
    /// Named surface support that may survive remeshing.
    SurfacePatch = 2,
    /// Contact attachment feature.
    ContactFeature = 3,
    /// Typed subsystem terminal.
    Terminal = 4,
    /// Coupling port.
    Port = 5,
    /// Owned state slot.
    StateSlot = 6,
}

impl MachineElementKind {
    /// Stable binary tag used by lineage schemas.
    #[must_use]
    pub const fn tag(self) -> u8 {
        self as u8
    }

    /// Stable diagnostic name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::SurfacePatch => "surface-patch",
            Self::ContactFeature => "contact-feature",
            Self::Terminal => "terminal",
            Self::Port => "port",
            Self::StateSlot => "state-slot",
        }
    }
}

/// Structured refusal from canonical Machine-IR key construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineIdError {
    /// The identifier was empty.
    Empty {
        /// Nominal identifier role.
        role: &'static str,
    },
    /// The identifier exceeded its public byte envelope.
    TooLong {
        /// Nominal identifier role.
        role: &'static str,
        /// Supplied UTF-8 byte count.
        bytes: usize,
        /// Maximum admitted byte count.
        max: usize,
    },
    /// A slash-delimited key segment was empty.
    EmptySegment {
        /// Nominal identifier role.
        role: &'static str,
        /// Zero-based segment index.
        segment: usize,
    },
    /// A segment did not begin with an ASCII lowercase letter.
    InvalidSegmentStart {
        /// Nominal identifier role.
        role: &'static str,
        /// Zero-based segment index.
        segment: usize,
        /// Byte offset in the complete key.
        at: usize,
        /// Offending byte.
        byte: u8,
    },
    /// A byte was outside the canonical `[a-z][a-z0-9-]*` segment grammar.
    InvalidByte {
        /// Nominal identifier role.
        role: &'static str,
        /// Byte offset in the complete key.
        at: usize,
        /// Offending byte.
        byte: u8,
    },
    /// A segment ended in `-`.
    TrailingSeparator {
        /// Nominal identifier role.
        role: &'static str,
        /// Zero-based segment index.
        segment: usize,
    },
    /// Adjacent `-` separators would create a noncanonical spelling.
    RepeatedSeparator {
        /// Nominal identifier role.
        role: &'static str,
        /// Byte offset of the second separator.
        at: usize,
    },
    /// The bounded canonical identity encoder refused publication.
    Identity(CanonicalError),
}

impl MachineIdError {
    /// Stable rule code for structured admission diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Empty { .. } => "MachineIdEmpty",
            Self::TooLong { .. } => "MachineIdTooLong",
            Self::EmptySegment { .. } => "MachineIdEmptySegment",
            Self::InvalidSegmentStart { .. } => "MachineIdInvalidSegmentStart",
            Self::InvalidByte { .. } => "MachineIdInvalidByte",
            Self::TrailingSeparator { .. } => "MachineIdTrailingSeparator",
            Self::RepeatedSeparator { .. } => "MachineIdRepeatedSeparator",
            Self::Identity(_) => "MachineIdIdentity",
        }
    }
}

impl fmt::Display for MachineIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { role } => write!(f, "{role} id must not be empty"),
            Self::TooLong { role, bytes, max } => {
                write!(f, "{role} id has {bytes} bytes; maximum is {max}")
            }
            Self::EmptySegment { role, segment } => {
                write!(f, "{role} id segment {segment} must not be empty")
            }
            Self::InvalidSegmentStart {
                role,
                segment,
                at,
                byte,
            } => write!(
                f,
                "{role} id segment {segment} must start with an ASCII lowercase letter; byte \
                 0x{byte:02x} at offset {at} is invalid"
            ),
            Self::InvalidByte { role, at, byte } => write!(
                f,
                "{role} id byte 0x{byte:02x} at offset {at} is outside the canonical \
                 [a-z][a-z0-9-]* segment grammar"
            ),
            Self::TrailingSeparator { role, segment } => {
                write!(f, "{role} id segment {segment} must not end in '-'")
            }
            Self::RepeatedSeparator { role, at } => write!(
                f,
                "{role} id contains repeated '-' separators ending at offset {at}"
            ),
            Self::Identity(error) => write!(f, "Machine-IR identity refused: {error}"),
        }
    }
}

impl core::error::Error for MachineIdError {}

impl From<CanonicalError> for MachineIdError {
    fn from(error: CanonicalError) -> Self {
        Self::Identity(error)
    }
}

fn validate_canonical_key(role: &'static str, value: &str) -> Result<(), MachineIdError> {
    if value.is_empty() {
        return Err(MachineIdError::Empty { role });
    }
    if value.len() > MAX_MACHINE_ENTITY_KEY_BYTES {
        return Err(MachineIdError::TooLong {
            role,
            bytes: value.len(),
            max: MAX_MACHINE_ENTITY_KEY_BYTES,
        });
    }

    let mut base = 0usize;
    for (segment_index, segment) in value.as_bytes().split(|byte| *byte == b'/').enumerate() {
        if segment.is_empty() {
            return Err(MachineIdError::EmptySegment {
                role,
                segment: segment_index,
            });
        }
        if !segment[0].is_ascii_lowercase() {
            return Err(MachineIdError::InvalidSegmentStart {
                role,
                segment: segment_index,
                at: base,
                byte: segment[0],
            });
        }
        let mut previous_separator = false;
        for (offset, byte) in segment.iter().copied().enumerate() {
            let at = base + offset;
            match byte {
                b'a'..=b'z' | b'0'..=b'9' => previous_separator = false,
                b'-' if previous_separator => {
                    return Err(MachineIdError::RepeatedSeparator { role, at });
                }
                b'-' => previous_separator = true,
                _ => return Err(MachineIdError::InvalidByte { role, at, byte }),
            }
        }
        if previous_separator {
            return Err(MachineIdError::TrailingSeparator {
                role,
                segment: segment_index,
            });
        }
        base += segment.len() + 1;
    }
    Ok(())
}

macro_rules! durable_machine_id {
    (
        $(#[$meta:meta])*
        $name:ident,
        $schema:ident,
        $identity:ident,
        $kind:expr,
        $role:literal,
        $domain:literal,
        $context:literal
    ) => {
        #[doc = concat!("Canonical schema marker for `", stringify!($name), "`.")]
        pub enum $schema {}

        impl CanonicalSchema for $schema {
            const DOMAIN: &'static str = $domain;
            const NAME: &'static str = $role;
            const VERSION: u32 = MACHINE_ENTITY_ID_SCHEMA_VERSION_V1;
            const CONTEXT: &'static str = $context;
            const FIELDS: &'static [FieldSpec] =
                &[FieldSpec::required("canonical-key", WireType::Utf8)];
        }

        #[doc = concat!("Typed durable entity digest for `", stringify!($name), "`.")]
        pub type $identity = EntityId<$schema>;

        $(#[$meta])*
        #[derive(Clone)]
        pub struct $name {
            canonical_key: Box<str>,
            receipt: IdentityReceipt<$identity>,
        }

        impl $name {
            /// Admit a canonical, human-auditable durable key.
            ///
            /// # Errors
            /// Refuses noncanonical text or a bounded identity-encoding error.
            pub fn new(key: impl Into<String>) -> Result<Self, MachineIdError> {
                let key = key.into();
                validate_canonical_key($role, &key)?;
                let receipt = CanonicalEncoder::<$identity, _>::new(
                    MACHINE_IDENTITY_LIMITS,
                    NeverCancel,
                )?
                .utf8(Field::new(0, "canonical-key"), &key)?
                .finish()?;
                Ok(Self {
                    canonical_key: key.into_boxed_str(),
                    receipt,
                })
            }

            /// Nominal entity kind.
            #[must_use]
            pub const fn kind(&self) -> MachineElementKind {
                $kind
            }

            /// Exact canonical key retained for diagnostics and wire lowering.
            #[must_use]
            pub fn canonical_key(&self) -> &str {
                &self.canonical_key
            }

            /// Domain- and role-separated durable entity identity.
            #[must_use]
            pub const fn identity(&self) -> $identity {
                self.receipt.id()
            }

            /// Complete canonical-preimage receipt for collision adjudication.
            #[must_use]
            pub const fn identity_receipt(&self) -> IdentityReceipt<$identity> {
                self.receipt
            }

            fn digest_bytes(&self) -> [u8; 32] {
                *self.identity().as_bytes()
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.identity() == other.identity()
            }
        }

        impl Eq for $name {}

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> core::cmp::Ordering {
                self.identity().cmp(&other.identity())
            }
        }

        impl Hash for $name {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.identity().hash(state);
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.canonical_key)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("canonical_key", &self.canonical_key)
                    .field("identity", &self.identity())
                    .finish()
            }
        }
    };
}

durable_machine_id!(
    /// Durable identity of a Machine-IR body occurrence.
    BodyId,
    BodyIdSchemaV1,
    BodyEntityIdV1,
    MachineElementKind::Body,
    "body-id",
    "org.frankensim.fs-ir.machine.body-id.v1",
    "one durable Machine-IR body occurrence named by a canonical hierarchical key"
);
durable_machine_id!(
    /// Durable identity of a surface support independent of mesh indices.
    SurfacePatchId,
    SurfacePatchIdSchemaV1,
    SurfacePatchEntityIdV1,
    MachineElementKind::SurfacePatch,
    "surface-patch-id",
    "org.frankensim.fs-ir.machine.surface-patch-id.v1",
    "one durable Machine-IR surface patch independent of discretization indices"
);
durable_machine_id!(
    /// Durable identity of a contact attachment feature.
    ContactFeatureId,
    ContactFeatureIdSchemaV1,
    ContactFeatureEntityIdV1,
    MachineElementKind::ContactFeature,
    "contact-feature-id",
    "org.frankensim.fs-ir.machine.contact-feature-id.v1",
    "one durable Machine-IR contact feature independent of solver ordering"
);
durable_machine_id!(
    /// Durable identity of a typed subsystem terminal.
    TerminalId,
    TerminalIdSchemaV1,
    TerminalEntityIdV1,
    MachineElementKind::Terminal,
    "terminal-id",
    "org.frankensim.fs-ir.machine.terminal-id.v1",
    "one durable Machine-IR terminal independent of graph serialization order"
);
durable_machine_id!(
    /// Durable identity of a coupling port.
    PortId,
    PortIdSchemaV1,
    PortEntityIdV1,
    MachineElementKind::Port,
    "port-id",
    "org.frankensim.fs-ir.machine.port-id.v1",
    "one durable Machine-IR coupling port independent of graph serialization order"
);
durable_machine_id!(
    /// Durable identity of an owned state slot.
    StateSlotId,
    StateSlotIdSchemaV1,
    StateSlotEntityIdV1,
    MachineElementKind::StateSlot,
    "state-slot-id",
    "org.frankensim.fs-ir.machine.state-slot-id.v1",
    "one durable Machine-IR state slot independent of vector position"
);

/// One of the six non-confusable durable Machine-IR entity roles.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MachineElementId {
    /// Body occurrence.
    Body(BodyId),
    /// Surface support.
    SurfacePatch(SurfacePatchId),
    /// Contact feature.
    ContactFeature(ContactFeatureId),
    /// Terminal.
    Terminal(TerminalId),
    /// Port.
    Port(PortId),
    /// State slot.
    StateSlot(StateSlotId),
}

impl MachineElementId {
    /// Nominal role.
    #[must_use]
    pub const fn kind(&self) -> MachineElementKind {
        match self {
            Self::Body(_) => MachineElementKind::Body,
            Self::SurfacePatch(_) => MachineElementKind::SurfacePatch,
            Self::ContactFeature(_) => MachineElementKind::ContactFeature,
            Self::Terminal(_) => MachineElementKind::Terminal,
            Self::Port(_) => MachineElementKind::Port,
            Self::StateSlot(_) => MachineElementKind::StateSlot,
        }
    }

    /// Human-auditable canonical key.
    #[must_use]
    pub fn canonical_key(&self) -> &str {
        match self {
            Self::Body(id) => id.canonical_key(),
            Self::SurfacePatch(id) => id.canonical_key(),
            Self::ContactFeature(id) => id.canonical_key(),
            Self::Terminal(id) => id.canonical_key(),
            Self::Port(id) => id.canonical_key(),
            Self::StateSlot(id) => id.canonical_key(),
        }
    }

    fn digest_bytes(&self) -> [u8; 32] {
        match self {
            Self::Body(id) => id.digest_bytes(),
            Self::SurfacePatch(id) => id.digest_bytes(),
            Self::ContactFeature(id) => id.digest_bytes(),
            Self::Terminal(id) => id.digest_bytes(),
            Self::Port(id) => id.digest_bytes(),
            Self::StateSlot(id) => id.digest_bytes(),
        }
    }

    fn canonical_row(&self) -> [u8; 33] {
        let mut row = [0u8; 33];
        row[0] = self.kind().tag();
        row[1..].copy_from_slice(&self.digest_bytes());
        row
    }
}

impl fmt::Display for MachineElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind().name(), self.canonical_key())
    }
}

macro_rules! element_conversion {
    ($id:ident, $variant:ident) => {
        impl From<$id> for MachineElementId {
            fn from(value: $id) -> Self {
                Self::$variant(value)
            }
        }
    };
}

element_conversion!(BodyId, Body);
element_conversion!(SurfacePatchId, SurfacePatch);
element_conversion!(ContactFeatureId, ContactFeature);
element_conversion!(TerminalId, Terminal);
element_conversion!(PortId, Port);
element_conversion!(StateSlotId, StateSlot);

/// Topology or discretization event represented by a lineage morphism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LineageEvent {
    /// One entity becomes multiple descendants.
    Split = 1,
    /// Multiple entities become one descendant.
    Merge = 2,
    /// Discretization changes while physical support may be preserved or split.
    Remesh = 3,
    /// Wear advances an entity to a successor support.
    Wear = 4,
    /// Fracture creates multiple descendants.
    Fracture = 5,
}

impl LineageEvent {
    /// Stable identity tag.
    #[must_use]
    pub const fn tag(self) -> u64 {
        self as u64
    }
}

/// One source entity and its explicitly declared target set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineageRelation {
    source: MachineElementId,
    targets: Vec<MachineElementId>,
}

impl LineageRelation {
    /// Construct and canonicalize one source relation.
    ///
    /// # Errors
    /// Refuses an empty/oversized target set, cross-role targets, or duplicates.
    pub fn new(
        source: MachineElementId,
        mut targets: Vec<MachineElementId>,
    ) -> Result<Self, LineageRefusal> {
        if targets.is_empty() {
            return Err(LineageRefusal::NoTargets { source });
        }
        if targets.len() > MAX_LINEAGE_TARGETS_PER_SOURCE {
            return Err(LineageRefusal::TargetLimit {
                source,
                count: targets.len(),
                max: MAX_LINEAGE_TARGETS_PER_SOURCE,
            });
        }
        targets.sort();
        if let Some(target) = targets.iter().find(|target| target.kind() != source.kind()) {
            return Err(LineageRefusal::TargetKindMismatch {
                source,
                target: target.clone(),
            });
        }
        if let Some(pair) = targets.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(LineageRefusal::DuplicateTarget {
                source,
                target: pair[0].clone(),
            });
        }
        Ok(Self { source, targets })
    }

    /// Source entity.
    #[must_use]
    pub const fn source(&self) -> &MachineElementId {
        &self.source
    }

    /// Canonically ordered target set.
    #[must_use]
    pub fn targets(&self) -> &[MachineElementId] {
        &self.targets
    }

    fn canonical_row(&self) -> Vec<u8> {
        let mut row = Vec::with_capacity(33 + 8 + self.targets.len() * 33);
        row.extend_from_slice(&self.source.canonical_row());
        row.extend_from_slice(&(self.targets.len() as u64).to_le_bytes());
        for target in &self.targets {
            row.extend_from_slice(&target.canonical_row());
        }
        row
    }
}

/// Downstream artifact class whose attachment must follow or invalidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DependentKind {
    /// Derived cache keyed by the source entity.
    Cache = 1,
    /// Contact relation attached to the source entity.
    Contact = 2,
    /// Winding or coil attachment.
    Winding = 3,
    /// Adjoint/tape dependency.
    Adjoint = 4,
}

impl DependentKind {
    /// Stable identity tag.
    #[must_use]
    pub const fn tag(self) -> u8 {
        self as u8
    }

    /// Stable diagnostic name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Contact => "contact",
            Self::Winding => "winding",
            Self::Adjoint => "adjoint",
        }
    }
}

/// One named downstream attachment to a durable source entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DependentBinding {
    kind: DependentKind,
    canonical_key: Box<str>,
    source: MachineElementId,
}

impl DependentBinding {
    /// Construct a checked dependent attachment.
    ///
    /// # Errors
    /// Refuses a noncanonical dependent key.
    pub fn new(
        kind: DependentKind,
        key: impl Into<String>,
        source: MachineElementId,
    ) -> Result<Self, MachineIdError> {
        let key = key.into();
        validate_canonical_key(kind.name(), &key)?;
        Ok(Self {
            kind,
            canonical_key: key.into_boxed_str(),
            source,
        })
    }

    /// Downstream artifact class.
    #[must_use]
    pub const fn kind(&self) -> DependentKind {
        self.kind
    }

    /// Canonical dependent identity key.
    #[must_use]
    pub fn canonical_key(&self) -> &str {
        &self.canonical_key
    }

    /// Entity to which this dependent is attached.
    #[must_use]
    pub const fn source(&self) -> &MachineElementId {
        &self.source
    }

    fn canonical_row(&self) -> Vec<u8> {
        let mut row = Vec::with_capacity(1 + 8 + self.canonical_key.len() + 33);
        row.push(self.kind.tag());
        row.extend_from_slice(&(self.canonical_key.len() as u64).to_le_bytes());
        row.extend_from_slice(self.canonical_key.as_bytes());
        row.extend_from_slice(&self.source.canonical_row());
        row
    }
}

/// One admitted deterministic attachment move.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineageRebinding {
    dependent: DependentBinding,
    target: MachineElementId,
}

impl LineageRebinding {
    /// Dependent whose attachment moves.
    #[must_use]
    pub const fn dependent(&self) -> &DependentBinding {
        &self.dependent
    }

    /// Unique target selected by the admitted relation.
    #[must_use]
    pub const fn target(&self) -> &MachineElementId {
        &self.target
    }

    fn canonical_row(&self) -> Vec<u8> {
        let mut row = self.dependent.canonical_row();
        row.extend_from_slice(&self.target.canonical_row());
        row
    }
}

/// Canonical schema marker for one admitted lineage record.
pub enum LineageRecordSchemaV1 {}

impl CanonicalSchema for LineageRecordSchemaV1 {
    const DOMAIN: &'static str = "org.frankensim.fs-ir.machine.lineage-record.v1";
    const NAME: &'static str = "machine-lineage-record";
    const VERSION: u32 = MACHINE_LINEAGE_SCHEMA_VERSION_V1;
    const CONTEXT: &'static str = "one canonical topology/discretization event, its explicit source-target relations, and only deterministic dependent rebindings";
    const FIELDS: &'static [FieldSpec] = &[
        FieldSpec::required("event", WireType::U64),
        FieldSpec::required("relations", WireType::OrderedBytes),
        FieldSpec::required("rebindings", WireType::OrderedBytes),
    ];
}

/// Semantic identity of one admitted lineage record.
pub type LineageRecordIdV1 = SemanticId<LineageRecordSchemaV1>;

/// Canonical schema marker for a fail-closed lineage invalidation.
pub enum LineageInvalidationSchemaV1 {}

impl CanonicalSchema for LineageInvalidationSchemaV1 {
    const DOMAIN: &'static str = "org.frankensim.fs-ir.machine.lineage-invalidation.v1";
    const NAME: &'static str = "machine-lineage-invalidation";
    const VERSION: u32 = MACHINE_LINEAGE_SCHEMA_VERSION_V1;
    const CONTEXT: &'static str = "one refused ambiguous lineage event, its complete source-target relation set, every considered dependent binding, the one-to-many subset that caused ambiguity, and every dependent invalidated instead of silently rebound";
    const FIELDS: &'static [FieldSpec] = &[
        FieldSpec::required("event", WireType::U64),
        FieldSpec::required("relations", WireType::OrderedBytes),
        FieldSpec::required("considered-dependents", WireType::OrderedBytes),
        FieldSpec::required("ambiguous-relations", WireType::OrderedBytes),
        FieldSpec::required("invalidated-dependents", WireType::OrderedBytes),
    ];
}

/// Semantic identity of one fail-closed invalidation set.
pub type LineageInvalidationIdV1 = SemanticId<LineageInvalidationSchemaV1>;

/// Published lineage event containing only unambiguous rebindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageRecord {
    event: LineageEvent,
    relations: Vec<LineageRelation>,
    rebindings: Vec<LineageRebinding>,
    receipt: IdentityReceipt<LineageRecordIdV1>,
}

impl LineageRecord {
    /// Admit a bounded lineage event and its dependent attachments.
    ///
    /// Inputs are canonicalized by durable identity, so caller order is not
    /// semantic. A dependent on a one-to-many source never reaches the success
    /// type: admission returns [`LineageRefusal::Ambiguous`] with a complete
    /// [`LineageInvalidation`] receipt.
    ///
    /// # Errors
    /// Refuses malformed event shapes, duplicates, missing sources, resource
    /// limits, identity publication failure, or ambiguous live attachments.
    pub fn admit(
        event: LineageEvent,
        relations: Vec<LineageRelation>,
        dependents: Vec<DependentBinding>,
    ) -> Result<Self, LineageRefusal> {
        Self::admit_with_decision(event, relations, dependents).into_result()
    }

    /// Execute admission and retain its deterministic outcome summary.
    ///
    /// The summary is suitable for structured tracing: it carries the attempted
    /// event and input counts together with either the admitted lineage receipt
    /// or the exact typed refusal. A replay ledger must separately retain the
    /// canonical attempted inputs; early refusals intentionally discard them.
    #[must_use]
    pub fn admit_with_decision(
        event: LineageEvent,
        relations: Vec<LineageRelation>,
        dependents: Vec<DependentBinding>,
    ) -> LineageAdmissionDecision {
        let submitted_relations = relations.len();
        let submitted_dependents = dependents.len();
        let result = Self::admit_inner(event, relations, dependents);
        LineageAdmissionDecision {
            event,
            submitted_relations,
            submitted_dependents,
            result,
        }
    }

    fn admit_inner(
        event: LineageEvent,
        mut relations: Vec<LineageRelation>,
        mut dependents: Vec<DependentBinding>,
    ) -> Result<Self, LineageRefusal> {
        if relations.is_empty() {
            return Err(LineageRefusal::NoRelations);
        }
        if relations.len() > MAX_LINEAGE_RELATIONS {
            return Err(LineageRefusal::RelationLimit {
                count: relations.len(),
                max: MAX_LINEAGE_RELATIONS,
            });
        }
        if dependents.len() > MAX_LINEAGE_DEPENDENTS {
            return Err(LineageRefusal::DependentLimit {
                count: dependents.len(),
                max: MAX_LINEAGE_DEPENDENTS,
            });
        }
        let endpoints = relations.iter().try_fold(0usize, |total, relation| {
            total.checked_add(relation.targets.len())
        });
        let Some(endpoints) = endpoints else {
            return Err(LineageRefusal::EndpointLimit {
                count: usize::MAX,
                max: MAX_LINEAGE_ENDPOINTS,
            });
        };
        if endpoints > MAX_LINEAGE_ENDPOINTS {
            return Err(LineageRefusal::EndpointLimit {
                count: endpoints,
                max: MAX_LINEAGE_ENDPOINTS,
            });
        }

        relations.sort_by(|left, right| left.source.cmp(&right.source));
        if let Some(pair) = relations
            .windows(2)
            .find(|pair| pair[0].source == pair[1].source)
        {
            return Err(LineageRefusal::DuplicateSource {
                source: pair[0].source.clone(),
            });
        }
        validate_event_shape(event, &relations)?;

        dependents.sort();
        if let Some(pair) = dependents.windows(2).find(|pair| {
            pair[0].kind == pair[1].kind && pair[0].canonical_key == pair[1].canonical_key
        }) {
            return Err(LineageRefusal::DuplicateDependent {
                kind: pair[0].kind,
                key: pair[0].canonical_key.clone(),
            });
        }
        for dependent in &dependents {
            if relations
                .binary_search_by(|relation| relation.source.cmp(&dependent.source))
                .is_err()
            {
                return Err(LineageRefusal::UnknownDependentSource {
                    kind: dependent.kind,
                    key: dependent.canonical_key.clone(),
                    source: dependent.source.clone(),
                });
            }
        }

        let ambiguous_relations: Vec<_> = relations
            .iter()
            .filter(|relation| relation.targets.len() != 1)
            .cloned()
            .collect();
        let invalidated_dependents: Vec<_> = dependents
            .iter()
            .filter(|dependent| {
                ambiguous_relations
                    .binary_search_by(|relation| relation.source.cmp(&dependent.source))
                    .is_ok()
            })
            .cloned()
            .collect();
        if !invalidated_dependents.is_empty() {
            return Err(LineageRefusal::Ambiguous(LineageInvalidation::new(
                event,
                relations,
                dependents,
                ambiguous_relations,
                invalidated_dependents,
            )?));
        }

        let mut rebindings = Vec::with_capacity(dependents.len());
        for dependent in dependents {
            let relation_index = relations
                .binary_search_by(|relation| relation.source.cmp(&dependent.source))
                .expect("dependent source membership checked above");
            let relation = &relations[relation_index];
            if let [target] = relation.targets.as_slice() {
                rebindings.push(LineageRebinding {
                    dependent,
                    target: target.clone(),
                });
            }
        }
        rebindings.sort();
        let receipt = lineage_record_identity(event, &relations, &rebindings)?;
        Ok(Self {
            event,
            relations,
            rebindings,
            receipt,
        })
    }

    /// Event kind.
    #[must_use]
    pub const fn event(&self) -> LineageEvent {
        self.event
    }

    /// Canonically ordered complete source-target relations.
    #[must_use]
    pub fn relations(&self) -> &[LineageRelation] {
        &self.relations
    }

    /// Canonically ordered deterministic attachment moves.
    #[must_use]
    pub fn rebindings(&self) -> &[LineageRebinding] {
        &self.rebindings
    }

    /// Domain-separated semantic identity.
    #[must_use]
    pub const fn identity(&self) -> LineageRecordIdV1 {
        self.receipt.id()
    }

    /// Complete canonical-preimage receipt.
    #[must_use]
    pub const fn identity_receipt(&self) -> IdentityReceipt<LineageRecordIdV1> {
        self.receipt
    }
}

/// Exact fail-closed payload for ambiguous live attachments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageInvalidation {
    event: LineageEvent,
    relations: Vec<LineageRelation>,
    considered_dependents: Vec<DependentBinding>,
    ambiguous_relations: Vec<LineageRelation>,
    invalidated_dependents: Vec<DependentBinding>,
    receipt: IdentityReceipt<LineageInvalidationIdV1>,
}

impl LineageInvalidation {
    fn new(
        event: LineageEvent,
        relations: Vec<LineageRelation>,
        considered_dependents: Vec<DependentBinding>,
        ambiguous_relations: Vec<LineageRelation>,
        invalidated_dependents: Vec<DependentBinding>,
    ) -> Result<Self, CanonicalError> {
        let receipt = lineage_invalidation_identity(
            event,
            &relations,
            &considered_dependents,
            &ambiguous_relations,
            &invalidated_dependents,
        )?;
        Ok(Self {
            event,
            relations,
            considered_dependents,
            ambiguous_relations,
            invalidated_dependents,
            receipt,
        })
    }

    /// Refused event kind.
    #[must_use]
    pub const fn event(&self) -> LineageEvent {
        self.event
    }

    /// Complete canonically ordered relation set of the refused event.
    #[must_use]
    pub fn relations(&self) -> &[LineageRelation] {
        &self.relations
    }

    /// Every canonically ordered dependent considered by the refused event.
    #[must_use]
    pub fn considered_dependents(&self) -> &[DependentBinding] {
        &self.considered_dependents
    }

    /// Every one-to-many relation in the refused event.
    #[must_use]
    pub fn ambiguous_relations(&self) -> &[LineageRelation] {
        &self.ambiguous_relations
    }

    /// Complete deterministic invalidation set; nothing was rebound.
    #[must_use]
    pub fn invalidated_dependents(&self) -> &[DependentBinding] {
        &self.invalidated_dependents
    }

    /// Domain-separated invalidation identity.
    #[must_use]
    pub const fn identity(&self) -> LineageInvalidationIdV1 {
        self.receipt.id()
    }

    /// Complete canonical-preimage receipt.
    #[must_use]
    pub const fn identity_receipt(&self) -> IdentityReceipt<LineageInvalidationIdV1> {
        self.receipt
    }
}

/// Structured fail-closed lineage refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineageRefusal {
    /// No source relation was supplied.
    NoRelations,
    /// Too many relations were supplied.
    RelationLimit {
        /// Supplied relation count.
        count: usize,
        /// Public maximum.
        max: usize,
    },
    /// One relation had no target.
    NoTargets {
        /// Offending source.
        source: MachineElementId,
    },
    /// One source had too many targets.
    TargetLimit {
        /// Offending source.
        source: MachineElementId,
        /// Supplied target count.
        count: usize,
        /// Public maximum.
        max: usize,
    },
    /// The whole event had too many target endpoints.
    EndpointLimit {
        /// Supplied or saturated count.
        count: usize,
        /// Public maximum.
        max: usize,
    },
    /// Source and target roles differed.
    TargetKindMismatch {
        /// Source entity.
        source: MachineElementId,
        /// Incompatible target entity.
        target: MachineElementId,
    },
    /// One source listed the same target twice.
    DuplicateTarget {
        /// Source entity.
        source: MachineElementId,
        /// Duplicate target.
        target: MachineElementId,
    },
    /// The event declared the same source more than once.
    DuplicateSource {
        /// Duplicate source.
        source: MachineElementId,
    },
    /// The relation cardinality did not match the named event.
    EventShape {
        /// Event kind.
        event: LineageEvent,
        /// Stable violated rule.
        rule: &'static str,
    },
    /// Too many dependent attachments were supplied.
    DependentLimit {
        /// Supplied dependent count.
        count: usize,
        /// Public maximum.
        max: usize,
    },
    /// A dependent named no source relation in this event.
    UnknownDependentSource {
        /// Dependent class.
        kind: DependentKind,
        /// Dependent key.
        key: Box<str>,
        /// Missing source.
        source: MachineElementId,
    },
    /// A dependent identity appeared more than once.
    DuplicateDependent {
        /// Dependent class.
        kind: DependentKind,
        /// Duplicate key.
        key: Box<str>,
    },
    /// Canonical identity publication failed.
    Identity(CanonicalError),
    /// One or more live dependents had multiple possible targets. The payload
    /// is the exact invalidation record; no success value was published.
    Ambiguous(LineageInvalidation),
}

impl LineageRefusal {
    /// Stable rule code for structured admission diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoRelations => "LineageNoRelations",
            Self::RelationLimit { .. } => "LineageRelationLimit",
            Self::NoTargets { .. } => "LineageNoTargets",
            Self::TargetLimit { .. } => "LineageTargetLimit",
            Self::EndpointLimit { .. } => "LineageEndpointLimit",
            Self::TargetKindMismatch { .. } => "LineageTargetKindMismatch",
            Self::DuplicateTarget { .. } => "LineageDuplicateTarget",
            Self::DuplicateSource { .. } => "LineageDuplicateSource",
            Self::EventShape { .. } => "LineageEventShape",
            Self::DependentLimit { .. } => "LineageDependentLimit",
            Self::UnknownDependentSource { .. } => "LineageUnknownDependentSource",
            Self::DuplicateDependent { .. } => "LineageDuplicateDependent",
            Self::Identity(_) => "LineageIdentity",
            Self::Ambiguous(_) => "LineageAmbiguous",
        }
    }

    /// Exact invalidation payload when this is an ambiguity refusal.
    #[must_use]
    pub const fn invalidation(&self) -> Option<&LineageInvalidation> {
        match self {
            Self::Ambiguous(invalidation) => Some(invalidation),
            _ => None,
        }
    }
}

/// Bounded deterministic outcome summary for one lineage-admission attempt.
///
/// The core library does not print or select a tracing backend. Callers can
/// inspect this summary without reconstructing a refusal from display text.
/// It is not a digest or replay record of the complete submitted graph.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageAdmissionDecision {
    event: LineageEvent,
    submitted_relations: usize,
    submitted_dependents: usize,
    result: Result<LineageRecord, LineageRefusal>,
}

impl LineageAdmissionDecision {
    /// Attempted lineage-event kind, including for early refusals.
    #[must_use]
    pub const fn event(&self) -> LineageEvent {
        self.event
    }

    /// Number of relations submitted before canonicalization or refusal.
    #[must_use]
    pub const fn submitted_relation_count(&self) -> usize {
        self.submitted_relations
    }

    /// Number of dependent bindings submitted before canonicalization.
    #[must_use]
    pub const fn submitted_dependent_count(&self) -> usize {
        self.submitted_dependents
    }

    /// Stable decision/rule code for structured event fields.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match &self.result {
            Ok(_) => "LineageAdmitted",
            Err(refusal) => refusal.code(),
        }
    }

    /// Borrow the admitted record, or the complete refusal.
    #[must_use]
    pub fn result(&self) -> Result<&LineageRecord, &LineageRefusal> {
        self.result.as_ref()
    }

    /// Consume the decision record and recover the conventional result.
    #[must_use]
    pub fn into_result(self) -> Result<LineageRecord, LineageRefusal> {
        self.result
    }
}

impl From<CanonicalError> for LineageRefusal {
    fn from(error: CanonicalError) -> Self {
        Self::Identity(error)
    }
}

impl fmt::Display for LineageRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRelations => f.write_str("lineage event must contain at least one relation"),
            Self::RelationLimit { count, max } => {
                write!(f, "lineage event has {count} relations; maximum is {max}")
            }
            Self::NoTargets { source } => write!(f, "lineage source {source} has no target"),
            Self::TargetLimit { source, count, max } => write!(
                f,
                "lineage source {source} has {count} targets; maximum is {max}"
            ),
            Self::EndpointLimit { count, max } => {
                write!(f, "lineage event has {count} endpoints; maximum is {max}")
            }
            Self::TargetKindMismatch { source, target } => write!(
                f,
                "lineage target {target} is not the same nominal entity kind as source {source}"
            ),
            Self::DuplicateTarget { source, target } => {
                write!(f, "lineage source {source} repeats target {target}")
            }
            Self::DuplicateSource { source } => {
                write!(f, "lineage event repeats source {source}")
            }
            Self::EventShape { event, rule } => {
                write!(f, "{event:?} lineage shape refused: {rule}")
            }
            Self::DependentLimit { count, max } => write!(
                f,
                "lineage event has {count} dependent attachments; maximum is {max}"
            ),
            Self::UnknownDependentSource { kind, key, source } => write!(
                f,
                "{} dependent `{key}` names source {source}, which is absent from the event",
                kind.name()
            ),
            Self::DuplicateDependent { kind, key } => {
                write!(
                    f,
                    "{} dependent `{key}` appears more than once",
                    kind.name()
                )
            }
            Self::Identity(error) => write!(f, "lineage identity refused: {error}"),
            Self::Ambiguous(invalidation) => write!(
                f,
                "ambiguous {:?} lineage invalidated {} dependent attachment(s); no automatic \
                 rebinding was published",
                invalidation.event,
                invalidation.invalidated_dependents.len()
            ),
        }
    }
}

impl core::error::Error for LineageRefusal {}

fn validate_event_shape(
    event: LineageEvent,
    relations: &[LineageRelation],
) -> Result<(), LineageRefusal> {
    match event {
        LineageEvent::Split | LineageEvent::Fracture => {
            if relations.len() != 1 || relations[0].targets.len() < 2 {
                return Err(LineageRefusal::EventShape {
                    event,
                    rule: "split/fracture requires exactly one source and at least two targets",
                });
            }
        }
        LineageEvent::Merge => {
            if relations.len() < 2 || relations.iter().any(|relation| relation.targets.len() != 1) {
                return Err(LineageRefusal::EventShape {
                    event,
                    rule: "merge requires at least two sources, each with exactly one target",
                });
            }
            let target = &relations[0].targets[0];
            if relations
                .iter()
                .skip(1)
                .any(|relation| relation.targets[0] != *target)
            {
                return Err(LineageRefusal::EventShape {
                    event,
                    rule: "every merge source must name the same target",
                });
            }
        }
        LineageEvent::Wear => {
            if relations.iter().any(|relation| relation.targets.len() != 1) {
                return Err(LineageRefusal::EventShape {
                    event,
                    rule: "wear requires exactly one successor for every source",
                });
            }
        }
        LineageEvent::Remesh => {}
    }
    Ok(())
}

fn lineage_record_identity(
    event: LineageEvent,
    relations: &[LineageRelation],
    rebindings: &[LineageRebinding],
) -> Result<IdentityReceipt<LineageRecordIdV1>, CanonicalError> {
    let relation_rows: Vec<_> = relations
        .iter()
        .map(LineageRelation::canonical_row)
        .collect();
    let rebinding_rows: Vec<_> = rebindings
        .iter()
        .map(LineageRebinding::canonical_row)
        .collect();
    CanonicalEncoder::<LineageRecordIdV1, _>::new(LINEAGE_IDENTITY_LIMITS, NeverCancel)?
        .u64(Field::new(0, "event"), event.tag())?
        .ordered_bytes(
            Field::new(1, "relations"),
            relation_rows.len() as u64,
            relation_rows.iter().map(Vec::as_slice),
        )?
        .ordered_bytes(
            Field::new(2, "rebindings"),
            rebinding_rows.len() as u64,
            rebinding_rows.iter().map(Vec::as_slice),
        )?
        .finish()
}

fn lineage_invalidation_identity(
    event: LineageEvent,
    relations: &[LineageRelation],
    considered_dependents: &[DependentBinding],
    ambiguous_relations: &[LineageRelation],
    dependents: &[DependentBinding],
) -> Result<IdentityReceipt<LineageInvalidationIdV1>, CanonicalError> {
    let relation_rows: Vec<_> = relations
        .iter()
        .map(LineageRelation::canonical_row)
        .collect();
    let considered_rows: Vec<_> = considered_dependents
        .iter()
        .map(DependentBinding::canonical_row)
        .collect();
    let ambiguous_rows: Vec<_> = ambiguous_relations
        .iter()
        .map(LineageRelation::canonical_row)
        .collect();
    let dependent_rows: Vec<_> = dependents
        .iter()
        .map(DependentBinding::canonical_row)
        .collect();
    CanonicalEncoder::<LineageInvalidationIdV1, _>::new(LINEAGE_IDENTITY_LIMITS, NeverCancel)?
        .u64(Field::new(0, "event"), event.tag())?
        .ordered_bytes(
            Field::new(1, "relations"),
            relation_rows.len() as u64,
            relation_rows.iter().map(Vec::as_slice),
        )?
        .ordered_bytes(
            Field::new(2, "considered-dependents"),
            considered_rows.len() as u64,
            considered_rows.iter().map(Vec::as_slice),
        )?
        .ordered_bytes(
            Field::new(3, "ambiguous-relations"),
            ambiguous_rows.len() as u64,
            ambiguous_rows.iter().map(Vec::as_slice),
        )?
        .ordered_bytes(
            Field::new(4, "invalidated-dependents"),
            dependent_rows.len() as u64,
            dependent_rows.iter().map(Vec::as_slice),
        )?
        .finish()
}
