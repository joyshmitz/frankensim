//! Typed structural morphisms between admitted RD.1a geometries (RD.1b).
//!
//! This first RD.1b slice admits category identities and strict maps, checks
//! the direction of evidence restriction/corestriction, and composes admitted
//! arrows with ordered content-addressed lineage.  It deliberately cannot mint
//! a non-identity equivalence: a witness digest is data, not a proof of an
//! inverse, quasi-isomorphism, refinement theorem, or physical crosswalk.

use core::fmt;

use fs_blake3::identity::{
    CanonicalEncoder, CanonicalError, CanonicalLimits, CanonicalSchema, EvidenceNodeId, Field,
    FieldSpec, IdentityReceipt, StrongIdentity, WireType,
};
use fs_evidence::ColorRank;
use fs_exec::Cx;

use crate::derived::{
    AdmittedDerivedGeometryV1, CoefficientSystemV1, DerivedFrameIdV1, DerivedGeometryIdV1,
    DerivedModelVersionIdV1, DerivedNoClaimIdV1, DerivedSubjectIdV1, DerivedUnitSystemIdV1,
    DerivedWitnessIdV1, GeometricCategoryV1,
};

/// Current schema for structural RD.1b morphism receipts.
pub const DERIVED_MORPHISM_SCHEMA_VERSION_V1: u32 = 1;
/// Maximum primitive strict arrows retained in one flattened composition.
pub const DERIVED_MORPHISM_MAX_FACTORS_V1: usize = 1024;
const DERIVED_MORPHISM_CANCELLATION_STRIDE_V1: usize = 64;
const DERIVED_MORPHISM_IDENTITY_LIMITS_V1: CanonicalLimits =
    CanonicalLimits::new(1 << 17, 1 << 16, 8, 1 << 11, 4096);

/// Domain-separated semantic identity for one admitted structural morphism.
pub enum DerivedMorphismIdentitySchemaV1 {}

impl CanonicalSchema for DerivedMorphismIdentitySchemaV1 {
    const DOMAIN: &'static str = "org.frankensim.fs-geom.derived-morphism.v1";
    const NAME: &'static str = "derived-geometry-structural-morphism";
    const VERSION: u32 = DERIVED_MORPHISM_SCHEMA_VERSION_V1;
    const CONTEXT: &'static str = "typed endpoints, strict map class, evidence variance, no-equivalence boundary, and ordered primitive lineage";
    const FIELDS: &'static [FieldSpec] = &[
        FieldSpec::required("source", WireType::Bytes),
        FieldSpec::required("target", WireType::Bytes),
        FieldSpec::required("class", WireType::Bytes),
        FieldSpec::required("evidence-transport", WireType::Bytes),
        FieldSpec::required("no-equivalence-claims", WireType::OrderedBytes),
        FieldSpec::required("primitive-lineage", WireType::OrderedBytes),
    ];
}

/// Typed identity of one admitted RD.1b structural morphism.
pub type DerivedMorphismIdV1 = EvidenceNodeId<DerivedMorphismIdentitySchemaV1>;

/// Caller-supplied primitive map class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedMorphismKindV1 {
    /// The categorical identity on one exact admitted geometry.
    Identity,
    /// A strict map whose construction is retained but not theorem-promoted.
    Strict {
        /// Exact map/construction artifact.
        witness: DerivedWitnessIdV1,
    },
}

/// Admitted map class. Composition flattens strict primitive lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmittedDerivedMorphismClassV1 {
    /// Exact categorical identity.
    Identity,
    /// One or more ordered strict primitive maps.
    Strict,
}

/// Direction in which evidence is transported along an object map `X -> Y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedEvidenceVarianceV1 {
    /// Identity evidence on one exact object.
    Identity,
    /// Sheaf-like restriction `E(Y) -> E(X)`.
    RestrictionContravariant,
    /// Cosheaf/balance-like corestriction `B(X) -> B(Y)`.
    BalanceCorestrictionCovariant,
}

/// Nominal caller-declared identity of an evidence artifact.
///
/// This identity binds structural transport lineage into a morphism receipt.
/// It does not authenticate a payload or grant evidence authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DerivedEvidenceArtifactIdV1([u8; 32]);

impl DerivedEvidenceArtifactIdV1 {
    /// Construct a nominal evidence-artifact identity from exact bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the exact identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Structural evidence transport attached to one object map.
///
/// Identity arrows are deliberately rank-neutral, so every admitted geometry
/// has one categorical identity. Strict variants bind caller-declared evidence
/// artifact identities and ranks, but do not authenticate either artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedEvidenceTransportV1 {
    /// Unique rank-neutral evidence transport on one identity arrow.
    Identity,
    /// Sheaf-like restriction `E(Y) -> E(X)` along `X -> Y`.
    RestrictionContravariant {
        /// Geometry from which evidence is consumed.
        input_geometry: DerivedGeometryIdV1,
        /// Geometry at which transported evidence is published.
        output_geometry: DerivedGeometryIdV1,
        /// Nominal consumed evidence artifact.
        input_evidence: DerivedEvidenceArtifactIdV1,
        /// Nominal published evidence artifact.
        output_evidence: DerivedEvidenceArtifactIdV1,
        /// Caller-declared authority rank consumed by the transport.
        input_rank: ColorRank,
        /// Caller-declared authority rank published by the transport.
        output_rank: ColorRank,
    },
    /// Cosheaf/balance-like corestriction `B(X) -> B(Y)` along `X -> Y`.
    BalanceCorestrictionCovariant {
        /// Geometry from which evidence is consumed.
        input_geometry: DerivedGeometryIdV1,
        /// Geometry at which transported evidence is published.
        output_geometry: DerivedGeometryIdV1,
        /// Nominal consumed evidence artifact.
        input_evidence: DerivedEvidenceArtifactIdV1,
        /// Nominal published evidence artifact.
        output_evidence: DerivedEvidenceArtifactIdV1,
        /// Caller-declared authority rank consumed by the transport.
        input_rank: ColorRank,
        /// Caller-declared authority rank published by the transport.
        output_rank: ColorRank,
    },
}

impl DerivedEvidenceTransportV1 {
    /// Variance of this structural evidence transport.
    #[must_use]
    pub const fn variance(self) -> DerivedEvidenceVarianceV1 {
        match self {
            Self::Identity => DerivedEvidenceVarianceV1::Identity,
            Self::RestrictionContravariant { .. } => {
                DerivedEvidenceVarianceV1::RestrictionContravariant
            }
            Self::BalanceCorestrictionCovariant { .. } => {
                DerivedEvidenceVarianceV1::BalanceCorestrictionCovariant
            }
        }
    }

    /// Caller-declared input/output ranks for a strict transport.
    #[must_use]
    pub const fn ranks(self) -> Option<(ColorRank, ColorRank)> {
        match self {
            Self::Identity => None,
            Self::RestrictionContravariant {
                input_rank,
                output_rank,
                ..
            }
            | Self::BalanceCorestrictionCovariant {
                input_rank,
                output_rank,
                ..
            } => Some((input_rank, output_rank)),
        }
    }
}

/// Explicit equivalence boundary carried by a primitive map request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedEquivalenceBoundaryV1 {
    /// Available only for an exact identity arrow.
    IdentityOnly,
    /// No inverse, equivalence, or physical correspondence is claimed.
    NoClaim {
        /// Retained no-claim artifact.
        artifact: DerivedNoClaimIdV1,
    },
}

/// Versioned primitive RD.1b morphism request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedMorphismIrV1 {
    /// Decoded schema version.
    pub schema_version: u32,
    /// Exact admitted source geometry.
    pub source: DerivedGeometryIdV1,
    /// Exact admitted target geometry.
    pub target: DerivedGeometryIdV1,
    /// Primitive map class.
    pub kind: DerivedMorphismKindV1,
    /// Directional evidence transport.
    pub evidence: DerivedEvidenceTransportV1,
    /// Honest equivalence/no-equivalence boundary.
    pub equivalence: DerivedEquivalenceBoundaryV1,
}

/// Structured refusal from RD.1b structural morphism admission/composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedMorphismErrorV1 {
    /// Unsupported decoded schema version.
    UnsupportedSchemaVersion {
        /// Supplied version.
        found: u32,
        /// Sole supported version.
        supported: u32,
    },
    /// The raw request did not name the exact supplied admitted endpoint.
    EndpointMismatch {
        /// Stable endpoint field.
        field: &'static str,
    },
    /// A required witness, no-claim, or evidence identity used the zero sentinel.
    MissingIdentity {
        /// Stable identity field.
        field: &'static str,
    },
    /// An identity arrow changed its object, transport, or boundary.
    InvalidIdentity,
    /// Strict source and target describe different physical subjects.
    SubjectMismatch,
    /// Strict source and target name different immutable model versions.
    ModelVersionMismatch,
    /// Strict source and target use different mathematical categories.
    CategoryMismatch,
    /// Strict source and target use different coefficient semantics.
    CoefficientMismatch,
    /// Strict source and target use different coordinate frames.
    FrameMismatch,
    /// Strict source and target use different unit systems.
    UnitSystemMismatch,
    /// Evidence endpoints contradict the declared variance.
    EvidenceOrientationMismatch,
    /// Declared evidence metadata attempted to strengthen authority.
    EvidenceStrengthening {
        /// Consumed authority.
        input: ColorRank,
        /// Attempted published authority.
        output: ColorRank,
    },
    /// A non-identity map attempted to claim identity/equivalence authority.
    EquivalenceLaundering,
    /// Two arrows do not share the exact middle object.
    CompositionEndpointMismatch,
    /// Strict arrows use incompatible evidence variance.
    CompositionVarianceMismatch,
    /// Evidence artifact identity or declared rank at the seam is inconsistent.
    CompositionEvidenceMismatch,
    /// Flattened lineage/no-claim retention exceeded the hard ceiling.
    ResourceLimit {
        /// Stable retained collection.
        field: &'static str,
        /// Requested entries.
        requested: usize,
        /// Hard limit.
        limit: usize,
    },
    /// A fallible lineage/no-claim allocation was refused.
    AllocationRefused {
        /// Stable retained collection.
        field: &'static str,
    },
    /// Cooperative cancellation was observed before publication.
    Cancelled {
        /// Stable admission/composition stage.
        stage: &'static str,
    },
    /// Canonical identity construction failed.
    Identity(CanonicalError),
}

impl fmt::Display for DerivedMorphismErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "derived geometry morphism refused: {self:?}")
    }
}

impl core::error::Error for DerivedMorphismErrorV1 {}

/// Sealed, content-addressed structural morphism.
#[derive(Debug, PartialEq, Eq)]
pub struct AdmittedDerivedMorphismV1 {
    source: DerivedGeometryIdV1,
    target: DerivedGeometryIdV1,
    class: AdmittedDerivedMorphismClassV1,
    evidence: DerivedEvidenceTransportV1,
    no_equivalence_claims: Vec<DerivedNoClaimIdV1>,
    primitive_factors: Vec<DerivedMorphismIdV1>,
    receipt: IdentityReceipt<DerivedMorphismIdV1>,
}

impl AdmittedDerivedMorphismV1 {
    /// Exact source geometry.
    #[must_use]
    pub const fn source(&self) -> DerivedGeometryIdV1 {
        self.source
    }

    /// Exact target geometry.
    #[must_use]
    pub const fn target(&self) -> DerivedGeometryIdV1 {
        self.target
    }

    /// Identity versus strict structural class.
    #[must_use]
    pub const fn class(&self) -> AdmittedDerivedMorphismClassV1 {
        self.class
    }

    /// Checked structural direction, nominal artifact identities, and declared ranks.
    #[must_use]
    pub const fn evidence(&self) -> DerivedEvidenceTransportV1 {
        self.evidence
    }

    /// Ordered primitive factors after associative flattening.
    #[must_use]
    pub fn primitive_factors(&self) -> &[DerivedMorphismIdV1] {
        &self.primitive_factors
    }

    /// Ordered retained no-equivalence artifacts.
    #[must_use]
    pub fn no_equivalence_claims(&self) -> &[DerivedNoClaimIdV1] {
        &self.no_equivalence_claims
    }

    /// Typed structural morphism identity.
    #[must_use]
    pub const fn id(&self) -> DerivedMorphismIdV1 {
        self.receipt.id()
    }

    /// Canonical receipt and construction limits.
    #[must_use]
    pub const fn identity_receipt(&self) -> IdentityReceipt<DerivedMorphismIdV1> {
        self.receipt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeometryEndpointV1 {
    id: DerivedGeometryIdV1,
    subject: DerivedSubjectIdV1,
    model_version: DerivedModelVersionIdV1,
    category: GeometricCategoryV1,
    coefficients: CoefficientSystemV1,
    frame: DerivedFrameIdV1,
    unit_system: DerivedUnitSystemIdV1,
}

impl GeometryEndpointV1 {
    fn from_admitted(value: &AdmittedDerivedGeometryV1) -> Self {
        let ir = value.ir();
        Self {
            id: value.id(),
            subject: ir.subject,
            model_version: ir.model_version,
            category: ir.category,
            coefficients: ir.coefficients,
            frame: ir.frame,
            unit_system: ir.unit_system,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ReceiptClassV1 {
    Identity,
    PrimitiveStrict(DerivedWitnessIdV1),
    CompositeStrict,
}

fn is_zero(bytes: &[u8; 32]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

fn rank_tag(rank: ColorRank) -> u8 {
    match rank {
        ColorRank::Estimated => 0,
        ColorRank::Validated => 1,
        ColorRank::Verified => 2,
    }
}

enum EvidenceBytesV1 {
    Identity([u8; 1]),
    Strict([u8; 131]),
}

impl EvidenceBytesV1 {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Identity(bytes) => bytes,
            Self::Strict(bytes) => bytes,
        }
    }
}

fn strict_evidence_bytes(
    tag: u8,
    input_geometry: DerivedGeometryIdV1,
    output_geometry: DerivedGeometryIdV1,
    input_evidence: DerivedEvidenceArtifactIdV1,
    output_evidence: DerivedEvidenceArtifactIdV1,
    input_rank: ColorRank,
    output_rank: ColorRank,
) -> EvidenceBytesV1 {
    let mut bytes = [0_u8; 131];
    bytes[0] = tag;
    bytes[1..33].copy_from_slice(input_geometry.as_bytes());
    bytes[33..65].copy_from_slice(output_geometry.as_bytes());
    bytes[65..97].copy_from_slice(input_evidence.as_bytes());
    bytes[97..129].copy_from_slice(output_evidence.as_bytes());
    bytes[129] = rank_tag(input_rank);
    bytes[130] = rank_tag(output_rank);
    EvidenceBytesV1::Strict(bytes)
}

fn evidence_bytes(evidence: DerivedEvidenceTransportV1) -> EvidenceBytesV1 {
    match evidence {
        DerivedEvidenceTransportV1::Identity => EvidenceBytesV1::Identity([0]),
        DerivedEvidenceTransportV1::RestrictionContravariant {
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
        } => strict_evidence_bytes(
            1,
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
        ),
        DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
        } => strict_evidence_bytes(
            2,
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
        ),
    }
}

enum ClassBytesV1 {
    Tag([u8; 1]),
    Primitive([u8; 33]),
}

impl ClassBytesV1 {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Tag(bytes) => bytes,
            Self::Primitive(bytes) => bytes,
        }
    }
}

fn class_bytes(class: ReceiptClassV1) -> ClassBytesV1 {
    match class {
        ReceiptClassV1::Identity => ClassBytesV1::Tag([0]),
        ReceiptClassV1::PrimitiveStrict(witness) => {
            let mut bytes = [0_u8; 33];
            bytes[0] = 1;
            bytes[1..].copy_from_slice(witness.as_bytes());
            ClassBytesV1::Primitive(bytes)
        }
        ReceiptClassV1::CompositeStrict => ClassBytesV1::Tag([2]),
    }
}

fn morphism_receipt(
    source: DerivedGeometryIdV1,
    target: DerivedGeometryIdV1,
    class: ReceiptClassV1,
    evidence: DerivedEvidenceTransportV1,
    no_claims: &[DerivedNoClaimIdV1],
    factors: &[DerivedMorphismIdV1],
    cx: &Cx<'_>,
) -> Result<IdentityReceipt<DerivedMorphismIdV1>, DerivedMorphismErrorV1> {
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled {
            stage: "identity-entry",
        });
    }
    let class = class_bytes(class);
    let evidence = evidence_bytes(evidence);
    let map_identity_error = |error| match error {
        CanonicalError::Cancelled { .. } => DerivedMorphismErrorV1::Cancelled { stage: "identity" },
        other => DerivedMorphismErrorV1::Identity(other),
    };
    CanonicalEncoder::<DerivedMorphismIdV1, _>::new(DERIVED_MORPHISM_IDENTITY_LIMITS_V1, || {
        cx.checkpoint().is_err()
    })
    .map_err(map_identity_error)?
    .bytes(Field::new(0, "source"), source.as_bytes())
    .and_then(|encoder| encoder.bytes(Field::new(1, "target"), target.as_bytes()))
    .and_then(|encoder| encoder.bytes(Field::new(2, "class"), class.as_slice()))
    .and_then(|encoder| encoder.bytes(Field::new(3, "evidence-transport"), evidence.as_slice()))
    .and_then(|encoder| {
        encoder.ordered_bytes(
            Field::new(4, "no-equivalence-claims"),
            no_claims.len() as u64,
            no_claims.iter().map(|claim| &claim.as_bytes()[..]),
        )
    })
    .and_then(|encoder| {
        encoder.ordered_bytes(
            Field::new(5, "primitive-lineage"),
            factors.len() as u64,
            factors.iter().map(|factor| &factor.as_bytes()[..]),
        )
    })
    .and_then(|encoder| encoder.finish())
    .map_err(map_identity_error)
}

fn validate_evidence(
    source: DerivedGeometryIdV1,
    target: DerivedGeometryIdV1,
    evidence: DerivedEvidenceTransportV1,
) -> Result<(), DerivedMorphismErrorV1> {
    let (
        input_geometry,
        output_geometry,
        input_evidence,
        output_evidence,
        input_rank,
        output_rank,
        expected_input,
        expected_output,
    ) = match evidence {
        DerivedEvidenceTransportV1::Identity => {
            return if source == target {
                Ok(())
            } else {
                Err(DerivedMorphismErrorV1::EvidenceOrientationMismatch)
            };
        }
        DerivedEvidenceTransportV1::RestrictionContravariant {
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
        } => (
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
            target,
            source,
        ),
        DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
        } => (
            input_geometry,
            output_geometry,
            input_evidence,
            output_evidence,
            input_rank,
            output_rank,
            source,
            target,
        ),
    };
    if input_geometry != expected_input || output_geometry != expected_output {
        return Err(DerivedMorphismErrorV1::EvidenceOrientationMismatch);
    }
    if is_zero(input_evidence.as_bytes()) {
        return Err(DerivedMorphismErrorV1::MissingIdentity {
            field: "input-evidence",
        });
    }
    if is_zero(output_evidence.as_bytes()) {
        return Err(DerivedMorphismErrorV1::MissingIdentity {
            field: "output-evidence",
        });
    }
    if output_rank > input_rank {
        return Err(DerivedMorphismErrorV1::EvidenceStrengthening {
            input: input_rank,
            output: output_rank,
        });
    }
    Ok(())
}

fn strict_compatibility(
    source: GeometryEndpointV1,
    target: GeometryEndpointV1,
) -> Result<(), DerivedMorphismErrorV1> {
    if source.subject != target.subject {
        return Err(DerivedMorphismErrorV1::SubjectMismatch);
    }
    if source.model_version != target.model_version {
        return Err(DerivedMorphismErrorV1::ModelVersionMismatch);
    }
    if source.category != target.category {
        return Err(DerivedMorphismErrorV1::CategoryMismatch);
    }
    if source.coefficients != target.coefficients {
        return Err(DerivedMorphismErrorV1::CoefficientMismatch);
    }
    if source.frame != target.frame {
        return Err(DerivedMorphismErrorV1::FrameMismatch);
    }
    if source.unit_system != target.unit_system {
        return Err(DerivedMorphismErrorV1::UnitSystemMismatch);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ValidatedMorphismClassV1 {
    admitted: AdmittedDerivedMorphismClassV1,
    receipt: ReceiptClassV1,
    no_claim: Option<DerivedNoClaimIdV1>,
}

fn validate_morphism_class(
    ir: DerivedMorphismIrV1,
    source: GeometryEndpointV1,
    target: GeometryEndpointV1,
) -> Result<ValidatedMorphismClassV1, DerivedMorphismErrorV1> {
    match (ir.kind, ir.equivalence) {
        (DerivedMorphismKindV1::Identity, DerivedEquivalenceBoundaryV1::IdentityOnly) => {
            if source.id != target.id || ir.evidence != DerivedEvidenceTransportV1::Identity {
                return Err(DerivedMorphismErrorV1::InvalidIdentity);
            }
            Ok(ValidatedMorphismClassV1 {
                admitted: AdmittedDerivedMorphismClassV1::Identity,
                receipt: ReceiptClassV1::Identity,
                no_claim: None,
            })
        }
        (
            DerivedMorphismKindV1::Strict { witness },
            DerivedEquivalenceBoundaryV1::NoClaim { artifact },
        ) => {
            if is_zero(witness.as_bytes()) {
                return Err(DerivedMorphismErrorV1::MissingIdentity {
                    field: "strict-witness",
                });
            }
            if is_zero(artifact.as_bytes()) {
                return Err(DerivedMorphismErrorV1::MissingIdentity {
                    field: "no-equivalence-claim",
                });
            }
            if ir.evidence == DerivedEvidenceTransportV1::Identity {
                return Err(DerivedMorphismErrorV1::EvidenceOrientationMismatch);
            }
            strict_compatibility(source, target)?;
            Ok(ValidatedMorphismClassV1 {
                admitted: AdmittedDerivedMorphismClassV1::Strict,
                receipt: ReceiptClassV1::PrimitiveStrict(witness),
                no_claim: Some(artifact),
            })
        }
        (DerivedMorphismKindV1::Identity, _) => Err(DerivedMorphismErrorV1::InvalidIdentity),
        (DerivedMorphismKindV1::Strict { .. }, _) => {
            Err(DerivedMorphismErrorV1::EquivalenceLaundering)
        }
    }
}

fn retain_no_claim(
    no_claim: Option<DerivedNoClaimIdV1>,
) -> Result<Vec<DerivedNoClaimIdV1>, DerivedMorphismErrorV1> {
    let mut retained = Vec::new();
    if let Some(claim) = no_claim {
        retained
            .try_reserve_exact(1)
            .map_err(|_| DerivedMorphismErrorV1::AllocationRefused {
                field: "no-equivalence-claims",
            })?;
        retained.push(claim);
    }
    Ok(retained)
}

fn admit_between_endpoints(
    ir: DerivedMorphismIrV1,
    source: GeometryEndpointV1,
    target: GeometryEndpointV1,
    cx: &Cx<'_>,
) -> Result<AdmittedDerivedMorphismV1, DerivedMorphismErrorV1> {
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled {
            stage: "admission-entry",
        });
    }
    if ir.schema_version != DERIVED_MORPHISM_SCHEMA_VERSION_V1 {
        return Err(DerivedMorphismErrorV1::UnsupportedSchemaVersion {
            found: ir.schema_version,
            supported: DERIVED_MORPHISM_SCHEMA_VERSION_V1,
        });
    }
    if ir.source != source.id {
        return Err(DerivedMorphismErrorV1::EndpointMismatch { field: "source" });
    }
    if ir.target != target.id {
        return Err(DerivedMorphismErrorV1::EndpointMismatch { field: "target" });
    }
    let validated = validate_morphism_class(ir, source, target)?;
    validate_evidence(source.id, target.id, ir.evidence)?;

    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled { stage: "admission" });
    }
    let no_equivalence_claims = retain_no_claim(validated.no_claim)?;
    let receipt = morphism_receipt(
        source.id,
        target.id,
        validated.receipt,
        ir.evidence,
        &no_equivalence_claims,
        &[],
        cx,
    )?;
    let mut primitive_factors = Vec::new();
    if validated.admitted == AdmittedDerivedMorphismClassV1::Strict {
        primitive_factors.try_reserve_exact(1).map_err(|_| {
            DerivedMorphismErrorV1::AllocationRefused {
                field: "primitive-lineage",
            }
        })?;
        primitive_factors.push(receipt.id());
    }
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled {
            stage: "publication",
        });
    }
    Ok(AdmittedDerivedMorphismV1 {
        source: source.id,
        target: target.id,
        class: validated.admitted,
        evidence: ir.evidence,
        no_equivalence_claims,
        primitive_factors,
        receipt,
    })
}

/// Admit one primitive identity or strict map against exact RD.1a endpoints.
///
/// This validates only structural compatibility and monotonicity of the
/// caller-declared evidence ranks. Nominal evidence identities are retained but
/// not authenticated. A strict witness cannot mint equivalence, inverse,
/// quasi-isomorphism, physical correspondence, or theorem authority.
///
/// # Errors
/// Returns a typed refusal for endpoint/model/category/coefficient/frame/unit,
/// evidence-direction/rank, equivalence-boundary, cancellation, allocation, or
/// canonical-identity defects.
#[must_use = "a raw morphism request has no structural authority"]
pub fn admit_derived_morphism_v1(
    ir: DerivedMorphismIrV1,
    source: &AdmittedDerivedGeometryV1,
    target: &AdmittedDerivedGeometryV1,
    cx: &Cx<'_>,
) -> Result<AdmittedDerivedMorphismV1, DerivedMorphismErrorV1> {
    admit_between_endpoints(
        ir,
        GeometryEndpointV1::from_admitted(source),
        GeometryEndpointV1::from_admitted(target),
        cx,
    )
}

/// Mint the exact identity arrow on one admitted geometry.
///
/// # Errors
/// Returns a cancellation, allocation, or canonical-identity refusal.
#[must_use = "identity construction must complete before composition"]
pub fn identity_derived_morphism_v1(
    object: &AdmittedDerivedGeometryV1,
    cx: &Cx<'_>,
) -> Result<AdmittedDerivedMorphismV1, DerivedMorphismErrorV1> {
    let endpoint = GeometryEndpointV1::from_admitted(object);
    admit_between_endpoints(
        DerivedMorphismIrV1 {
            schema_version: DERIVED_MORPHISM_SCHEMA_VERSION_V1,
            source: endpoint.id,
            target: endpoint.id,
            kind: DerivedMorphismKindV1::Identity,
            evidence: DerivedEvidenceTransportV1::Identity,
            equivalence: DerivedEquivalenceBoundaryV1::IdentityOnly,
        },
        endpoint,
        endpoint,
        cx,
    )
}

fn checked_combined_len(
    field: &'static str,
    left: usize,
    right: usize,
) -> Result<usize, DerivedMorphismErrorV1> {
    let requested = left
        .checked_add(right)
        .ok_or(DerivedMorphismErrorV1::ResourceLimit {
            field,
            requested: usize::MAX,
            limit: DERIVED_MORPHISM_MAX_FACTORS_V1,
        })?;
    if requested > DERIVED_MORPHISM_MAX_FACTORS_V1 {
        return Err(DerivedMorphismErrorV1::ResourceLimit {
            field,
            requested,
            limit: DERIVED_MORPHISM_MAX_FACTORS_V1,
        });
    }
    Ok(requested)
}

fn combine_slices<T: Copy>(
    field: &'static str,
    left: &[T],
    right: &[T],
    cx: &Cx<'_>,
) -> Result<Vec<T>, DerivedMorphismErrorV1> {
    let len = checked_combined_len(field, left.len(), right.len())?;
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled { stage: field });
    }
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| DerivedMorphismErrorV1::AllocationRefused { field })?;
    for (index, value) in left.iter().chain(right).copied().enumerate() {
        out.push(value);
        if (index + 1) % DERIVED_MORPHISM_CANCELLATION_STRIDE_V1 == 0 && cx.checkpoint().is_err() {
            return Err(DerivedMorphismErrorV1::Cancelled { stage: field });
        }
    }
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled { stage: field });
    }
    Ok(out)
}

fn copy_admitted_morphism(
    value: &AdmittedDerivedMorphismV1,
    cx: &Cx<'_>,
) -> Result<AdmittedDerivedMorphismV1, DerivedMorphismErrorV1> {
    let primitive_factors =
        combine_slices("primitive-lineage-copy", &value.primitive_factors, &[], cx)?;
    let no_equivalence_claims = combine_slices(
        "no-equivalence-claims-copy",
        &value.no_equivalence_claims,
        &[],
        cx,
    )?;
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled {
            stage: "composition-publication",
        });
    }
    Ok(AdmittedDerivedMorphismV1 {
        source: value.source,
        target: value.target,
        class: value.class,
        evidence: value.evidence,
        no_equivalence_claims,
        primitive_factors,
        receipt: value.receipt,
    })
}

fn compose_evidence(
    first: DerivedEvidenceTransportV1,
    second: DerivedEvidenceTransportV1,
) -> Result<DerivedEvidenceTransportV1, DerivedMorphismErrorV1> {
    match (first, second) {
        (
            DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
                input_geometry,
                output_geometry: seam_from_first,
                input_evidence,
                output_evidence: seam_evidence_from_first,
                input_rank,
                output_rank: seam_rank_from_first,
            },
            DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
                input_geometry: seam_from_second,
                output_geometry,
                input_evidence: seam_evidence_from_second,
                output_evidence,
                input_rank: seam_rank_from_second,
                output_rank,
            },
        ) => {
            if seam_from_first != seam_from_second
                || seam_evidence_from_first != seam_evidence_from_second
                || seam_rank_from_first != seam_rank_from_second
            {
                return Err(DerivedMorphismErrorV1::CompositionEvidenceMismatch);
            }
            Ok(DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
                input_geometry,
                output_geometry,
                input_evidence,
                output_evidence,
                input_rank,
                output_rank,
            })
        }
        (
            DerivedEvidenceTransportV1::RestrictionContravariant {
                input_geometry: seam_from_first,
                output_geometry,
                input_evidence: seam_evidence_from_first,
                output_evidence,
                input_rank: seam_rank_from_first,
                output_rank,
            },
            DerivedEvidenceTransportV1::RestrictionContravariant {
                input_geometry,
                output_geometry: seam_from_second,
                input_evidence,
                output_evidence: seam_evidence_from_second,
                input_rank,
                output_rank: seam_rank_from_second,
            },
        ) => {
            if seam_from_second != seam_from_first
                || seam_evidence_from_second != seam_evidence_from_first
                || seam_rank_from_second != seam_rank_from_first
            {
                return Err(DerivedMorphismErrorV1::CompositionEvidenceMismatch);
            }
            Ok(DerivedEvidenceTransportV1::RestrictionContravariant {
                input_geometry,
                output_geometry,
                input_evidence,
                output_evidence,
                input_rank,
                output_rank,
            })
        }
        _ => Err(DerivedMorphismErrorV1::CompositionVarianceMismatch),
    }
}

/// Compose `first: X -> Y` followed by `second: Y -> Z`.
///
/// Strict factors and no-equivalence artifacts are flattened in semantic
/// order, so parenthesization does not change the receipt. Identity arrows are
/// unique per geometry and rank-neutral, so they are exact composition units.
///
/// # Errors
/// Returns a typed refusal for endpoint, variance, evidence-seam, lineage-cap,
/// allocation, cancellation, or identity-construction defects.
#[must_use = "composition refusal must not be treated as a morphism"]
pub fn compose_derived_morphisms_v1(
    first: &AdmittedDerivedMorphismV1,
    second: &AdmittedDerivedMorphismV1,
    cx: &Cx<'_>,
) -> Result<AdmittedDerivedMorphismV1, DerivedMorphismErrorV1> {
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled {
            stage: "composition-entry",
        });
    }
    if first.target != second.source {
        return Err(DerivedMorphismErrorV1::CompositionEndpointMismatch);
    }

    if first.class == AdmittedDerivedMorphismClassV1::Identity {
        return copy_admitted_morphism(second, cx);
    }
    if second.class == AdmittedDerivedMorphismClassV1::Identity {
        return copy_admitted_morphism(first, cx);
    }

    let evidence = compose_evidence(first.evidence, second.evidence)?;
    let primitive_factors = combine_slices(
        "primitive-lineage",
        &first.primitive_factors,
        &second.primitive_factors,
        cx,
    )?;
    let no_equivalence_claims = combine_slices(
        "no-equivalence-claims",
        &first.no_equivalence_claims,
        &second.no_equivalence_claims,
        cx,
    )?;
    let receipt = morphism_receipt(
        first.source,
        second.target,
        ReceiptClassV1::CompositeStrict,
        evidence,
        &no_equivalence_claims,
        &primitive_factors,
        cx,
    )?;
    if cx.checkpoint().is_err() {
        return Err(DerivedMorphismErrorV1::Cancelled {
            stage: "composition-publication",
        });
    }
    Ok(AdmittedDerivedMorphismV1 {
        source: first.source,
        target: second.target,
        class: AdmittedDerivedMorphismClassV1::Strict,
        evidence,
        no_equivalence_claims,
        primitive_factors,
        receipt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use asupersync::types::Budget;
    use fs_exec::{CancelGate, ExecMode, StreamKey};

    fn with_cx<R>(cancelled: bool, f: impl FnOnce(&Cx<'_>) -> R) -> R {
        let gate = CancelGate::new_clock_free();
        if cancelled {
            gate.request();
        }
        let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
        pool.scope(|arena| {
            let cx = Cx::new(
                &gate,
                arena,
                StreamKey {
                    seed: 0x5244_3162,
                    kernel_id: 1,
                    tile: 0,
                    iteration: 0,
                },
                Budget::INFINITE,
                ExecMode::Deterministic,
            );
            f(&cx)
        })
    }

    fn geometry_id(seed: u8) -> DerivedGeometryIdV1 {
        DerivedGeometryIdV1::parse_slice(&[seed; 32]).expect("nonzero geometry identity")
    }

    fn endpoint(seed: u8) -> GeometryEndpointV1 {
        GeometryEndpointV1 {
            id: geometry_id(seed),
            subject: DerivedSubjectIdV1::from_bytes([1; 32]),
            model_version: DerivedModelVersionIdV1::from_bytes([4; 32]),
            category: GeometricCategoryV1::Semialgebraic,
            coefficients: CoefficientSystemV1::RationalReal,
            frame: DerivedFrameIdV1::from_bytes([2; 32]),
            unit_system: DerivedUnitSystemIdV1::from_bytes([3; 32]),
        }
    }

    fn evidence_id(geometry: DerivedGeometryIdV1) -> DerivedEvidenceArtifactIdV1 {
        DerivedEvidenceArtifactIdV1::from_bytes(*geometry.as_bytes())
    }

    fn strict_ir(
        source: GeometryEndpointV1,
        target: GeometryEndpointV1,
        witness: u8,
        input_rank: ColorRank,
        output_rank: ColorRank,
    ) -> DerivedMorphismIrV1 {
        DerivedMorphismIrV1 {
            schema_version: DERIVED_MORPHISM_SCHEMA_VERSION_V1,
            source: source.id,
            target: target.id,
            kind: DerivedMorphismKindV1::Strict {
                witness: DerivedWitnessIdV1::from_bytes([witness; 32]),
            },
            evidence: DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
                input_geometry: source.id,
                output_geometry: target.id,
                input_evidence: evidence_id(source.id),
                output_evidence: evidence_id(target.id),
                input_rank,
                output_rank,
            },
            equivalence: DerivedEquivalenceBoundaryV1::NoClaim {
                artifact: DerivedNoClaimIdV1::from_bytes([witness.wrapping_add(64); 32]),
            },
        }
    }

    fn restriction_ir(
        source: GeometryEndpointV1,
        target: GeometryEndpointV1,
        witness: u8,
        input_rank: ColorRank,
        output_rank: ColorRank,
    ) -> DerivedMorphismIrV1 {
        let mut ir = strict_ir(source, target, witness, input_rank, output_rank);
        ir.evidence = DerivedEvidenceTransportV1::RestrictionContravariant {
            input_geometry: target.id,
            output_geometry: source.id,
            input_evidence: evidence_id(target.id),
            output_evidence: evidence_id(source.id),
            input_rank,
            output_rank,
        };
        ir
    }

    fn admit_strict(
        source: GeometryEndpointV1,
        target: GeometryEndpointV1,
        witness: u8,
        input_rank: ColorRank,
        output_rank: ColorRank,
        cx: &Cx<'_>,
    ) -> AdmittedDerivedMorphismV1 {
        admit_between_endpoints(
            strict_ir(source, target, witness, input_rank, output_rank),
            source,
            target,
            cx,
        )
        .expect("valid strict morphism")
    }

    fn admit_identity(object: GeometryEndpointV1, cx: &Cx<'_>) -> AdmittedDerivedMorphismV1 {
        admit_between_endpoints(
            DerivedMorphismIrV1 {
                schema_version: DERIVED_MORPHISM_SCHEMA_VERSION_V1,
                source: object.id,
                target: object.id,
                kind: DerivedMorphismKindV1::Identity,
                evidence: DerivedEvidenceTransportV1::Identity,
                equivalence: DerivedEquivalenceBoundaryV1::IdentityOnly,
            },
            object,
            object,
            cx,
        )
        .expect("valid identity morphism")
    }

    #[test]
    fn identity_is_neutral_and_composition_is_associative_by_receipt() {
        with_cx(false, |cx| {
            let x = endpoint(10);
            let y = endpoint(11);
            let z = endpoint(12);
            let w = endpoint(13);
            let f = admit_strict(x, y, 20, ColorRank::Verified, ColorRank::Validated, cx);
            let g = admit_strict(y, z, 21, ColorRank::Validated, ColorRank::Estimated, cx);
            let h = admit_strict(z, w, 22, ColorRank::Estimated, ColorRank::Estimated, cx);
            let left_identity = admit_identity(x, cx);
            let right_identity = admit_identity(w, cx);
            assert_eq!(left_identity, admit_identity(x, cx));
            assert_eq!(
                compose_derived_morphisms_v1(&left_identity, &f, cx).expect("left identity"),
                f
            );
            assert_eq!(
                compose_derived_morphisms_v1(&h, &right_identity, cx).expect("right identity"),
                h
            );

            let fg = compose_derived_morphisms_v1(&f, &g, cx).expect("f then g");
            let gh = compose_derived_morphisms_v1(&g, &h, cx).expect("g then h");
            let left = compose_derived_morphisms_v1(&fg, &h, cx).expect("(fg)h");
            let right = compose_derived_morphisms_v1(&f, &gh, cx).expect("f(gh)");
            assert_eq!(left, right);
            assert_eq!(left.primitive_factors().len(), 3);
            assert_eq!(left.no_equivalence_claims().len(), 3);
            assert_eq!(
                left.evidence().ranks(),
                Some((ColorRank::Verified, ColorRank::Estimated))
            );
        });
    }

    #[test]
    fn factor_order_is_semantic_and_middle_endpoint_is_exact() {
        with_cx(false, |cx| {
            let x = endpoint(30);
            let a = admit_strict(x, x, 31, ColorRank::Validated, ColorRank::Validated, cx);
            let b = admit_strict(x, x, 32, ColorRank::Validated, ColorRank::Validated, cx);
            let ab = compose_derived_morphisms_v1(&a, &b, cx).expect("a then b");
            let ba = compose_derived_morphisms_v1(&b, &a, cx).expect("b then a");
            assert_ne!(ab.id(), ba.id());
            assert_eq!(ab.primitive_factors(), &[a.id(), b.id()]);
            assert_eq!(
                ab.no_equivalence_claims(),
                &[a.no_equivalence_claims()[0], b.no_equivalence_claims()[0]]
            );

            let y = endpoint(33);
            let z = endpoint(34);
            let yz = admit_strict(y, z, 35, ColorRank::Validated, ColorRank::Validated, cx);
            assert_eq!(
                compose_derived_morphisms_v1(&a, &yz, cx),
                Err(DerivedMorphismErrorV1::CompositionEndpointMismatch)
            );
        });
    }

    #[test]
    fn evidence_cannot_reverse_variance_or_strengthen() {
        with_cx(false, |cx| {
            let x = endpoint(40);
            let y = endpoint(41);
            let strengthening = strict_ir(x, y, 42, ColorRank::Estimated, ColorRank::Verified);
            assert!(matches!(
                admit_between_endpoints(strengthening, x, y, cx),
                Err(DerivedMorphismErrorV1::EvidenceStrengthening { .. })
            ));

            let mut reversed = restriction_ir(x, y, 43, ColorRank::Verified, ColorRank::Validated);
            reversed.evidence = DerivedEvidenceTransportV1::RestrictionContravariant {
                input_geometry: x.id,
                output_geometry: y.id,
                input_evidence: evidence_id(x.id),
                output_evidence: evidence_id(y.id),
                input_rank: ColorRank::Verified,
                output_rank: ColorRank::Validated,
            };
            assert_eq!(
                admit_between_endpoints(reversed, x, y, cx),
                Err(DerivedMorphismErrorV1::EvidenceOrientationMismatch)
            );

            let mut missing = strict_ir(x, y, 44, ColorRank::Validated, ColorRank::Estimated);
            missing.evidence = DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
                input_geometry: x.id,
                output_geometry: y.id,
                input_evidence: DerivedEvidenceArtifactIdV1::from_bytes([0; 32]),
                output_evidence: evidence_id(y.id),
                input_rank: ColorRank::Validated,
                output_rank: ColorRank::Estimated,
            };
            assert_eq!(
                admit_between_endpoints(missing, x, y, cx),
                Err(DerivedMorphismErrorV1::MissingIdentity {
                    field: "input-evidence"
                })
            );
        });
    }

    #[test]
    fn contravariant_restrictions_compose_in_evidence_order() {
        with_cx(false, |cx| {
            let x = endpoint(44);
            let y = endpoint(45);
            let z = endpoint(46);
            let first = admit_between_endpoints(
                restriction_ir(x, y, 47, ColorRank::Verified, ColorRank::Validated),
                x,
                y,
                cx,
            )
            .expect("restriction Y to X");
            let second = admit_between_endpoints(
                restriction_ir(y, z, 48, ColorRank::Verified, ColorRank::Verified),
                y,
                z,
                cx,
            )
            .expect("restriction Z to Y");
            let composite =
                compose_derived_morphisms_v1(&first, &second, cx).expect("restriction Z to X");
            assert_eq!(
                composite.evidence(),
                DerivedEvidenceTransportV1::RestrictionContravariant {
                    input_geometry: z.id,
                    output_geometry: x.id,
                    input_evidence: evidence_id(z.id),
                    output_evidence: evidence_id(x.id),
                    input_rank: ColorRank::Verified,
                    output_rank: ColorRank::Validated,
                }
            );
        });
    }

    #[test]
    fn composition_refuses_nominal_evidence_seam_and_variance_mismatch() {
        with_cx(false, |cx| {
            let x = endpoint(80);
            let y = endpoint(81);
            let z = endpoint(82);
            let first = admit_strict(x, y, 83, ColorRank::Validated, ColorRank::Validated, cx);

            let mut mismatched_ir = strict_ir(y, z, 84, ColorRank::Validated, ColorRank::Estimated);
            mismatched_ir.evidence = DerivedEvidenceTransportV1::BalanceCorestrictionCovariant {
                input_geometry: y.id,
                output_geometry: z.id,
                input_evidence: DerivedEvidenceArtifactIdV1::from_bytes([99; 32]),
                output_evidence: evidence_id(z.id),
                input_rank: ColorRank::Validated,
                output_rank: ColorRank::Estimated,
            };
            let mismatched = admit_between_endpoints(mismatched_ir, y, z, cx)
                .expect("nominal evidence IDs are structural declarations");
            assert_eq!(
                compose_derived_morphisms_v1(&first, &mismatched, cx),
                Err(DerivedMorphismErrorV1::CompositionEvidenceMismatch)
            );

            let rank_mismatched =
                admit_strict(y, z, 86, ColorRank::Verified, ColorRank::Estimated, cx);
            assert_eq!(
                compose_derived_morphisms_v1(&first, &rank_mismatched, cx),
                Err(DerivedMorphismErrorV1::CompositionEvidenceMismatch)
            );

            let restriction = admit_between_endpoints(
                restriction_ir(y, z, 85, ColorRank::Verified, ColorRank::Validated),
                y,
                z,
                cx,
            )
            .expect("valid restriction");
            assert_eq!(
                compose_derived_morphisms_v1(&first, &restriction, cx),
                Err(DerivedMorphismErrorV1::CompositionVarianceMismatch)
            );
        });
    }

    #[test]
    fn strict_maps_cannot_launder_equivalence_or_conventions() {
        with_cx(false, |cx| {
            let x = endpoint(50);
            let mut y = endpoint(51);
            let mut equivalence = strict_ir(x, y, 52, ColorRank::Validated, ColorRank::Validated);
            equivalence.equivalence = DerivedEquivalenceBoundaryV1::IdentityOnly;
            assert_eq!(
                admit_between_endpoints(equivalence, x, y, cx),
                Err(DerivedMorphismErrorV1::EquivalenceLaundering)
            );

            let mut version_changed = y;
            version_changed.model_version = DerivedModelVersionIdV1::from_bytes([98; 32]);
            assert_eq!(
                admit_between_endpoints(
                    strict_ir(
                        x,
                        version_changed,
                        53,
                        ColorRank::Validated,
                        ColorRank::Validated,
                    ),
                    x,
                    version_changed,
                    cx,
                ),
                Err(DerivedMorphismErrorV1::ModelVersionMismatch)
            );

            y.frame = DerivedFrameIdV1::from_bytes([99; 32]);
            assert_eq!(
                admit_between_endpoints(
                    strict_ir(x, y, 54, ColorRank::Validated, ColorRank::Validated,),
                    x,
                    y,
                    cx,
                ),
                Err(DerivedMorphismErrorV1::FrameMismatch)
            );
        });
    }

    #[test]
    fn replay_is_deterministic_and_entry_cancellation_fails_closed() {
        with_cx(false, |cx| {
            let x = endpoint(60);
            let y = endpoint(61);
            let first = admit_strict(x, y, 62, ColorRank::Verified, ColorRank::Validated, cx);
            let replay = admit_strict(x, y, 62, ColorRank::Verified, ColorRank::Validated, cx);
            assert_eq!(first, replay);
        });
        with_cx(true, |cx| {
            let x = endpoint(60);
            let y = endpoint(61);
            assert!(matches!(
                admit_between_endpoints(
                    strict_ir(x, y, 62, ColorRank::Verified, ColorRank::Validated,),
                    x,
                    y,
                    cx,
                ),
                Err(DerivedMorphismErrorV1::Cancelled { .. })
            ));
        });
    }

    #[test]
    fn lineage_cap_and_composition_entry_cancellation_fail_closed() {
        assert_eq!(
            checked_combined_len("primitive-lineage", DERIVED_MORPHISM_MAX_FACTORS_V1 - 1, 1,),
            Ok(DERIVED_MORPHISM_MAX_FACTORS_V1)
        );
        assert_eq!(
            checked_combined_len("primitive-lineage", DERIVED_MORPHISM_MAX_FACTORS_V1, 1,),
            Err(DerivedMorphismErrorV1::ResourceLimit {
                field: "primitive-lineage",
                requested: DERIVED_MORPHISM_MAX_FACTORS_V1 + 1,
                limit: DERIVED_MORPHISM_MAX_FACTORS_V1,
            })
        );

        let (identity, strict) = with_cx(false, |cx| {
            let x = endpoint(70);
            let y = endpoint(71);
            (
                admit_identity(x, cx),
                admit_strict(x, y, 72, ColorRank::Validated, ColorRank::Estimated, cx),
            )
        });
        with_cx(true, |cx| {
            assert!(matches!(
                compose_derived_morphisms_v1(&identity, &strict, cx),
                Err(DerivedMorphismErrorV1::Cancelled { .. })
            ));
        });
    }
}
