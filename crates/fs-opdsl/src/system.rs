//! Multi-field equation/type IR with a stable semantic identity
//! (bead i94v.1.1.1). This is the neutral compiler-facing layer above
//! the single-field [`crate::expr`] machinery: block fields declare
//! their differential-form degree, basis/frame/orientation, quantity
//! kind, state ownership, clock, spatial support, and conservation
//! roles EXPLICITLY, and every cross-field composition is admitted
//! before any lowering. Ill-typed contractions, mixed frames, affine
//! temperature misuse, clock mismatches, and non-power-conjugate port
//! pairings are structured refusals — they cannot reach code
//! generation.
//!
//! Identity discipline: a [`SystemDef`]'s [`SystemId`] hashes the
//! canonical STRUCTURE and semantic payloads only. Field and equation
//! display names are never hash inputs, and both tables are re-sorted
//! into a canonical order before encoding, so equivalent renaming or
//! serialization order preserves identity while any meaningful
//! convention change (degree, dims, quantity kind, frame, orientation,
//! clock, support, state slot, pairing, scalar convention) moves it.
//! Two fields whose complete semantic payloads are byte-identical are
//! refused as ambiguous rather than silently tie-broken.

use crate::expr::Space;
use fs_blake3::identity::{
    CanonicalEncoder, CanonicalLimits, CanonicalSchema, Field, FieldSpec, NeverCancel, SemanticId,
    WireType,
};
use fs_couple::{PortKind, PortOrientation};
use fs_qty::Dims;
use fs_qty::semantic::SemanticType;

/// IR language version for the multi-field system surface. Bound into
/// every [`SystemId`]; parsers/consumers refuse other versions unless
/// an explicit audited migration runs first.
pub const SYSTEM_IR_VERSION: u32 = 1;

/// Depth cap for [`SystemExpr`] trees: adversarial nesting refuses
/// structurally instead of exhausting the stack (all traversals here
/// are explicit-stack iterative; the cap bounds work, not recursion).
pub const MAX_SYSTEM_EXPR_DEPTH: usize = 512;

/// Maximum fields/equations per system: a bounded, auditable IR.
pub const MAX_SYSTEM_FIELDS: usize = 256;

/// Maximum opaque-extension bytes retained (and hashed) per system.
pub const MAX_SYSTEM_EXTENSION_BYTES: usize = 4096;

const IDENTITY_LIMITS: CanonicalLimits = CanonicalLimits::new(1 << 20, 1 << 18, 64, 16_384, 4096);
const POWER_DIMS: Dims = Dims([2, 1, -3, 0, 0, 0]);

/// A validated machine identifier for frames, bases, charts, and
/// clocks: 1..=64 ASCII graphic bytes. These are REFERENCES into
/// caller-owned registries — the IR binds which frame/clock a field
/// uses, never what the frame/clock is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConventionRef(String);

impl ConventionRef {
    /// Validate and wrap a convention reference.
    ///
    /// # Errors
    /// [`SystemTypeError::InvalidConventionRef`] for empty, oversized,
    /// or non-graphic-ASCII input.
    pub fn new(raw: impl Into<String>) -> Result<Self, SystemTypeError> {
        let raw = raw.into();
        if raw.is_empty() || raw.len() > 64 || !raw.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(SystemTypeError::InvalidConventionRef { raw });
        }
        Ok(Self(raw))
    }

    /// The validated reference text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a field's stored values mean, beyond raw dimensions.
/// Dimensional equality is not semantic equality (fs-qty semantic
/// kinds): torque is not energy, absolute temperature is not a
/// temperature difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldQuantity {
    /// Plain dimensional values with no further semantic claim.
    Dimensional(Dims),
    /// A semantic quantity kind + value form; its expected dims are
    /// authoritative and must agree with the field's [`Space`].
    Semantic(SemanticType),
}

impl FieldQuantity {
    /// The dimensions this quantity carries.
    #[must_use]
    pub fn dims(&self) -> Dims {
        match self {
            FieldQuantity::Dimensional(dims) => *dims,
            FieldQuantity::Semantic(semantic) => semantic.expected_dims(),
        }
    }

    /// Affine absolute quantities (absolute temperature) admit neither
    /// scaling nor summation — only differences move them.
    #[must_use]
    pub fn is_affine_absolute(&self) -> bool {
        matches!(
            self,
            FieldQuantity::Semantic(semantic)
                if semantic.kind() == fs_qty::semantic::QuantityKind::AbsoluteTemperature
        )
    }
}

/// Where a field lives on the domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialSupport {
    /// Values over the interior of the named chart's domain.
    Interior,
    /// A boundary trace (the k-form restricted to the boundary).
    BoundaryTrace,
}

/// Who owns a field's storage across time steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOwnership {
    /// Evolving state owned by this system in a distinct slot.
    Owned {
        /// The state-slot index; distinct per owned field.
        slot: u32,
    },
    /// Read-only values supplied per evaluation by the caller.
    External,
    /// A parameter (design/material/control) with a declared role tag.
    Parameter {
        /// Which parameter table this field indexes.
        role: ParameterRole,
    },
}

/// The role a parameter field plays (identity-bearing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterRole {
    /// Design/geometry parameters.
    Design,
    /// Material/constitutive parameters.
    Material,
    /// Control/actuation parameters.
    Control,
}

/// The coordinate convention a field's components are expressed in:
/// which basis and frame (as references) and the orientation sense.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinateConvention {
    /// Basis reference (e.g. a chart's coordinate basis id).
    pub basis: ConventionRef,
    /// Frame reference (observer/material frame id).
    pub frame: ConventionRef,
    /// Orientation sense, reusing the port-schema vocabulary so port
    /// pairings and field conventions cannot drift apart.
    pub orientation: PortOrientation,
}

/// Scalar convention for the whole system (identity-bearing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarConvention {
    /// Real arithmetic only.
    RealOnly,
    /// Complex fields under the Hermitian inner-product convention
    /// (conjugate-linear in the FIRST argument).
    ComplexHermitian,
}

/// One declared block field. The `name` is DISPLAY ONLY — never a hash
/// input; every other member is identity-bearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    /// Human-facing display name (diagnostics only).
    pub name: String,
    /// The field's discrete space: cochain degree, dof count, dims.
    pub space: Space,
    /// What the values mean (dims or a semantic quantity kind).
    pub quantity: FieldQuantity,
    /// Basis/frame/orientation convention references.
    pub coordinates: CoordinateConvention,
    /// The time clock this field advances on (reference).
    pub clock: ConventionRef,
    /// Interior or boundary-trace support.
    pub support: SpatialSupport,
    /// State ownership.
    pub state: StateOwnership,
}

/// A checked handle into a [`SystemDef`]'s field table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldId(pub usize);

/// The multi-field expression tree. Cross-field structure only —
/// single-field operator chains keep using [`crate::expr::Expr`]
/// inside each block via atoms.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemExpr {
    /// A declared field's current value.
    FieldRef(FieldId),
    /// Apply a registered single-field atom (by index into the
    /// system's atom space table) to a sub-expression.
    Apply {
        /// Index into [`SystemDef`]'s atom-space table.
        atom: usize,
        /// The argument.
        arg: Box<SystemExpr>,
    },
    /// Scale by a real constant (refused on affine-absolute operands).
    Scale(f64, Box<SystemExpr>),
    /// Sum of two expressions in the same admitted space/convention.
    Add(Box<SystemExpr>, Box<SystemExpr>),
    /// A power-conjugate pairing of two sub-expressions (effort, flow)
    /// under a declared port kind: admitted only when the operand
    /// dimensions multiply (with the measure) to power.
    PortPair {
        /// The physical port family this pairing claims.
        kind: PortKind,
        /// Effort side.
        effort: Box<SystemExpr>,
        /// Flow side.
        flow: Box<SystemExpr>,
        /// Integration-measure dimensions applied by the pairing
        /// (e.g. volume for field duality; NONE for scalar ports).
        measure_dims: Dims,
    },
}

/// An atom signature registered on the system: the in/out spaces plus
/// the coordinate convention and clock it preserves. (The numerical
/// atom itself stays in the single-field layer; the system IR only
/// needs its type.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomSignature {
    /// Display name (diagnostics only; not a hash input).
    pub name: String,
    /// Input space.
    pub in_space: Space,
    /// Output space.
    pub out_space: Space,
}

/// One block equation: `residual(target) = rhs`, i.e. the rhs
/// expression contributes to the target field's residual block. The
/// rhs space must exactly match the target field's space, convention,
/// and clock.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEquation {
    /// Display name (diagnostics only; not a hash input).
    pub name: String,
    /// The field whose residual block this equation defines.
    pub target: FieldId,
    /// The right-hand side.
    pub rhs: SystemExpr,
}

/// Structured refusals: every admissibility failure names what failed
/// and both sides, with a teaching hint. These fire BEFORE lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemTypeError {
    /// A convention reference was empty/oversized/non-ASCII-graphic.
    InvalidConventionRef {
        /// The offending raw text.
        raw: String,
    },
    /// A field/atom/equation id out of range.
    UnknownId {
        /// Which table.
        what: &'static str,
        /// The offending index.
        id: usize,
    },
    /// A semantic quantity kind whose expected dims disagree with the
    /// field's space dims.
    QuantityDimsMismatch {
        /// Field display name.
        field: String,
        /// Dims declared on the space.
        space_dims: Dims,
        /// Dims the semantic kind requires.
        kind_dims: Dims,
    },
    /// Two owned fields share a state slot.
    DuplicateStateSlot {
        /// The shared slot.
        slot: u32,
        /// First field display name.
        first: String,
        /// Second field display name.
        second: String,
    },
    /// Cross-field composition across different clocks.
    ClockMismatch {
        /// Left clock reference.
        left: String,
        /// Right clock reference.
        right: String,
    },
    /// Cross-field composition across different frames/bases or
    /// non-composable orientations.
    ConventionMismatch {
        /// Left (basis, frame) references.
        left: (String, String),
        /// Right (basis, frame) references.
        right: (String, String),
    },
    /// Sum/apply across mismatched spaces (degree, dof count, dims).
    SpaceMismatch {
        /// What was being composed.
        context: &'static str,
        /// Expected space.
        expected: Space,
        /// Found space.
        found: Space,
    },
    /// Scaling or summing an affine absolute quantity (temperature):
    /// only differences move affine quantities.
    AffineQuantityMisuse {
        /// The operation attempted.
        operation: &'static str,
        /// The field display name carrying the affine kind.
        field: String,
    },
    /// A port pairing whose effort x flow (x measure) is not power.
    NonConjugatePairing {
        /// The claimed port family.
        kind: PortKind,
        /// Effort dims found.
        effort_dims: Dims,
        /// Flow dims found.
        flow_dims: Dims,
        /// Measure dims declared.
        measure_dims: Dims,
    },
    /// Expression nesting beyond [`MAX_SYSTEM_EXPR_DEPTH`].
    DepthExceeded {
        /// The cap that was crossed.
        cap: usize,
    },
    /// Table growth beyond [`MAX_SYSTEM_FIELDS`].
    TooManyFields {
        /// The cap.
        cap: usize,
    },
    /// Two fields whose complete semantic payloads are byte-identical:
    /// identity cannot order them canonically, so the system is
    /// ambiguous — distinguish them by slot, role, clock, or kind.
    IndistinguishableFields {
        /// First field display name.
        first: String,
        /// Second field display name.
        second: String,
    },
    /// Duplicate equations (identical target + canonical rhs).
    DuplicateEquation {
        /// The equation display name of the later duplicate.
        name: String,
    },
    /// Opaque extension payload beyond its byte bound.
    ExtensionTooLarge {
        /// Bytes supplied.
        len: usize,
        /// The cap.
        cap: usize,
    },
    /// The canonical identity encoder refused (resource bounds).
    IdentityEncoding {
        /// The encoder's message.
        detail: String,
    },
    /// A versioned payload from another IR version: run an explicit
    /// audited migration first.
    VersionMismatch {
        /// Version found.
        found: u32,
        /// Version this build reads/writes.
        supported: u32,
    },
    /// A non-finite scale constant.
    NonFiniteScale {
        /// The offending bits.
        bits: u64,
    },
}

impl std::fmt::Display for SystemTypeError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConventionRef { raw } => write!(
                out,
                "convention reference {raw:?} must be 1..=64 ASCII graphic bytes"
            ),
            Self::UnknownId { what, id } => write!(out, "unknown {what} id {id}"),
            Self::QuantityDimsMismatch {
                field,
                space_dims,
                kind_dims,
            } => write!(
                out,
                "field {field:?}: space dims {space_dims:?} disagree with the semantic kind's expected dims {kind_dims:?}"
            ),
            Self::DuplicateStateSlot {
                slot,
                first,
                second,
            } => write!(
                out,
                "fields {first:?} and {second:?} both own state slot {slot}"
            ),
            Self::ClockMismatch { left, right } => write!(
                out,
                "cross-field composition across clocks {left:?} and {right:?}: interpose an explicit clock-transfer operator"
            ),
            Self::ConventionMismatch { left, right } => write!(
                out,
                "cross-field composition across conventions (basis,frame) {left:?} vs {right:?}: interpose an explicit pullback"
            ),
            Self::SpaceMismatch {
                context,
                expected,
                found,
            } => write!(
                out,
                "{context}: expected space {expected:?}, found {found:?}"
            ),
            Self::AffineQuantityMisuse { operation, field } => write!(
                out,
                "{operation} on affine absolute quantity of field {field:?}: only differences move affine quantities"
            ),
            Self::NonConjugatePairing {
                kind,
                effort_dims,
                flow_dims,
                measure_dims,
            } => write!(
                out,
                "{kind:?} pairing is not power-conjugate: effort {effort_dims:?} x flow {flow_dims:?} x measure {measure_dims:?} != power"
            ),
            Self::DepthExceeded { cap } => write!(out, "expression depth exceeds the {cap} cap"),
            Self::TooManyFields { cap } => write!(out, "system exceeds the {cap}-field cap"),
            Self::IndistinguishableFields { first, second } => write!(
                out,
                "fields {first:?} and {second:?} have byte-identical semantic payloads: identity is ambiguous — distinguish by slot, role, clock, or kind"
            ),
            Self::DuplicateEquation { name } => {
                write!(out, "equation {name:?} duplicates an earlier one")
            }
            Self::ExtensionTooLarge { len, cap } => {
                write!(out, "opaque extension is {len} bytes; the cap is {cap}")
            }
            Self::IdentityEncoding { detail } => write!(out, "identity encoding refused: {detail}"),
            Self::VersionMismatch { found, supported } => write!(
                out,
                "system IR version {found} is not the supported {supported}: run an explicit audited migration"
            ),
            Self::NonFiniteScale { bits } => {
                write!(out, "scale constant is not finite (bits {bits:#018x})")
            }
        }
    }
}

impl std::error::Error for SystemTypeError {}

/// Identity schema for one admitted multi-field system.
pub struct SystemIdentitySchemaV1;

impl CanonicalSchema for SystemIdentitySchemaV1 {
    const DOMAIN: &'static str = "org.frankensim.fs-opdsl.system.v1";
    const NAME: &'static str = "multi-field-system";
    const VERSION: u32 = 1;
    const CONTEXT: &'static str = "canonical multi-field equation/type IR: fields and equations in canonical order, display names excluded";
    const FIELDS: &'static [FieldSpec] = &[
        FieldSpec::required("system-ir-version", WireType::U64),
        FieldSpec::required("scalar-convention", WireType::Variant),
        FieldSpec::required("atom-signatures", WireType::OrderedBytes),
        FieldSpec::required("fields", WireType::OrderedBytes),
        FieldSpec::required("equations", WireType::OrderedBytes),
        FieldSpec::optional_bytes("opaque-extension"),
    ];
}

/// The stable semantic identity of an admitted system.
pub type SystemId = SemanticId<SystemIdentitySchemaV1>;

/// A validated multi-field system: fields, atom signatures, and block
/// equations, all admitted, plus the canonical identity.
#[derive(Debug)]
pub struct AdmittedSystem {
    fields: Vec<FieldDecl>,
    atoms: Vec<AtomSignature>,
    equations: Vec<BlockEquation>,
    scalar_convention: ScalarConvention,
    extension: Vec<u8>,
    identity: SystemId,
}

impl AdmittedSystem {
    /// The declared fields, in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[FieldDecl] {
        &self.fields
    }

    /// The registered atom signatures.
    #[must_use]
    pub fn atoms(&self) -> &[AtomSignature] {
        &self.atoms
    }

    /// The block equations, in declaration order.
    #[must_use]
    pub fn equations(&self) -> &[BlockEquation] {
        &self.equations
    }

    /// The system's scalar convention.
    #[must_use]
    pub fn scalar_convention(&self) -> ScalarConvention {
        self.scalar_convention
    }

    /// The retained opaque extension bytes (identity-bearing).
    #[must_use]
    pub fn extension(&self) -> &[u8] {
        &self.extension
    }

    /// The stable semantic identity.
    #[must_use]
    pub fn identity(&self) -> SystemId {
        self.identity
    }
}

/// Builder for a multi-field system. Declaration order is preserved
/// for display; identity is order- and name-independent.
#[derive(Debug, Default)]
pub struct SystemDef {
    fields: Vec<FieldDecl>,
    atoms: Vec<AtomSignature>,
    equations: Vec<BlockEquation>,
    scalar_convention: Option<ScalarConvention>,
    extension: Vec<u8>,
}

impl SystemDef {
    /// An empty system under [`ScalarConvention::RealOnly`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the scalar convention (default [`ScalarConvention::RealOnly`]).
    #[must_use]
    pub fn scalar_convention(mut self, convention: ScalarConvention) -> Self {
        self.scalar_convention = Some(convention);
        self
    }

    /// Attach opaque extension bytes: retained verbatim and
    /// identity-bearing, so future dialects can extend without
    /// silently aliasing v1 identities.
    ///
    /// # Errors
    /// [`SystemTypeError::ExtensionTooLarge`] beyond the byte cap.
    pub fn with_extension(mut self, bytes: Vec<u8>) -> Result<Self, SystemTypeError> {
        if bytes.len() > MAX_SYSTEM_EXTENSION_BYTES {
            return Err(SystemTypeError::ExtensionTooLarge {
                len: bytes.len(),
                cap: MAX_SYSTEM_EXTENSION_BYTES,
            });
        }
        self.extension = bytes;
        Ok(self)
    }

    /// Declare a field.
    ///
    /// # Errors
    /// Refuses table overflow, semantic-kind/space dims disagreement,
    /// and duplicate owned state slots.
    pub fn declare_field(&mut self, field: FieldDecl) -> Result<FieldId, SystemTypeError> {
        if self.fields.len() >= MAX_SYSTEM_FIELDS {
            return Err(SystemTypeError::TooManyFields {
                cap: MAX_SYSTEM_FIELDS,
            });
        }
        if let FieldQuantity::Semantic(semantic) = &field.quantity {
            let kind_dims = semantic.expected_dims();
            if kind_dims != field.space.dims {
                return Err(SystemTypeError::QuantityDimsMismatch {
                    field: field.name.clone(),
                    space_dims: field.space.dims,
                    kind_dims,
                });
            }
        }
        if let StateOwnership::Owned { slot } = field.state {
            if let Some(previous) = self.fields.iter().find(
                |existing| matches!(existing.state, StateOwnership::Owned { slot: s } if s == slot),
            ) {
                return Err(SystemTypeError::DuplicateStateSlot {
                    slot,
                    first: previous.name.clone(),
                    second: field.name.clone(),
                });
            }
        }
        self.fields.push(field);
        Ok(FieldId(self.fields.len() - 1))
    }

    /// Register an atom signature.
    pub fn register_atom(&mut self, atom: AtomSignature) -> usize {
        self.atoms.push(atom);
        self.atoms.len() - 1
    }

    /// Add a block equation after full admissibility checking of its
    /// right-hand side against the target field.
    ///
    /// # Errors
    /// Any [`SystemTypeError`] the rhs or target binding produces.
    pub fn add_equation(&mut self, equation: BlockEquation) -> Result<(), SystemTypeError> {
        let target = self
            .fields
            .get(equation.target.0)
            .ok_or(SystemTypeError::UnknownId {
                what: "field",
                id: equation.target.0,
            })?;
        let admitted = self.admit_expr(&equation.rhs)?;
        if admitted.space != target.space {
            return Err(SystemTypeError::SpaceMismatch {
                context: "equation rhs vs target field",
                expected: target.space,
                found: admitted.space,
            });
        }
        if let Some(convention) = &admitted.convention {
            check_convention_match(convention, target)?;
        }
        self.equations.push(equation);
        Ok(())
    }

    /// Admit the whole system and mint its canonical identity.
    ///
    /// # Errors
    /// Any residual admissibility refusal (indistinguishable fields,
    /// duplicate equations, encoder bounds).
    pub fn admit(self) -> Result<AdmittedSystem, SystemTypeError> {
        let scalar_convention = self.scalar_convention.unwrap_or(ScalarConvention::RealOnly);

        // Canonical field payloads (names excluded).
        let mut field_payloads: Vec<(Vec<u8>, usize)> = self
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| (canonical_field_bytes(field), index))
            .collect();
        field_payloads.sort();
        for pair in field_payloads.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(SystemTypeError::IndistinguishableFields {
                    first: self.fields[pair[0].1].name.clone(),
                    second: self.fields[pair[1].1].name.clone(),
                });
            }
        }
        // Declaration ordinal -> canonical ordinal.
        let mut remap = vec![0usize; self.fields.len()];
        for (canonical, (_, declaration)) in field_payloads.iter().enumerate() {
            remap[*declaration] = canonical;
        }

        let mut atom_payloads: Vec<Vec<u8>> = self.atoms.iter().map(canonical_atom_bytes).collect();
        atom_payloads.sort();

        let mut equation_payloads: Vec<Vec<u8>> = self
            .equations
            .iter()
            .map(|equation| canonical_equation_bytes(equation, &remap))
            .collect();
        equation_payloads.sort();
        for pair in equation_payloads.windows(2) {
            if pair[0] == pair[1] {
                let duplicate = self
                    .equations
                    .last()
                    .map(|equation| equation.name.clone())
                    .unwrap_or_default();
                return Err(SystemTypeError::DuplicateEquation { name: duplicate });
            }
        }

        let encode = || -> Result<_, fs_blake3::identity::CanonicalError> {
            let mut encoder = CanonicalEncoder::<SystemId, _>::new(IDENTITY_LIMITS, NeverCancel)?;
            encoder = encoder.u64(
                Field::new(0, "system-ir-version"),
                u64::from(SYSTEM_IR_VERSION),
            )?;
            encoder = encoder.variant(
                Field::new(1, "scalar-convention"),
                match scalar_convention {
                    ScalarConvention::RealOnly => 0,
                    ScalarConvention::ComplexHermitian => 1,
                },
                &[],
            )?;
            encoder = encoder.ordered_bytes(
                Field::new(2, "atom-signatures"),
                atom_payloads.len() as u64,
                atom_payloads.iter().map(Vec::as_slice),
            )?;
            encoder = encoder.ordered_bytes(
                Field::new(3, "fields"),
                field_payloads.len() as u64,
                field_payloads.iter().map(|(bytes, _)| bytes.as_slice()),
            )?;
            encoder = encoder.ordered_bytes(
                Field::new(4, "equations"),
                equation_payloads.len() as u64,
                equation_payloads.iter().map(Vec::as_slice),
            )?;
            encoder = encoder.optional_bytes(
                Field::new(5, "opaque-extension"),
                if self.extension.is_empty() {
                    None
                } else {
                    Some(self.extension.as_slice())
                },
            )?;
            encoder.finish()
        };
        let receipt = encode().map_err(|error| SystemTypeError::IdentityEncoding {
            detail: error.to_string(),
        })?;

        Ok(AdmittedSystem {
            fields: self.fields,
            atoms: self.atoms,
            equations: self.equations,
            scalar_convention,
            extension: self.extension,
            identity: receipt.id(),
        })
    }

    /// Admit an expression: explicit-stack two-pass (validate + infer)
    /// so adversarial nesting cannot overflow the call stack.
    fn admit_expr(&self, root: &SystemExpr) -> Result<AdmittedExpr, SystemTypeError> {
        // Pass 1: depth + reference validation, iteratively.
        let mut stack: Vec<(&SystemExpr, usize)> = vec![(root, 1)];
        while let Some((node, depth)) = stack.pop() {
            if depth > MAX_SYSTEM_EXPR_DEPTH {
                return Err(SystemTypeError::DepthExceeded {
                    cap: MAX_SYSTEM_EXPR_DEPTH,
                });
            }
            match node {
                SystemExpr::FieldRef(field) => {
                    if field.0 >= self.fields.len() {
                        return Err(SystemTypeError::UnknownId {
                            what: "field",
                            id: field.0,
                        });
                    }
                }
                SystemExpr::Apply { atom, arg } => {
                    if *atom >= self.atoms.len() {
                        return Err(SystemTypeError::UnknownId {
                            what: "atom",
                            id: *atom,
                        });
                    }
                    stack.push((arg, depth + 1));
                }
                SystemExpr::Scale(value, inner) => {
                    if !value.is_finite() {
                        return Err(SystemTypeError::NonFiniteScale {
                            bits: value.to_bits(),
                        });
                    }
                    stack.push((inner, depth + 1));
                }
                SystemExpr::Add(left, right) => {
                    stack.push((left, depth + 1));
                    stack.push((right, depth + 1));
                }
                SystemExpr::PortPair { effort, flow, .. } => {
                    stack.push((effort, depth + 1));
                    stack.push((flow, depth + 1));
                }
            }
        }
        // Pass 2: post-order type inference with an explicit value stack.
        enum Step<'e> {
            Enter(&'e SystemExpr),
            Exit(&'e SystemExpr),
        }
        let mut work = vec![Step::Enter(root)];
        let mut values: Vec<AdmittedExpr> = Vec::new();
        while let Some(step) = work.pop() {
            match step {
                Step::Enter(node) => {
                    work.push(Step::Exit(node));
                    match node {
                        SystemExpr::FieldRef(_) => {}
                        SystemExpr::Apply { arg, .. } => work.push(Step::Enter(arg)),
                        SystemExpr::Scale(_, inner) => work.push(Step::Enter(inner)),
                        SystemExpr::Add(left, right) => {
                            work.push(Step::Enter(right));
                            work.push(Step::Enter(left));
                        }
                        SystemExpr::PortPair { effort, flow, .. } => {
                            work.push(Step::Enter(flow));
                            work.push(Step::Enter(effort));
                        }
                    }
                }
                Step::Exit(node) => {
                    let admitted = self.admit_node(node, &mut values)?;
                    values.push(admitted);
                }
            }
        }
        debug_assert_eq!(values.len(), 1);
        values.pop().ok_or(SystemTypeError::IdentityEncoding {
            detail: "expression evaluation stack imbalance".to_string(),
        })
    }

    fn admit_node(
        &self,
        node: &SystemExpr,
        values: &mut Vec<AdmittedExpr>,
    ) -> Result<AdmittedExpr, SystemTypeError> {
        match node {
            SystemExpr::FieldRef(field) => {
                let declared = &self.fields[field.0];
                Ok(AdmittedExpr {
                    space: declared.space,
                    convention: Some(FieldConvention {
                        basis: declared.coordinates.basis.clone(),
                        frame: declared.coordinates.frame.clone(),
                        clock: declared.clock.clone(),
                    }),
                    affine_field: declared
                        .quantity
                        .is_affine_absolute()
                        .then(|| declared.name.clone()),
                })
            }
            SystemExpr::Apply { atom, .. } => {
                let arg = values.pop().expect("apply argument admitted");
                let signature = &self.atoms[*atom];
                if arg.space != signature.in_space {
                    return Err(SystemTypeError::SpaceMismatch {
                        context: "atom application",
                        expected: signature.in_space,
                        found: arg.space,
                    });
                }
                // Applying an operator to an affine absolute quantity is a
                // difference-producing act only if the atom says so; v1 takes
                // the conservative route and refuses.
                if let Some(field) = arg.affine_field {
                    return Err(SystemTypeError::AffineQuantityMisuse {
                        operation: "atom application",
                        field,
                    });
                }
                Ok(AdmittedExpr {
                    space: signature.out_space,
                    convention: arg.convention,
                    affine_field: None,
                })
            }
            SystemExpr::Scale(_, _) => {
                let inner = values.pop().expect("scale operand admitted");
                if let Some(field) = inner.affine_field {
                    return Err(SystemTypeError::AffineQuantityMisuse {
                        operation: "scaling",
                        field,
                    });
                }
                Ok(inner)
            }
            SystemExpr::Add(_, _) => {
                let right = values.pop().expect("add rhs admitted");
                let left = values.pop().expect("add lhs admitted");
                if let Some(field) = left.affine_field.or(right.affine_field) {
                    return Err(SystemTypeError::AffineQuantityMisuse {
                        operation: "summation",
                        field,
                    });
                }
                if left.space != right.space {
                    return Err(SystemTypeError::SpaceMismatch {
                        context: "summation",
                        expected: left.space,
                        found: right.space,
                    });
                }
                let convention = match (&left.convention, &right.convention) {
                    (Some(a), Some(b)) => {
                        if a.clock != b.clock {
                            return Err(SystemTypeError::ClockMismatch {
                                left: a.clock.as_str().to_string(),
                                right: b.clock.as_str().to_string(),
                            });
                        }
                        if a.basis != b.basis || a.frame != b.frame {
                            return Err(SystemTypeError::ConventionMismatch {
                                left: (a.basis.as_str().to_string(), a.frame.as_str().to_string()),
                                right: (b.basis.as_str().to_string(), b.frame.as_str().to_string()),
                            });
                        }
                        Some(a.clone())
                    }
                    (Some(a), None) | (None, Some(a)) => Some(a.clone()),
                    (None, None) => None,
                };
                Ok(AdmittedExpr {
                    space: left.space,
                    convention,
                    affine_field: None,
                })
            }
            SystemExpr::PortPair {
                kind, measure_dims, ..
            } => {
                let flow = values.pop().expect("pair flow admitted");
                let effort = values.pop().expect("pair effort admitted");
                if let (Some(a), Some(b)) = (&effort.convention, &flow.convention) {
                    if a.clock != b.clock {
                        return Err(SystemTypeError::ClockMismatch {
                            left: a.clock.as_str().to_string(),
                            right: b.clock.as_str().to_string(),
                        });
                    }
                }
                let product = effort
                    .space
                    .dims
                    .checked_plus(flow.space.dims)
                    .and_then(|sum| sum.checked_plus(*measure_dims));
                if product != Some(POWER_DIMS) {
                    return Err(SystemTypeError::NonConjugatePairing {
                        kind: *kind,
                        effort_dims: effort.space.dims,
                        flow_dims: flow.space.dims,
                        measure_dims: *measure_dims,
                    });
                }
                // A pairing yields a scalar power density block.
                Ok(AdmittedExpr {
                    space: Space {
                        degree: 0,
                        n: 1,
                        dims: POWER_DIMS,
                    },
                    convention: effort.convention,
                    affine_field: None,
                })
            }
        }
    }
}

struct FieldConvention {
    basis: ConventionRef,
    frame: ConventionRef,
    clock: ConventionRef,
}

impl Clone for FieldConvention {
    fn clone(&self) -> Self {
        Self {
            basis: self.basis.clone(),
            frame: self.frame.clone(),
            clock: self.clock.clone(),
        }
    }
}

struct AdmittedExpr {
    space: Space,
    convention: Option<FieldConvention>,
    affine_field: Option<String>,
}

fn check_convention_match(
    convention: &FieldConvention,
    target: &FieldDecl,
) -> Result<(), SystemTypeError> {
    if convention.clock != target.clock {
        return Err(SystemTypeError::ClockMismatch {
            left: convention.clock.as_str().to_string(),
            right: target.clock.as_str().to_string(),
        });
    }
    if convention.basis != target.coordinates.basis || convention.frame != target.coordinates.frame
    {
        return Err(SystemTypeError::ConventionMismatch {
            left: (
                convention.basis.as_str().to_string(),
                convention.frame.as_str().to_string(),
            ),
            right: (
                target.coordinates.basis.as_str().to_string(),
                target.coordinates.frame.as_str().to_string(),
            ),
        });
    }
    Ok(())
}

// ---- canonical byte payloads (display names excluded) ----

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_dims(bytes: &mut Vec<u8>, dims: Dims) {
    for exponent in dims.0 {
        bytes.push(exponent.cast_unsigned());
    }
}

fn push_ref(bytes: &mut Vec<u8>, reference: &ConventionRef) {
    push_u32(
        bytes,
        u32::try_from(reference.as_str().len()).expect("bounded ref"),
    );
    bytes.extend_from_slice(reference.as_str().as_bytes());
}

fn push_space(bytes: &mut Vec<u8>, space: &Space) {
    bytes.push(space.degree);
    bytes.extend_from_slice(&(space.n as u64).to_be_bytes());
    push_dims(bytes, space.dims);
}

fn canonical_field_bytes(field: &FieldDecl) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_space(&mut bytes, &field.space);
    match &field.quantity {
        FieldQuantity::Dimensional(dims) => {
            bytes.push(0);
            push_dims(&mut bytes, *dims);
        }
        FieldQuantity::Semantic(semantic) => {
            bytes.push(1);
            // Debug rendering of the sealed kind/form pair is stable and
            // versioned by SYSTEM_IR_VERSION; fs-qty exposes no canonical
            // byte encoding for SemanticType yet (documented no-claim).
            let rendered = format!("{semantic:?}");
            push_u32(&mut bytes, u32::try_from(rendered.len()).expect("bounded"));
            bytes.extend_from_slice(rendered.as_bytes());
        }
    }
    push_ref(&mut bytes, &field.coordinates.basis);
    push_ref(&mut bytes, &field.coordinates.frame);
    bytes.push(match field.coordinates.orientation {
        PortOrientation::OutwardFromOwner => 0,
        PortOrientation::AlongFrame => 1,
        PortOrientation::AgainstFrame => 2,
    });
    push_ref(&mut bytes, &field.clock);
    bytes.push(match field.support {
        SpatialSupport::Interior => 0,
        SpatialSupport::BoundaryTrace => 1,
    });
    match field.state {
        StateOwnership::Owned { slot } => {
            bytes.push(0);
            push_u32(&mut bytes, slot);
        }
        StateOwnership::External => bytes.push(1),
        StateOwnership::Parameter { role } => {
            bytes.push(2);
            bytes.push(match role {
                ParameterRole::Design => 0,
                ParameterRole::Material => 1,
                ParameterRole::Control => 2,
            });
        }
    }
    bytes
}

fn canonical_atom_bytes(atom: &AtomSignature) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_space(&mut bytes, &atom.in_space);
    push_space(&mut bytes, &atom.out_space);
    bytes
}

fn canonical_equation_bytes(equation: &BlockEquation, remap: &[usize]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(
        &mut bytes,
        u32::try_from(remap[equation.target.0]).expect("bounded field table"),
    );
    // Iterative pre-order serialization with explicit child counts: the
    // tree shape is unambiguous without recursion.
    let mut stack = vec![&equation.rhs];
    while let Some(node) = stack.pop() {
        match node {
            SystemExpr::FieldRef(field) => {
                bytes.push(0);
                push_u32(
                    &mut bytes,
                    u32::try_from(remap[field.0]).expect("bounded field table"),
                );
            }
            SystemExpr::Apply { atom, arg } => {
                bytes.push(1);
                push_u32(
                    &mut bytes,
                    u32::try_from(*atom).expect("bounded atom table"),
                );
                stack.push(arg);
            }
            SystemExpr::Scale(value, inner) => {
                bytes.push(2);
                bytes.extend_from_slice(&value.to_bits().to_be_bytes());
                stack.push(inner);
            }
            SystemExpr::Add(left, right) => {
                bytes.push(3);
                stack.push(right);
                stack.push(left);
            }
            SystemExpr::PortPair {
                kind,
                effort,
                flow,
                measure_dims,
            } => {
                bytes.push(4);
                let rendered = format!("{kind:?}");
                push_u32(&mut bytes, u32::try_from(rendered.len()).expect("bounded"));
                bytes.extend_from_slice(rendered.as_bytes());
                push_dims(&mut bytes, *measure_dims);
                stack.push(flow);
                stack.push(effort);
            }
        }
    }
    bytes
}
