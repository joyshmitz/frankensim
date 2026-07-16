//! Feature-gated I01.3 lowering from neutral [`fs_couple::PortSchema`]
//! declarations into the multi-field [`crate::system`] IR.
//!
//! This module is intentionally a type-and-accounting compiler, not a numeric
//! interface solver. It re-derives the power dimensions, binds the complete
//! port schema into the resulting [`crate::SystemId`], makes orientation
//! reversal an explicit algebraic sign, and refuses source/loss terms without
//! exactly one ownership disposition. Numeric contraction, quadrature, port
//! adapter truth, and closed-window conservation remain external obligations.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use fs_couple::{
    ConservationRole, FieldMeasureSide, PORT_SCHEMA_VERSION, PortKind, PortOrientation, PortSchema,
    PortValueShape, PowerPairing, StableId,
};
use fs_iface::SpaceType;
use fs_qty::Dims;

use crate::expr::Space;
use crate::system::{
    AdmittedSystem, BlockEquation, ConventionRef, CoordinateConvention, FieldDecl, FieldQuantity,
    MAX_SYSTEM_EXTENSION_BYTES, SpatialSupport, StateOwnership, SystemDef, SystemExpr, SystemId,
    SystemTypeError,
};

/// Canonical schema label for [`PortEquationReceipt::to_json`].
pub const PORT_EQUATION_RECEIPT_SCHEMA_V1: &str = "fs-opdsl-port-equation-receipt-v1";

/// Maximum port equations admitted in one deterministic batch.
pub const MAX_PORT_EQUATIONS: usize = 4_096;

const RAW_VECTOR_DEGREE: u8 = 255;
const POWER_DIMS: Dims = Dims([2, 1, -3, 0, 0, 0]);
const PORT_EXTENSION_VERSION: u32 = 1;
const LOSS_OWNERSHIP_DOMAIN_V1: &str = "org.frankensim.fs-opdsl.loss-ownership.v1";

/// Nominal content identity for one concretely owned dissipative term.
///
/// The compiler derives it from the complete source schema, discretization,
/// dissipative role, and declared concrete owner after normalizing only the
/// algebraic equation sense. Reversal therefore does not invent a second
/// physical loss owner, while any physical/schema change moves the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LossOwnershipId(fs_blake3::ContentHash);

impl LossOwnershipId {
    /// Exact digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    /// Lowercase hexadecimal rendering.
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }

    /// Parse an exact 64-digit hexadecimal transport. Parsing adds no
    /// authority; callers compare it with a freshly compiled receipt.
    #[must_use]
    pub fn parse_hex(value: &str) -> Option<Self> {
        fs_blake3::ContentHash::from_hex(value).map(Self)
    }
}

impl core::fmt::Display for LossOwnershipId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Whether the compiled equation follows or reverses the schema's declared
/// positive orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortEquationSense {
    /// Preserve the declared positive sense.
    AsDeclared,
    /// Apply the explicit negative of the declared power contribution.
    Reversed,
}

impl PortEquationSense {
    /// Exact multiplier inserted into the generated expression.
    #[must_use]
    pub const fn sign(self) -> i8 {
        match self {
            Self::AsDeclared => 1,
            Self::Reversed => -1,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::AsDeclared => "as-declared",
            Self::Reversed => "reversed",
        }
    }
}

/// Accounting role of one generated power term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountingTermKind {
    /// Lossless/reversible interface exchange. No source or loss owner exists.
    Reversible,
    /// Stored-energy contribution. A concrete owner is mandatory.
    Storage,
    /// Source or reservoir contribution.
    Source,
    /// Irreversible production/loss contribution.
    Dissipation,
}

impl AccountingTermKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Reversible => "reversible",
            Self::Storage => "storage",
            Self::Source => "source",
            Self::Dissipation => "dissipation",
        }
    }
}

/// Explicit ownership status for an accounting term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipDisposition {
    /// Only reversible terms may carry no ownership concept.
    NotApplicable,
    /// Exactly one stable component/operator owns the term.
    Owned(StableId),
    /// A source/loss is intentionally unowned under a retained rationale ID.
    ExplicitlyUnowned {
        /// Durable reason, policy, or scope-exclusion identifier.
        rationale: StableId,
    },
}

impl OwnershipDisposition {
    /// Borrow the unique owner when one exists.
    #[must_use]
    pub fn owner(&self) -> Option<&StableId> {
        match self {
            Self::Owned(owner) => Some(owner),
            Self::NotApplicable | Self::ExplicitlyUnowned { .. } => None,
        }
    }

    fn diagnostic(&self) -> String {
        match self {
            Self::NotApplicable => "not-applicable".to_string(),
            Self::Owned(owner) => format!("owned:{}", owner.as_str()),
            Self::ExplicitlyUnowned { rationale } => {
                format!("explicitly-unowned:{}", rationale.as_str())
            }
        }
    }
}

/// Discretization selected for a neutral port shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDiscretization {
    /// Scalar/vector/tensor port coordinates represented as one raw block.
    Lumped,
    /// Field-duality port with explicit nonzero effort/flow dof counts.
    Field {
        /// Total effort-coordinate dofs, including all components.
        effort_dofs: NonZeroUsize,
        /// Total flow-coordinate dofs, including all components.
        flow_dofs: NonZeroUsize,
    },
}

impl PortDiscretization {
    /// Canonical lumped port discretization.
    #[must_use]
    pub const fn lumped() -> Self {
        Self::Lumped
    }

    /// Construct a field discretization without admitting empty vectors.
    ///
    /// # Errors
    /// [`PortEquationError::ZeroFieldDofs`] names the empty side.
    pub fn field(effort_dofs: usize, flow_dofs: usize) -> Result<Self, PortEquationError> {
        let effort_dofs = NonZeroUsize::new(effort_dofs)
            .ok_or(PortEquationError::ZeroFieldDofs { variable: "effort" })?;
        let flow_dofs = NonZeroUsize::new(flow_dofs)
            .ok_or(PortEquationError::ZeroFieldDofs { variable: "flow" })?;
        Ok(Self::Field {
            effort_dofs,
            flow_dofs,
        })
    }
}

/// Complete request to compile one admitted port schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortEquationSpec {
    schema: PortSchema,
    discretization: PortDiscretization,
    sense: PortEquationSense,
    term_kind: AccountingTermKind,
    ownership: OwnershipDisposition,
}

impl PortEquationSpec {
    /// Bind one port schema to its discretization, algebraic sense, and exact
    /// accounting ownership declaration. Validation occurs transactionally in
    /// [`compile_port_equation`] or [`compile_port_equations`].
    #[must_use]
    pub fn new(
        schema: PortSchema,
        discretization: PortDiscretization,
        sense: PortEquationSense,
        term_kind: AccountingTermKind,
        ownership: OwnershipDisposition,
    ) -> Self {
        Self {
            schema,
            discretization,
            sense,
            term_kind,
            ownership,
        }
    }

    /// Borrow the neutral source schema.
    #[must_use]
    pub const fn schema(&self) -> &PortSchema {
        &self.schema
    }
}

/// Structured refusal from port-equation lowering.
#[derive(Debug, PartialEq)]
pub enum PortEquationError {
    /// A deterministic batch was empty.
    EmptyBatch,
    /// The request exceeded the static equation ceiling.
    TooManyEquations {
        /// Requested equations.
        count: usize,
        /// Static ceiling.
        cap: usize,
    },
    /// Two source declarations used the same port identity.
    DuplicatePortId {
        /// Duplicated stable port identity.
        port: String,
    },
    /// One owner was assigned to two independently generated terms.
    DuplicateOwnership {
        /// Duplicated owner identity.
        owner: String,
        /// First port using the owner.
        first_port: String,
        /// Second port using the owner.
        second_port: String,
    },
    /// A field discretization had zero dofs on one side.
    ZeroFieldDofs {
        /// `effort` or `flow`.
        variable: &'static str,
    },
    /// Lumped and field port shapes used incompatible discretizations.
    DiscretizationMismatch {
        /// Shape expected by the schema.
        expected: &'static str,
        /// Discretization supplied by the caller.
        actual: &'static str,
    },
    /// Field dofs did not contain a whole number of component tuples.
    FieldComponentMismatch {
        /// `effort` or `flow`.
        variable: &'static str,
        /// Total dofs.
        dofs: usize,
        /// Components per field point.
        components: usize,
    },
    /// Tensor component count overflowed the platform index type.
    ShapeExtentOverflow,
    /// The upstream schema version is not the version this compiler binds.
    PortSchemaVersionMismatch {
        /// Supported schema version.
        expected: u16,
        /// Received schema version.
        actual: u16,
    },
    /// A future/upstream schema no longer re-derived exact power dimensions.
    SchemaPowerDrift {
        /// Effort dimensions.
        effort: Dims,
        /// Flow dimensions.
        flow: Dims,
        /// Integration-measure dimensions.
        measure: Dims,
    },
    /// The ownership declaration contradicted the accounting term kind.
    OwnershipMismatch {
        /// Declared accounting role.
        term_kind: AccountingTermKind,
        /// Stable diagnostic for the supplied disposition.
        disposition: String,
    },
    /// Identity-bearing compiler metadata exceeded the system extension cap.
    CompilerMetadataTooLarge {
        /// Estimated bytes.
        bytes: usize,
        /// Static cap.
        cap: usize,
    },
    /// A bounded metadata allocation was refused.
    Resource,
    /// The underlying system IR refused the generated structure.
    System(Box<SystemTypeError>),
}

impl core::fmt::Display for PortEquationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyBatch => f.write_str("port-equation batch is empty"),
            Self::TooManyEquations { count, cap } => {
                write!(f, "port-equation count {count} exceeds cap {cap}")
            }
            Self::DuplicatePortId { port } => {
                write!(f, "port identity {port} appears more than once")
            }
            Self::DuplicateOwnership {
                owner,
                first_port,
                second_port,
            } => write!(
                f,
                "accounting owner {owner} is assigned to both ports {first_port} and {second_port}"
            ),
            Self::ZeroFieldDofs { variable } => {
                write!(f, "field port has zero {variable} dofs")
            }
            Self::DiscretizationMismatch { expected, actual } => write!(
                f,
                "port shape requires {expected} discretization, received {actual}"
            ),
            Self::FieldComponentMismatch {
                variable,
                dofs,
                components,
            } => write!(
                f,
                "field port {variable} dof count {dofs} is not divisible by component count {components}"
            ),
            Self::ShapeExtentOverflow => {
                f.write_str("tensor port component count overflowed usize")
            }
            Self::PortSchemaVersionMismatch { expected, actual } => write!(
                f,
                "port schema version {actual} is unsupported; expected {expected}"
            ),
            Self::SchemaPowerDrift {
                effort,
                flow,
                measure,
            } => write!(
                f,
                "port schema no longer re-derives power dimensions: effort {effort:?}, flow {flow:?}, measure {measure:?}"
            ),
            Self::OwnershipMismatch {
                term_kind,
                disposition,
            } => write!(
                f,
                "{} term has inadmissible ownership disposition {disposition}",
                term_kind.as_str()
            ),
            Self::CompilerMetadataTooLarge { bytes, cap } => write!(
                f,
                "port compiler metadata estimate {bytes} bytes exceeds cap {cap}"
            ),
            Self::Resource => f.write_str("port compiler metadata allocation was refused"),
            Self::System(error) => write!(f, "generated system IR refused: {error}"),
        }
    }
}

impl std::error::Error for PortEquationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::System(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<SystemTypeError> for PortEquationError {
    fn from(value: SystemTypeError) -> Self {
        Self::System(Box::new(value))
    }
}

/// Structural proof receipt for one generated port power equation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortEquationReceipt {
    port_id: String,
    port_schema_version: u16,
    system_identity: SystemId,
    kind: PortKind,
    pairing: PowerPairing,
    effort_dims: Dims,
    flow_dims: Dims,
    measure_dims: Dims,
    product_dims: Dims,
    sense: PortEquationSense,
    term_kind: AccountingTermKind,
    ownership: OwnershipDisposition,
    loss_ownership_id: Option<LossOwnershipId>,
}

impl PortEquationReceipt {
    /// Stable source port identity.
    #[must_use]
    pub fn port_id(&self) -> &str {
        &self.port_id
    }

    /// Neutral source schema version bound by this compiler.
    #[must_use]
    pub const fn port_schema_version(&self) -> u16 {
        self.port_schema_version
    }

    /// Identity of the generated, fully admitted system fragment.
    #[must_use]
    pub const fn system_identity(&self) -> SystemId {
        self.system_identity
    }

    /// Physical effort/flow port kind.
    #[must_use]
    pub const fn kind(&self) -> PortKind {
        self.kind
    }

    /// Exact contraction declared by the neutral schema.
    #[must_use]
    pub const fn pairing(&self) -> PowerPairing {
        self.pairing
    }

    /// Effort-coordinate dimensions before measure application.
    #[must_use]
    pub const fn effort_dims(&self) -> Dims {
        self.effort_dims
    }

    /// Flow-coordinate dimensions before measure application.
    #[must_use]
    pub const fn flow_dims(&self) -> Dims {
        self.flow_dims
    }

    /// Integration-measure dimensions used by the contraction.
    #[must_use]
    pub const fn measure_dims(&self) -> Dims {
        self.measure_dims
    }

    /// Explicit declared/reversed algebraic sense.
    #[must_use]
    pub const fn sense(&self) -> PortEquationSense {
        self.sense
    }

    /// Exact multiplier inserted for orientation handling.
    #[must_use]
    pub const fn sign(&self) -> i8 {
        self.sense.sign()
    }

    /// Declared accounting role.
    #[must_use]
    pub const fn term_kind(&self) -> AccountingTermKind {
        self.term_kind
    }

    /// Exact ownership disposition retained by the compiler.
    #[must_use]
    pub const fn ownership(&self) -> &OwnershipDisposition {
        &self.ownership
    }

    /// Compiler-derived nominal identity for a concretely owned dissipative
    /// term. Explicitly unowned losses and non-loss roles return `None`.
    #[must_use]
    pub const fn loss_ownership_id(&self) -> Option<LossOwnershipId> {
        self.loss_ownership_id
    }

    /// Re-derived power dimensions after the pairing measure is applied.
    #[must_use]
    pub const fn product_dims(&self) -> Dims {
        self.product_dims
    }

    /// Deterministic diagnostic transport. This is a receipt view, not a
    /// substitute for the typed [`SystemId`].
    #[must_use]
    pub fn to_json(&self) -> String {
        let loss_ownership_id = self
            .loss_ownership_id
            .map_or_else(|| "null".to_string(), |id| format!("\"{}\"", id.to_hex()));
        format!(
            "{{\"schema\":\"{}\",\"port_id\":\"{}\",\
             \"compiler_version\":\"{}\",\"feature\":\"port-equations\",\
             \"port_schema_version\":{},\"system_id\":\"{}\",\
             \"port_kind\":\"{}\",\"pairing\":\"{}\",\
             \"effort_dims\":{},\"flow_dims\":{},\"measure_dims\":{},\
             \"product_dims\":{},\"sense\":\"{}\",\"sign\":{},\
             \"term_kind\":\"{}\",\"ownership\":\"{}\",\
             \"loss_ownership_id\":{},\
             \"authority\":\"structural-generated\",\
             \"no_claim\":\"numeric contraction, quadrature, adapter truth, and closed-window conservation remain external\"}}",
            PORT_EQUATION_RECEIPT_SCHEMA_V1,
            self.port_id,
            crate::VERSION,
            self.port_schema_version,
            self.system_identity,
            port_kind_name(self.kind),
            pairing_name(self.pairing),
            dims_json(self.effort_dims),
            dims_json(self.flow_dims),
            dims_json(self.measure_dims),
            dims_json(self.product_dims),
            self.sense.as_str(),
            self.sign(),
            self.term_kind.as_str(),
            self.ownership.diagnostic(),
            loss_ownership_id,
        )
    }
}

/// One generated and fully admitted system fragment plus its receipt.
#[derive(Debug)]
pub struct CompiledPortEquation {
    system: AdmittedSystem,
    receipt: PortEquationReceipt,
}

impl CompiledPortEquation {
    /// Borrow the admitted system fragment.
    #[must_use]
    pub const fn system(&self) -> &AdmittedSystem {
        &self.system
    }

    /// Borrow the structural lowering receipt.
    #[must_use]
    pub const fn receipt(&self) -> &PortEquationReceipt {
        &self.receipt
    }

    /// Consume the wrapper and return the admitted system fragment.
    #[must_use]
    pub fn into_system(self) -> AdmittedSystem {
        self.system
    }
}

/// Canonically port-ID-ordered result of a transactional compile batch.
#[derive(Debug)]
pub struct PortEquationBatch {
    equations: Vec<CompiledPortEquation>,
}

impl PortEquationBatch {
    /// Generated equations in canonical source-port order.
    #[must_use]
    pub fn equations(&self) -> &[CompiledPortEquation] {
        &self.equations
    }

    /// Consume the batch.
    #[must_use]
    pub fn into_equations(self) -> Vec<CompiledPortEquation> {
        self.equations
    }
}

/// Compile one neutral port declaration transactionally.
///
/// # Errors
/// Any schema, discretization, ownership, metadata, or system-IR refusal.
pub fn compile_port_equation(
    spec: PortEquationSpec,
) -> Result<CompiledPortEquation, PortEquationError> {
    validate_ownership(spec.term_kind, &spec.ownership)?;
    compile_one(spec)
}

/// Compile a deterministic batch. Input order is irrelevant; duplicate port
/// identities and duplicate concrete owners refuse before any output is
/// returned.
///
/// # Errors
/// Any batch, ownership, schema, discretization, metadata, or system refusal.
pub fn compile_port_equations(
    mut specs: Vec<PortEquationSpec>,
) -> Result<PortEquationBatch, PortEquationError> {
    if specs.is_empty() {
        return Err(PortEquationError::EmptyBatch);
    }
    if specs.len() > MAX_PORT_EQUATIONS {
        return Err(PortEquationError::TooManyEquations {
            count: specs.len(),
            cap: MAX_PORT_EQUATIONS,
        });
    }
    specs.sort_by(|left, right| left.schema.id().cmp(right.schema.id()));
    for pair in specs.windows(2) {
        if pair[0].schema.id() == pair[1].schema.id() {
            return Err(PortEquationError::DuplicatePortId {
                port: pair[0].schema.id().as_str().to_string(),
            });
        }
    }

    let mut owner_ports: BTreeMap<String, String> = BTreeMap::new();
    for spec in &specs {
        validate_ownership(spec.term_kind, &spec.ownership)?;
        extension_estimate(spec)?;
        if let Some(owner) = spec.ownership.owner()
            && let Some(first_port) = owner_ports.insert(
                owner.as_str().to_string(),
                spec.schema.id().as_str().to_string(),
            )
        {
            return Err(PortEquationError::DuplicateOwnership {
                owner: owner.as_str().to_string(),
                first_port,
                second_port: spec.schema.id().as_str().to_string(),
            });
        }
    }

    let mut equations = Vec::new();
    equations
        .try_reserve_exact(specs.len())
        .map_err(|_| PortEquationError::Resource)?;
    for spec in specs {
        equations.push(compile_one(spec)?);
    }
    Ok(PortEquationBatch { equations })
}

fn validate_ownership(
    term_kind: AccountingTermKind,
    ownership: &OwnershipDisposition,
) -> Result<(), PortEquationError> {
    let admitted = match term_kind {
        AccountingTermKind::Reversible => {
            matches!(ownership, OwnershipDisposition::NotApplicable)
        }
        AccountingTermKind::Storage => matches!(ownership, OwnershipDisposition::Owned(_)),
        AccountingTermKind::Source | AccountingTermKind::Dissipation => matches!(
            ownership,
            OwnershipDisposition::Owned(_) | OwnershipDisposition::ExplicitlyUnowned { .. }
        ),
    };
    if admitted {
        Ok(())
    } else {
        Err(PortEquationError::OwnershipMismatch {
            term_kind,
            disposition: ownership.diagnostic(),
        })
    }
}

fn compile_one(spec: PortEquationSpec) -> Result<CompiledPortEquation, PortEquationError> {
    if spec.schema.version() != PORT_SCHEMA_VERSION {
        return Err(PortEquationError::PortSchemaVersionMismatch {
            expected: PORT_SCHEMA_VERSION,
            actual: spec.schema.version(),
        });
    }
    let (effort_space, flow_space) = resolve_spaces(&spec.schema, spec.discretization)?;
    let measure_dims = pairing_measure(spec.schema.power_pairing());
    let product_dims = spec
        .schema
        .effort_dimensions()
        .checked_plus(spec.schema.flow_dimensions())
        .and_then(|sum| sum.checked_plus(measure_dims))
        .ok_or(PortEquationError::SchemaPowerDrift {
            effort: spec.schema.effort_dimensions(),
            flow: spec.schema.flow_dimensions(),
            measure: measure_dims,
        })?;
    if product_dims != POWER_DIMS {
        return Err(PortEquationError::SchemaPowerDrift {
            effort: spec.schema.effort_dimensions(),
            flow: spec.schema.flow_dimensions(),
            measure: measure_dims,
        });
    }

    let extension = encode_extension(&spec, effort_space, flow_space)?;
    let loss_ownership_id = derive_loss_ownership_id(&spec, effort_space, flow_space)?;
    let coordinates = coordinate_convention(&spec.schema)?;
    let clock = ConventionRef::new(spec.schema.timestamp().clock().as_str().to_string())?;
    let mut system = SystemDef::new().with_extension(extension)?;
    let effort = system.declare_field(FieldDecl {
        name: "port-effort".to_string(),
        space: effort_space,
        quantity: FieldQuantity::Dimensional(effort_space.dims),
        coordinates: coordinates.clone(),
        clock: clock.clone(),
        support: SpatialSupport::BoundaryTrace,
        state: StateOwnership::External,
    })?;
    let flow = system.declare_field(FieldDecl {
        name: "port-flow".to_string(),
        space: flow_space,
        quantity: FieldQuantity::Dimensional(flow_space.dims),
        coordinates: coordinates.clone(),
        clock: clock.clone(),
        support: SpatialSupport::BoundaryTrace,
        state: StateOwnership::External,
    })?;
    let power = system.declare_field(FieldDecl {
        name: "port-power".to_string(),
        space: Space {
            degree: 0,
            n: 1,
            dims: POWER_DIMS,
        },
        quantity: FieldQuantity::Dimensional(POWER_DIMS),
        coordinates,
        clock,
        support: SpatialSupport::BoundaryTrace,
        state: StateOwnership::External,
    })?;
    let pairing = SystemExpr::PortPair {
        kind: spec.schema.kind(),
        effort: Box::new(SystemExpr::FieldRef(effort)),
        flow: Box::new(SystemExpr::FieldRef(flow)),
        measure_dims,
    };
    let rhs = match spec.sense {
        PortEquationSense::AsDeclared => pairing,
        PortEquationSense::Reversed => SystemExpr::Scale(-1.0, Box::new(pairing)),
    };
    system.add_equation(BlockEquation {
        name: "port-power-balance".to_string(),
        target: power,
        rhs,
    })?;
    let system = system.admit()?;
    let receipt = PortEquationReceipt {
        port_id: spec.schema.id().as_str().to_string(),
        port_schema_version: spec.schema.version(),
        system_identity: system.identity(),
        kind: spec.schema.kind(),
        pairing: spec.schema.power_pairing(),
        effort_dims: spec.schema.effort_dimensions(),
        flow_dims: spec.schema.flow_dimensions(),
        measure_dims,
        product_dims,
        sense: spec.sense,
        term_kind: spec.term_kind,
        ownership: spec.ownership,
        loss_ownership_id,
    };
    Ok(CompiledPortEquation { system, receipt })
}

fn derive_loss_ownership_id(
    spec: &PortEquationSpec,
    effort_space: Space,
    flow_space: Space,
) -> Result<Option<LossOwnershipId>, PortEquationError> {
    let AccountingTermKind::Dissipation = spec.term_kind else {
        return Ok(None);
    };
    let OwnershipDisposition::Owned(_) = &spec.ownership else {
        return Ok(None);
    };
    let mut canonical = spec.clone();
    canonical.sense = PortEquationSense::AsDeclared;
    let payload = encode_extension(&canonical, effort_space, flow_space)?;
    Ok(Some(LossOwnershipId(fs_blake3::hash_domain(
        LOSS_OWNERSHIP_DOMAIN_V1,
        &payload,
    ))))
}

fn resolve_spaces(
    schema: &PortSchema,
    discretization: PortDiscretization,
) -> Result<(Space, Space), PortEquationError> {
    let (effort_degree, effort_dofs, flow_degree, flow_dofs) =
        match (schema.shape(), discretization) {
            (PortValueShape::Scalar, PortDiscretization::Lumped) => {
                (RAW_VECTOR_DEGREE, 1, RAW_VECTOR_DEGREE, 1)
            }
            (PortValueShape::Vector(components), PortDiscretization::Lumped) => (
                RAW_VECTOR_DEGREE,
                components.get(),
                RAW_VECTOR_DEGREE,
                components.get(),
            ),
            (PortValueShape::Tensor { rows, columns }, PortDiscretization::Lumped) => {
                let components = rows
                    .get()
                    .checked_mul(columns.get())
                    .ok_or(PortEquationError::ShapeExtentOverflow)?;
                (RAW_VECTOR_DEGREE, components, RAW_VECTOR_DEGREE, components)
            }
            (
                PortValueShape::Field {
                    components,
                    effort_space,
                    flow_space,
                },
                PortDiscretization::Field {
                    effort_dofs,
                    flow_dofs,
                },
            ) => {
                for (variable, dofs) in [("effort", effort_dofs.get()), ("flow", flow_dofs.get())] {
                    if dofs % components.get() != 0 {
                        return Err(PortEquationError::FieldComponentMismatch {
                            variable,
                            dofs,
                            components: components.get(),
                        });
                    }
                }
                (
                    effort_space.form_degree(),
                    effort_dofs.get(),
                    flow_space.form_degree(),
                    flow_dofs.get(),
                )
            }
            (PortValueShape::Field { .. }, PortDiscretization::Lumped) => {
                return Err(PortEquationError::DiscretizationMismatch {
                    expected: "field",
                    actual: "lumped",
                });
            }
            (_, PortDiscretization::Field { .. }) => {
                return Err(PortEquationError::DiscretizationMismatch {
                    expected: "lumped",
                    actual: "field",
                });
            }
        };
    Ok((
        Space {
            degree: effort_degree,
            n: effort_dofs,
            dims: schema.effort_dimensions(),
        },
        Space {
            degree: flow_degree,
            n: flow_dofs,
            dims: schema.flow_dimensions(),
        },
    ))
}

fn coordinate_convention(schema: &PortSchema) -> Result<CoordinateConvention, PortEquationError> {
    Ok(CoordinateConvention {
        basis: ConventionRef::new(schema.coordinates().basis().as_str().to_string())?,
        frame: ConventionRef::new(schema.coordinates().frame().as_str().to_string())?,
        orientation: schema.coordinates().orientation(),
    })
}

fn pairing_measure(pairing: PowerPairing) -> Dims {
    match pairing {
        PowerPairing::ScalarProduct | PowerPairing::EuclideanDot => Dims::NONE,
        PowerPairing::FieldDuality {
            measure_dimensions, ..
        } => measure_dimensions,
    }
}

fn encode_extension(
    spec: &PortEquationSpec,
    effort_space: Space,
    flow_space: Space,
) -> Result<Vec<u8>, PortEquationError> {
    let estimated = extension_estimate(spec)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(estimated)
        .map_err(|_| PortEquationError::Resource)?;
    bytes.extend_from_slice(&PORT_EXTENSION_VERSION.to_le_bytes());
    push_text(&mut bytes, crate::VERSION);
    bytes.extend_from_slice(&spec.schema.version().to_le_bytes());
    push_text(&mut bytes, spec.schema.id().as_str());
    bytes.push(port_kind_tag(spec.schema.kind()));
    push_dims(&mut bytes, spec.schema.effort_dimensions());
    push_dims(&mut bytes, spec.schema.flow_dimensions());
    encode_shape(&mut bytes, spec.schema.shape());
    push_text(&mut bytes, spec.schema.coordinates().basis().as_str());
    push_text(&mut bytes, spec.schema.coordinates().frame().as_str());
    bytes.push(orientation_tag(spec.schema.coordinates().orientation()));
    encode_pairing(&mut bytes, spec.schema.power_pairing());
    push_text(&mut bytes, spec.schema.timestamp().clock().as_str());
    bytes.extend_from_slice(&spec.schema.timestamp().tick().to_le_bytes());
    bytes.extend_from_slice(&(spec.schema.conservation_roles().len() as u64).to_le_bytes());
    for role in spec.schema.conservation_roles() {
        bytes.push(conservation_role_tag(*role));
    }
    bytes.push(spec.sense.sign() as u8);
    bytes.push(accounting_kind_tag(spec.term_kind));
    encode_ownership(&mut bytes, &spec.ownership);
    bytes.push(effort_space.degree);
    bytes.extend_from_slice(&(effort_space.n as u64).to_le_bytes());
    bytes.push(flow_space.degree);
    bytes.extend_from_slice(&(flow_space.n as u64).to_le_bytes());
    if bytes.len() > MAX_SYSTEM_EXTENSION_BYTES {
        return Err(PortEquationError::CompilerMetadataTooLarge {
            bytes: bytes.len(),
            cap: MAX_SYSTEM_EXTENSION_BYTES,
        });
    }
    Ok(bytes)
}

fn extension_estimate(spec: &PortEquationSpec) -> Result<usize, PortEquationError> {
    let ownership_bytes = match &spec.ownership {
        OwnershipDisposition::NotApplicable => 0,
        OwnershipDisposition::Owned(owner) => owner.as_str().len(),
        OwnershipDisposition::ExplicitlyUnowned { rationale } => rationale.as_str().len(),
    };
    let mut variable_bytes = 0usize;
    for len in [
        crate::VERSION.len(),
        spec.schema.id().as_str().len(),
        spec.schema.coordinates().basis().as_str().len(),
        spec.schema.coordinates().frame().as_str().len(),
        spec.schema.timestamp().clock().as_str().len(),
        ownership_bytes,
    ] {
        variable_bytes =
            variable_bytes
                .checked_add(len)
                .ok_or(PortEquationError::CompilerMetadataTooLarge {
                    bytes: usize::MAX,
                    cap: MAX_SYSTEM_EXTENSION_BYTES,
                })?;
    }
    let estimated =
        variable_bytes
            .checked_add(256)
            .ok_or(PortEquationError::CompilerMetadataTooLarge {
                bytes: usize::MAX,
                cap: MAX_SYSTEM_EXTENSION_BYTES,
            })?;
    if estimated > MAX_SYSTEM_EXTENSION_BYTES {
        return Err(PortEquationError::CompilerMetadataTooLarge {
            bytes: estimated,
            cap: MAX_SYSTEM_EXTENSION_BYTES,
        });
    }
    Ok(estimated)
}

fn push_text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn push_dims(bytes: &mut Vec<u8>, dims: Dims) {
    bytes.extend(dims.0.map(|exponent| exponent as u8));
}

fn encode_shape(bytes: &mut Vec<u8>, shape: PortValueShape) {
    match shape {
        PortValueShape::Scalar => bytes.push(0),
        PortValueShape::Vector(components) => {
            bytes.push(1);
            bytes.extend_from_slice(&(components.get() as u64).to_le_bytes());
        }
        PortValueShape::Tensor { rows, columns } => {
            bytes.push(2);
            bytes.extend_from_slice(&(rows.get() as u64).to_le_bytes());
            bytes.extend_from_slice(&(columns.get() as u64).to_le_bytes());
        }
        PortValueShape::Field {
            components,
            effort_space,
            flow_space,
        } => {
            bytes.push(3);
            bytes.extend_from_slice(&(components.get() as u64).to_le_bytes());
            bytes.push(space_type_tag(effort_space));
            bytes.push(space_type_tag(flow_space));
        }
    }
}

fn encode_pairing(bytes: &mut Vec<u8>, pairing: PowerPairing) {
    match pairing {
        PowerPairing::ScalarProduct => bytes.push(0),
        PowerPairing::EuclideanDot => bytes.push(1),
        PowerPairing::FieldDuality {
            measure_dimensions,
            measure_side,
        } => {
            bytes.push(2);
            push_dims(bytes, measure_dimensions);
            bytes.push(match measure_side {
                FieldMeasureSide::Effort => 0,
                FieldMeasureSide::Flow => 1,
            });
        }
    }
}

fn encode_ownership(bytes: &mut Vec<u8>, ownership: &OwnershipDisposition) {
    match ownership {
        OwnershipDisposition::NotApplicable => bytes.push(0),
        OwnershipDisposition::Owned(owner) => {
            bytes.push(1);
            push_text(bytes, owner.as_str());
        }
        OwnershipDisposition::ExplicitlyUnowned { rationale } => {
            bytes.push(2);
            push_text(bytes, rationale.as_str());
        }
    }
}

const fn port_kind_tag(kind: PortKind) -> u8 {
    match kind {
        PortKind::MechanicalForceVelocity => 0,
        PortKind::FluidPressureFlux => 1,
        PortKind::ThermalTemperatureEntropy => 2,
        PortKind::RotationalTorqueAngularVelocity => 3,
        PortKind::ElectricalVoltageCurrent => 4,
        PortKind::MagneticMmfFluxRate => 5,
        PortKind::ChemicalPotentialAmountFlow => 6,
    }
}

const fn port_kind_name(kind: PortKind) -> &'static str {
    match kind {
        PortKind::MechanicalForceVelocity => "mechanical-force-velocity",
        PortKind::FluidPressureFlux => "fluid-pressure-flux",
        PortKind::ThermalTemperatureEntropy => "thermal-temperature-entropy",
        PortKind::RotationalTorqueAngularVelocity => "rotational-torque-angular-velocity",
        PortKind::ElectricalVoltageCurrent => "electrical-voltage-current",
        PortKind::MagneticMmfFluxRate => "magnetic-mmf-flux-rate",
        PortKind::ChemicalPotentialAmountFlow => "chemical-potential-amount-flow",
    }
}

const fn pairing_name(pairing: PowerPairing) -> &'static str {
    match pairing {
        PowerPairing::ScalarProduct => "scalar-product",
        PowerPairing::EuclideanDot => "euclidean-dot",
        PowerPairing::FieldDuality { .. } => "field-duality",
    }
}

const fn orientation_tag(orientation: PortOrientation) -> u8 {
    match orientation {
        PortOrientation::OutwardFromOwner => 0,
        PortOrientation::AlongFrame => 1,
        PortOrientation::AgainstFrame => 2,
    }
}

const fn accounting_kind_tag(kind: AccountingTermKind) -> u8 {
    match kind {
        AccountingTermKind::Reversible => 0,
        AccountingTermKind::Storage => 1,
        AccountingTermKind::Source => 2,
        AccountingTermKind::Dissipation => 3,
    }
}

const fn conservation_role_tag(role: ConservationRole) -> u8 {
    match role {
        ConservationRole::Energy => 0,
        ConservationRole::Mass => 1,
        ConservationRole::Amount => 2,
        ConservationRole::LinearMomentum => 3,
        ConservationRole::AngularMomentum => 4,
        ConservationRole::Entropy => 5,
        ConservationRole::ElectricCharge => 6,
    }
}

const fn space_type_tag(space: SpaceType) -> u8 {
    match space {
        SpaceType::HGrad => 0,
        SpaceType::HCurl => 1,
        SpaceType::HDiv => 2,
        SpaceType::L2 => 3,
    }
}

fn dims_json(dims: Dims) -> String {
    format!(
        "[{},{},{},{},{},{}]",
        dims.0[0], dims.0[1], dims.0[2], dims.0[3], dims.0[4], dims.0[5]
    )
}
