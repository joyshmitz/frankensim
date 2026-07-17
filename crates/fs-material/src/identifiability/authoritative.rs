//! Authority-separated, multi-case identifiability schema.
//!
//! This module is the public I10.1 contract.  It deliberately separates four
//! monotone stages that answer different questions:
//!
//! 1. [`IdentifiabilityProblemDocument`] is an unresolved, canonical statement
//!    of the physical/statistical question.  Decoding bytes never grants source
//!    authority.
//! 2. [`AdmittedIdentifiabilityProblem`] resolves every source against concrete
//!    artifacts or an explicit authority disposition and mints [`ProblemId`].
//! 3. [`IdentifiabilityExecutionPlan`] binds coordinates, algorithms, seeds,
//!    tolerances, budgets, and build semantics and mints [`ExecutionId`].
//! 4. [`IdentifiabilityAssessment`] binds typed claims to evidence and mints
//!    [`AssessmentId`].
//!
//! Consequently, changing a coordinate system cannot change the physical
//! problem identity, and adding evidence cannot silently rewrite either the
//! problem or the execution that generated it.  Multi-case campaigns are a
//! first-class v1 primitive: complementary protocols, specimens, environments,
//! and observation operators can jointly break symmetries that no single case
//! can resolve.

use super::*;
use fs_evidence::vv::ClockSynchronization;

/// Umbrella API generation for the authority-separated I10.1 module. Identity
/// preimages use the four stage-specific versions below, so changing one stage
/// never silently rewrites the other three identities.
pub const IDENTIFIABILITY_AUTHORITY_SCHEMA_VERSION: u32 = 1;
pub const IDENTIFIABILITY_PROBLEM_IDENTITY_VERSION: u32 = 1;
pub const IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_VERSION: u32 = 1;
pub const IDENTIFIABILITY_EXECUTION_IDENTITY_VERSION: u32 = 1;
pub const IDENTIFIABILITY_ASSESSMENT_IDENTITY_VERSION: u32 = 1;

const PROBLEM_MAGIC: &[u8] = b"fs-material-identifiability-problem\0";
const SOURCE_ADMISSION_MAGIC: &[u8] = b"fs-material-identifiability-source-admission\0";
const EXECUTION_MAGIC: &[u8] = b"fs-material-identifiability-execution\0";
const ASSESSMENT_MAGIC: &[u8] = b"fs-material-identifiability-assessment\0";
pub const IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN: &str =
    "org.frankensim.fs-material.identifiability-problem.v1";
pub const IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_DOMAIN: &str =
    "org.frankensim.fs-material.identifiability-source-admission.v1";
pub const IDENTIFIABILITY_EXECUTION_IDENTITY_DOMAIN: &str =
    "org.frankensim.fs-material.identifiability-execution.v1";
pub const IDENTIFIABILITY_ASSESSMENT_IDENTITY_DOMAIN: &str =
    "org.frankensim.fs-material.identifiability-assessment.v1";

/// Exact fs-evidence V&V artifact digest domain accepted by typed admission.
pub const VV_ARTIFACT_SOURCE_DOMAIN: &str = "org.frankensim.fs-evidence.vv-artifact.v1";
/// Exact fs-matdb material-card digest domain accepted by typed admission.
pub const MATERIAL_CARD_SOURCE_DOMAIN: &str = "org.frankensim.fs-matdb.material-card.v1";
/// Exact fs-matdb constitutive-model-card digest domain accepted by typed admission.
pub const CONSTITUTIVE_MODEL_CARD_SOURCE_DOMAIN: &str =
    "org.frankensim.fs-matdb.constitutive-model-card.v1";

macro_rules! authority_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(ContentHash);

        impl $name {
            /// Inspect the domain-separated digest.
            #[must_use]
            pub const fn digest(self) -> ContentHash {
                self.0
            }
        }
    };
}

authority_id!(
    ProblemId,
    "Source-resolved identity of a physical identifiability question."
);
authority_id!(
    SourceAdmissionId,
    "Identity of the source-resolution and authority envelope."
);
authority_id!(ExecutionId, "Identity of one numerical execution plan.");
authority_id!(
    AssessmentId,
    "Identity of one typed, evidence-bound assessment."
);

macro_rules! authority_token {
    ($name:ident, $field:literal) => {
        #[doc = concat!("Canonical ", $field, " token.")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Construct a bounded ASCII machine token.
            pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifiabilityError> {
                let value = value.into();
                validate_token(&value, $field)?;
                Ok(Self(value))
            }

            /// Inspect canonical token text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

authority_token!(CaseId, "study case");
authority_token!(SourceKey, "source key");
authority_token!(ConstraintId, "joint constraint");
authority_token!(InfluenceId, "influence declaration");
authority_token!(ClaimId, "identifiability claim");

/// Composite observation identity.  Local channel names are never treated as
/// globally unique across a campaign.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationKey {
    case: CaseId,
    channel: ObservationChannelId,
}

impl ObservationKey {
    /// Construct a case-qualified observation endpoint.
    #[must_use]
    pub const fn new(case: CaseId, channel: ObservationChannelId) -> Self {
        Self { case, channel }
    }

    /// Owning case.
    #[must_use]
    pub const fn case(&self) -> &CaseId {
        &self.case
    }

    /// Case-local channel.
    #[must_use]
    pub const fn channel(&self) -> &ObservationChannelId {
        &self.channel
    }
}

/// Semantic class of an immutable source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    ContextOfUse,
    MaterialCard,
    ConstitutiveModelCard,
    ConstitutiveGraph,
    ExperimentArtifact,
    CalibrationSplit,
    ForwardModel,
    Geometry,
    Process,
    Protocol,
    ObservationOperator,
    Metrology,
    Parser,
    Preprocessing,
    Likelihood,
    Prior,
    Constraint,
    GaugeAction,
    GaugeSection,
    Discrepancy,
    Assumption,
    Analyzer,
    DerivativeProvider,
    Build,
    EvidenceReceipt,
    ExternalManifold,
}

/// Unresolved content reference under an exact digest domain and source
/// contract version. A hash is a binding, not an authentication or scientific
/// correctness claim; authority is supplied only during source admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    key: SourceKey,
    kind: SourceKind,
    expected_hash: ContentHash,
    content_hash_domain: String,
    contract_version: u32,
}

impl SourceRef {
    /// Construct a source reference with an exact fs-blake3 domain and source
    /// contract version.
    pub fn try_new(
        key: SourceKey,
        kind: SourceKind,
        expected_hash: ContentHash,
        content_hash_domain: impl Into<String>,
        contract_version: u32,
    ) -> Result<Self, IdentifiabilityError> {
        let content_hash_domain = content_hash_domain.into();
        validate_token(&content_hash_domain, "source content-hash domain")?;
        if !hash_is_nonzero(expected_hash) {
            return Err(IdentifiabilityError::ZeroIdentity {
                field: "source reference",
            });
        }
        if contract_version == 0 {
            return Err(IdentifiabilityError::VersionMismatch {
                field: "source contract",
                expected: 1,
                actual: 0,
            });
        }
        Ok(Self {
            key,
            kind,
            expected_hash,
            content_hash_domain,
            contract_version,
        })
    }

    /// Construct an exact typed reference to a Context-of-Use artifact.
    pub fn context(key: SourceKey, context: &ContextOfUse) -> Result<Self, IdentifiabilityError> {
        let expected_hash = context
            .content_hash()
            .map_err(|error| IdentifiabilityError::Vv {
                detail: error.to_string(),
            })?;
        Self::try_new(
            key,
            SourceKind::ContextOfUse,
            expected_hash,
            VV_ARTIFACT_SOURCE_DOMAIN,
            VV_SCHEMA_VERSION,
        )
    }

    /// Construct an exact typed reference to an experiment artifact.
    pub fn experiment(
        key: SourceKey,
        experiment: &ExperimentArtifact,
    ) -> Result<Self, IdentifiabilityError> {
        let expected_hash =
            experiment
                .content_hash()
                .map_err(|error| IdentifiabilityError::Vv {
                    detail: error.to_string(),
                })?;
        Self::try_new(
            key,
            SourceKind::ExperimentArtifact,
            expected_hash,
            VV_ARTIFACT_SOURCE_DOMAIN,
            VV_SCHEMA_VERSION,
        )
    }

    /// Construct an exact typed reference to a calibration split.
    pub fn calibration_split(
        key: SourceKey,
        split: &CalibrationSplit,
    ) -> Result<Self, IdentifiabilityError> {
        let expected_hash = split
            .content_hash()
            .map_err(|error| IdentifiabilityError::Vv {
                detail: error.to_string(),
            })?;
        Self::try_new(
            key,
            SourceKind::CalibrationSplit,
            expected_hash,
            VV_ARTIFACT_SOURCE_DOMAIN,
            VV_SCHEMA_VERSION,
        )
    }

    /// Construct an exact typed reference to a material card.
    pub fn material_card(
        key: SourceKey,
        material: &MaterialCard,
    ) -> Result<Self, IdentifiabilityError> {
        Self::try_new(
            key,
            SourceKind::MaterialCard,
            material.content_hash(),
            MATERIAL_CARD_SOURCE_DOMAIN,
            MATDB_SCHEMA_VERSION,
        )
    }

    /// Construct an exact typed reference to a constitutive-model card.
    pub fn constitutive_model_card(
        key: SourceKey,
        model: &ConstitutiveModelCard,
    ) -> Result<Self, IdentifiabilityError> {
        Self::try_new(
            key,
            SourceKind::ConstitutiveModelCard,
            model.content_hash(),
            CONSTITUTIVE_MODEL_CARD_SOURCE_DOMAIN,
            MATDB_SCHEMA_VERSION,
        )
    }

    #[must_use]
    pub const fn key(&self) -> &SourceKey {
        &self.key
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        self.kind
    }

    #[must_use]
    pub const fn expected_hash(&self) -> ContentHash {
        self.expected_hash
    }

    #[must_use]
    pub fn content_hash_domain(&self) -> &str {
        &self.content_hash_domain
    }

    #[must_use]
    pub const fn contract_version(&self) -> u32 {
        self.contract_version
    }
}

/// Authority attached to a resolved source. `ContentVerified` proves only
/// byte identity. `ExternalTrustReceipt` additionally retains an external
/// trust-policy receipt that this crate does not authenticate; neither variant
/// asserts scientific correctness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityDisposition {
    ContentVerified,
    ExternalTrustReceipt { trust_receipt: ContentHash },
    Unverified { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolutionVerification {
    TypedArtifact,
    CanonicalPreimage { byte_len: u64 },
    Unverified,
}

/// Resolution supplied for one opaque source key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceResolution {
    key: SourceKey,
    kind: SourceKind,
    resolved_hash: ContentHash,
    content_hash_domain: String,
    contract_version: u32,
    authority: AuthorityDisposition,
    verification: ResolutionVerification,
}

impl SourceResolution {
    /// Verify an opaque source from its retained canonical preimage. The
    /// `SourceRef.content_hash_domain` is the fs-blake3 domain; callers cannot
    /// self-assert content equality without supplying bytes that reproduce the
    /// expected digest.
    pub fn verify(
        reference: &SourceRef,
        canonical_preimage: &[u8],
        authority: AuthorityDisposition,
    ) -> Result<Self, IdentifiabilityError> {
        validate_authority_disposition(&authority)?;
        if matches!(&authority, AuthorityDisposition::Unverified { .. }) {
            return Err(IdentifiabilityError::InvalidText {
                field: "source verification",
                detail: "verified resolution cannot carry Unverified authority".to_string(),
            });
        }
        let actual = hash_domain(&reference.content_hash_domain, canonical_preimage);
        if actual != reference.expected_hash {
            return Err(IdentifiabilityError::SourceMismatch {
                field: "opaque source canonical preimage",
            });
        }
        Ok(Self {
            key: reference.key.clone(),
            kind: reference.kind,
            resolved_hash: actual,
            content_hash_domain: reference.content_hash_domain.clone(),
            contract_version: reference.contract_version,
            authority,
            verification: ResolutionVerification::CanonicalPreimage {
                byte_len: u64::try_from(canonical_preimage.len()).map_err(|_| {
                    IdentifiabilityError::Cardinality {
                        field: "source canonical preimage",
                        detail: "source preimage length exceeds u64".to_string(),
                    }
                })?,
            },
        })
    }

    /// Retain an explicit unresolved record for diagnostics. Admission rejects
    /// this variant deterministically.
    pub fn unresolved(
        reference: &SourceRef,
        reason: impl Into<String>,
    ) -> Result<Self, IdentifiabilityError> {
        let reason = reason.into();
        validate_reason(&reason, "unverified source reason")?;
        Ok(Self {
            key: reference.key.clone(),
            kind: reference.kind,
            resolved_hash: reference.expected_hash,
            content_hash_domain: reference.content_hash_domain.clone(),
            contract_version: reference.contract_version,
            authority: AuthorityDisposition::Unverified { reason },
            verification: ResolutionVerification::Unverified,
        })
    }

    #[must_use]
    pub const fn key(&self) -> &SourceKey {
        &self.key
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        self.kind
    }

    #[must_use]
    pub const fn resolved_hash(&self) -> ContentHash {
        self.resolved_hash
    }

    #[must_use]
    pub fn content_hash_domain(&self) -> &str {
        &self.content_hash_domain
    }

    #[must_use]
    pub const fn contract_version(&self) -> u32 {
        self.contract_version
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthorityDisposition {
        &self.authority
    }
}

/// Exact opaque-source resolutions.  Duplicate keys are refused instead of
/// taking last-writer-wins authority.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceResolutionSet {
    entries: BTreeMap<SourceKey, SourceResolution>,
}

/// Retained preimage of source-authority identity.  This is deliberately
/// distinct from the physical problem document: trust-policy receipts may move
/// this record without rewriting the scientific question.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceAdmissionRecord {
    schema_version: u32,
    problem_id: ProblemId,
    resolutions: BTreeMap<SourceKey, SourceResolution>,
}

impl SourceResolutionSet {
    /// Canonicalize a bounded resolution set.
    pub fn try_new(entries: Vec<SourceResolution>) -> Result<Self, IdentifiabilityError> {
        if entries.len() > MAX_IDENTIFIABILITY_ITEMS {
            return Err(IdentifiabilityError::Cardinality {
                field: "source resolutions",
                detail: "too many source resolutions".to_string(),
            });
        }
        let mut by_key = BTreeMap::new();
        for entry in entries {
            let key = entry.key.clone();
            if by_key.insert(key.clone(), entry).is_some() {
                return Err(IdentifiabilityError::Duplicate {
                    field: "source resolution",
                    id: key.to_string(),
                });
            }
        }
        Ok(Self { entries: by_key })
    }

    #[must_use]
    pub const fn entries(&self) -> &BTreeMap<SourceKey, SourceResolution> {
        &self.entries
    }
}

/// Decision-facing role of a physical parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterPurpose {
    Estimand,
    Nuisance,
    Hyperparameter,
    CalibrationControl,
}

/// Exact value and provenance for a parameter conditioned outside inference.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionedValue {
    value_si: f64,
    source: SourceKey,
}

impl ConditionedValue {
    pub fn try_new(value_si: f64, source: SourceKey) -> Result<Self, IdentifiabilityError> {
        if !value_si.is_finite() {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "conditioned parameter value",
                detail: "value must be finite".to_string(),
            });
        }
        Ok(Self {
            value_si: canonical_f64(value_si),
            source,
        })
    }

    #[must_use]
    pub const fn value_si(&self) -> f64 {
        self.value_si
    }

    #[must_use]
    pub const fn source(&self) -> &SourceKey {
        &self.source
    }
}

/// Inferential treatment is orthogonal to decision purpose.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterTreatment {
    Estimated,
    Profiled,
    Marginalized,
    Conditioned(ConditionedValue),
    Derived {
        definition: SourceKey,
        parents: BTreeSet<ParameterRoleId>,
    },
}

/// Prior semantics distinguish absence from not-applicable.
#[derive(Debug, Clone, PartialEq)]
pub enum PriorPolicy {
    Distribution(ParameterPrior),
    Absent { reason: String },
    NotApplicable { reason: String },
}

/// Whether schema-level influence connectivity is declared.  This is not an
/// identifiability result and contains no evidence receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfluenceCoverage {
    Declared,
    IntentionallyAbsent { reason: String },
    NotApplicable { reason: String },
}

/// Semantic owner with an immutable payload binding.  A bare category label is
/// insufficient to distinguish two instruments, discrepancy families, or
/// protocol controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterOwnerBinding {
    ConstitutiveModel,
    InitialState { state_path: SourceKey },
    Instrument { instrument: SourceKey },
    Discrepancy { family: SourceKey },
    ControlledInput { protocol: SourceKey },
    Population { hierarchy: SourceKey },
}

/// Population/realization scope, including explicit multi-case scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterScopeBinding {
    Global,
    Cases(BTreeSet<CaseId>),
    MaterialLot {
        lot: ArtifactId,
    },
    Specimen {
        case: CaseId,
        specimen: ArtifactId,
    },
    Field {
        support: SourceKey,
    },
    Hierarchical {
        population: ArtifactId,
        level: u32,
        hierarchy: SourceKey,
    },
}

/// Coordinate-free physical parameter declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct StudyParameter {
    role: ParameterRoleId,
    quantity: QuantitySpec,
    domain: ParameterDomain,
    purpose: ParameterPurpose,
    treatment: ParameterTreatment,
    owner: ParameterOwnerBinding,
    scope: ParameterScopeBinding,
    prior: PriorPolicy,
    influence_coverage: InfluenceCoverage,
}

impl StudyParameter {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        role: ParameterRoleId,
        quantity: QuantitySpec,
        domain: ParameterDomain,
        purpose: ParameterPurpose,
        treatment: ParameterTreatment,
        owner: ParameterOwnerBinding,
        scope: ParameterScopeBinding,
        mut prior: PriorPolicy,
        influence_coverage: InfluenceCoverage,
    ) -> Result<Self, IdentifiabilityError> {
        if !domain.lo.is_finite() || !domain.hi.is_finite() || domain.lo > domain.hi {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "physical parameter domain",
                detail: format!("parameter {role} has invalid finite bounds"),
            });
        }
        match &mut prior {
            PriorPolicy::Distribution(ParameterPrior::None { .. }) => {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "prior policy",
                    detail: "PriorPolicy::Absent is the sole representation of no prior"
                        .to_string(),
                });
            }
            PriorPolicy::Distribution(distribution) => distribution.validate_against(domain)?,
            PriorPolicy::Absent { reason } => validate_reason(reason, "prior absence reason")?,
            PriorPolicy::NotApplicable { reason } => {
                validate_reason(reason, "prior not-applicable reason")?
            }
        }
        match &treatment {
            ParameterTreatment::Estimated
            | ParameterTreatment::Profiled
            | ParameterTreatment::Marginalized
                if domain.is_degenerate() =>
            {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "free parameter domain",
                    detail: format!("free parameter {role} needs a non-degenerate domain"),
                });
            }
            ParameterTreatment::Conditioned(value) => {
                if value.value_si < domain.lo || value.value_si > domain.hi {
                    return Err(IdentifiabilityError::InvalidNumeric {
                        field: "conditioned parameter value",
                        detail: format!("conditioned value for {role} lies outside its domain"),
                    });
                }
                if !matches!(&prior, PriorPolicy::NotApplicable { .. }) {
                    return Err(IdentifiabilityError::InvalidNumeric {
                        field: "conditioned parameter prior",
                        detail: format!(
                            "conditioned parameter {role} requires NotApplicable prior"
                        ),
                    });
                }
            }
            ParameterTreatment::Derived { parents, .. } => {
                if parents.is_empty() || parents.contains(&role) {
                    return Err(IdentifiabilityError::UnknownReference {
                        field: "derived parameter parent",
                        id: role.to_string(),
                    });
                }
                if !matches!(&prior, PriorPolicy::NotApplicable { .. }) {
                    return Err(IdentifiabilityError::InvalidNumeric {
                        field: "derived parameter prior",
                        detail: format!("derived parameter {role} requires NotApplicable prior"),
                    });
                }
            }
            _ => {}
        }
        if matches!(&treatment, ParameterTreatment::Marginalized)
            && !matches!(&prior, PriorPolicy::Distribution(_))
        {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "marginalized parameter prior",
                detail: "marginalization requires an explicit probability measure".to_string(),
            });
        }
        let free = matches!(
            &treatment,
            ParameterTreatment::Estimated
                | ParameterTreatment::Profiled
                | ParameterTreatment::Marginalized
        );
        match &influence_coverage {
            InfluenceCoverage::IntentionallyAbsent { reason } if free => {
                validate_reason(reason, "intentionally absent influence reason")?;
            }
            InfluenceCoverage::NotApplicable { reason } if !free => {
                validate_reason(reason, "not-applicable influence reason")?;
            }
            InfluenceCoverage::Declared => {}
            InfluenceCoverage::IntentionallyAbsent { .. } => {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "parameter influence coverage",
                    detail: "only free inference parameters may carry an influence no-claim"
                        .to_string(),
                });
            }
            InfluenceCoverage::NotApplicable { .. } => {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "parameter influence coverage",
                    detail: "free inference parameters cannot mark influence not applicable"
                        .to_string(),
                });
            }
        }
        Ok(Self {
            role,
            quantity,
            domain,
            purpose,
            treatment,
            owner,
            scope,
            prior,
            influence_coverage,
        })
    }

    #[must_use]
    pub const fn role(&self) -> &ParameterRoleId {
        &self.role
    }

    #[must_use]
    pub const fn quantity(&self) -> QuantitySpec {
        self.quantity
    }

    #[must_use]
    pub const fn domain(&self) -> ParameterDomain {
        self.domain
    }

    #[must_use]
    pub const fn treatment(&self) -> &ParameterTreatment {
        &self.treatment
    }

    #[must_use]
    pub const fn purpose(&self) -> ParameterPurpose {
        self.purpose
    }

    #[must_use]
    pub const fn owner(&self) -> &ParameterOwnerBinding {
        &self.owner
    }

    #[must_use]
    pub const fn scope(&self) -> &ParameterScopeBinding {
        &self.scope
    }

    #[must_use]
    pub const fn prior(&self) -> &PriorPolicy {
        &self.prior
    }

    #[must_use]
    pub const fn influence_coverage(&self) -> &InfluenceCoverage {
        &self.influence_coverage
    }
}

/// A typed scalar coefficient used by a joint affine constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct AffineConstraintTerm {
    parameter: ParameterRoleId,
    coefficient: f64,
    coefficient_quantity: QuantitySpec,
}

impl AffineConstraintTerm {
    pub fn try_new(
        parameter: ParameterRoleId,
        coefficient: f64,
        coefficient_quantity: QuantitySpec,
    ) -> Result<Self, IdentifiabilityError> {
        if !coefficient.is_finite() {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "constraint coefficient",
                detail: "coefficient must be finite".to_string(),
            });
        }
        Ok(Self {
            parameter,
            coefficient: canonical_f64(coefficient),
            coefficient_quantity,
        })
    }

    #[must_use]
    pub const fn parameter(&self) -> &ParameterRoleId {
        &self.parameter
    }

    #[must_use]
    pub const fn coefficient(&self) -> f64 {
        self.coefficient
    }

    #[must_use]
    pub const fn coefficient_quantity(&self) -> QuantitySpec {
        self.coefficient_quantity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintRelation {
    Equal,
    LessOrEqual,
    GreaterOrEqual,
}

/// Cross-parameter admissible-domain constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum JointConstraintKind {
    Affine {
        terms: Vec<AffineConstraintTerm>,
        relation: ConstraintRelation,
        rhs_si: f64,
        residual_quantity: QuantitySpec,
    },
    Simplex {
        members: BTreeSet<ParameterRoleId>,
        total_si: f64,
        quantity: QuantitySpec,
    },
    Ordered {
        members: Vec<ParameterRoleId>,
        strict: bool,
    },
    ExternalManifold {
        members: BTreeSet<ParameterRoleId>,
        definition: SourceKey,
        codimension: u32,
    },
    StochasticCoupling {
        members: BTreeSet<ParameterRoleId>,
        distribution: SourceKey,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointConstraint {
    id: ConstraintId,
    kind: JointConstraintKind,
}

impl JointConstraint {
    #[must_use]
    pub const fn new(id: ConstraintId, kind: JointConstraintKind) -> Self {
        Self { id, kind }
    }

    #[must_use]
    pub const fn id(&self) -> &ConstraintId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> &JointConstraintKind {
        &self.kind
    }
}

/// Why a case participates in the campaign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CasePurpose {
    Calibration,
    SymmetryBreaking,
    ValidationOnly,
    BlindFalsification,
    ProspectiveDesign,
    Complementary { reason: String },
}

/// Whether observations already exist.  Retrospective lineage is re-derived
/// from concrete V&V artifacts at admission; it is never trusted from bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseDataDeclaration {
    Prospective,
    Retrospective {
        experiment: SourceKey,
        split: SourceKey,
        parser: SourceKey,
        preprocessing: SourceKey,
        parser_version: u32,
        split_grouping: ArtifactId,
    },
}

/// Raw-row declaration for one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationRows {
    Prospective,
    Retrospective(BTreeSet<ObservationId>),
}

/// Marginal noise family.  Joint dependence is modeled separately so bounded
/// or unknown marginals are never silently assigned a standard deviation.
#[derive(Debug, Clone, PartialEq)]
pub enum MarginalNoiseSpec {
    Gaussian {
        standard_deviation: f64,
    },
    StudentT {
        scale: f64,
        degrees_of_freedom: f64,
    },
    Empirical {
        distribution: SourceKey,
        standard_deviation: f64,
        finite_variance_model: SourceKey,
    },
    Bounded {
        half_width: f64,
    },
    Unknown {
        reason: String,
    },
}

impl MarginalNoiseSpec {
    fn finite_standard_deviation(&self) -> bool {
        match self {
            Self::Gaussian { standard_deviation } => {
                standard_deviation.is_finite() && *standard_deviation > 0.0
            }
            Self::StudentT {
                scale,
                degrees_of_freedom,
            } => scale.is_finite() && *scale > 0.0 && *degrees_of_freedom > 2.0,
            Self::Empirical {
                standard_deviation, ..
            } => standard_deviation.is_finite() && *standard_deviation > 0.0,
            Self::Bounded { .. } | Self::Unknown { .. } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingnessAssumption {
    Complete { assumption: SourceKey },
    Modeled { mechanism: SourceKey },
    Unknown { reason: String },
}

/// Evidence-free physical observation schema.
#[derive(Debug, Clone, PartialEq)]
pub struct StudyObservation {
    id: ObservationChannelId,
    qoi: QoiId,
    unit: UnitId,
    quantity: QuantitySpec,
    frame: FrameBinding,
    graph_node: String,
    graph_port: String,
    operator: SourceKey,
    aggregation: SourceKey,
    sensor: SourceKey,
    instrument: ArtifactId,
    clock: ArtifactId,
    operator_version: u32,
    noise: MarginalNoiseSpec,
    missingness: MissingnessAssumption,
    saturation: Option<ParameterDomain>,
    protocol_version: u32,
    refinement_version: u32,
    rows: ObservationRows,
}

impl StudyObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: ObservationChannelId,
        qoi: QoiId,
        unit: UnitId,
        quantity: QuantitySpec,
        frame: FrameBinding,
        graph_node: impl Into<String>,
        graph_port: impl Into<String>,
        operator: SourceKey,
        aggregation: SourceKey,
        sensor: SourceKey,
        instrument: ArtifactId,
        clock: ArtifactId,
        operator_version: u32,
        mut noise: MarginalNoiseSpec,
        missingness: MissingnessAssumption,
        saturation: Option<ParameterDomain>,
        protocol_version: u32,
        refinement_version: u32,
        rows: ObservationRows,
    ) -> Result<Self, IdentifiabilityError> {
        let graph_node = graph_node.into();
        let graph_port = graph_port.into();
        validate_token(&graph_node, "observation graph node")?;
        validate_token(&graph_port, "observation graph port")?;
        if operator_version == 0 || protocol_version == 0 || refinement_version == 0 {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "observation versions",
                detail: "operator, protocol, and refinement versions must be positive".to_string(),
            });
        }
        match &mut noise {
            MarginalNoiseSpec::Gaussian { standard_deviation }
                if standard_deviation.is_finite() && *standard_deviation > 0.0 => {}
            MarginalNoiseSpec::StudentT {
                scale,
                degrees_of_freedom,
            } if scale.is_finite()
                && *scale > 0.0
                && degrees_of_freedom.is_finite()
                && *degrees_of_freedom > 0.0 => {}
            MarginalNoiseSpec::Empirical {
                standard_deviation, ..
            } if standard_deviation.is_finite() && *standard_deviation > 0.0 => {}
            MarginalNoiseSpec::Bounded { half_width }
                if half_width.is_finite() && *half_width >= 0.0 =>
            {
                *half_width = canonical_f64(*half_width);
            }
            MarginalNoiseSpec::Unknown { reason } => {
                validate_reason(reason, "unknown noise reason")?;
            }
            _ => {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "marginal noise",
                    detail: "noise parameters must be finite and physically admissible".to_string(),
                });
            }
        }
        if let MissingnessAssumption::Unknown { reason } = &missingness {
            validate_reason(reason, "unknown missingness reason")?;
        }
        if let ObservationRows::Retrospective(rows) = &rows {
            if rows.is_empty() || rows.len() > MAX_IDENTIFIABILITY_ITEMS {
                return Err(IdentifiabilityError::Cardinality {
                    field: "observation rows",
                    detail: "retrospective channels need bounded nonempty raw-row sets".to_string(),
                });
            }
        }
        Ok(Self {
            id,
            qoi,
            unit,
            quantity,
            frame,
            graph_node,
            graph_port,
            operator,
            aggregation,
            sensor,
            instrument,
            clock,
            operator_version,
            noise,
            missingness,
            saturation,
            protocol_version,
            refinement_version,
            rows,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &ObservationChannelId {
        &self.id
    }

    #[must_use]
    pub const fn quantity(&self) -> QuantitySpec {
        self.quantity
    }

    #[must_use]
    pub const fn qoi(&self) -> &QoiId {
        &self.qoi
    }

    #[must_use]
    pub const fn unit(&self) -> &UnitId {
        &self.unit
    }

    #[must_use]
    pub const fn frame(&self) -> &FrameBinding {
        &self.frame
    }

    #[must_use]
    pub fn graph_node(&self) -> &str {
        &self.graph_node
    }

    #[must_use]
    pub fn graph_port(&self) -> &str {
        &self.graph_port
    }

    #[must_use]
    pub const fn operator(&self) -> &SourceKey {
        &self.operator
    }

    #[must_use]
    pub const fn aggregation(&self) -> &SourceKey {
        &self.aggregation
    }

    #[must_use]
    pub const fn sensor(&self) -> &SourceKey {
        &self.sensor
    }

    #[must_use]
    pub const fn instrument(&self) -> &ArtifactId {
        &self.instrument
    }

    #[must_use]
    pub const fn clock(&self) -> &ArtifactId {
        &self.clock
    }

    #[must_use]
    pub const fn operator_version(&self) -> u32 {
        self.operator_version
    }

    #[must_use]
    pub const fn noise(&self) -> &MarginalNoiseSpec {
        &self.noise
    }

    #[must_use]
    pub const fn missingness(&self) -> &MissingnessAssumption {
        &self.missingness
    }

    #[must_use]
    pub const fn saturation(&self) -> Option<ParameterDomain> {
        self.saturation
    }

    #[must_use]
    pub const fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    #[must_use]
    pub const fn refinement_version(&self) -> u32 {
        self.refinement_version
    }

    #[must_use]
    pub const fn rows(&self) -> &ObservationRows {
        &self.rows
    }
}

/// Joint noise/correlation semantics over composite observation keys.
#[derive(Debug, Clone, PartialEq)]
pub enum JointNoiseModel {
    Independent,
    DenseCorrelation {
        order: Vec<ObservationKey>,
        correlation: CovarianceMatrix,
        model: SourceKey,
    },
    ExternalKernel {
        model: SourceKey,
    },
    Unknown {
        reason: String,
    },
}

/// Discrepancy is never inferred from absence.  Even an assumed-zero model is
/// an explicit, source-bound assumption rather than evidence of correctness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudyDiscrepancy {
    Uncharacterized {
        reason: String,
    },
    NotApplicable {
        reason: String,
    },
    AssumedZero {
        assumption: SourceKey,
    },
    Modeled {
        family: SourceKey,
        parameters: BTreeSet<ParameterRoleId>,
        support: SourceKey,
        confounding_guard: SourceKey,
    },
}

/// One physical or prospective campaign case.
#[derive(Debug, Clone, PartialEq)]
pub struct StudyCaseDocument {
    id: CaseId,
    purpose: CasePurpose,
    initial_state: InitialStateBinding,
    specimen: SpecimenBinding,
    protocol: ProtocolBinding,
    forward_model: SourceKey,
    data: CaseDataDeclaration,
    observations: BTreeMap<ObservationChannelId, StudyObservation>,
    discrepancies: BTreeMap<ObservationChannelId, StudyDiscrepancy>,
}

impl StudyCaseDocument {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: CaseId,
        purpose: CasePurpose,
        initial_state: InitialStateBinding,
        specimen: SpecimenBinding,
        protocol: ProtocolBinding,
        forward_model: SourceKey,
        data: CaseDataDeclaration,
        observations: Vec<StudyObservation>,
        discrepancies: Vec<(ObservationChannelId, StudyDiscrepancy)>,
    ) -> Result<Self, IdentifiabilityError> {
        if let CasePurpose::Complementary { reason } = &purpose {
            validate_reason(reason, "complementary case reason")?;
        }
        if observations.is_empty() || observations.len() > MAX_IDENTIFIABILITY_ITEMS {
            return Err(IdentifiabilityError::Cardinality {
                field: "case observations",
                detail: "each case needs bounded nonempty observations".to_string(),
            });
        }
        let mut observation_map = BTreeMap::new();
        for observation in observations {
            let channel = observation.id.clone();
            if observation_map
                .insert(channel.clone(), observation)
                .is_some()
            {
                return Err(IdentifiabilityError::Duplicate {
                    field: "case observation",
                    id: channel.to_string(),
                });
            }
        }
        let mut discrepancy_map = BTreeMap::new();
        for (channel, discrepancy) in discrepancies {
            if !observation_map.contains_key(&channel) {
                return Err(IdentifiabilityError::UnknownReference {
                    field: "discrepancy observation",
                    id: channel.to_string(),
                });
            }
            if discrepancy_map
                .insert(channel.clone(), discrepancy)
                .is_some()
            {
                return Err(IdentifiabilityError::Duplicate {
                    field: "discrepancy observation",
                    id: channel.to_string(),
                });
            }
        }
        if discrepancy_map.len() != observation_map.len() {
            return Err(IdentifiabilityError::Cardinality {
                field: "case discrepancies",
                detail: "every observation needs explicit discrepancy semantics".to_string(),
            });
        }
        Ok(Self {
            id,
            purpose,
            initial_state,
            specimen,
            protocol,
            forward_model,
            data,
            observations: observation_map,
            discrepancies: discrepancy_map,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &CaseId {
        &self.id
    }

    #[must_use]
    pub const fn observations(&self) -> &BTreeMap<ObservationChannelId, StudyObservation> {
        &self.observations
    }

    #[must_use]
    pub const fn purpose(&self) -> &CasePurpose {
        &self.purpose
    }

    #[must_use]
    pub const fn initial_state(&self) -> InitialStateBinding {
        self.initial_state
    }

    #[must_use]
    pub const fn specimen(&self) -> &SpecimenBinding {
        &self.specimen
    }

    #[must_use]
    pub const fn protocol(&self) -> &ProtocolBinding {
        &self.protocol
    }

    #[must_use]
    pub const fn forward_model(&self) -> &SourceKey {
        &self.forward_model
    }

    #[must_use]
    pub const fn data(&self) -> &CaseDataDeclaration {
        &self.data
    }

    #[must_use]
    pub const fn discrepancies(&self) -> &BTreeMap<ObservationChannelId, StudyDiscrepancy> {
        &self.discrepancies
    }
}

/// Exact observation-distribution functional whose parameter dependence is
/// part of the physical question.  The derivative quantity is derived from
/// endpoint quantities and therefore cannot be supplied inconsistently.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DistributionFunctional {
    Location {
        observation: ObservationKey,
    },
    LogScale {
        observation: ObservationKey,
    },
    Correlation {
        left: ObservationKey,
        right: ObservationKey,
    },
    MissingnessLogit {
        observation: ObservationKey,
    },
    CensoringLogit {
        observation: ObservationKey,
    },
}

/// Structural representation of an influence declaration.  Receipts proving
/// nonzero influence belong to an assessment, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfluenceRepresentation {
    Direct,
    StateMediated {
        state_path: SourceKey,
    },
    Composite {
        operator: SourceKey,
        inputs: BTreeSet<InfluenceId>,
    },
    ExternalDefinition {
        definition: SourceKey,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfluenceDeclaration {
    id: InfluenceId,
    parameter: ParameterRoleId,
    functional: DistributionFunctional,
    representation: InfluenceRepresentation,
}

impl InfluenceDeclaration {
    #[must_use]
    pub const fn new(
        id: InfluenceId,
        parameter: ParameterRoleId,
        functional: DistributionFunctional,
        representation: InfluenceRepresentation,
    ) -> Self {
        Self {
            id,
            parameter,
            functional,
            representation,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &InfluenceId {
        &self.id
    }

    #[must_use]
    pub const fn parameter(&self) -> &ParameterRoleId {
        &self.parameter
    }

    #[must_use]
    pub const fn functional(&self) -> &DistributionFunctional {
        &self.functional
    }

    #[must_use]
    pub const fn representation(&self) -> &InfluenceRepresentation {
        &self.representation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GaugeKind {
    Continuous {
        dimension: u32,
    },
    Discrete {
        group_order: u64,
    },
    Mixed {
        continuous_dimension: u32,
        discrete_order: u64,
    },
    Stratified {
        strata: SourceKey,
    },
    Suspected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GaugeHandling {
    Quotient {
        quotient_map: SourceKey,
        local_sections: SourceKey,
    },
    Slice {
        constraint: ConstraintId,
    },
    Retained {
        reason: String,
    },
    Unresolved {
        reason: String,
    },
}

/// Declared continuous/discrete/mixed/stratified gauge.  v1 intentionally
/// rejects overlapping classes because nontrivial groupoid composition needs a
/// separate, explicit future schema rather than order-dependent semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaugeDeclaration {
    id: GaugeClassId,
    members: BTreeSet<ParameterRoleId>,
    action: SourceKey,
    kind: GaugeKind,
    handling: GaugeHandling,
}

impl GaugeDeclaration {
    pub fn try_new(
        id: GaugeClassId,
        members: BTreeSet<ParameterRoleId>,
        action: SourceKey,
        kind: GaugeKind,
        handling: GaugeHandling,
    ) -> Result<Self, IdentifiabilityError> {
        if members.len() < 2 || members.len() > MAX_IDENTIFIABILITY_ITEMS {
            return Err(IdentifiabilityError::InvalidGauge {
                gauge: id,
                detail: "a gauge needs at least two bounded members".to_string(),
            });
        }
        match &kind {
            GaugeKind::Continuous { dimension } if *dimension > 0 => {}
            GaugeKind::Discrete { group_order } if *group_order > 1 => {}
            GaugeKind::Mixed {
                continuous_dimension,
                discrete_order,
            } if *continuous_dimension > 0 && *discrete_order > 1 => {}
            GaugeKind::Stratified { .. } => {}
            GaugeKind::Suspected { reason } => validate_reason(reason, "suspected gauge reason")?,
            _ => {
                return Err(IdentifiabilityError::InvalidGauge {
                    gauge: id,
                    detail: "gauge dimensions/orders must be nontrivial".to_string(),
                });
            }
        }
        match &handling {
            GaugeHandling::Retained { reason } | GaugeHandling::Unresolved { reason } => {
                validate_reason(reason, "gauge handling reason")?;
            }
            _ => {}
        }
        Ok(Self {
            id,
            members,
            action,
            kind,
            handling,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &GaugeClassId {
        &self.id
    }

    #[must_use]
    pub const fn members(&self) -> &BTreeSet<ParameterRoleId> {
        &self.members
    }

    #[must_use]
    pub const fn action(&self) -> &SourceKey {
        &self.action
    }

    #[must_use]
    pub const fn kind(&self) -> &GaugeKind {
        &self.kind
    }

    #[must_use]
    pub const fn handling(&self) -> &GaugeHandling {
        &self.handling
    }
}

/// Explicit sharing group for cases that intentionally reuse observations or
/// raw experiment sources under one joint likelihood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSharingGroup {
    cases: BTreeSet<CaseId>,
    joint_likelihood: SourceKey,
    justification: String,
}

impl DataSharingGroup {
    pub fn try_new(
        cases: BTreeSet<CaseId>,
        joint_likelihood: SourceKey,
        justification: impl Into<String>,
    ) -> Result<Self, IdentifiabilityError> {
        let justification = justification.into();
        if cases.len() < 2 || cases.len() > MAX_IDENTIFIABILITY_ITEMS {
            return Err(IdentifiabilityError::Cardinality {
                field: "data sharing group",
                detail: "a sharing group needs at least two bounded cases".to_string(),
            });
        }
        validate_reason(&justification, "data sharing justification")?;
        Ok(Self {
            cases,
            joint_likelihood,
            justification,
        })
    }

    #[must_use]
    pub const fn cases(&self) -> &BTreeSet<CaseId> {
        &self.cases
    }

    #[must_use]
    pub const fn joint_likelihood(&self) -> &SourceKey {
        &self.joint_likelihood
    }

    #[must_use]
    pub fn justification(&self) -> &str {
        &self.justification
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataReusePolicy {
    Disjoint,
    Shared { groups: Vec<DataSharingGroup> },
}

fn retrospective_experiment(case: &StudyCaseDocument) -> Option<&SourceKey> {
    match &case.data {
        CaseDataDeclaration::Retrospective { experiment, .. } => Some(experiment),
        CaseDataDeclaration::Prospective => None,
    }
}

fn sharing_group_membership(policy: &DataReusePolicy, case: &CaseId) -> Option<usize> {
    match policy {
        DataReusePolicy::Disjoint => None,
        DataReusePolicy::Shared { groups } => {
            groups.iter().position(|group| group.cases.contains(case))
        }
    }
}

fn lineages_share_raw_data(left: &DataLineage, right: &DataLineage) -> bool {
    if left.source_bytes() == right.source_bytes() || left.raw_manifest() == right.raw_manifest() {
        return true;
    }
    let left_sources = left
        .row_sources()
        .values()
        .copied()
        .collect::<BTreeSet<_>>();
    right
        .row_sources()
        .values()
        .any(|source| left_sources.contains(source))
}

fn normalize_joint_noise(noise: JointNoiseModel) -> Result<JointNoiseModel, IdentifiabilityError> {
    let JointNoiseModel::DenseCorrelation {
        order,
        correlation,
        model,
    } = noise
    else {
        return Ok(noise);
    };
    let positions = order
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect::<BTreeMap<_, _>>();
    if positions.len() != order.len() || correlation.dimension() != order.len() {
        return Err(IdentifiabilityError::Covariance {
            detail: "dense-correlation order must be unique and match matrix dimension".to_string(),
        });
    }
    let sorted = positions.keys().cloned().collect::<Vec<_>>();
    let mut lower = Vec::with_capacity(correlation.lower_triangle().len());
    for row in 0..sorted.len() {
        for column in 0..=row {
            lower.push(matrix_get(
                &correlation,
                positions[&sorted[row]],
                positions[&sorted[column]],
            ));
        }
    }
    let correlation = CovarianceMatrix::try_new(sorted.len(), lower).map_err(|error| {
        IdentifiabilityError::Vv {
            detail: error.to_string(),
        }
    })?;
    Ok(JointNoiseModel::DenseCorrelation {
        order: sorted,
        correlation,
        model,
    })
}

fn problem_source_reachability(
    context_source: &SourceKey,
    material_source: &SourceKey,
    model_source: &SourceKey,
    graph_source: &SourceKey,
    parameters: &BTreeMap<ParameterRoleId, StudyParameter>,
    constraints: &BTreeMap<ConstraintId, JointConstraint>,
    cases: &BTreeMap<CaseId, StudyCaseDocument>,
    influences: &BTreeMap<InfluenceId, InfluenceDeclaration>,
    gauges: &BTreeMap<GaugeClassId, GaugeDeclaration>,
    joint_noise: &JointNoiseModel,
    data_reuse: &DataReusePolicy,
) -> BTreeSet<SourceKey> {
    let mut used = BTreeSet::from([
        context_source.clone(),
        material_source.clone(),
        model_source.clone(),
        graph_source.clone(),
    ]);
    for parameter in parameters.values() {
        match &parameter.treatment {
            ParameterTreatment::Conditioned(value) => {
                used.insert(value.source.clone());
            }
            ParameterTreatment::Derived { definition, .. } => {
                used.insert(definition.clone());
            }
            _ => {}
        }
        match &parameter.owner {
            ParameterOwnerBinding::ConstitutiveModel => {}
            ParameterOwnerBinding::InitialState { state_path } => {
                used.insert(state_path.clone());
            }
            ParameterOwnerBinding::Instrument { instrument } => {
                used.insert(instrument.clone());
            }
            ParameterOwnerBinding::Discrepancy { family } => {
                used.insert(family.clone());
            }
            ParameterOwnerBinding::ControlledInput { protocol } => {
                used.insert(protocol.clone());
            }
            ParameterOwnerBinding::Population { hierarchy } => {
                used.insert(hierarchy.clone());
            }
        }
        match &parameter.scope {
            ParameterScopeBinding::Field { support } => {
                used.insert(support.clone());
            }
            ParameterScopeBinding::Hierarchical { hierarchy, .. } => {
                used.insert(hierarchy.clone());
            }
            _ => {}
        }
    }
    for constraint in constraints.values() {
        match &constraint.kind {
            JointConstraintKind::ExternalManifold { definition, .. } => {
                used.insert(definition.clone());
            }
            JointConstraintKind::StochasticCoupling { distribution, .. } => {
                used.insert(distribution.clone());
            }
            _ => {}
        }
    }
    for case in cases.values() {
        used.insert(case.forward_model.clone());
        if let CaseDataDeclaration::Retrospective {
            experiment,
            split,
            parser,
            preprocessing,
            ..
        } = &case.data
        {
            used.extend([
                experiment.clone(),
                split.clone(),
                parser.clone(),
                preprocessing.clone(),
            ]);
        }
        for observation in case.observations.values() {
            used.extend([
                observation.operator.clone(),
                observation.aggregation.clone(),
                observation.sensor.clone(),
            ]);
            if let MarginalNoiseSpec::Empirical {
                distribution,
                finite_variance_model,
                ..
            } = &observation.noise
            {
                used.extend([distribution.clone(), finite_variance_model.clone()]);
            }
            match &observation.missingness {
                MissingnessAssumption::Complete { assumption } => {
                    used.insert(assumption.clone());
                }
                MissingnessAssumption::Modeled { mechanism } => {
                    used.insert(mechanism.clone());
                }
                MissingnessAssumption::Unknown { .. } => {}
            }
        }
        for discrepancy in case.discrepancies.values() {
            match discrepancy {
                StudyDiscrepancy::AssumedZero { assumption } => {
                    used.insert(assumption.clone());
                }
                StudyDiscrepancy::Modeled {
                    family,
                    support,
                    confounding_guard,
                    ..
                } => {
                    used.extend([family.clone(), support.clone(), confounding_guard.clone()]);
                }
                _ => {}
            }
        }
    }
    for influence in influences.values() {
        match &influence.representation {
            InfluenceRepresentation::StateMediated { state_path } => {
                used.insert(state_path.clone());
            }
            InfluenceRepresentation::Composite { operator, .. } => {
                used.insert(operator.clone());
            }
            InfluenceRepresentation::ExternalDefinition { definition } => {
                used.insert(definition.clone());
            }
            InfluenceRepresentation::Direct => {}
        }
    }
    for gauge in gauges.values() {
        used.insert(gauge.action.clone());
        if let GaugeKind::Stratified { strata } = &gauge.kind {
            used.insert(strata.clone());
        }
        if let GaugeHandling::Quotient {
            quotient_map,
            local_sections,
        } = &gauge.handling
        {
            used.extend([quotient_map.clone(), local_sections.clone()]);
        }
    }
    match joint_noise {
        JointNoiseModel::DenseCorrelation { model, .. }
        | JointNoiseModel::ExternalKernel { model } => {
            used.insert(model.clone());
        }
        _ => {}
    }
    if let DataReusePolicy::Shared { groups } = data_reuse {
        for group in groups {
            used.insert(group.joint_likelihood.clone());
        }
    }
    used
}

/// Canonical unresolved physical/statistical question.  No coordinate,
/// tolerance, algorithm, random seed, build fingerprint, or result receipt is
/// permitted in this type.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifiabilityProblemDocument {
    schema_version: u32,
    context_source: SourceKey,
    material_source: SourceKey,
    model_source: SourceKey,
    graph_source: SourceKey,
    sources: BTreeMap<SourceKey, SourceRef>,
    parameters: BTreeMap<ParameterRoleId, StudyParameter>,
    constraints: BTreeMap<ConstraintId, JointConstraint>,
    cases: BTreeMap<CaseId, StudyCaseDocument>,
    influences: BTreeMap<InfluenceId, InfluenceDeclaration>,
    gauges: BTreeMap<GaugeClassId, GaugeDeclaration>,
    joint_noise: JointNoiseModel,
    data_reuse: DataReusePolicy,
}

fn require_source<'a>(
    sources: &'a BTreeMap<SourceKey, SourceRef>,
    key: &SourceKey,
    field: &'static str,
) -> Result<&'a SourceRef, IdentifiabilityError> {
    sources
        .get(key)
        .ok_or_else(|| IdentifiabilityError::UnknownReference {
            field,
            id: key.to_string(),
        })
}

fn require_source_kind(
    sources: &BTreeMap<SourceKey, SourceRef>,
    key: &SourceKey,
    expected: SourceKind,
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    let source = require_source(sources, key, field)?;
    if source.kind != expected {
        return Err(IdentifiabilityError::InvalidText {
            field,
            detail: format!(
                "source {} has kind {:?}, expected {:?}",
                key, source.kind, expected
            ),
        });
    }
    Ok(())
}

fn require_source_kind_in(
    sources: &BTreeMap<SourceKey, SourceRef>,
    key: &SourceKey,
    allowed: &[SourceKind],
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    let source = require_source(sources, key, field)?;
    if !allowed.contains(&source.kind) {
        return Err(IdentifiabilityError::InvalidText {
            field,
            detail: format!(
                "source {} has kind {:?}, expected one of {allowed:?}",
                source.key, source.kind
            ),
        });
    }
    Ok(())
}

fn insert_unique<K: Ord + Clone + fmt::Display, V>(
    rows: Vec<V>,
    field: &'static str,
    key_of: impl Fn(&V) -> &K,
) -> Result<BTreeMap<K, V>, IdentifiabilityError> {
    if rows.is_empty() || rows.len() > MAX_IDENTIFIABILITY_ITEMS {
        return Err(IdentifiabilityError::Cardinality {
            field,
            detail: "collection must be bounded and nonempty".to_string(),
        });
    }
    let mut result = BTreeMap::new();
    for row in rows {
        let key = key_of(&row).clone();
        if result.insert(key.clone(), row).is_some() {
            return Err(IdentifiabilityError::Duplicate {
                field,
                id: key.to_string(),
            });
        }
    }
    Ok(result)
}

fn validate_source_key(
    sources: &BTreeMap<SourceKey, SourceRef>,
    key: &SourceKey,
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    require_source(sources, key, field).map(|_| ())
}

fn validate_derived_parameter_dag(
    parameters: &BTreeMap<ParameterRoleId, StudyParameter>,
) -> Result<(), IdentifiabilityError> {
    fn visit(
        role: &ParameterRoleId,
        parameters: &BTreeMap<ParameterRoleId, StudyParameter>,
        visiting: &mut BTreeSet<ParameterRoleId>,
        visited: &mut BTreeSet<ParameterRoleId>,
    ) -> Result<(), IdentifiabilityError> {
        if visited.contains(role) {
            return Ok(());
        }
        if !visiting.insert(role.clone()) {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "derived parameter graph",
                detail: format!("cycle reaches parameter {role}"),
            });
        }
        let parameter = &parameters[role];
        if let ParameterTreatment::Derived { parents, .. } = &parameter.treatment {
            for parent in parents {
                if !parameters.contains_key(parent) {
                    return Err(IdentifiabilityError::UnknownReference {
                        field: "derived parameter parent",
                        id: parent.to_string(),
                    });
                }
                visit(parent, parameters, visiting, visited)?;
            }
        }
        visiting.remove(role);
        visited.insert(role.clone());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for role in parameters.keys() {
        visit(role, parameters, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn observation_for<'a>(
    cases: &'a BTreeMap<CaseId, StudyCaseDocument>,
    key: &ObservationKey,
) -> Result<&'a StudyObservation, IdentifiabilityError> {
    cases
        .get(&key.case)
        .and_then(|case| case.observations.get(&key.channel))
        .ok_or_else(|| IdentifiabilityError::UnknownReference {
            field: "composite observation key",
            id: format!("{}:{}", key.case, key.channel),
        })
}

fn initial_state_schema_version(state: InitialStateBinding) -> u32 {
    match state {
        InitialStateBinding::Zero { schema_version }
        | InitialStateBinding::Explicit { schema_version, .. } => schema_version,
    }
}

fn experiment_contains_clock(experiment: &ExperimentArtifact, clock: &ArtifactId) -> bool {
    match experiment.clocks() {
        ClockSynchronization::SingleClock { clock_id } => clock_id == clock,
        ClockSynchronization::Synchronized { clock_ids, .. } => clock_ids.contains(clock),
    }
}

fn functional_observations(functional: &DistributionFunctional) -> Vec<&ObservationKey> {
    match functional {
        DistributionFunctional::Location { observation }
        | DistributionFunctional::LogScale { observation }
        | DistributionFunctional::MissingnessLogit { observation }
        | DistributionFunctional::CensoringLogit { observation } => vec![observation],
        DistributionFunctional::Correlation { left, right } => vec![left, right],
    }
}

fn validate_joint_constraint(
    constraint: &JointConstraint,
    parameters: &BTreeMap<ParameterRoleId, StudyParameter>,
    sources: &BTreeMap<SourceKey, SourceRef>,
) -> Result<(), IdentifiabilityError> {
    let require_members = |members: &BTreeSet<ParameterRoleId>| {
        if members.len() < 2 || members.len() > MAX_IDENTIFIABILITY_ITEMS {
            return Err(IdentifiabilityError::Cardinality {
                field: "joint constraint members",
                detail: "joint constraints need at least two bounded members".to_string(),
            });
        }
        for member in members {
            if !parameters.contains_key(member) {
                return Err(IdentifiabilityError::UnknownReference {
                    field: "joint constraint member",
                    id: member.to_string(),
                });
            }
        }
        Ok(())
    };
    match &constraint.kind {
        JointConstraintKind::Affine {
            terms,
            relation,
            rhs_si,
            residual_quantity,
        } => {
            if terms.len() < 2 || terms.len() > MAX_IDENTIFIABILITY_ITEMS || !rhs_si.is_finite() {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "affine joint constraint",
                    detail: "requires at least two bounded terms and a finite RHS".to_string(),
                });
            }
            let mut seen = BTreeSet::new();
            let mut minimum = 0.0;
            let mut maximum = 0.0;
            for term in terms {
                let parameter = parameters.get(&term.parameter).ok_or_else(|| {
                    IdentifiabilityError::UnknownReference {
                        field: "affine constraint member",
                        id: term.parameter.to_string(),
                    }
                })?;
                if !seen.insert(term.parameter.clone()) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "affine constraint member",
                        id: term.parameter.to_string(),
                    });
                }
                let product =
                    checked_add_dims(parameter.quantity.dims(), term.coefficient_quantity.dims())
                        .ok_or_else(|| IdentifiabilityError::InvalidNumeric {
                        field: "affine constraint units",
                        detail: "dimension exponent overflow".to_string(),
                    })?;
                if product != residual_quantity.dims() {
                    return Err(IdentifiabilityError::InvalidNumeric {
                        field: "affine constraint units",
                        detail: format!(
                            "coefficient times {} does not have the residual dimensions",
                            term.parameter
                        ),
                    });
                }
                let endpoints = [
                    term.coefficient * parameter.domain.lo,
                    term.coefficient * parameter.domain.hi,
                ];
                minimum += endpoints[0].min(endpoints[1]);
                maximum += endpoints[0].max(endpoints[1]);
            }
            let feasible = match relation {
                ConstraintRelation::Equal => *rhs_si >= minimum && *rhs_si <= maximum,
                ConstraintRelation::LessOrEqual => minimum <= *rhs_si,
                ConstraintRelation::GreaterOrEqual => maximum >= *rhs_si,
            };
            if !minimum.is_finite() || !maximum.is_finite() || !feasible {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "affine constraint feasibility",
                    detail: "affine constraint has no witness in the Cartesian domain enclosure"
                        .to_string(),
                });
            }
        }
        JointConstraintKind::Simplex {
            members,
            total_si,
            quantity,
        } => {
            require_members(members)?;
            if !total_si.is_finite()
                || members
                    .iter()
                    .any(|role| parameters[role].quantity != *quantity)
            {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "simplex constraint",
                    detail: "members require one exact quantity and a finite total".to_string(),
                });
            }
            let minimum = members
                .iter()
                .map(|role| parameters[role].domain.lo)
                .sum::<f64>();
            let maximum = members
                .iter()
                .map(|role| parameters[role].domain.hi)
                .sum::<f64>();
            if members.iter().any(|role| parameters[role].domain.lo < 0.0)
                || !minimum.is_finite()
                || !maximum.is_finite()
                || *total_si < minimum
                || *total_si > maximum
            {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "simplex constraint feasibility",
                    detail: "simplex members must be nonnegative and their total attainable"
                        .to_string(),
                });
            }
        }
        JointConstraintKind::Ordered { members, .. } => {
            let member_set = members.iter().cloned().collect::<BTreeSet<_>>();
            require_members(&member_set)?;
            if member_set.len() != members.len() {
                return Err(IdentifiabilityError::Duplicate {
                    field: "ordered constraint member",
                    id: constraint.id.to_string(),
                });
            }
            let first = parameters[&members[0]].quantity;
            if members
                .iter()
                .any(|role| parameters[role].quantity != first)
            {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "ordered constraint units",
                    detail: "ordered members need one exact quantity".to_string(),
                });
            }
        }
        JointConstraintKind::ExternalManifold {
            members,
            definition,
            codimension,
        } => {
            require_members(members)?;
            require_source_kind(
                sources,
                definition,
                SourceKind::ExternalManifold,
                "external manifold",
            )?;
            if *codimension == 0 || *codimension as usize >= members.len() {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "external manifold codimension",
                    detail: "codimension must lie in 1..member-count".to_string(),
                });
            }
        }
        JointConstraintKind::StochasticCoupling {
            members,
            distribution,
        } => {
            require_members(members)?;
            require_source_kind(
                sources,
                distribution,
                SourceKind::Prior,
                "joint distribution",
            )?;
        }
    }
    Ok(())
}

impl IdentifiabilityProblemDocument {
    /// Validate and canonicalize a multi-case physical question.  This is
    /// structural admission only; [`Self::from_canonical_bytes`] returns the
    /// same unresolved type and cannot mint [`ProblemId`].
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        context_source: SourceKey,
        material_source: SourceKey,
        model_source: SourceKey,
        graph_source: SourceKey,
        sources: Vec<SourceRef>,
        parameters: Vec<StudyParameter>,
        mut constraints: Vec<JointConstraint>,
        cases: Vec<StudyCaseDocument>,
        mut influences: Vec<InfluenceDeclaration>,
        gauges: Vec<GaugeDeclaration>,
        joint_noise: JointNoiseModel,
        mut data_reuse: DataReusePolicy,
    ) -> Result<Self, IdentifiabilityError> {
        let sources = insert_unique(sources, "source registry", |source| &source.key)?;
        require_source_kind(
            &sources,
            &context_source,
            SourceKind::ContextOfUse,
            "context source",
        )?;
        require_source_kind(
            &sources,
            &material_source,
            SourceKind::MaterialCard,
            "material source",
        )?;
        require_source_kind(
            &sources,
            &model_source,
            SourceKind::ConstitutiveModelCard,
            "model source",
        )?;
        require_source_kind(
            &sources,
            &graph_source,
            SourceKind::ConstitutiveGraph,
            "graph source",
        )?;
        let parameters =
            insert_unique(parameters, "study parameters", |parameter| &parameter.role)?;
        if !parameters.values().any(|parameter| {
            matches!(
                &parameter.treatment,
                ParameterTreatment::Estimated
                    | ParameterTreatment::Profiled
                    | ParameterTreatment::Marginalized
            )
        }) {
            return Err(IdentifiabilityError::Cardinality {
                field: "inferential parameter targets",
                detail: "an identifiability problem needs at least one free inferential target"
                    .to_string(),
            });
        }
        validate_derived_parameter_dag(&parameters)?;
        for constraint in &mut constraints {
            if let JointConstraintKind::Affine { terms, .. } = &mut constraint.kind {
                terms.sort_by(|left, right| left.parameter.cmp(&right.parameter));
            }
        }
        let constraints = if constraints.is_empty() {
            BTreeMap::new()
        } else {
            insert_unique(constraints, "joint constraints", |constraint| {
                &constraint.id
            })?
        };
        for constraint in constraints.values() {
            validate_joint_constraint(constraint, &parameters, &sources)?;
        }
        let cases = insert_unique(cases, "study cases", |case| &case.id)?;
        for influence in &mut influences {
            if let DistributionFunctional::Correlation { left, right } = &mut influence.functional
                && right < left
            {
                core::mem::swap(left, right);
            }
        }
        let influences = if influences.is_empty() {
            BTreeMap::new()
        } else {
            insert_unique(influences, "influence declarations", |influence| {
                &influence.id
            })?
        };
        let gauges = if gauges.is_empty() {
            BTreeMap::new()
        } else {
            insert_unique(gauges, "gauge declarations", |gauge| &gauge.id)?
        };
        let joint_noise = normalize_joint_noise(joint_noise)?;
        if let DataReusePolicy::Shared { groups } = &mut data_reuse {
            groups.sort_by(|left, right| {
                (&left.cases, &left.joint_likelihood, &left.justification).cmp(&(
                    &right.cases,
                    &right.joint_likelihood,
                    &right.justification,
                ))
            });
        }

        for parameter in parameters.values() {
            match &parameter.owner {
                ParameterOwnerBinding::ConstitutiveModel => {}
                ParameterOwnerBinding::InitialState { state_path } => require_source_kind(
                    &sources,
                    state_path,
                    SourceKind::Assumption,
                    "initial-state owner",
                )?,
                ParameterOwnerBinding::Instrument { instrument } => require_source_kind(
                    &sources,
                    instrument,
                    SourceKind::Metrology,
                    "instrument owner",
                )?,
                ParameterOwnerBinding::Discrepancy { family } => require_source_kind(
                    &sources,
                    family,
                    SourceKind::Discrepancy,
                    "discrepancy owner",
                )?,
                ParameterOwnerBinding::ControlledInput { protocol } => require_source_kind(
                    &sources,
                    protocol,
                    SourceKind::Protocol,
                    "controlled-input owner",
                )?,
                ParameterOwnerBinding::Population { hierarchy } => {
                    require_source_kind(&sources, hierarchy, SourceKind::Prior, "population owner")?
                }
            }
            match &parameter.scope {
                ParameterScopeBinding::Global | ParameterScopeBinding::MaterialLot { .. } => {}
                ParameterScopeBinding::Cases(scoped) => {
                    if scoped.is_empty() {
                        return Err(IdentifiabilityError::Cardinality {
                            field: "parameter case scope",
                            detail: "case scope cannot be empty".to_string(),
                        });
                    }
                    for case in scoped {
                        if !cases.contains_key(case) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "parameter case scope",
                                id: case.to_string(),
                            });
                        }
                    }
                }
                ParameterScopeBinding::Specimen { case, specimen } => {
                    let case_doc =
                        cases
                            .get(case)
                            .ok_or_else(|| IdentifiabilityError::UnknownReference {
                                field: "parameter specimen case",
                                id: case.to_string(),
                            })?;
                    if case_doc.specimen.id() != specimen {
                        return Err(IdentifiabilityError::UnknownReference {
                            field: "parameter specimen",
                            id: specimen.as_str().to_string(),
                        });
                    }
                }
                ParameterScopeBinding::Field { support } => require_source_kind_in(
                    &sources,
                    support,
                    &[SourceKind::Geometry, SourceKind::ExternalManifold],
                    "field support",
                )?,
                ParameterScopeBinding::Hierarchical {
                    level, hierarchy, ..
                } => {
                    if *level == 0 {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "hierarchical level",
                            detail: "level zero is reserved for the global population".to_string(),
                        });
                    }
                    require_source_kind(
                        &sources,
                        hierarchy,
                        SourceKind::Prior,
                        "hierarchy source",
                    )?;
                }
            }
            match &parameter.treatment {
                ParameterTreatment::Conditioned(value) => require_source_kind_in(
                    &sources,
                    &value.source,
                    &[SourceKind::EvidenceReceipt, SourceKind::Metrology],
                    "conditioned value source",
                )?,
                ParameterTreatment::Derived { definition, .. } => require_source_kind(
                    &sources,
                    definition,
                    SourceKind::Constraint,
                    "derived parameter definition",
                )?,
                _ => {}
            }
        }

        let mut all_observations = BTreeMap::new();
        let mut modeled_discrepancy_parameters = BTreeSet::new();
        for (case_id, case) in &cases {
            if case.protocol.state_schema_version
                != initial_state_schema_version(case.initial_state)
            {
                return Err(IdentifiabilityError::VersionMismatch {
                    field: "case initial-state/protocol schema",
                    expected: case.protocol.state_schema_version,
                    actual: initial_state_schema_version(case.initial_state),
                });
            }
            require_source_kind(
                &sources,
                &case.forward_model,
                SourceKind::ForwardModel,
                "case forward model",
            )?;
            match &case.data {
                CaseDataDeclaration::Prospective => {
                    if case.observations.values().any(|observation| {
                        !matches!(&observation.rows, ObservationRows::Prospective)
                    }) {
                        return Err(IdentifiabilityError::InvalidText {
                            field: "prospective observation rows",
                            detail: format!("case {case_id} contains retrospective row IDs"),
                        });
                    }
                }
                CaseDataDeclaration::Retrospective {
                    experiment,
                    split,
                    parser,
                    preprocessing,
                    parser_version,
                    ..
                } => {
                    if matches!(&case.purpose, CasePurpose::ProspectiveDesign) {
                        return Err(IdentifiabilityError::InvalidText {
                            field: "prospective-design case data",
                            detail: format!(
                                "case {case_id} is ProspectiveDesign but binds retrospective data"
                            ),
                        });
                    }
                    require_source_kind(
                        &sources,
                        experiment,
                        SourceKind::ExperimentArtifact,
                        "case experiment",
                    )?;
                    require_source_kind(
                        &sources,
                        split,
                        SourceKind::CalibrationSplit,
                        "case split",
                    )?;
                    require_source_kind(&sources, parser, SourceKind::Parser, "case parser")?;
                    require_source_kind(
                        &sources,
                        preprocessing,
                        SourceKind::Preprocessing,
                        "case preprocessing",
                    )?;
                    if *parser_version == 0
                        || case.observations.values().any(|observation| {
                            !matches!(&observation.rows, ObservationRows::Retrospective(_))
                        })
                    {
                        return Err(IdentifiabilityError::InvalidText {
                            field: "retrospective case",
                            detail: format!(
                                "case {case_id} needs a positive parser version and raw rows"
                            ),
                        });
                    }
                }
            }
            for (channel, observation) in &case.observations {
                if &observation.frame != case.specimen.frame() {
                    return Err(IdentifiabilityError::SourceMismatch {
                        field: "observation/specimen frame",
                    });
                }
                if observation.protocol_version != case.protocol.version
                    || observation.refinement_version != case.protocol.refinement_version
                    || observation.clock != case.protocol.clock
                {
                    return Err(IdentifiabilityError::VersionMismatch {
                        field: "case observation protocol/refinement/clock",
                        expected: case.protocol.version,
                        actual: observation.protocol_version,
                    });
                }
                require_source_kind(
                    &sources,
                    &observation.operator,
                    SourceKind::ObservationOperator,
                    "observation operator",
                )?;
                require_source_kind(
                    &sources,
                    &observation.aggregation,
                    SourceKind::ObservationOperator,
                    "observation aggregation",
                )?;
                require_source_kind(
                    &sources,
                    &observation.sensor,
                    SourceKind::Metrology,
                    "observation sensor",
                )?;
                if let MarginalNoiseSpec::Empirical {
                    distribution,
                    finite_variance_model,
                    ..
                } = &observation.noise
                {
                    require_source_kind_in(
                        &sources,
                        distribution,
                        &[SourceKind::Likelihood, SourceKind::EvidenceReceipt],
                        "empirical noise",
                    )?;
                    require_source_kind(
                        &sources,
                        finite_variance_model,
                        SourceKind::EvidenceReceipt,
                        "empirical finite-variance model",
                    )?;
                }
                match &observation.missingness {
                    MissingnessAssumption::Complete { assumption } => require_source_kind(
                        &sources,
                        assumption,
                        SourceKind::Assumption,
                        "completeness assumption",
                    )?,
                    MissingnessAssumption::Modeled { mechanism } => require_source_kind(
                        &sources,
                        mechanism,
                        SourceKind::Likelihood,
                        "missingness mechanism",
                    )?,
                    MissingnessAssumption::Unknown { .. } => {}
                }
                let key = ObservationKey::new(case_id.clone(), channel.clone());
                all_observations.insert(key, observation);
            }
            for discrepancy in case.discrepancies.values() {
                match discrepancy {
                    StudyDiscrepancy::Uncharacterized { reason }
                    | StudyDiscrepancy::NotApplicable { reason } => {
                        validate_reason(reason, "discrepancy reason")?
                    }
                    StudyDiscrepancy::AssumedZero { assumption } => require_source_kind(
                        &sources,
                        assumption,
                        SourceKind::Assumption,
                        "zero-discrepancy assumption",
                    )?,
                    StudyDiscrepancy::Modeled {
                        family,
                        parameters: discrepancy_parameters,
                        support,
                        confounding_guard,
                    } => {
                        require_source_kind(
                            &sources,
                            family,
                            SourceKind::Discrepancy,
                            "modeled discrepancy family",
                        )?;
                        require_source_kind_in(
                            &sources,
                            support,
                            &[SourceKind::Geometry, SourceKind::ExternalManifold],
                            "modeled discrepancy support",
                        )?;
                        require_source_kind(
                            &sources,
                            confounding_guard,
                            SourceKind::Constraint,
                            "modeled discrepancy confounding guard",
                        )?;
                        if discrepancy_parameters.is_empty() {
                            return Err(IdentifiabilityError::Cardinality {
                                field: "discrepancy parameters",
                                detail: "modeled discrepancy needs explicit parameter roles"
                                    .to_string(),
                            });
                        }
                        for role in discrepancy_parameters {
                            let parameter = parameters.get(role).ok_or_else(|| {
                                IdentifiabilityError::UnknownReference {
                                    field: "discrepancy parameter",
                                    id: role.to_string(),
                                }
                            })?;
                            if !matches!(
                                &parameter.owner,
                                ParameterOwnerBinding::Discrepancy {
                                    family: owner_family
                                } if owner_family == family
                            ) {
                                return Err(IdentifiabilityError::InvalidText {
                                    field: "discrepancy parameter owner",
                                    detail: format!(
                                        "parameter {role} is not owned by modeled family {family}"
                                    ),
                                });
                            }
                            modeled_discrepancy_parameters.insert(role.clone());
                        }
                    }
                }
            }
        }
        for parameter in parameters.values() {
            if matches!(&parameter.owner, ParameterOwnerBinding::Discrepancy { .. })
                && !modeled_discrepancy_parameters.contains(&parameter.role)
            {
                return Err(IdentifiabilityError::UnknownReference {
                    field: "modeled discrepancy parameter",
                    id: parameter.role.to_string(),
                });
            }
        }

        let mut influenced_parameters = BTreeSet::new();
        for influence in influences.values() {
            if !parameters.contains_key(&influence.parameter) {
                return Err(IdentifiabilityError::UnknownReference {
                    field: "influence parameter",
                    id: influence.parameter.to_string(),
                });
            }
            if let DistributionFunctional::Correlation { left, right } = &influence.functional {
                let left_observation = observation_for(&cases, left)?;
                let right_observation = observation_for(&cases, right)?;
                if !left_observation.noise.finite_standard_deviation()
                    || !right_observation.noise.finite_standard_deviation()
                {
                    return Err(IdentifiabilityError::Covariance {
                        detail: "Pearson-correlation influence requires two finite-second-moment marginals"
                            .to_string(),
                    });
                }
            }
            for key in functional_observations(&influence.functional) {
                let observation = observation_for(&cases, key)?;
                match &influence.functional {
                    DistributionFunctional::Correlation { left, right } if left == right => {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "correlation functional",
                            detail: "self-correlation is a constant, not an identifiability route"
                                .to_string(),
                        });
                    }
                    DistributionFunctional::MissingnessLogit { .. }
                        if matches!(
                            &observation.missingness,
                            MissingnessAssumption::Complete { .. }
                        ) =>
                    {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "missingness functional",
                            detail: "a Complete channel cannot also expose missingness influence"
                                .to_string(),
                        });
                    }
                    DistributionFunctional::CensoringLogit { .. }
                        if observation.saturation.is_none() =>
                    {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "censoring functional",
                            detail: "censoring influence requires a saturation domain".to_string(),
                        });
                    }
                    _ => {}
                }
            }
            match &influence.representation {
                InfluenceRepresentation::Direct => {}
                InfluenceRepresentation::StateMediated { state_path } => require_source_kind(
                    &sources,
                    state_path,
                    SourceKind::Assumption,
                    "state-mediated influence",
                )?,
                InfluenceRepresentation::Composite { operator, inputs } => {
                    require_source_kind(
                        &sources,
                        operator,
                        SourceKind::Analyzer,
                        "composite influence operator",
                    )?;
                    if inputs.is_empty() || inputs.contains(&influence.id) {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "composite influence inputs",
                            detail: format!("influence {} has empty or self input", influence.id),
                        });
                    }
                    for input in inputs {
                        if !influences.contains_key(input) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "composite influence input",
                                id: input.to_string(),
                            });
                        }
                    }
                }
                InfluenceRepresentation::ExternalDefinition { definition } => {
                    require_source_kind(
                        &sources,
                        definition,
                        SourceKind::Constraint,
                        "external influence definition",
                    )?;
                }
            }
            influenced_parameters.insert(influence.parameter.clone());
        }
        for parameter in parameters.values() {
            let free = matches!(
                &parameter.treatment,
                ParameterTreatment::Estimated
                    | ParameterTreatment::Profiled
                    | ParameterTreatment::Marginalized
            );
            match (&parameter.influence_coverage, free) {
                (InfluenceCoverage::Declared, _)
                    if !influenced_parameters.contains(&parameter.role) =>
                {
                    return Err(IdentifiabilityError::DisconnectedEstimatedParameter {
                        parameter: parameter.role.clone(),
                    });
                }
                (InfluenceCoverage::IntentionallyAbsent { .. }, true)
                    if influenced_parameters.contains(&parameter.role) =>
                {
                    return Err(IdentifiabilityError::InvalidNumeric {
                        field: "parameter influence coverage",
                        detail: format!(
                            "parameter {} both declares and denies influence routes",
                            parameter.role
                        ),
                    });
                }
                (InfluenceCoverage::NotApplicable { .. }, false)
                    if influenced_parameters.contains(&parameter.role) =>
                {
                    return Err(IdentifiabilityError::InvalidNumeric {
                        field: "parameter influence coverage",
                        detail: format!(
                            "parameter {} marks influence not applicable but owns a route",
                            parameter.role
                        ),
                    });
                }
                _ => {}
            }
        }

        // Composite influence declarations form a DAG; otherwise their
        // semantics depend on evaluation order.
        fn visit_influence(
            id: &InfluenceId,
            influences: &BTreeMap<InfluenceId, InfluenceDeclaration>,
            visiting: &mut BTreeSet<InfluenceId>,
            visited: &mut BTreeSet<InfluenceId>,
        ) -> Result<(), IdentifiabilityError> {
            if visited.contains(id) {
                return Ok(());
            }
            if !visiting.insert(id.clone()) {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "composite influence graph",
                    detail: format!("cycle reaches influence {id}"),
                });
            }
            if let InfluenceRepresentation::Composite { inputs, .. } =
                &influences[id].representation
            {
                for input in inputs {
                    visit_influence(input, influences, visiting, visited)?;
                }
            }
            visiting.remove(id);
            visited.insert(id.clone());
            Ok(())
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for id in influences.keys() {
            visit_influence(id, &influences, &mut visiting, &mut visited)?;
        }

        let mut gauged_members = BTreeSet::new();
        for gauge in gauges.values() {
            for member in &gauge.members {
                if !parameters.contains_key(member) {
                    return Err(IdentifiabilityError::UnknownReference {
                        field: "gauge member",
                        id: member.to_string(),
                    });
                }
                if !gauged_members.insert(member.clone()) {
                    return Err(IdentifiabilityError::InvalidGauge {
                        gauge: gauge.id.clone(),
                        detail: format!("overlapping v1 gauge member {member}"),
                    });
                }
            }
            require_source_kind(
                &sources,
                &gauge.action,
                SourceKind::GaugeAction,
                "gauge action",
            )?;
            match &gauge.kind {
                GaugeKind::Continuous { dimension }
                    if *dimension as usize > gauge.members.len() =>
                {
                    return Err(IdentifiabilityError::InvalidGauge {
                        gauge: gauge.id.clone(),
                        detail: "continuous dimension exceeds member count".to_string(),
                    });
                }
                GaugeKind::Mixed {
                    continuous_dimension,
                    ..
                } if *continuous_dimension as usize > gauge.members.len() => {
                    return Err(IdentifiabilityError::InvalidGauge {
                        gauge: gauge.id.clone(),
                        detail: "mixed continuous dimension exceeds member count".to_string(),
                    });
                }
                GaugeKind::Stratified { strata } => require_source_kind(
                    &sources,
                    strata,
                    SourceKind::ExternalManifold,
                    "gauge strata",
                )?,
                _ => {}
            }
            match &gauge.handling {
                GaugeHandling::Quotient {
                    quotient_map,
                    local_sections,
                } => {
                    require_source_kind(
                        &sources,
                        quotient_map,
                        SourceKind::GaugeAction,
                        "gauge quotient map",
                    )?;
                    require_source_kind(
                        &sources,
                        local_sections,
                        SourceKind::GaugeSection,
                        "gauge local sections",
                    )?;
                }
                GaugeHandling::Slice { constraint } => {
                    if !constraints.contains_key(constraint) {
                        return Err(IdentifiabilityError::UnknownReference {
                            field: "gauge slice constraint",
                            id: constraint.to_string(),
                        });
                    }
                }
                GaugeHandling::Retained { .. } | GaugeHandling::Unresolved { .. } => {}
            }
        }

        match &joint_noise {
            JointNoiseModel::Independent => {}
            JointNoiseModel::DenseCorrelation {
                order,
                correlation,
                model,
            } => {
                require_source_kind(
                    &sources,
                    model,
                    SourceKind::Likelihood,
                    "dense correlation model",
                )?;
                let unique = order.iter().cloned().collect::<BTreeSet<_>>();
                let all = all_observations.keys().cloned().collect::<BTreeSet<_>>();
                if order.len() != all.len()
                    || unique != all
                    || correlation.dimension() != order.len()
                    || order
                        .iter()
                        .any(|key| !all_observations[key].noise.finite_standard_deviation())
                {
                    return Err(IdentifiabilityError::Covariance {
                        detail: "dense correlation needs every composite channel exactly once and finite marginal standard deviations"
                            .to_string(),
                    });
                }
                for index in 0..order.len() {
                    if !same_f64(matrix_get(correlation, index, index), 1.0) {
                        return Err(IdentifiabilityError::Covariance {
                            detail: format!("correlation diagonal {index} is not exactly one"),
                        });
                    }
                }
            }
            JointNoiseModel::ExternalKernel { model } => require_source_kind(
                &sources,
                model,
                SourceKind::Likelihood,
                "external noise kernel",
            )?,
            JointNoiseModel::Unknown { reason } => {
                validate_reason(reason, "unknown joint noise reason")?
            }
        }

        match &data_reuse {
            DataReusePolicy::Disjoint => {
                let mut seen = BTreeMap::<ContentHash, CaseId>::new();
                for (case_id, case) in &cases {
                    if let Some(experiment) = retrospective_experiment(case) {
                        let hash = sources[experiment].expected_hash;
                        if let Some(other) = seen.insert(hash, case_id.clone()) {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "data reuse policy",
                                detail: format!(
                                    "cases {other} and {case_id} reuse one experiment under Disjoint"
                                ),
                            });
                        }
                    }
                }
            }
            DataReusePolicy::Shared { groups } => {
                if groups.is_empty() || groups.len() > MAX_IDENTIFIABILITY_ITEMS {
                    return Err(IdentifiabilityError::Cardinality {
                        field: "data sharing groups",
                        detail: "Shared policy needs bounded nonempty groups".to_string(),
                    });
                }
                let mut membership = BTreeMap::<CaseId, usize>::new();
                let mut shared_hash_owners = BTreeMap::<ContentHash, usize>::new();
                for (index, group) in groups.iter().enumerate() {
                    require_source_kind(
                        &sources,
                        &group.joint_likelihood,
                        SourceKind::Likelihood,
                        "sharing-group likelihood",
                    )?;
                    for case_id in &group.cases {
                        if membership.insert(case_id.clone(), index).is_some() {
                            return Err(IdentifiabilityError::Duplicate {
                                field: "data sharing group membership",
                                id: case_id.to_string(),
                            });
                        }
                        let case = cases.get(case_id).ok_or_else(|| {
                            IdentifiabilityError::UnknownReference {
                                field: "data sharing case",
                                id: case_id.to_string(),
                            }
                        })?;
                        let experiment = retrospective_experiment(case).ok_or_else(|| {
                            IdentifiabilityError::InvalidText {
                                field: "data sharing case",
                                detail: format!("prospective case {case_id} cannot share raw data"),
                            }
                        })?;
                        let hash = sources[experiment].expected_hash;
                        if let Some(other) = shared_hash_owners.insert(hash, index)
                            && other != index
                        {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "data reuse policy",
                                detail: format!(
                                    "sharing groups {other} and {index} reuse one experiment"
                                ),
                            });
                        }
                    }
                }
                let mut ungrouped = BTreeMap::<ContentHash, CaseId>::new();
                for (case_id, case) in &cases {
                    if membership.contains_key(case_id) {
                        continue;
                    }
                    if let Some(experiment) = retrospective_experiment(case) {
                        let hash = sources[experiment].expected_hash;
                        if shared_hash_owners.contains_key(&hash) {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "data reuse policy",
                                detail: format!(
                                    "ungrouped case {case_id} reuses an experiment owned by a sharing group"
                                ),
                            });
                        }
                        if let Some(other) = ungrouped.insert(hash, case_id.clone()) {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "data reuse policy",
                                detail: format!(
                                    "ungrouped cases {other} and {case_id} reuse one experiment"
                                ),
                            });
                        }
                    }
                }
            }
        }

        let reachable = problem_source_reachability(
            &context_source,
            &material_source,
            &model_source,
            &graph_source,
            &parameters,
            &constraints,
            &cases,
            &influences,
            &gauges,
            &joint_noise,
            &data_reuse,
        );
        let registered = sources.keys().cloned().collect::<BTreeSet<_>>();
        if reachable != registered {
            let detail = registered.difference(&reachable).next().map_or_else(
                || "a referenced source is absent from the registry".to_string(),
                |unused| format!("source {unused} is registered but unreachable"),
            );
            return Err(IdentifiabilityError::InvalidText {
                field: "source registry closure",
                detail,
            });
        }

        Ok(Self {
            schema_version: IDENTIFIABILITY_PROBLEM_IDENTITY_VERSION,
            context_source,
            material_source,
            model_source,
            graph_source,
            sources,
            parameters,
            constraints,
            cases,
            influences,
            gauges,
            joint_noise,
            data_reuse,
        })
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn context_source(&self) -> &SourceKey {
        &self.context_source
    }

    #[must_use]
    pub const fn material_source(&self) -> &SourceKey {
        &self.material_source
    }

    #[must_use]
    pub const fn model_source(&self) -> &SourceKey {
        &self.model_source
    }

    #[must_use]
    pub const fn graph_source(&self) -> &SourceKey {
        &self.graph_source
    }

    #[must_use]
    pub const fn sources(&self) -> &BTreeMap<SourceKey, SourceRef> {
        &self.sources
    }

    #[must_use]
    pub const fn parameters(&self) -> &BTreeMap<ParameterRoleId, StudyParameter> {
        &self.parameters
    }

    #[must_use]
    pub const fn cases(&self) -> &BTreeMap<CaseId, StudyCaseDocument> {
        &self.cases
    }

    #[must_use]
    pub const fn constraints(&self) -> &BTreeMap<ConstraintId, JointConstraint> {
        &self.constraints
    }

    #[must_use]
    pub const fn influences(&self) -> &BTreeMap<InfluenceId, InfluenceDeclaration> {
        &self.influences
    }

    #[must_use]
    pub const fn gauges(&self) -> &BTreeMap<GaugeClassId, GaugeDeclaration> {
        &self.gauges
    }

    #[must_use]
    pub const fn joint_noise(&self) -> &JointNoiseModel {
        &self.joint_noise
    }

    #[must_use]
    pub const fn data_reuse(&self) -> &DataReusePolicy {
        &self.data_reuse
    }

    /// Derived derivative quantity for an influence functional with respect to
    /// its physical parameter.  Log-scale, correlation, missingness-logit, and
    /// censoring-logit functionals are dimensionless by definition.
    pub fn influence_derivative_quantity(
        &self,
        id: &InfluenceId,
    ) -> Result<QuantitySpec, IdentifiabilityError> {
        let influence =
            self.influences
                .get(id)
                .ok_or_else(|| IdentifiabilityError::UnknownReference {
                    field: "influence derivative",
                    id: id.to_string(),
                })?;
        let output_dims = match &influence.functional {
            DistributionFunctional::Location { observation } => {
                observation_for(&self.cases, observation)?.quantity.dims()
            }
            DistributionFunctional::LogScale { .. }
            | DistributionFunctional::Correlation { .. }
            | DistributionFunctional::MissingnessLogit { .. }
            | DistributionFunctional::CensoringLogit { .. } => Dims([0; 6]),
        };
        let input_dims = self.parameters[&influence.parameter].quantity.dims();
        let dims = checked_derivative_dims(output_dims, input_dims).ok_or_else(|| {
            IdentifiabilityError::InvalidNumeric {
                field: "influence derivative quantity",
                detail: "dimension exponent overflow".to_string(),
            }
        })?;
        Ok(QuantitySpec::dimensional(dims))
    }

    /// Canonical unresolved bytes.  These bytes contain no source authority.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, IdentifiabilityError> {
        encode_problem(self)
    }

    /// Decode and fully revalidate an unresolved document.  This method cannot
    /// return an admitted problem or mint a [`ProblemId`].
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IdentifiabilityError> {
        decode_problem(bytes)
    }
}

/// Concrete V&V sources for one retrospective case.
#[derive(Debug, Clone, Copy)]
pub struct CaseSourceBundle<'a> {
    experiment: &'a ExperimentArtifact,
    split: &'a CalibrationSplit,
    blind_release: Option<&'a BlindReleaseReceipt>,
}

impl<'a> CaseSourceBundle<'a> {
    #[must_use]
    pub const fn new(experiment: &'a ExperimentArtifact, split: &'a CalibrationSplit) -> Self {
        Self {
            experiment,
            split,
            blind_release: None,
        }
    }

    /// Attach the authority release required to consume sealed blind rows.
    /// Supplying a release to a non-blind case is rejected during admission.
    #[must_use]
    pub const fn with_blind_release(mut self, release: &'a BlindReleaseReceipt) -> Self {
        self.blind_release = Some(release);
        self
    }

    #[must_use]
    pub const fn experiment(&self) -> &'a ExperimentArtifact {
        self.experiment
    }

    #[must_use]
    pub const fn split(&self) -> &'a CalibrationSplit {
        self.split
    }

    #[must_use]
    pub const fn blind_release(&self) -> Option<&'a BlindReleaseReceipt> {
        self.blind_release
    }
}

/// Concrete and opaque artifacts required to resolve a problem document.
/// Extra, missing, unverified, stale-kind, or stale-version resolutions refuse.
#[derive(Debug)]
pub struct ProblemSourceBundle<'a> {
    context: &'a ContextOfUse,
    material: &'a MaterialCard,
    model: &'a ConstitutiveModelCard,
    cases: BTreeMap<CaseId, CaseSourceBundle<'a>>,
    opaque: SourceResolutionSet,
    concrete_authority: BTreeMap<SourceKey, AuthorityDisposition>,
}

impl<'a> ProblemSourceBundle<'a> {
    #[must_use]
    pub fn new(
        context: &'a ContextOfUse,
        material: &'a MaterialCard,
        model: &'a ConstitutiveModelCard,
        cases: BTreeMap<CaseId, CaseSourceBundle<'a>>,
        opaque: SourceResolutionSet,
    ) -> Self {
        Self {
            context,
            material,
            model,
            cases,
            opaque,
            concrete_authority: BTreeMap::new(),
        }
    }

    /// Attach external trust-policy dispositions to concrete sources. Missing
    /// entries remain honestly `ContentVerified`; duplicate or malformed
    /// dispositions refuse.
    pub fn with_concrete_authority(
        mut self,
        entries: Vec<(SourceKey, AuthorityDisposition)>,
    ) -> Result<Self, IdentifiabilityError> {
        for (key, authority) in entries {
            validate_authority_disposition(&authority)?;
            if self
                .concrete_authority
                .insert(key.clone(), authority)
                .is_some()
            {
                return Err(IdentifiabilityError::Duplicate {
                    field: "concrete source authority",
                    id: key.to_string(),
                });
            }
        }
        Ok(self)
    }
}

/// Source-resolved problem.  Its retained document remains inspectable, while
/// all derived bindings are read-only and recomputable from the source bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedIdentifiabilityProblem {
    document: IdentifiabilityProblemDocument,
    problem_id: ProblemId,
    source_admission_id: SourceAdmissionId,
    context: ContextBinding,
    model: MaterialModelBinding,
    data: BTreeMap<CaseId, DataLineage>,
    source_admission: SourceAdmissionRecord,
}

fn concrete_resolution(
    reference: &SourceRef,
    actual_kind: SourceKind,
    actual_hash: ContentHash,
    authority: AuthorityDisposition,
) -> Result<SourceResolution, IdentifiabilityError> {
    validate_authority_disposition(&authority)?;
    if matches!(&authority, AuthorityDisposition::Unverified { .. }) {
        return Err(IdentifiabilityError::InvalidText {
            field: "source authority",
            detail: format!(
                "concrete source {} is unresolved and cannot mint ProblemId",
                reference.key
            ),
        });
    }
    if reference.kind != actual_kind
        || reference.expected_hash != actual_hash
        || !hash_is_nonzero(actual_hash)
    {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "concrete source",
        });
    }
    let (expected_domain, expected_version) = match actual_kind {
        SourceKind::ContextOfUse
        | SourceKind::ExperimentArtifact
        | SourceKind::CalibrationSplit => (VV_ARTIFACT_SOURCE_DOMAIN, VV_SCHEMA_VERSION),
        SourceKind::MaterialCard => (MATERIAL_CARD_SOURCE_DOMAIN, MATDB_SCHEMA_VERSION),
        SourceKind::ConstitutiveModelCard => {
            (CONSTITUTIVE_MODEL_CARD_SOURCE_DOMAIN, MATDB_SCHEMA_VERSION)
        }
        _ => {
            return Err(IdentifiabilityError::InvalidText {
                field: "typed source contract",
                detail: format!("source kind {actual_kind:?} has no typed resolver in fs-material"),
            });
        }
    };
    if reference.content_hash_domain != expected_domain
        || reference.contract_version != expected_version
    {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "typed source digest domain/contract version",
        });
    }
    Ok(SourceResolution {
        key: reference.key.clone(),
        kind: actual_kind,
        resolved_hash: actual_hash,
        content_hash_domain: reference.content_hash_domain.clone(),
        contract_version: reference.contract_version,
        authority,
        verification: ResolutionVerification::TypedArtifact,
    })
}

fn validate_authority_disposition(
    authority: &AuthorityDisposition,
) -> Result<(), IdentifiabilityError> {
    match authority {
        AuthorityDisposition::ContentVerified => Ok(()),
        AuthorityDisposition::ExternalTrustReceipt { trust_receipt }
            if hash_is_nonzero(*trust_receipt) =>
        {
            Ok(())
        }
        AuthorityDisposition::ExternalTrustReceipt { .. } => {
            Err(IdentifiabilityError::ZeroIdentity {
                field: "source trust receipt",
            })
        }
        AuthorityDisposition::Unverified { reason } => {
            validate_reason(reason, "unverified source reason")
        }
    }
}

fn concrete_authority_for(
    bundle: &ProblemSourceBundle<'_>,
    key: &SourceKey,
) -> AuthorityDisposition {
    bundle
        .concrete_authority
        .get(key)
        .cloned()
        .unwrap_or(AuthorityDisposition::ContentVerified)
}

fn insert_exact_resolution(
    resolutions: &mut BTreeMap<SourceKey, SourceResolution>,
    key: &SourceKey,
    resolution: SourceResolution,
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    if let Some(existing) = resolutions.get(key) {
        if existing != &resolution {
            return Err(IdentifiabilityError::SourceMismatch { field });
        }
    } else {
        resolutions.insert(key.clone(), resolution);
    }
    Ok(())
}

fn admit_opaque_resolution(
    reference: &SourceRef,
    resolution: &SourceResolution,
) -> Result<(), IdentifiabilityError> {
    if resolution.key != reference.key
        || resolution.kind != reference.kind
        || resolution.resolved_hash != reference.expected_hash
        || resolution.content_hash_domain != reference.content_hash_domain
        || resolution.contract_version != reference.contract_version
    {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "opaque source resolution",
        });
    }
    if matches!(
        &resolution.authority,
        AuthorityDisposition::Unverified { .. }
    ) || matches!(&resolution.verification, ResolutionVerification::Unverified)
    {
        return Err(IdentifiabilityError::InvalidText {
            field: "source authority",
            detail: format!(
                "source {} is explicitly unresolved and cannot mint ProblemId",
                reference.key
            ),
        });
    }
    Ok(())
}

fn bind_source_reference(
    references: &mut BTreeMap<SourceKey, SourceRef>,
    source: &SourceRef,
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    if let Some(prior) = references.insert(source.key.clone(), source.clone())
        && prior != *source
    {
        return Err(IdentifiabilityError::SourceMismatch { field });
    }
    Ok(())
}

fn validate_source_authority_closure(
    references: &BTreeMap<SourceKey, SourceRef>,
    authority: &SourceResolutionSet,
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    if authority.entries.len() != references.len() {
        return Err(IdentifiabilityError::Cardinality {
            field,
            detail: "every referenced source needs exactly one locally verified resolution"
                .to_string(),
        });
    }
    for (key, reference) in references {
        let resolution =
            authority
                .entries
                .get(key)
                .ok_or_else(|| IdentifiabilityError::UnknownReference {
                    field,
                    id: key.to_string(),
                })?;
        admit_opaque_resolution(reference, resolution)?;
    }
    Ok(())
}

fn encode_source_admission(
    admission: &SourceAdmissionRecord,
) -> Result<Vec<u8>, IdentifiabilityError> {
    check_source_admission_identity_version(admission.schema_version)?;
    let mut writer = CanonicalWriter::new();
    writer.bytes.extend_from_slice(SOURCE_ADMISSION_MAGIC);
    writer.u32(admission.schema_version);
    writer.hash(admission.problem_id.0);
    writer.count(admission.resolutions.len(), "source admission resolutions")?;
    for resolution in admission.resolutions.values() {
        encode_resolution_entry(&mut writer, resolution)?;
    }
    writer.finish()
}

fn source_admission_identity_hash(
    admission: &SourceAdmissionRecord,
) -> Result<SourceAdmissionId, IdentifiabilityError> {
    Ok(SourceAdmissionId(hash_domain(
        IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_DOMAIN,
        &encode_source_admission(admission)?,
    )))
}

fn problem_identity_hash(
    document: &IdentifiabilityProblemDocument,
) -> Result<ProblemId, IdentifiabilityError> {
    Ok(ProblemId(hash_domain(
        IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN,
        &encode_problem(document)?,
    )))
}

impl AdmittedIdentifiabilityProblem {
    /// Resolve exact concrete sources, require a closed authority set for every
    /// opaque reference, re-derive V&V bindings/lineage, and only then mint
    /// problem and source-admission identities.
    pub fn resolve_and_admit(
        document: IdentifiabilityProblemDocument,
        bundle: ProblemSourceBundle<'_>,
    ) -> Result<Self, IdentifiabilityError> {
        let context = ContextBinding::from_vv(bundle.context)?;
        let graph_hash = document.sources[&document.graph_source].expected_hash;
        let model = MaterialModelBinding::from_cards(bundle.material, bundle.model, graph_hash)?;
        let mut resolutions = BTreeMap::new();
        let mut concrete_keys = BTreeSet::new();

        let context_ref = &document.sources[&document.context_source];
        let context_resolution = concrete_resolution(
            context_ref,
            SourceKind::ContextOfUse,
            context.reference.hash(),
            concrete_authority_for(&bundle, &context_ref.key),
        )?;
        concrete_keys.insert(context_ref.key.clone());
        resolutions.insert(context_ref.key.clone(), context_resolution);

        let material_ref = &document.sources[&document.material_source];
        let material_resolution = concrete_resolution(
            material_ref,
            SourceKind::MaterialCard,
            bundle.material.content_hash(),
            concrete_authority_for(&bundle, &material_ref.key),
        )?;
        concrete_keys.insert(material_ref.key.clone());
        resolutions.insert(material_ref.key.clone(), material_resolution);

        let model_ref = &document.sources[&document.model_source];
        let model_resolution = concrete_resolution(
            model_ref,
            SourceKind::ConstitutiveModelCard,
            bundle.model.content_hash(),
            concrete_authority_for(&bundle, &model_ref.key),
        )?;
        concrete_keys.insert(model_ref.key.clone());
        resolutions.insert(model_ref.key.clone(), model_resolution);

        // The physical parameter roster is closed against the exact model card.
        for (role, roster) in &model.parameter_roster {
            let parameter = document.parameters.get(role).ok_or_else(|| {
                IdentifiabilityError::UnknownReference {
                    field: "model-card parameter declaration",
                    id: role.to_string(),
                }
            })?;
            if !matches!(&parameter.owner, ParameterOwnerBinding::ConstitutiveModel)
                || parameter.quantity != roster.quantity
                || roster.nominal() < parameter.domain.lo
                || roster.nominal() > parameter.domain.hi
            {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "model-card parameter binding",
                    detail: format!(
                        "parameter {role} must match owner, exact quantity, and nominal domain"
                    ),
                });
            }
        }
        for parameter in document.parameters.values() {
            if matches!(&parameter.owner, ParameterOwnerBinding::ConstitutiveModel)
                && !model.parameter_roster.contains_key(&parameter.role)
            {
                return Err(IdentifiabilityError::UnknownReference {
                    field: "constitutive-model parameter",
                    id: parameter.role.to_string(),
                });
            }
        }
        if matches!(
            model.initial_state_policy,
            InitialStatePolicy::ZeroInternalState
        ) && document
            .parameters
            .values()
            .any(|parameter| matches!(&parameter.owner, ParameterOwnerBinding::InitialState { .. }))
        {
            return Err(IdentifiabilityError::InitialStatePolicy {
                detail:
                    "zero-internal-state model cannot expose inferential initial-state parameters"
                        .to_string(),
            });
        }

        // A blind release is authority over a split, not an observation-local
        // annotation. Pre-scan by split key so shared uses cannot acquire
        // order-dependent or contradictory authority dispositions.
        let mut blind_releases = BTreeMap::<SourceKey, &BlindReleaseReceipt>::new();
        for (case_id, case) in &document.cases {
            let CaseDataDeclaration::Retrospective { split, .. } = &case.data else {
                continue;
            };
            let case_sources = bundle.cases.get(case_id).ok_or_else(|| {
                IdentifiabilityError::UnknownReference {
                    field: "retrospective case source bundle",
                    id: case_id.to_string(),
                }
            })?;
            match (&case.purpose, case_sources.blind_release) {
                (CasePurpose::BlindFalsification, Some(release)) => {
                    if let Some(existing) = blind_releases.insert(split.clone(), release)
                        && existing != release
                    {
                        return Err(IdentifiabilityError::SourceMismatch {
                            field: "shared split blind release",
                        });
                    }
                }
                (CasePurpose::BlindFalsification, None) => {
                    return Err(IdentifiabilityError::InvalidText {
                        field: "blind release",
                        detail: format!(
                            "blind-falsification case {case_id} requires an authority release"
                        ),
                    });
                }
                (_, Some(_)) => {
                    return Err(IdentifiabilityError::InvalidText {
                        field: "blind release",
                        detail: format!(
                            "non-blind case {case_id} must not receive blind-release authority"
                        ),
                    });
                }
                (_, None) => {}
            }
        }

        let mut data = BTreeMap::new();
        for (case_id, case) in &document.cases {
            case.initial_state.validate_against(&model)?;
            if case.protocol.state_schema_version != model.state_schema_version {
                return Err(IdentifiabilityError::VersionMismatch {
                    field: "case protocol/model state schema",
                    expected: model.state_schema_version,
                    actual: case.protocol.state_schema_version,
                });
            }
            for observation in case.observations.values() {
                if context.qoi_units.get(&observation.qoi) != Some(&observation.unit) {
                    return Err(IdentifiabilityError::UnknownReference {
                        field: "context QoI/unit",
                        id: observation.qoi.as_str().to_string(),
                    });
                }
            }
            match &case.data {
                CaseDataDeclaration::Prospective => {
                    if bundle.cases.contains_key(case_id) {
                        return Err(IdentifiabilityError::InvalidText {
                            field: "prospective case source bundle",
                            detail: format!(
                                "prospective case {case_id} must not receive experiment data"
                            ),
                        });
                    }
                }
                CaseDataDeclaration::Retrospective {
                    experiment,
                    split,
                    parser,
                    preprocessing,
                    parser_version,
                    split_grouping,
                } => {
                    let case_sources = bundle.cases.get(case_id).ok_or_else(|| {
                        IdentifiabilityError::UnknownReference {
                            field: "retrospective case source bundle",
                            id: case_id.to_string(),
                        }
                    })?;
                    let experiment_hash =
                        case_sources.experiment.content_hash().map_err(|error| {
                            IdentifiabilityError::Vv {
                                detail: error.to_string(),
                            }
                        })?;
                    let split_hash = case_sources.split.content_hash().map_err(|error| {
                        IdentifiabilityError::Vv {
                            detail: error.to_string(),
                        }
                    })?;
                    let experiment_resolution = concrete_resolution(
                        &document.sources[experiment],
                        SourceKind::ExperimentArtifact,
                        experiment_hash,
                        concrete_authority_for(&bundle, experiment),
                    )?;
                    concrete_keys.insert(experiment.clone());
                    insert_exact_resolution(
                        &mut resolutions,
                        experiment,
                        experiment_resolution,
                        "shared experiment source resolution",
                    )?;

                    let release_authority = blind_releases.get(split).map(|release| {
                        AuthorityDisposition::ExternalTrustReceipt {
                            trust_receipt: release.authority_receipt_hash(),
                        }
                    });
                    if let (Some(required), Some(declared)) = (
                        release_authority.as_ref(),
                        bundle.concrete_authority.get(split),
                    ) && required != declared
                    {
                        return Err(IdentifiabilityError::SourceMismatch {
                            field: "blind release/concrete source authority",
                        });
                    }
                    let split_authority =
                        release_authority.unwrap_or_else(|| concrete_authority_for(&bundle, split));
                    let split_resolution = concrete_resolution(
                        &document.sources[split],
                        SourceKind::CalibrationSplit,
                        split_hash,
                        split_authority,
                    )?;
                    concrete_keys.insert(split.clone());
                    insert_exact_resolution(
                        &mut resolutions,
                        split,
                        split_resolution,
                        "shared split source resolution",
                    )?;
                    let parser_hash = document.sources[parser].expected_hash;
                    let preprocessing_hash = document.sources[preprocessing].expected_hash;
                    let lineage = DataLineage::from_vv(
                        case_sources.experiment,
                        case_sources.split,
                        parser_hash,
                        *parser_version,
                        preprocessing_hash,
                        split_grouping.clone(),
                    )?;
                    for observation in case.observations.values() {
                        if !lineage.qois().contains(&observation.qoi) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "experiment observation QoI",
                                id: observation.qoi.as_str().to_string(),
                            });
                        }
                        let instrument = case_sources
                            .experiment
                            .instruments()
                            .iter()
                            .find(|instrument| {
                                instrument.instrument_id() == &observation.instrument
                            })
                            .ok_or_else(|| IdentifiabilityError::UnknownReference {
                                field: "experiment observation instrument",
                                id: observation.instrument.as_str().to_string(),
                            })?;
                        if document.sources[&observation.sensor].expected_hash
                            != instrument.certificate_hash()
                        {
                            return Err(IdentifiabilityError::SourceMismatch {
                                field: "observation sensor/instrument calibration",
                            });
                        }
                        if !experiment_contains_clock(case_sources.experiment, &observation.clock) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "experiment observation clock",
                                id: observation.clock.as_str().to_string(),
                            });
                        }
                    }
                    let allowed_rows: BTreeSet<ObservationId> = match &case.purpose {
                        CasePurpose::Calibration
                        | CasePurpose::SymmetryBreaking
                        | CasePurpose::Complementary { .. } => lineage.calibration_ids.clone(),
                        CasePurpose::ValidationOnly => lineage.validation_ids.clone(),
                        CasePurpose::BlindFalsification => {
                            lineage.blind_sources.keys().cloned().collect()
                        }
                        CasePurpose::ProspectiveDesign => BTreeSet::new(),
                    };
                    let declared_rows = case
                        .observations
                        .values()
                        .filter_map(|observation| match &observation.rows {
                            ObservationRows::Retrospective(rows) => Some(rows.iter().cloned()),
                            ObservationRows::Prospective => None,
                        })
                        .flatten()
                        .collect::<BTreeSet<_>>();
                    let mut seen_rows = BTreeSet::new();
                    let mut reused_row = false;
                    for observation in case.observations.values() {
                        let ObservationRows::Retrospective(rows) = &observation.rows else {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "retrospective observation rows",
                                detail: format!(
                                    "case {case_id} contains a prospective observation after structural admission"
                                ),
                            });
                        };
                        if !rows.is_subset(&lineage.observation_ids) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "observation raw row",
                                id: observation.id.to_string(),
                            });
                        }
                        if !rows.is_subset(&allowed_rows) {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "case-purpose data partition",
                                detail: format!(
                                    "observation {} consumes rows outside the partition authorized by case {case_id} purpose",
                                    observation.id
                                ),
                            });
                        }
                        for row in rows {
                            if !seen_rows.insert(row.clone()) {
                                reused_row = true;
                            }
                        }
                    }
                    if matches!(&case.purpose, CasePurpose::BlindFalsification) {
                        let release = blind_releases
                            .get(split)
                            .expect("blind release pre-scan established exact presence");
                        let split_reference = ArtifactRef::new(
                            ArtifactKind::CalibrationSplit,
                            case_sources.split.id().clone(),
                            split_hash,
                        );
                        case_sources
                            .split
                            .blind_selection(
                                split_reference,
                                declared_rows.iter().cloned().collect(),
                                (**release).clone(),
                            )
                            .map_err(|error| IdentifiabilityError::Vv {
                                detail: error.to_string(),
                            })?;
                    }
                    if reused_row && matches!(&document.joint_noise, JointNoiseModel::Independent) {
                        return Err(IdentifiabilityError::Covariance {
                            detail: format!(
                                "case {case_id} reuses a raw row across channels under Independent noise"
                            ),
                        });
                    }
                    data.insert(case_id.clone(), lineage);
                }
            }
        }
        if bundle.cases.len() != data.len() {
            return Err(IdentifiabilityError::Cardinality {
                field: "case source bundles",
                detail: "source bundle contains an unknown or prospective case".to_string(),
            });
        }

        let data_entries = data.iter().collect::<Vec<_>>();
        let mut sharing_participation = BTreeSet::<CaseId>::new();
        for left_index in 0..data_entries.len() {
            for right_index in left_index + 1..data_entries.len() {
                let (left_id, left) = data_entries[left_index];
                let (right_id, right) = data_entries[right_index];
                let overlaps = lineages_share_raw_data(left, right);
                let left_group = sharing_group_membership(&document.data_reuse, left_id);
                let right_group = sharing_group_membership(&document.data_reuse, right_id);
                let explicitly_shared = left_group.is_some() && left_group == right_group;
                if overlaps && !explicitly_shared {
                    return Err(IdentifiabilityError::InvalidText {
                        field: "data reuse policy",
                        detail: format!(
                            "cases {left_id} and {right_id} share raw bytes, a manifest, or immutable row sources without one joint sharing group"
                        ),
                    });
                }
                if overlaps {
                    sharing_participation.insert(left_id.clone());
                    sharing_participation.insert(right_id.clone());
                }
            }
        }
        if let DataReusePolicy::Shared { groups } = &document.data_reuse {
            for group in groups {
                for case_id in &group.cases {
                    if !sharing_participation.contains(case_id) {
                        return Err(IdentifiabilityError::InvalidText {
                            field: "data sharing group",
                            detail: format!(
                                "case {case_id} declares raw-data sharing but overlaps no peer by admitted bytes, manifest, or row source"
                            ),
                        });
                    }
                }
            }
        }

        for (key, reference) in &document.sources {
            if concrete_keys.contains(key) {
                if bundle.opaque.entries.contains_key(key) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "concrete/opaque source resolution",
                        id: key.to_string(),
                    });
                }
                continue;
            }
            let resolution = bundle.opaque.entries.get(key).ok_or_else(|| {
                IdentifiabilityError::UnknownReference {
                    field: "opaque source resolution",
                    id: key.to_string(),
                }
            })?;
            admit_opaque_resolution(reference, resolution)?;
            resolutions.insert(key.clone(), resolution.clone());
        }
        if bundle.opaque.entries.len() != document.sources.len() - concrete_keys.len()
            || resolutions.len() != document.sources.len()
        {
            return Err(IdentifiabilityError::Cardinality {
                field: "source resolution closure",
                detail: "resolution set has missing or extra source keys".to_string(),
            });
        }
        if let Some(key) = bundle
            .concrete_authority
            .keys()
            .find(|key| !concrete_keys.contains(*key))
        {
            return Err(IdentifiabilityError::UnknownReference {
                field: "concrete source authority",
                id: key.to_string(),
            });
        }

        let problem_id = problem_identity_hash(&document)?;
        let source_admission = SourceAdmissionRecord {
            schema_version: IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_VERSION,
            problem_id,
            resolutions,
        };
        Ok(Self {
            problem_id,
            source_admission_id: source_admission_identity_hash(&source_admission)?,
            document,
            context,
            model,
            data,
            source_admission,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ProblemId {
        self.problem_id
    }

    #[must_use]
    pub const fn source_admission_id(&self) -> SourceAdmissionId {
        self.source_admission_id
    }

    #[must_use]
    pub const fn document(&self) -> &IdentifiabilityProblemDocument {
        &self.document
    }

    #[must_use]
    pub const fn data(&self) -> &BTreeMap<CaseId, DataLineage> {
        &self.data
    }

    #[must_use]
    pub const fn context(&self) -> &ContextBinding {
        &self.context
    }

    #[must_use]
    pub const fn model(&self) -> &MaterialModelBinding {
        &self.model
    }

    #[must_use]
    pub const fn source_resolutions(&self) -> &BTreeMap<SourceKey, SourceResolution> {
        &self.source_admission.resolutions
    }

    /// Exact source-admission identity preimage retained for ledger audit.
    pub fn source_admission_canonical_bytes(&self) -> Result<Vec<u8>, IdentifiabilityError> {
        encode_source_admission(&self.source_admission)
    }
}

/// One explicit action for every physical parameter.  Conditioned and derived
/// parameters remain explicit so a plan cannot silently omit them.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterExecutionAction {
    Optimize {
        coordinate: ParameterCoordinate,
    },
    Profile {
        coordinate: ParameterCoordinate,
    },
    Marginalize {
        coordinate: ParameterCoordinate,
        integrator: SourceRef,
    },
    Conditioned,
    Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestedClaimAxis {
    Structural,
    Local,
    Generic,
    Global,
    Practical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticPolicy {
    ExactSymbolic,
    CertifiedInterval,
    DeterministicFloatingPoint,
    FastFloatingPoint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdentifiabilityNumericalPolicy {
    rank_tolerance: f64,
    singular_value_floor: f64,
    maximum_condition_number: f64,
    arithmetic: ArithmeticPolicy,
}

impl IdentifiabilityNumericalPolicy {
    pub fn try_new(
        rank_tolerance: f64,
        singular_value_floor: f64,
        maximum_condition_number: f64,
        arithmetic: ArithmeticPolicy,
    ) -> Result<Self, IdentifiabilityError> {
        if !rank_tolerance.is_finite()
            || rank_tolerance <= 0.0
            || !singular_value_floor.is_finite()
            || singular_value_floor < 0.0
            || !maximum_condition_number.is_finite()
            || maximum_condition_number < 1.0
        {
            return Err(IdentifiabilityError::InvalidNumeric {
                field: "identifiability numerical policy",
                detail: "tolerances must be finite and physically ordered".to_string(),
            });
        }
        Ok(Self {
            rank_tolerance,
            singular_value_floor,
            maximum_condition_number,
            arithmetic,
        })
    }

    #[must_use]
    pub const fn rank_tolerance(&self) -> f64 {
        self.rank_tolerance
    }

    #[must_use]
    pub const fn singular_value_floor(&self) -> f64 {
        self.singular_value_floor
    }

    #[must_use]
    pub const fn maximum_condition_number(&self) -> f64 {
        self.maximum_condition_number
    }

    #[must_use]
    pub const fn arithmetic(&self) -> ArithmeticPolicy {
        self.arithmetic
    }
}

/// Numerical configuration whose identity is deliberately separate from the
/// physical problem.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifiabilityExecutionPlan {
    schema_version: u32,
    header: ArtifactHeader,
    problem_id: ProblemId,
    source_admission_id: SourceAdmissionId,
    analyzer: SourceRef,
    build: SourceRef,
    derivative_provider: Option<SourceRef>,
    requested_axes: BTreeSet<RequestedClaimAxis>,
    actions: BTreeMap<ParameterRoleId, ParameterExecutionAction>,
    numerical: IdentifiabilityNumericalPolicy,
    initialization: SourceRef,
    stopping: SourceRef,
    determinism_contract: SourceRef,
    source_authority: SourceResolutionSet,
}

fn validate_coordinate_for_parameter(
    parameter: &StudyParameter,
    coordinate: &ParameterCoordinate,
) -> Result<(), IdentifiabilityError> {
    match coordinate.transform {
        CoordinateTransform::Identity => {
            if coordinate.quantity != parameter.quantity {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "execution coordinate quantity",
                    detail: format!(
                        "identity coordinate for {} must preserve exact QuantitySpec",
                        parameter.role
                    ),
                });
            }
        }
        CoordinateTransform::Affine { scale_quantity, .. } => {
            let mapped = checked_add_dims(coordinate.quantity.dims(), scale_quantity.dims())
                .ok_or_else(|| IdentifiabilityError::InvalidNumeric {
                    field: "execution affine coordinate",
                    detail: "dimension exponent overflow".to_string(),
                })?;
            if mapped != parameter.quantity.dims() {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "execution affine coordinate",
                    detail: format!("coordinate for {} has wrong dimensions", parameter.role),
                });
            }
        }
        CoordinateTransform::LogPositive { .. } => {
            if coordinate.quantity.dims() != Dims([0; 6]) || parameter.domain.lo <= 0.0 {
                return Err(IdentifiabilityError::InvalidNumeric {
                    field: "execution log coordinate",
                    detail: format!(
                        "parameter {} is not positive/dimensionless-charted",
                        parameter.role
                    ),
                });
            }
        }
    }
    let mapped = coordinate.transform.mapped_domain(coordinate.domain)?;
    if !same_f64(mapped.lo, parameter.domain.lo) || !same_f64(mapped.hi, parameter.domain.hi) {
        return Err(IdentifiabilityError::InvalidNumeric {
            field: "execution coordinate domain",
            detail: format!(
                "coordinate does not bijectively cover parameter {}",
                parameter.role
            ),
        });
    }
    Ok(())
}

impl IdentifiabilityExecutionPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        header: ArtifactHeader,
        problem: &AdmittedIdentifiabilityProblem,
        analyzer: SourceRef,
        build: SourceRef,
        derivative_provider: Option<SourceRef>,
        requested_axes: BTreeSet<RequestedClaimAxis>,
        actions: Vec<(ParameterRoleId, ParameterExecutionAction)>,
        numerical: IdentifiabilityNumericalPolicy,
        initialization: SourceRef,
        stopping: SourceRef,
        determinism_contract: SourceRef,
        source_authority: SourceResolutionSet,
    ) -> Result<Self, IdentifiabilityError> {
        validate_header_profile(&header)?;
        if !header.capabilities().contains("identifiability.execute") {
            return Err(IdentifiabilityError::InvalidText {
                field: "execution capability",
                detail: "missing identifiability.execute capability".to_string(),
            });
        }
        if requested_axes.is_empty() {
            return Err(IdentifiabilityError::Cardinality {
                field: "requested claim axes",
                detail: "execution must request at least one claim axis".to_string(),
            });
        }
        let mut execution_sources = BTreeMap::new();
        for (source, kind, field) in [
            (&analyzer, SourceKind::Analyzer, "analyzer"),
            (&build, SourceKind::Build, "build"),
            (&initialization, SourceKind::Assumption, "initialization"),
            (&stopping, SourceKind::Assumption, "stopping policy"),
            (
                &determinism_contract,
                SourceKind::Assumption,
                "determinism contract",
            ),
        ] {
            if source.kind != kind {
                return Err(IdentifiabilityError::InvalidText {
                    field,
                    detail: format!("source {} has wrong kind", source.key),
                });
            }
            bind_source_reference(&mut execution_sources, source, "execution source alias")?;
        }
        if let Some(provider) = &derivative_provider {
            if provider.kind != SourceKind::DerivativeProvider {
                return Err(IdentifiabilityError::InvalidText {
                    field: "derivative provider",
                    detail: "source has wrong kind".to_string(),
                });
            }
            bind_source_reference(&mut execution_sources, provider, "execution source alias")?;
        }
        let mut action_map = BTreeMap::new();
        for (role, action) in actions {
            if action_map.insert(role.clone(), action).is_some() {
                return Err(IdentifiabilityError::Duplicate {
                    field: "execution parameter action",
                    id: role.to_string(),
                });
            }
        }
        if action_map.len() != problem.document.parameters.len() {
            return Err(IdentifiabilityError::Cardinality {
                field: "execution parameter actions",
                detail: "every physical parameter needs exactly one explicit action".to_string(),
            });
        }
        let mut coordinate_ids = BTreeSet::new();
        for (role, parameter) in &problem.document.parameters {
            let action =
                action_map
                    .get(role)
                    .ok_or_else(|| IdentifiabilityError::UnknownReference {
                        field: "execution parameter action",
                        id: role.to_string(),
                    })?;
            match (&parameter.treatment, action) {
                (
                    ParameterTreatment::Estimated,
                    ParameterExecutionAction::Optimize { coordinate },
                )
                | (
                    ParameterTreatment::Profiled,
                    ParameterExecutionAction::Profile { coordinate },
                ) => {
                    validate_coordinate_for_parameter(parameter, coordinate)?;
                }
                (
                    ParameterTreatment::Marginalized,
                    ParameterExecutionAction::Marginalize {
                        coordinate,
                        integrator,
                    },
                ) => {
                    validate_coordinate_for_parameter(parameter, coordinate)?;
                    if integrator.kind != SourceKind::Analyzer {
                        return Err(IdentifiabilityError::InvalidText {
                            field: "marginalization integrator",
                            detail: "integrator source must have Analyzer kind".to_string(),
                        });
                    }
                    bind_source_reference(
                        &mut execution_sources,
                        integrator,
                        "execution source alias",
                    )?;
                }
                (ParameterTreatment::Conditioned(_), ParameterExecutionAction::Conditioned)
                | (ParameterTreatment::Derived { .. }, ParameterExecutionAction::Derived) => {}
                _ => {
                    return Err(IdentifiabilityError::InvalidText {
                        field: "execution parameter treatment",
                        detail: format!("action for {role} contradicts physical treatment"),
                    });
                }
            }
            let coordinate = match action {
                ParameterExecutionAction::Optimize { coordinate }
                | ParameterExecutionAction::Profile { coordinate }
                | ParameterExecutionAction::Marginalize { coordinate, .. } => Some(coordinate),
                ParameterExecutionAction::Conditioned | ParameterExecutionAction::Derived => None,
            };
            if let Some(coordinate) = coordinate
                && !coordinate_ids.insert(coordinate.id().clone())
            {
                return Err(IdentifiabilityError::Duplicate {
                    field: "execution scalar coordinate",
                    id: coordinate.id().to_string(),
                });
            }
        }
        validate_source_authority_closure(
            &execution_sources,
            &source_authority,
            "execution source authority",
        )?;
        for (key, resolution) in &source_authority.entries {
            if let Some(problem_resolution) = problem.source_admission.resolutions.get(key)
                && problem_resolution != resolution
            {
                return Err(IdentifiabilityError::SourceMismatch {
                    field: "execution/problem source authority",
                });
            }
        }
        Ok(Self {
            schema_version: IDENTIFIABILITY_EXECUTION_IDENTITY_VERSION,
            header,
            problem_id: problem.problem_id,
            source_admission_id: problem.source_admission_id,
            analyzer,
            build,
            derivative_provider,
            requested_axes,
            actions: action_map,
            numerical,
            initialization,
            stopping,
            determinism_contract,
            source_authority,
        })
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, IdentifiabilityError> {
        encode_execution(self)
    }

    pub fn id(&self) -> Result<ExecutionId, IdentifiabilityError> {
        execution_identity_hash(self)
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn header(&self) -> &ArtifactHeader {
        &self.header
    }

    #[must_use]
    pub const fn problem_id(&self) -> ProblemId {
        self.problem_id
    }

    #[must_use]
    pub const fn source_admission_id(&self) -> SourceAdmissionId {
        self.source_admission_id
    }

    /// Analyzer identity to which assessment methods must be exactly bound.
    #[must_use]
    pub const fn analyzer(&self) -> &SourceRef {
        &self.analyzer
    }

    #[must_use]
    pub const fn build(&self) -> &SourceRef {
        &self.build
    }

    #[must_use]
    pub const fn derivative_provider(&self) -> Option<&SourceRef> {
        self.derivative_provider.as_ref()
    }

    #[must_use]
    pub const fn requested_axes(&self) -> &BTreeSet<RequestedClaimAxis> {
        &self.requested_axes
    }

    #[must_use]
    pub const fn actions(&self) -> &BTreeMap<ParameterRoleId, ParameterExecutionAction> {
        &self.actions
    }

    /// Numerical policy that bounds every downstream claimed tolerance.
    #[must_use]
    pub const fn numerical_policy(&self) -> &IdentifiabilityNumericalPolicy {
        &self.numerical
    }

    /// Locally verified authority for every analyzer, build, derivative,
    /// integrator, initialization, stopping, and determinism source.
    #[must_use]
    pub const fn source_authority(&self) -> &SourceResolutionSet {
        &self.source_authority
    }

    #[must_use]
    pub const fn initialization(&self) -> &SourceRef {
        &self.initialization
    }

    #[must_use]
    pub const fn stopping(&self) -> &SourceRef {
        &self.stopping
    }

    #[must_use]
    pub const fn determinism_contract(&self) -> &SourceRef {
        &self.determinism_contract
    }

    pub fn from_canonical_bytes(
        bytes: &[u8],
        problem: &AdmittedIdentifiabilityProblem,
        verified_sources: &SourceResolutionSet,
    ) -> Result<Self, IdentifiabilityError> {
        decode_execution(bytes, problem, verified_sources)
    }
}

/// Information assumed by an identifiability claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InformationRegime {
    StructuralExactModel,
    ExactInputOutputMap,
    NoisyFiniteData,
    PosteriorUnderDeclaredPrior,
}

/// Extent is independent of information regime and quantifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifiabilityExtent {
    Local,
    Global,
    SetValued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarDomain {
    Real,
    Complex,
    MixedDiscreteContinuous,
}

/// Mathematical quantifier and its exact domain/measure source.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimQuantifier {
    AtRealization {
        realization: SourceRef,
    },
    AlmostEverywhere {
        measure: SourceRef,
    },
    ForAll {
        domain: SourceRef,
    },
    ProbabilityAtLeast {
        probability: f64,
        measure: SourceRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimSubject {
    Parameter(ParameterRoleId),
    ParameterTuple(BTreeSet<ParameterRoleId>),
    Influence(InfluenceId),
    GaugeClass(GaugeClassId),
    WholeProblem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimScope {
    WholeCampaign,
    Cases(BTreeSet<CaseId>),
    Stratum { definition: SourceKey },
}

/// Coordinate-free proposition.  Its truth status and receipts live in the
/// paired [`ClaimAssessment`], preserving a product type instead of collapsing
/// “structural/local/generic/global/practical” into one ordinal label.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedIdentifiabilityClaim {
    id: ClaimId,
    information: InformationRegime,
    extent: IdentifiabilityExtent,
    quantifier: ClaimQuantifier,
    scalar_domain: ScalarDomain,
    subject: ClaimSubject,
    scope: ClaimScope,
}

impl TypedIdentifiabilityClaim {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        id: ClaimId,
        information: InformationRegime,
        extent: IdentifiabilityExtent,
        quantifier: ClaimQuantifier,
        scalar_domain: ScalarDomain,
        subject: ClaimSubject,
        scope: ClaimScope,
    ) -> Self {
        Self {
            id,
            information,
            extent,
            quantifier,
            scalar_domain,
            subject,
            scope,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &ClaimId {
        &self.id
    }

    #[must_use]
    pub const fn information(&self) -> InformationRegime {
        self.information
    }

    #[must_use]
    pub const fn extent(&self) -> IdentifiabilityExtent {
        self.extent
    }

    #[must_use]
    pub const fn quantifier(&self) -> &ClaimQuantifier {
        &self.quantifier
    }

    #[must_use]
    pub const fn scalar_domain(&self) -> ScalarDomain {
        self.scalar_domain
    }

    #[must_use]
    pub const fn subject(&self) -> &ClaimSubject {
        &self.subject
    }

    #[must_use]
    pub const fn scope(&self) -> &ClaimScope {
        &self.scope
    }
}

/// Evidence-bound *claim* about a proposition. Positive/refuting variants are
/// deliberately prefixed `Claimed`: content verification and even an external
/// trust receipt do not, by themselves, prove the mathematical proposition.
/// A future method-specific verifier may promote a subject-bound receipt to a
/// sealed theorem token without changing this honest transport layer.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimAssessment {
    ClaimedEstablished {
        method: SourceRef,
        receipt: SourceRef,
        tolerance: f64,
    },
    ClaimedRefuted {
        method: SourceRef,
        receipt: SourceRef,
        tolerance: f64,
    },
    ClaimedInconclusive {
        method: Option<SourceRef>,
        receipt: Option<SourceRef>,
        reason: String,
    },
    NotAssessed {
        reason: String,
    },
}

/// Typed conclusions for one exact execution.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifiabilityAssessment {
    schema_version: u32,
    header: ArtifactHeader,
    problem_id: ProblemId,
    execution_id: ExecutionId,
    claims: BTreeMap<ClaimId, TypedIdentifiabilityClaim>,
    evidence: BTreeMap<ClaimId, ClaimAssessment>,
    source_authority: SourceResolutionSet,
}

#[allow(dead_code)]
fn classify_identifiability_problem_identity_fields(document: &IdentifiabilityProblemDocument) {
    let IdentifiabilityProblemDocument {
        schema_version,
        context_source,
        material_source,
        model_source,
        graph_source,
        sources,
        parameters,
        constraints,
        cases,
        influences,
        gauges,
        joint_noise,
        data_reuse,
    } = document;
    let _ = (
        schema_version,
        context_source,
        material_source,
        model_source,
        graph_source,
        sources,
        parameters,
        constraints,
        cases,
        influences,
        gauges,
        joint_noise,
        data_reuse,
    );
}

#[allow(dead_code)]
fn classify_identifiability_source_admission_identity_fields(admission: &SourceAdmissionRecord) {
    let SourceAdmissionRecord {
        schema_version,
        problem_id,
        resolutions,
    } = admission;
    let _ = (schema_version, problem_id, resolutions);
}

#[allow(dead_code)]
fn classify_identifiability_execution_identity_fields(plan: &IdentifiabilityExecutionPlan) {
    let IdentifiabilityExecutionPlan {
        schema_version,
        header,
        problem_id,
        source_admission_id,
        analyzer,
        build,
        derivative_provider,
        requested_axes,
        actions,
        numerical,
        initialization,
        stopping,
        determinism_contract,
        source_authority,
    } = plan;
    let _ = (
        schema_version,
        header,
        problem_id,
        source_admission_id,
        analyzer,
        build,
        derivative_provider,
        requested_axes,
        actions,
        numerical,
        initialization,
        stopping,
        determinism_contract,
        source_authority,
    );
}

#[allow(dead_code)]
fn classify_identifiability_assessment_identity_fields(assessment: &IdentifiabilityAssessment) {
    let IdentifiabilityAssessment {
        schema_version,
        header,
        problem_id,
        execution_id,
        claims,
        evidence,
        source_authority,
    } = assessment;
    let _ = (
        schema_version,
        header,
        problem_id,
        execution_id,
        claims,
        evidence,
        source_authority,
    );
}

/// Owner-local declaration for the unresolved physical-question identity.
///
/// The declaration is intentionally literal rather than macro-generated:
/// `xtask check-identities` fingerprints this exact source surface and requires
/// every owner field to remain deliberately classified.
#[allow(dead_code)]
pub const IDENTIFIABILITY_PROBLEM_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
    "frankensim-identity-schema-v1",
    "id=fs-material:identifiability-problem",
    "version_const=IDENTIFIABILITY_PROBLEM_IDENTITY_VERSION",
    "version=1",
    "domain=org.frankensim.fs-material.identifiability-problem.v1",
    "domain_const=IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN",
    "encoder=problem_identity_hash",
    "encoder_helpers=IdentifiabilityProblemDocument::canonical_bytes,encode_problem",
    "schema_constants=IDENTIFIABILITY_PROBLEM_IDENTITY_VERSION,IDENTIFIABILITY_PROBLEM_IDENTITY_DOMAIN,PROBLEM_MAGIC,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ID_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_TEXT_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ITEMS,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_CANONICAL_BYTES",
    "schema_functions=IdentifiabilityProblemDocument::try_new,decode_problem,check_problem_identity_version,encode_source_key,decode_source_key,encode_case_id,decode_case_id,encode_role,decode_role,encode_channel,decode_channel,encode_source_kind,decode_source_kind,encode_source_ref,decode_source_ref,encode_observation_key,decode_observation_key,encode_parameter_treatment,decode_parameter_treatment,encode_prior_policy,decode_prior_policy,encode_owner,decode_owner,encode_scope,decode_scope,encode_study_parameter,decode_study_parameter,encode_constraint,decode_constraint,encode_marginal_noise,decode_marginal_noise,encode_missingness,decode_missingness,encode_study_observation,decode_study_observation,encode_discrepancy,decode_discrepancy,encode_case,decode_case,encode_functional,decode_functional,encode_influence,decode_influence,encode_gauge,decode_gauge,encode_joint_noise,decode_joint_noise,encode_data_reuse,decode_data_reuse,crates/fs-material/src/identifiability.rs#canonical_f64,crates/fs-material/src/identifiability.rs#validate_token,crates/fs-material/src/identifiability.rs#validate_reason,crates/fs-material/src/identifiability.rs#CanonicalWriter::new,crates/fs-material/src/identifiability.rs#CanonicalWriter::byte,crates/fs-material/src/identifiability.rs#CanonicalWriter::u32,crates/fs-material/src/identifiability.rs#CanonicalWriter::u64,crates/fs-material/src/identifiability.rs#CanonicalWriter::f64,crates/fs-material/src/identifiability.rs#CanonicalWriter::count,crates/fs-material/src/identifiability.rs#CanonicalWriter::text,crates/fs-material/src/identifiability.rs#CanonicalWriter::hash,crates/fs-material/src/identifiability.rs#CanonicalWriter::quantity,crates/fs-material/src/identifiability.rs#CanonicalWriter::finish,crates/fs-material/src/identifiability.rs#CanonicalReader::new,crates/fs-material/src/identifiability.rs#CanonicalReader::take,crates/fs-material/src/identifiability.rs#CanonicalReader::byte,crates/fs-material/src/identifiability.rs#CanonicalReader::u32,crates/fs-material/src/identifiability.rs#CanonicalReader::u64,crates/fs-material/src/identifiability.rs#CanonicalReader::f64,crates/fs-material/src/identifiability.rs#CanonicalReader::length,crates/fs-material/src/identifiability.rs#CanonicalReader::count,crates/fs-material/src/identifiability.rs#CanonicalReader::text,crates/fs-material/src/identifiability.rs#CanonicalReader::token,crates/fs-material/src/identifiability.rs#CanonicalReader::reason,crates/fs-material/src/identifiability.rs#CanonicalReader::hash,crates/fs-material/src/identifiability.rs#CanonicalReader::quantity,crates/fs-material/src/identifiability.rs#CanonicalReader::expect_byte,crates/fs-material/src/identifiability.rs#CanonicalReader::finish,crates/fs-blake3/src/lib.rs#hash_domain,crates/fs-qty/src/semantic.rs#QuantitySpec::canonical_bytes,crates/fs-qty/src/semantic.rs#QuantitySpec::from_canonical_bytes",
    "schema_dependencies=none",
    "digest=blake3-256-domain-separated",
    "encoding=canonical-transport-exact-bits",
    "sources=IdentifiabilityProblemDocument",
    "source_fields=IdentifiabilityProblemDocument.schema_version:semantic,IdentifiabilityProblemDocument.context_source:semantic,IdentifiabilityProblemDocument.material_source:semantic,IdentifiabilityProblemDocument.model_source:semantic,IdentifiabilityProblemDocument.graph_source:semantic,IdentifiabilityProblemDocument.sources:semantic,IdentifiabilityProblemDocument.parameters:semantic,IdentifiabilityProblemDocument.constraints:semantic,IdentifiabilityProblemDocument.cases:semantic,IdentifiabilityProblemDocument.influences:semantic,IdentifiabilityProblemDocument.gauges:semantic,IdentifiabilityProblemDocument.joint_noise:semantic,IdentifiabilityProblemDocument.data_reuse:semantic",
    "source_bindings=IdentifiabilityProblemDocument.schema_version>wire-schema-version,IdentifiabilityProblemDocument.context_source>context-source-binding,IdentifiabilityProblemDocument.material_source>material-source-binding,IdentifiabilityProblemDocument.model_source>model-source-binding,IdentifiabilityProblemDocument.graph_source>graph-source-binding,IdentifiabilityProblemDocument.sources>source-registry,IdentifiabilityProblemDocument.parameters>parameter-registry,IdentifiabilityProblemDocument.constraints>joint-constraint-registry,IdentifiabilityProblemDocument.cases>study-case-registry,IdentifiabilityProblemDocument.influences>influence-registry,IdentifiabilityProblemDocument.gauges>gauge-registry,IdentifiabilityProblemDocument.joint_noise>joint-noise-model,IdentifiabilityProblemDocument.data_reuse>data-reuse-policy",
    "external_semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,fixed-numeric-little-endian",
    "semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,fixed-numeric-little-endian,wire-schema-version,context-source-binding,material-source-binding,model-source-binding,graph-source-binding,source-registry,parameter-registry,joint-constraint-registry,study-case-registry,influence-registry,gauge-registry,joint-noise-model,data-reuse-policy",
    "excluded_fields=source-authority-envelope:admission-authority-not-physical-question,execution-configuration:belongs-to-execution-identity,assessment-claims-and-evidence:belongs-to-assessment-identity,caller-container-order:canonicalized-before-identity",
    "consumers=AdmittedIdentifiabilityProblem::resolve_and_admit,AdmittedIdentifiabilityProblem::id,IdentifiabilityExecutionPlan::try_new",
    "mutations=identity-domain:crates/fs-material/tests/identifiability_authority.rs#identity_domains_and_wire_magics_are_stage_separated,identity-version:crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact,wire-magic:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_versions_and_transports_fail_closed,canonical-field-order:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,collection-count-u32-le:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,fixed-numeric-little-endian:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,wire-schema-version:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_versions_and_transports_fail_closed,context-source-binding:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,material-source-binding:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,model-source-binding:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,graph-source-binding:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,source-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,parameter-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,joint-constraint-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,study-case-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,influence-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,gauge-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,joint-noise-model:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence,data-reuse-policy:crates/fs-material/tests/identifiability_authority.rs#identifiability_problem_identity_bindings_have_exact_mutation_evidence",
    "nonsemantic_mutations=source-authority-envelope:crates/fs-material/tests/identifiability_authority.rs#problem_and_source_admission_identities_separate_question_from_trust_envelope,execution-configuration:crates/fs-material/tests/identifiability_authority.rs#coordinates_do_not_move_problem_identity,assessment-claims-and-evidence:crates/fs-material/tests/identifiability_authority.rs#evidence_changes_assessment_not_problem_or_execution,caller-container-order:crates/fs-material/tests/identifiability_authority.rs#case_and_registry_input_order_are_nonsemantic",
    "field_guard=classify_identifiability_problem_identity_fields",
    "transport_guard=IdentifiabilityProblemDocument::from_canonical_bytes",
    "version_guard=crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact",
    "coupling_surface=fs-material:identifiability-problem",
];

/// Owner-local declaration for the exact source-resolution authority envelope.
#[allow(dead_code)]
pub const IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
    "frankensim-identity-schema-v1",
    "id=fs-material:identifiability-source-admission",
    "version_const=IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_VERSION",
    "version=1",
    "domain=org.frankensim.fs-material.identifiability-source-admission.v1",
    "domain_const=IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_DOMAIN",
    "encoder=source_admission_identity_hash",
    "encoder_helpers=AdmittedIdentifiabilityProblem::source_admission_canonical_bytes,encode_source_admission,encode_resolution_entry",
    "schema_constants=IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_VERSION,IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_DOMAIN,SOURCE_ADMISSION_MAGIC,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ID_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_TEXT_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ITEMS,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_CANONICAL_BYTES",
    "schema_functions=check_source_admission_identity_version,encode_source_key,encode_source_kind,encode_resolution_verification,crates/fs-material/src/identifiability.rs#CanonicalWriter::new,crates/fs-material/src/identifiability.rs#CanonicalWriter::byte,crates/fs-material/src/identifiability.rs#CanonicalWriter::u32,crates/fs-material/src/identifiability.rs#CanonicalWriter::u64,crates/fs-material/src/identifiability.rs#CanonicalWriter::count,crates/fs-material/src/identifiability.rs#CanonicalWriter::text,crates/fs-material/src/identifiability.rs#CanonicalWriter::hash,crates/fs-material/src/identifiability.rs#CanonicalWriter::finish,crates/fs-blake3/src/lib.rs#hash_domain",
    "schema_dependencies=fs-material:identifiability-problem",
    "digest=blake3-256-domain-separated",
    "encoding=typed-binary",
    "sources=SourceAdmissionRecord",
    "source_fields=SourceAdmissionRecord.schema_version:semantic,SourceAdmissionRecord.problem_id:semantic,SourceAdmissionRecord.resolutions:semantic",
    "source_bindings=SourceAdmissionRecord.schema_version>wire-schema-version,SourceAdmissionRecord.problem_id>problem-id,SourceAdmissionRecord.resolutions>source-resolution-registry",
    "external_semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le",
    "semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,wire-schema-version,problem-id,source-resolution-registry",
    "excluded_fields=execution-configuration:belongs-to-execution-identity,assessment-claims-and-evidence:belongs-to-assessment-identity,caller-container-order:canonicalized-before-identity",
    "consumers=AdmittedIdentifiabilityProblem::resolve_and_admit,AdmittedIdentifiabilityProblem::source_admission_id,IdentifiabilityExecutionPlan::try_new",
    "mutations=identity-domain:crates/fs-material/tests/identifiability_authority.rs#identity_domains_and_wire_magics_are_stage_separated,identity-version:crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact,wire-magic:crates/fs-material/tests/identifiability_authority.rs#identity_domains_and_wire_magics_are_stage_separated,canonical-field-order:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,collection-count-u32-le:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,wire-schema-version:crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact,problem-id:crates/fs-material/tests/identifiability_authority.rs#identifiability_source_admission_identity_bindings_have_exact_mutation_evidence,source-resolution-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_source_admission_identity_bindings_have_exact_mutation_evidence",
    "nonsemantic_mutations=execution-configuration:crates/fs-material/tests/identifiability_authority.rs#source_admission_id_is_stable_across_execution_variants,assessment-claims-and-evidence:crates/fs-material/tests/identifiability_authority.rs#evidence_changes_assessment_not_problem_or_execution,caller-container-order:crates/fs-material/tests/identifiability_authority.rs#source_resolution_input_order_is_nonsemantic",
    "field_guard=classify_identifiability_source_admission_identity_fields",
    "transport_guard=AdmittedIdentifiabilityProblem::source_admission_canonical_bytes",
    "version_guard=crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact",
    "coupling_surface=fs-material:identifiability-source-admission",
];

/// Owner-local declaration for an exact, source-authorized execution identity.
#[allow(dead_code)]
pub const IDENTIFIABILITY_EXECUTION_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
    "frankensim-identity-schema-v1",
    "id=fs-material:identifiability-execution",
    "version_const=IDENTIFIABILITY_EXECUTION_IDENTITY_VERSION",
    "version=1",
    "domain=org.frankensim.fs-material.identifiability-execution.v1",
    "domain_const=IDENTIFIABILITY_EXECUTION_IDENTITY_DOMAIN",
    "encoder=execution_identity_hash",
    "encoder_helpers=encode_execution_identity,encode_execution_with_header_mode",
    "schema_constants=IDENTIFIABILITY_EXECUTION_IDENTITY_VERSION,IDENTIFIABILITY_EXECUTION_IDENTITY_DOMAIN,EXECUTION_MAGIC,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ID_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_TEXT_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ITEMS,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_CANONICAL_BYTES",
    "schema_functions=IdentifiabilityExecutionPlan::try_new,IdentifiabilityExecutionPlan::canonical_bytes,IdentifiabilityExecutionPlan::from_canonical_bytes,encode_execution,decode_execution,check_execution_identity_version,encode_execution_action,decode_execution_action,encode_source_ref,decode_source_ref,encode_source_kind,decode_source_kind,encode_source_key,decode_source_key,encode_role,decode_role,encode_resolution_entry,encode_resolution_set,decode_resolution_set,encode_resolution_verification,decode_resolution_verification,crates/fs-material/src/identifiability.rs#encode_header,crates/fs-material/src/identifiability.rs#decode_header,crates/fs-material/src/identifiability.rs#canonical_f64,crates/fs-material/src/identifiability.rs#validate_token,crates/fs-material/src/identifiability.rs#validate_reason,crates/fs-material/src/identifiability.rs#CanonicalWriter::new,crates/fs-material/src/identifiability.rs#CanonicalWriter::byte,crates/fs-material/src/identifiability.rs#CanonicalWriter::u32,crates/fs-material/src/identifiability.rs#CanonicalWriter::u64,crates/fs-material/src/identifiability.rs#CanonicalWriter::f64,crates/fs-material/src/identifiability.rs#CanonicalWriter::count,crates/fs-material/src/identifiability.rs#CanonicalWriter::text,crates/fs-material/src/identifiability.rs#CanonicalWriter::hash,crates/fs-material/src/identifiability.rs#CanonicalWriter::quantity,crates/fs-material/src/identifiability.rs#CanonicalWriter::finish,crates/fs-material/src/identifiability.rs#CanonicalReader::new,crates/fs-material/src/identifiability.rs#CanonicalReader::take,crates/fs-material/src/identifiability.rs#CanonicalReader::byte,crates/fs-material/src/identifiability.rs#CanonicalReader::u32,crates/fs-material/src/identifiability.rs#CanonicalReader::u64,crates/fs-material/src/identifiability.rs#CanonicalReader::f64,crates/fs-material/src/identifiability.rs#CanonicalReader::length,crates/fs-material/src/identifiability.rs#CanonicalReader::count,crates/fs-material/src/identifiability.rs#CanonicalReader::text,crates/fs-material/src/identifiability.rs#CanonicalReader::token,crates/fs-material/src/identifiability.rs#CanonicalReader::reason,crates/fs-material/src/identifiability.rs#CanonicalReader::hash,crates/fs-material/src/identifiability.rs#CanonicalReader::quantity,crates/fs-material/src/identifiability.rs#CanonicalReader::expect_byte,crates/fs-material/src/identifiability.rs#CanonicalReader::finish,crates/fs-blake3/src/lib.rs#hash_domain,crates/fs-qty/src/semantic.rs#QuantitySpec::canonical_bytes,crates/fs-qty/src/semantic.rs#QuantitySpec::from_canonical_bytes",
    "schema_dependencies=fs-material:identifiability-problem,fs-material:identifiability-source-admission",
    "digest=blake3-256-domain-separated",
    "encoding=typed-binary",
    "sources=IdentifiabilityExecutionPlan",
    "source_fields=IdentifiabilityExecutionPlan.schema_version:semantic,IdentifiabilityExecutionPlan.header:derived:identity-projection-excludes-artifact-id,IdentifiabilityExecutionPlan.problem_id:semantic,IdentifiabilityExecutionPlan.source_admission_id:semantic,IdentifiabilityExecutionPlan.analyzer:semantic,IdentifiabilityExecutionPlan.build:semantic,IdentifiabilityExecutionPlan.derivative_provider:semantic,IdentifiabilityExecutionPlan.requested_axes:semantic,IdentifiabilityExecutionPlan.actions:semantic,IdentifiabilityExecutionPlan.numerical:semantic,IdentifiabilityExecutionPlan.initialization:semantic,IdentifiabilityExecutionPlan.stopping:semantic,IdentifiabilityExecutionPlan.determinism_contract:semantic,IdentifiabilityExecutionPlan.source_authority:semantic",
    "source_bindings=IdentifiabilityExecutionPlan.schema_version>wire-schema-version,IdentifiabilityExecutionPlan.problem_id>problem-id,IdentifiabilityExecutionPlan.source_admission_id>source-admission-id,IdentifiabilityExecutionPlan.analyzer>analyzer-source,IdentifiabilityExecutionPlan.build>build-source,IdentifiabilityExecutionPlan.derivative_provider>derivative-provider-source,IdentifiabilityExecutionPlan.requested_axes>requested-claim-axes,IdentifiabilityExecutionPlan.actions>parameter-execution-actions,IdentifiabilityExecutionPlan.numerical>numerical-policy,IdentifiabilityExecutionPlan.initialization>initialization-source,IdentifiabilityExecutionPlan.stopping>stopping-source,IdentifiabilityExecutionPlan.determinism_contract>determinism-contract-source,IdentifiabilityExecutionPlan.source_authority>execution-source-authority",
    "external_semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,fixed-numeric-little-endian,identity-header-projection-marker,header-units,header-seed,header-accuracy,header-time-ms,header-memory-bytes,header-versions,header-capabilities",
    "semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,fixed-numeric-little-endian,identity-header-projection-marker,header-units,header-seed,header-accuracy,header-time-ms,header-memory-bytes,header-versions,header-capabilities,wire-schema-version,problem-id,source-admission-id,analyzer-source,build-source,derivative-provider-source,requested-claim-axes,parameter-execution-actions,numerical-policy,initialization-source,stopping-source,determinism-contract-source,execution-source-authority",
    "excluded_fields=ArtifactHeader.id:ledger-label-not-scientific-identity,assessment-claims-and-evidence:belongs-to-assessment-identity,caller-container-order:canonicalized-before-identity",
    "consumers=IdentifiabilityExecutionPlan::try_new,IdentifiabilityExecutionPlan::id,IdentifiabilityAssessment::try_new",
    "mutations=identity-domain:crates/fs-material/tests/identifiability_authority.rs#identity_domains_and_wire_magics_are_stage_separated,identity-version:crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact,wire-magic:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_versions_and_transports_fail_closed,canonical-field-order:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,collection-count-u32-le:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,fixed-numeric-little-endian:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,identity-header-projection-marker:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,header-units:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,header-seed:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,header-accuracy:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,header-time-ms:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,header-memory-bytes:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,header-versions:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,header-capabilities:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,wire-schema-version:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_versions_and_transports_fail_closed,problem-id:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,source-admission-id:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,analyzer-source:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,build-source:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,derivative-provider-source:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,requested-claim-axes:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,parameter-execution-actions:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,numerical-policy:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,initialization-source:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,stopping-source:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,determinism-contract-source:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence,execution-source-authority:crates/fs-material/tests/identifiability_authority.rs#identifiability_execution_identity_bindings_have_exact_mutation_evidence",
    "nonsemantic_mutations=ArtifactHeader.id:crates/fs-material/tests/identifiability_authority.rs#artifact_labels_do_not_move_execution_or_assessment_identity,assessment-claims-and-evidence:crates/fs-material/tests/identifiability_authority.rs#evidence_changes_assessment_not_problem_or_execution,caller-container-order:crates/fs-material/tests/identifiability_authority.rs#execution_action_input_order_is_nonsemantic",
    "field_guard=classify_identifiability_execution_identity_fields",
    "transport_guard=IdentifiabilityExecutionPlan::from_canonical_bytes",
    "version_guard=crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact",
    "coupling_surface=fs-material:identifiability-execution",
];

/// Owner-local declaration for exact claim/evidence assessment identity.
#[allow(dead_code)]
pub const IDENTIFIABILITY_ASSESSMENT_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
    "frankensim-identity-schema-v1",
    "id=fs-material:identifiability-assessment",
    "version_const=IDENTIFIABILITY_ASSESSMENT_IDENTITY_VERSION",
    "version=1",
    "domain=org.frankensim.fs-material.identifiability-assessment.v1",
    "domain_const=IDENTIFIABILITY_ASSESSMENT_IDENTITY_DOMAIN",
    "encoder=assessment_identity_hash",
    "encoder_helpers=encode_assessment_identity,encode_assessment_with_header_mode",
    "schema_constants=IDENTIFIABILITY_ASSESSMENT_IDENTITY_VERSION,IDENTIFIABILITY_ASSESSMENT_IDENTITY_DOMAIN,ASSESSMENT_MAGIC,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ID_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_TEXT_BYTES,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_ITEMS,crates/fs-material/src/identifiability.rs#MAX_IDENTIFIABILITY_CANONICAL_BYTES",
    "schema_functions=IdentifiabilityAssessment::try_new,IdentifiabilityAssessment::canonical_bytes,IdentifiabilityAssessment::from_canonical_bytes,encode_assessment,decode_assessment,check_assessment_identity_version,encode_claim,decode_claim,encode_claim_assessment,decode_claim_assessment,decode_optional_source_ref,encode_source_ref,decode_source_ref,encode_source_kind,decode_source_kind,encode_source_key,decode_source_key,encode_role,decode_role,encode_case_id,decode_case_id,encode_channel,decode_channel,encode_observation_key,decode_observation_key,encode_resolution_entry,encode_resolution_set,decode_resolution_set,encode_resolution_verification,decode_resolution_verification,crates/fs-material/src/identifiability.rs#encode_header,crates/fs-material/src/identifiability.rs#decode_header,crates/fs-material/src/identifiability.rs#canonical_f64,crates/fs-material/src/identifiability.rs#validate_token,crates/fs-material/src/identifiability.rs#validate_reason,crates/fs-material/src/identifiability.rs#CanonicalWriter::new,crates/fs-material/src/identifiability.rs#CanonicalWriter::byte,crates/fs-material/src/identifiability.rs#CanonicalWriter::u32,crates/fs-material/src/identifiability.rs#CanonicalWriter::u64,crates/fs-material/src/identifiability.rs#CanonicalWriter::f64,crates/fs-material/src/identifiability.rs#CanonicalWriter::count,crates/fs-material/src/identifiability.rs#CanonicalWriter::text,crates/fs-material/src/identifiability.rs#CanonicalWriter::hash,crates/fs-material/src/identifiability.rs#CanonicalWriter::finish,crates/fs-material/src/identifiability.rs#CanonicalReader::new,crates/fs-material/src/identifiability.rs#CanonicalReader::take,crates/fs-material/src/identifiability.rs#CanonicalReader::byte,crates/fs-material/src/identifiability.rs#CanonicalReader::u32,crates/fs-material/src/identifiability.rs#CanonicalReader::u64,crates/fs-material/src/identifiability.rs#CanonicalReader::f64,crates/fs-material/src/identifiability.rs#CanonicalReader::length,crates/fs-material/src/identifiability.rs#CanonicalReader::count,crates/fs-material/src/identifiability.rs#CanonicalReader::text,crates/fs-material/src/identifiability.rs#CanonicalReader::token,crates/fs-material/src/identifiability.rs#CanonicalReader::reason,crates/fs-material/src/identifiability.rs#CanonicalReader::hash,crates/fs-material/src/identifiability.rs#CanonicalReader::finish,crates/fs-blake3/src/lib.rs#hash_domain",
    "schema_dependencies=fs-material:identifiability-problem,fs-material:identifiability-execution",
    "digest=blake3-256-domain-separated",
    "encoding=typed-binary",
    "sources=IdentifiabilityAssessment",
    "source_fields=IdentifiabilityAssessment.schema_version:semantic,IdentifiabilityAssessment.header:derived:identity-projection-excludes-artifact-id,IdentifiabilityAssessment.problem_id:semantic,IdentifiabilityAssessment.execution_id:semantic,IdentifiabilityAssessment.claims:semantic,IdentifiabilityAssessment.evidence:semantic,IdentifiabilityAssessment.source_authority:semantic",
    "source_bindings=IdentifiabilityAssessment.schema_version>wire-schema-version,IdentifiabilityAssessment.problem_id>problem-id,IdentifiabilityAssessment.execution_id>execution-id,IdentifiabilityAssessment.claims>typed-claim-registry,IdentifiabilityAssessment.evidence>claim-assessment-registry,IdentifiabilityAssessment.source_authority>assessment-source-authority",
    "external_semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,fixed-numeric-little-endian,identity-header-projection-marker,header-units,header-seed,header-accuracy,header-time-ms,header-memory-bytes,header-versions,header-capabilities",
    "semantic_fields=identity-domain,identity-version,wire-magic,canonical-field-order,collection-count-u32-le,fixed-numeric-little-endian,identity-header-projection-marker,header-units,header-seed,header-accuracy,header-time-ms,header-memory-bytes,header-versions,header-capabilities,wire-schema-version,problem-id,execution-id,typed-claim-registry,claim-assessment-registry,assessment-source-authority",
    "excluded_fields=ArtifactHeader.id:ledger-label-not-scientific-identity,caller-container-order:canonicalized-before-identity",
    "consumers=IdentifiabilityAssessment::try_new,IdentifiabilityAssessment::id",
    "mutations=identity-domain:crates/fs-material/tests/identifiability_authority.rs#identity_domains_and_wire_magics_are_stage_separated,identity-version:crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact,wire-magic:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_versions_and_transports_fail_closed,canonical-field-order:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,collection-count-u32-le:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,fixed-numeric-little-endian:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,identity-header-projection-marker:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_preimages_have_exact_wire_layout,header-units:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,header-seed:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,header-accuracy:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,header-time-ms:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,header-memory-bytes:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,header-versions:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,header-capabilities:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,wire-schema-version:crates/fs-material/tests/identifiability_authority.rs#identifiability_identity_versions_and_transports_fail_closed,problem-id:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,execution-id:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,typed-claim-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,claim-assessment-registry:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence,assessment-source-authority:crates/fs-material/tests/identifiability_authority.rs#identifiability_assessment_identity_bindings_have_exact_mutation_evidence",
    "nonsemantic_mutations=ArtifactHeader.id:crates/fs-material/tests/identifiability_authority.rs#artifact_labels_do_not_move_execution_or_assessment_identity,caller-container-order:crates/fs-material/tests/identifiability_authority.rs#assessment_input_order_is_nonsemantic",
    "field_guard=classify_identifiability_assessment_identity_fields",
    "transport_guard=IdentifiabilityAssessment::from_canonical_bytes",
    "version_guard=crates/fs-material/tests/identifiability_authority.rs#identity_version_guard_is_exact",
    "coupling_surface=fs-material:identifiability-assessment",
];

fn required_axes(claim: &TypedIdentifiabilityClaim) -> BTreeSet<RequestedClaimAxis> {
    let mut required = BTreeSet::new();
    match claim.information {
        InformationRegime::StructuralExactModel | InformationRegime::ExactInputOutputMap => {
            required.insert(RequestedClaimAxis::Structural);
        }
        InformationRegime::NoisyFiniteData | InformationRegime::PosteriorUnderDeclaredPrior => {
            required.insert(RequestedClaimAxis::Practical);
        }
    }
    required.insert(match claim.extent {
        IdentifiabilityExtent::Local => RequestedClaimAxis::Local,
        IdentifiabilityExtent::Global | IdentifiabilityExtent::SetValued => {
            RequestedClaimAxis::Global
        }
    });
    if matches!(claim.quantifier, ClaimQuantifier::AlmostEverywhere { .. }) {
        required.insert(RequestedClaimAxis::Generic);
    }
    required
}

fn validate_claim_source(
    source: &SourceRef,
    field: &'static str,
) -> Result<(), IdentifiabilityError> {
    if !matches!(
        source.kind,
        SourceKind::Analyzer
            | SourceKind::Assumption
            | SourceKind::EvidenceReceipt
            | SourceKind::ExternalManifold
    ) {
        return Err(IdentifiabilityError::InvalidText {
            field,
            detail: format!("source {} has an inadmissible claim kind", source.key),
        });
    }
    Ok(())
}

impl IdentifiabilityAssessment {
    pub fn try_new(
        header: ArtifactHeader,
        problem: &AdmittedIdentifiabilityProblem,
        execution: &IdentifiabilityExecutionPlan,
        claims: Vec<TypedIdentifiabilityClaim>,
        evidence: Vec<(ClaimId, ClaimAssessment)>,
        source_authority: SourceResolutionSet,
    ) -> Result<Self, IdentifiabilityError> {
        validate_header_profile(&header)?;
        if !header.capabilities().contains("identifiability.assess") {
            return Err(IdentifiabilityError::InvalidText {
                field: "assessment capability",
                detail: "missing identifiability.assess capability".to_string(),
            });
        }
        if execution.problem_id != problem.problem_id
            || execution.source_admission_id != problem.source_admission_id
        {
            return Err(IdentifiabilityError::SourceMismatch {
                field: "assessment problem/execution",
            });
        }
        let claims = insert_unique(claims, "identifiability claims", |claim| &claim.id)?;
        let mut evidence_map = BTreeMap::new();
        for (id, conclusion) in evidence {
            if evidence_map.insert(id.clone(), conclusion).is_some() {
                return Err(IdentifiabilityError::Duplicate {
                    field: "claim assessment",
                    id: id.to_string(),
                });
            }
        }
        let claim_ids = claims.keys().cloned().collect::<BTreeSet<_>>();
        let evidence_ids = evidence_map.keys().cloned().collect::<BTreeSet<_>>();
        if evidence_ids != claim_ids {
            return Err(IdentifiabilityError::Cardinality {
                field: "claim assessments",
                detail: "claim and assessment identity sets must match exactly".to_string(),
            });
        }
        let mut referenced_sources = BTreeMap::<SourceKey, SourceRef>::new();
        let mut bind_source = |source: &SourceRef| -> Result<(), IdentifiabilityError> {
            if let Some(prior) = referenced_sources.insert(source.key.clone(), source.clone()) {
                if &prior != source {
                    return Err(IdentifiabilityError::SourceMismatch {
                        field: "assessment source alias",
                    });
                }
            }
            Ok(())
        };
        for (id, claim) in &claims {
            let missing_axes = required_axes(claim)
                .difference(&execution.requested_axes)
                .copied()
                .collect::<Vec<_>>();
            if !missing_axes.is_empty() {
                return Err(IdentifiabilityError::InvalidText {
                    field: "unrequested claim axis",
                    detail: format!("claim {id} was not preregistered for axes {missing_axes:?}"),
                });
            }
            match &claim.subject {
                ClaimSubject::Parameter(role) => {
                    if !problem.document.parameters.contains_key(role) {
                        return Err(IdentifiabilityError::UnknownReference {
                            field: "claim parameter",
                            id: role.to_string(),
                        });
                    }
                }
                ClaimSubject::ParameterTuple(roles) => {
                    if roles.len() < 2 {
                        return Err(IdentifiabilityError::Cardinality {
                            field: "claim parameter tuple",
                            detail: "tuple claims need at least two parameters".to_string(),
                        });
                    }
                    for role in roles {
                        if !problem.document.parameters.contains_key(role) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "claim parameter tuple",
                                id: role.to_string(),
                            });
                        }
                    }
                }
                ClaimSubject::Influence(influence) => {
                    if !problem.document.influences.contains_key(influence) {
                        return Err(IdentifiabilityError::UnknownReference {
                            field: "claim influence",
                            id: influence.to_string(),
                        });
                    }
                }
                ClaimSubject::GaugeClass(gauge) => {
                    if !problem.document.gauges.contains_key(gauge) {
                        return Err(IdentifiabilityError::UnknownReference {
                            field: "claim gauge",
                            id: gauge.to_string(),
                        });
                    }
                }
                ClaimSubject::WholeProblem => {}
            }
            match &claim.scope {
                ClaimScope::WholeCampaign => {}
                ClaimScope::Cases(cases) => {
                    if cases.is_empty() {
                        return Err(IdentifiabilityError::Cardinality {
                            field: "claim case scope",
                            detail: "claim case scope cannot be empty".to_string(),
                        });
                    }
                    for case in cases {
                        if !problem.document.cases.contains_key(case) {
                            return Err(IdentifiabilityError::UnknownReference {
                                field: "claim case scope",
                                id: case.to_string(),
                            });
                        }
                    }
                }
                ClaimScope::Stratum { definition } => {
                    validate_source_key(&problem.document.sources, definition, "claim stratum")?;
                }
            }
            match &claim.quantifier {
                ClaimQuantifier::AtRealization { realization }
                | ClaimQuantifier::AlmostEverywhere {
                    measure: realization,
                }
                | ClaimQuantifier::ForAll {
                    domain: realization,
                } => {
                    validate_claim_source(realization, "claim quantifier source")?;
                    bind_source(realization)?;
                }
                ClaimQuantifier::ProbabilityAtLeast {
                    probability,
                    measure,
                } => {
                    if !probability.is_finite() || *probability <= 0.0 || *probability > 1.0 {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "claim probability",
                            detail: "probability must lie in (0,1]".to_string(),
                        });
                    }
                    validate_claim_source(measure, "claim probability measure")?;
                    bind_source(measure)?;
                }
            }
            match &evidence_map[id] {
                ClaimAssessment::ClaimedEstablished {
                    method,
                    receipt,
                    tolerance,
                }
                | ClaimAssessment::ClaimedRefuted {
                    method,
                    receipt,
                    tolerance,
                } => {
                    if method.kind != SourceKind::Analyzer
                        || receipt.kind != SourceKind::EvidenceReceipt
                        || !tolerance.is_finite()
                        || *tolerance < 0.0
                    {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "claim evidence",
                            detail: "established/refuted claims need analyzer, receipt, and finite nonnegative tolerance"
                            .to_string(),
                        });
                    }
                    if method != &execution.analyzer {
                        return Err(IdentifiabilityError::SourceMismatch {
                            field: "claim analyzer/execution analyzer",
                        });
                    }
                    let numerical_floor = execution
                        .numerical
                        .rank_tolerance
                        .max(execution.numerical.singular_value_floor);
                    if *tolerance < numerical_floor {
                        return Err(IdentifiabilityError::InvalidNumeric {
                            field: "claim evidence tolerance",
                            detail: format!(
                                "claim tolerance {tolerance:e} is tighter than execution floor {numerical_floor:e}"
                            ),
                        });
                    }
                    bind_source(method)?;
                    bind_source(receipt)?;
                }
                ClaimAssessment::ClaimedInconclusive {
                    method,
                    receipt,
                    reason,
                } => {
                    validate_reason(reason, "inconclusive claim reason")?;
                    if let Some(method) = method {
                        if method.kind != SourceKind::Analyzer {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "inconclusive method",
                                detail: "method source must have Analyzer kind".to_string(),
                            });
                        }
                        if method != &execution.analyzer {
                            return Err(IdentifiabilityError::SourceMismatch {
                                field: "inconclusive analyzer/execution analyzer",
                            });
                        }
                        bind_source(method)?;
                    }
                    if let Some(receipt) = receipt {
                        if receipt.kind != SourceKind::EvidenceReceipt {
                            return Err(IdentifiabilityError::InvalidText {
                                field: "inconclusive receipt",
                                detail: "receipt source must have EvidenceReceipt kind".to_string(),
                            });
                        }
                        bind_source(receipt)?;
                    }
                }
                ClaimAssessment::NotAssessed { reason } => {
                    validate_reason(reason, "not-assessed claim reason")?
                }
            }
        }
        if source_authority.entries.len() != referenced_sources.len() {
            return Err(IdentifiabilityError::Cardinality {
                field: "assessment source authority",
                detail: "every claim/method/receipt source needs exactly one resolution"
                    .to_string(),
            });
        }
        for (key, reference) in &referenced_sources {
            let resolution = source_authority.entries.get(key).ok_or_else(|| {
                IdentifiabilityError::UnknownReference {
                    field: "assessment source authority",
                    id: key.to_string(),
                }
            })?;
            admit_opaque_resolution(reference, resolution)?;
            if let Some(execution_resolution) = execution.source_authority.entries.get(key)
                && execution_resolution != resolution
            {
                return Err(IdentifiabilityError::SourceMismatch {
                    field: "assessment/execution source authority",
                });
            }
            if let Some(problem_resolution) = problem.source_resolutions().get(key)
                && problem_resolution != resolution
            {
                return Err(IdentifiabilityError::SourceMismatch {
                    field: "assessment/problem source authority",
                });
            }
        }
        Ok(Self {
            schema_version: IDENTIFIABILITY_ASSESSMENT_IDENTITY_VERSION,
            header,
            problem_id: problem.problem_id,
            execution_id: execution.id()?,
            claims,
            evidence: evidence_map,
            source_authority,
        })
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, IdentifiabilityError> {
        encode_assessment(self)
    }

    pub fn id(&self) -> Result<AssessmentId, IdentifiabilityError> {
        assessment_identity_hash(self)
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn header(&self) -> &ArtifactHeader {
        &self.header
    }

    #[must_use]
    pub const fn problem_id(&self) -> ProblemId {
        self.problem_id
    }

    #[must_use]
    pub const fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }

    #[must_use]
    pub const fn claims(&self) -> &BTreeMap<ClaimId, TypedIdentifiabilityClaim> {
        &self.claims
    }

    #[must_use]
    pub const fn evidence(&self) -> &BTreeMap<ClaimId, ClaimAssessment> {
        &self.evidence
    }

    /// Locally verified source resolutions required to replay this assessment.
    /// Serialized verification markers are never accepted as a substitute.
    #[must_use]
    pub const fn source_authority(&self) -> &SourceResolutionSet {
        &self.source_authority
    }

    pub fn from_canonical_bytes(
        bytes: &[u8],
        problem: &AdmittedIdentifiabilityProblem,
        execution: &IdentifiabilityExecutionPlan,
        verified_sources: &SourceResolutionSet,
    ) -> Result<Self, IdentifiabilityError> {
        decode_assessment(bytes, problem, execution, verified_sources)
    }
}

fn encode_source_key(
    writer: &mut CanonicalWriter,
    key: &SourceKey,
) -> Result<(), IdentifiabilityError> {
    writer.text(key.as_str(), "source key")
}

fn decode_source_key(reader: &mut CanonicalReader<'_>) -> Result<SourceKey, IdentifiabilityError> {
    SourceKey::try_new(reader.token("source key")?)
}

fn encode_case_id(writer: &mut CanonicalWriter, id: &CaseId) -> Result<(), IdentifiabilityError> {
    writer.text(id.as_str(), "case id")
}

fn decode_case_id(reader: &mut CanonicalReader<'_>) -> Result<CaseId, IdentifiabilityError> {
    CaseId::try_new(reader.token("case id")?)
}

fn encode_role(
    writer: &mut CanonicalWriter,
    role: &ParameterRoleId,
) -> Result<(), IdentifiabilityError> {
    writer.text(role.as_str(), "parameter role")
}

fn decode_role(reader: &mut CanonicalReader<'_>) -> Result<ParameterRoleId, IdentifiabilityError> {
    ParameterRoleId::try_new(reader.token("parameter role")?)
}

fn encode_channel(
    writer: &mut CanonicalWriter,
    channel: &ObservationChannelId,
) -> Result<(), IdentifiabilityError> {
    writer.text(channel.as_str(), "observation channel")
}

fn decode_channel(
    reader: &mut CanonicalReader<'_>,
) -> Result<ObservationChannelId, IdentifiabilityError> {
    ObservationChannelId::try_new(reader.token("observation channel")?)
}

fn encode_source_kind(writer: &mut CanonicalWriter, kind: SourceKind) {
    writer.byte(match kind {
        SourceKind::ContextOfUse => 0,
        SourceKind::MaterialCard => 1,
        SourceKind::ConstitutiveModelCard => 2,
        SourceKind::ConstitutiveGraph => 3,
        SourceKind::ExperimentArtifact => 4,
        SourceKind::CalibrationSplit => 5,
        SourceKind::ForwardModel => 6,
        SourceKind::Geometry => 7,
        SourceKind::Process => 8,
        SourceKind::Protocol => 9,
        SourceKind::ObservationOperator => 10,
        SourceKind::Metrology => 11,
        SourceKind::Parser => 12,
        SourceKind::Preprocessing => 13,
        SourceKind::Likelihood => 14,
        SourceKind::Prior => 15,
        SourceKind::Constraint => 16,
        SourceKind::GaugeAction => 17,
        SourceKind::GaugeSection => 18,
        SourceKind::Discrepancy => 19,
        SourceKind::Assumption => 20,
        SourceKind::Analyzer => 21,
        SourceKind::DerivativeProvider => 22,
        SourceKind::Build => 23,
        SourceKind::EvidenceReceipt => 24,
        SourceKind::ExternalManifold => 25,
    });
}

fn decode_source_kind(
    reader: &mut CanonicalReader<'_>,
) -> Result<SourceKind, IdentifiabilityError> {
    match reader.byte("source kind")? {
        0 => Ok(SourceKind::ContextOfUse),
        1 => Ok(SourceKind::MaterialCard),
        2 => Ok(SourceKind::ConstitutiveModelCard),
        3 => Ok(SourceKind::ConstitutiveGraph),
        4 => Ok(SourceKind::ExperimentArtifact),
        5 => Ok(SourceKind::CalibrationSplit),
        6 => Ok(SourceKind::ForwardModel),
        7 => Ok(SourceKind::Geometry),
        8 => Ok(SourceKind::Process),
        9 => Ok(SourceKind::Protocol),
        10 => Ok(SourceKind::ObservationOperator),
        11 => Ok(SourceKind::Metrology),
        12 => Ok(SourceKind::Parser),
        13 => Ok(SourceKind::Preprocessing),
        14 => Ok(SourceKind::Likelihood),
        15 => Ok(SourceKind::Prior),
        16 => Ok(SourceKind::Constraint),
        17 => Ok(SourceKind::GaugeAction),
        18 => Ok(SourceKind::GaugeSection),
        19 => Ok(SourceKind::Discrepancy),
        20 => Ok(SourceKind::Assumption),
        21 => Ok(SourceKind::Analyzer),
        22 => Ok(SourceKind::DerivativeProvider),
        23 => Ok(SourceKind::Build),
        24 => Ok(SourceKind::EvidenceReceipt),
        25 => Ok(SourceKind::ExternalManifold),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown source kind tag {tag}"),
        }),
    }
}

fn encode_source_ref(
    writer: &mut CanonicalWriter,
    source: &SourceRef,
) -> Result<(), IdentifiabilityError> {
    encode_source_key(writer, &source.key)?;
    encode_source_kind(writer, source.kind);
    writer.hash(source.expected_hash);
    writer.text(&source.content_hash_domain, "source content-hash domain")?;
    writer.u32(source.contract_version);
    Ok(())
}

fn decode_source_ref(reader: &mut CanonicalReader<'_>) -> Result<SourceRef, IdentifiabilityError> {
    SourceRef::try_new(
        decode_source_key(reader)?,
        decode_source_kind(reader)?,
        reader.hash("source hash")?,
        reader.token("source content-hash domain")?,
        reader.u32("source contract version")?,
    )
}

fn encode_resolution_verification(
    writer: &mut CanonicalWriter,
    verification: &ResolutionVerification,
) {
    match verification {
        ResolutionVerification::TypedArtifact => writer.byte(0),
        ResolutionVerification::CanonicalPreimage { byte_len } => {
            writer.byte(1);
            writer.u64(*byte_len);
        }
        ResolutionVerification::Unverified => writer.byte(2),
    }
}

fn decode_resolution_verification(
    reader: &mut CanonicalReader<'_>,
) -> Result<ResolutionVerification, IdentifiabilityError> {
    match reader.byte("source verification")? {
        0 => Ok(ResolutionVerification::TypedArtifact),
        1 => {
            let byte_len = reader.u64("verified source byte length")?;
            Ok(ResolutionVerification::CanonicalPreimage { byte_len })
        }
        2 => Ok(ResolutionVerification::Unverified),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown source-verification tag {tag}"),
        }),
    }
}

fn encode_resolution_entry(
    writer: &mut CanonicalWriter,
    resolution: &SourceResolution,
) -> Result<(), IdentifiabilityError> {
    encode_source_key(writer, &resolution.key)?;
    encode_source_kind(writer, resolution.kind);
    writer.hash(resolution.resolved_hash);
    writer.text(
        &resolution.content_hash_domain,
        "resolved source content-hash domain",
    )?;
    writer.u32(resolution.contract_version);
    encode_resolution_verification(writer, &resolution.verification);
    match &resolution.authority {
        AuthorityDisposition::ContentVerified => writer.byte(0),
        AuthorityDisposition::ExternalTrustReceipt { trust_receipt } => {
            writer.byte(1);
            writer.hash(*trust_receipt);
        }
        AuthorityDisposition::Unverified { reason } => {
            writer.byte(2);
            writer.text(reason, "unverified source reason")?;
        }
    }
    Ok(())
}

fn encode_resolution_set(
    writer: &mut CanonicalWriter,
    resolutions: &SourceResolutionSet,
) -> Result<(), IdentifiabilityError> {
    writer.count(resolutions.entries.len(), "source resolutions")?;
    for resolution in resolutions.entries.values() {
        encode_resolution_entry(writer, resolution)?;
    }
    Ok(())
}

fn decode_resolution_set(
    reader: &mut CanonicalReader<'_>,
) -> Result<SourceResolutionSet, IdentifiabilityError> {
    let count = reader.count("source resolutions")?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = decode_source_key(reader)?;
        let kind = decode_source_kind(reader)?;
        let resolved_hash = reader.hash("resolved source hash")?;
        let content_hash_domain = reader.token("resolved source content-hash domain")?;
        let contract_version = reader.u32("resolved source contract version")?;
        let verification = decode_resolution_verification(reader)?;
        let authority = match reader.byte("source authority")? {
            0 => AuthorityDisposition::ContentVerified,
            1 => AuthorityDisposition::ExternalTrustReceipt {
                trust_receipt: reader.hash("source trust receipt")?,
            },
            2 => AuthorityDisposition::Unverified {
                reason: reader.reason("unverified source reason")?,
            },
            tag => {
                return Err(IdentifiabilityError::Canonical {
                    at: reader.at.saturating_sub(1),
                    detail: format!("unknown source-authority tag {tag}"),
                });
            }
        };
        validate_authority_disposition(&authority)?;
        if !hash_is_nonzero(resolved_hash) || contract_version == 0 {
            return Err(IdentifiabilityError::InvalidText {
                field: "transported source resolution",
                detail: "resolved hash and source contract version must be nonzero".to_string(),
            });
        }
        entries.push(SourceResolution {
            key,
            kind,
            resolved_hash,
            content_hash_domain,
            contract_version,
            authority,
            verification,
        });
    }
    SourceResolutionSet::try_new(entries)
}

fn encode_observation_key(
    writer: &mut CanonicalWriter,
    key: &ObservationKey,
) -> Result<(), IdentifiabilityError> {
    encode_case_id(writer, &key.case)?;
    encode_channel(writer, &key.channel)
}

fn decode_observation_key(
    reader: &mut CanonicalReader<'_>,
) -> Result<ObservationKey, IdentifiabilityError> {
    Ok(ObservationKey::new(
        decode_case_id(reader)?,
        decode_channel(reader)?,
    ))
}

fn encode_parameter_treatment(
    writer: &mut CanonicalWriter,
    treatment: &ParameterTreatment,
) -> Result<(), IdentifiabilityError> {
    match treatment {
        ParameterTreatment::Estimated => writer.byte(0),
        ParameterTreatment::Profiled => writer.byte(1),
        ParameterTreatment::Marginalized => writer.byte(2),
        ParameterTreatment::Conditioned(value) => {
            writer.byte(3);
            writer.f64(value.value_si);
            encode_source_key(writer, &value.source)?;
        }
        ParameterTreatment::Derived {
            definition,
            parents,
        } => {
            writer.byte(4);
            encode_source_key(writer, definition)?;
            writer.count(parents.len(), "derived parameter parents")?;
            for parent in parents {
                encode_role(writer, parent)?;
            }
        }
    }
    Ok(())
}

fn decode_parameter_treatment(
    reader: &mut CanonicalReader<'_>,
) -> Result<ParameterTreatment, IdentifiabilityError> {
    match reader.byte("parameter treatment")? {
        0 => Ok(ParameterTreatment::Estimated),
        1 => Ok(ParameterTreatment::Profiled),
        2 => Ok(ParameterTreatment::Marginalized),
        3 => Ok(ParameterTreatment::Conditioned(ConditionedValue::try_new(
            reader.f64("conditioned value")?,
            decode_source_key(reader)?,
        )?)),
        4 => {
            let definition = decode_source_key(reader)?;
            let count = reader.count("derived parameter parents")?;
            let mut parents = BTreeSet::new();
            for _ in 0..count {
                let parent = decode_role(reader)?;
                if !parents.insert(parent.clone()) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "derived parameter parent",
                        id: parent.to_string(),
                    });
                }
            }
            Ok(ParameterTreatment::Derived {
                definition,
                parents,
            })
        }
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown parameter treatment tag {tag}"),
        }),
    }
}

fn encode_prior_policy(
    writer: &mut CanonicalWriter,
    prior: &PriorPolicy,
) -> Result<(), IdentifiabilityError> {
    match prior {
        PriorPolicy::Distribution(distribution) => {
            writer.byte(0);
            encode_prior(writer, distribution)?;
        }
        PriorPolicy::Absent { reason } => {
            writer.byte(1);
            writer.text(reason, "prior absence reason")?;
        }
        PriorPolicy::NotApplicable { reason } => {
            writer.byte(2);
            writer.text(reason, "prior not-applicable reason")?;
        }
    }
    Ok(())
}

fn decode_prior_policy(
    reader: &mut CanonicalReader<'_>,
) -> Result<PriorPolicy, IdentifiabilityError> {
    match reader.byte("prior policy")? {
        0 => Ok(PriorPolicy::Distribution(decode_prior(reader)?)),
        1 => Ok(PriorPolicy::Absent {
            reason: reader.reason("prior absence reason")?,
        }),
        2 => Ok(PriorPolicy::NotApplicable {
            reason: reader.reason("prior not-applicable reason")?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown prior policy tag {tag}"),
        }),
    }
}

fn encode_owner(
    writer: &mut CanonicalWriter,
    owner: &ParameterOwnerBinding,
) -> Result<(), IdentifiabilityError> {
    match owner {
        ParameterOwnerBinding::ConstitutiveModel => writer.byte(0),
        ParameterOwnerBinding::InitialState { state_path } => {
            writer.byte(1);
            encode_source_key(writer, state_path)?;
        }
        ParameterOwnerBinding::Instrument { instrument } => {
            writer.byte(2);
            encode_source_key(writer, instrument)?;
        }
        ParameterOwnerBinding::Discrepancy { family } => {
            writer.byte(3);
            encode_source_key(writer, family)?;
        }
        ParameterOwnerBinding::ControlledInput { protocol } => {
            writer.byte(4);
            encode_source_key(writer, protocol)?;
        }
        ParameterOwnerBinding::Population { hierarchy } => {
            writer.byte(5);
            encode_source_key(writer, hierarchy)?;
        }
    }
    Ok(())
}

fn decode_owner(
    reader: &mut CanonicalReader<'_>,
) -> Result<ParameterOwnerBinding, IdentifiabilityError> {
    match reader.byte("parameter owner")? {
        0 => Ok(ParameterOwnerBinding::ConstitutiveModel),
        1 => Ok(ParameterOwnerBinding::InitialState {
            state_path: decode_source_key(reader)?,
        }),
        2 => Ok(ParameterOwnerBinding::Instrument {
            instrument: decode_source_key(reader)?,
        }),
        3 => Ok(ParameterOwnerBinding::Discrepancy {
            family: decode_source_key(reader)?,
        }),
        4 => Ok(ParameterOwnerBinding::ControlledInput {
            protocol: decode_source_key(reader)?,
        }),
        5 => Ok(ParameterOwnerBinding::Population {
            hierarchy: decode_source_key(reader)?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown parameter owner tag {tag}"),
        }),
    }
}

fn encode_scope(
    writer: &mut CanonicalWriter,
    scope: &ParameterScopeBinding,
) -> Result<(), IdentifiabilityError> {
    match scope {
        ParameterScopeBinding::Global => writer.byte(0),
        ParameterScopeBinding::Cases(cases) => {
            writer.byte(1);
            writer.count(cases.len(), "parameter case scope")?;
            for case in cases {
                encode_case_id(writer, case)?;
            }
        }
        ParameterScopeBinding::MaterialLot { lot } => {
            writer.byte(2);
            encode_artifact_id(writer, lot)?;
        }
        ParameterScopeBinding::Specimen { case, specimen } => {
            writer.byte(3);
            encode_case_id(writer, case)?;
            encode_artifact_id(writer, specimen)?;
        }
        ParameterScopeBinding::Field { support } => {
            writer.byte(4);
            encode_source_key(writer, support)?;
        }
        ParameterScopeBinding::Hierarchical {
            population,
            level,
            hierarchy,
        } => {
            writer.byte(5);
            encode_artifact_id(writer, population)?;
            writer.u32(*level);
            encode_source_key(writer, hierarchy)?;
        }
    }
    Ok(())
}

fn decode_scope(
    reader: &mut CanonicalReader<'_>,
) -> Result<ParameterScopeBinding, IdentifiabilityError> {
    match reader.byte("parameter scope")? {
        0 => Ok(ParameterScopeBinding::Global),
        1 => {
            let count = reader.count("parameter case scope")?;
            let mut cases = BTreeSet::new();
            for _ in 0..count {
                let case = decode_case_id(reader)?;
                if !cases.insert(case.clone()) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "parameter case scope",
                        id: case.to_string(),
                    });
                }
            }
            Ok(ParameterScopeBinding::Cases(cases))
        }
        2 => Ok(ParameterScopeBinding::MaterialLot {
            lot: decode_artifact_id(reader)?,
        }),
        3 => Ok(ParameterScopeBinding::Specimen {
            case: decode_case_id(reader)?,
            specimen: decode_artifact_id(reader)?,
        }),
        4 => Ok(ParameterScopeBinding::Field {
            support: decode_source_key(reader)?,
        }),
        5 => Ok(ParameterScopeBinding::Hierarchical {
            population: decode_artifact_id(reader)?,
            level: reader.u32("hierarchical level")?,
            hierarchy: decode_source_key(reader)?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown parameter scope tag {tag}"),
        }),
    }
}

fn encode_study_parameter(
    writer: &mut CanonicalWriter,
    parameter: &StudyParameter,
) -> Result<(), IdentifiabilityError> {
    encode_role(writer, &parameter.role)?;
    writer.quantity(parameter.quantity);
    encode_parameter_domain(writer, parameter.domain);
    writer.byte(match parameter.purpose {
        ParameterPurpose::Estimand => 0,
        ParameterPurpose::Nuisance => 1,
        ParameterPurpose::Hyperparameter => 2,
        ParameterPurpose::CalibrationControl => 3,
    });
    encode_parameter_treatment(writer, &parameter.treatment)?;
    encode_owner(writer, &parameter.owner)?;
    encode_scope(writer, &parameter.scope)?;
    encode_prior_policy(writer, &parameter.prior)?;
    match &parameter.influence_coverage {
        InfluenceCoverage::Declared => writer.byte(0),
        InfluenceCoverage::IntentionallyAbsent { reason } => {
            writer.byte(1);
            writer.text(reason, "influence absence reason")?;
        }
        InfluenceCoverage::NotApplicable { reason } => {
            writer.byte(2);
            writer.text(reason, "influence not-applicable reason")?;
        }
    }
    Ok(())
}

fn decode_study_parameter(
    reader: &mut CanonicalReader<'_>,
) -> Result<StudyParameter, IdentifiabilityError> {
    let role = decode_role(reader)?;
    let quantity = reader.quantity("physical parameter quantity")?;
    let domain = decode_parameter_domain(reader)?;
    let purpose = match reader.byte("parameter purpose")? {
        0 => ParameterPurpose::Estimand,
        1 => ParameterPurpose::Nuisance,
        2 => ParameterPurpose::Hyperparameter,
        3 => ParameterPurpose::CalibrationControl,
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown parameter purpose tag {tag}"),
            });
        }
    };
    let treatment = decode_parameter_treatment(reader)?;
    let owner = decode_owner(reader)?;
    let scope = decode_scope(reader)?;
    let prior = decode_prior_policy(reader)?;
    let influence_coverage = match reader.byte("influence coverage")? {
        0 => InfluenceCoverage::Declared,
        1 => InfluenceCoverage::IntentionallyAbsent {
            reason: reader.reason("influence absence reason")?,
        },
        2 => InfluenceCoverage::NotApplicable {
            reason: reader.reason("influence not-applicable reason")?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown influence coverage tag {tag}"),
            });
        }
    };
    StudyParameter::try_new(
        role,
        quantity,
        domain,
        purpose,
        treatment,
        owner,
        scope,
        prior,
        influence_coverage,
    )
}

fn encode_constraint(
    writer: &mut CanonicalWriter,
    constraint: &JointConstraint,
) -> Result<(), IdentifiabilityError> {
    writer.text(constraint.id.as_str(), "constraint id")?;
    match &constraint.kind {
        JointConstraintKind::Affine {
            terms,
            relation,
            rhs_si,
            residual_quantity,
        } => {
            writer.byte(0);
            writer.count(terms.len(), "affine terms")?;
            for term in terms {
                encode_role(writer, &term.parameter)?;
                writer.f64(term.coefficient);
                writer.quantity(term.coefficient_quantity);
            }
            writer.byte(match relation {
                ConstraintRelation::Equal => 0,
                ConstraintRelation::LessOrEqual => 1,
                ConstraintRelation::GreaterOrEqual => 2,
            });
            writer.f64(*rhs_si);
            writer.quantity(*residual_quantity);
        }
        JointConstraintKind::Simplex {
            members,
            total_si,
            quantity,
        } => {
            writer.byte(1);
            writer.count(members.len(), "simplex members")?;
            for member in members {
                encode_role(writer, member)?;
            }
            writer.f64(*total_si);
            writer.quantity(*quantity);
        }
        JointConstraintKind::Ordered { members, strict } => {
            writer.byte(2);
            writer.count(members.len(), "ordered members")?;
            for member in members {
                encode_role(writer, member)?;
            }
            writer.byte(u8::from(*strict));
        }
        JointConstraintKind::ExternalManifold {
            members,
            definition,
            codimension,
        } => {
            writer.byte(3);
            writer.count(members.len(), "manifold members")?;
            for member in members {
                encode_role(writer, member)?;
            }
            encode_source_key(writer, definition)?;
            writer.u32(*codimension);
        }
        JointConstraintKind::StochasticCoupling {
            members,
            distribution,
        } => {
            writer.byte(4);
            writer.count(members.len(), "stochastic members")?;
            for member in members {
                encode_role(writer, member)?;
            }
            encode_source_key(writer, distribution)?;
        }
    }
    Ok(())
}

fn decode_role_set(
    reader: &mut CanonicalReader<'_>,
    field: &'static str,
) -> Result<BTreeSet<ParameterRoleId>, IdentifiabilityError> {
    let count = reader.count(field)?;
    let mut result = BTreeSet::new();
    for _ in 0..count {
        let role = decode_role(reader)?;
        if !result.insert(role.clone()) {
            return Err(IdentifiabilityError::Duplicate {
                field,
                id: role.to_string(),
            });
        }
    }
    Ok(result)
}

fn decode_constraint(
    reader: &mut CanonicalReader<'_>,
) -> Result<JointConstraint, IdentifiabilityError> {
    let id = ConstraintId::try_new(reader.token("constraint id")?)?;
    let kind = match reader.byte("constraint kind")? {
        0 => {
            let count = reader.count("affine terms")?;
            let mut terms = Vec::with_capacity(count);
            for _ in 0..count {
                terms.push(AffineConstraintTerm::try_new(
                    decode_role(reader)?,
                    reader.f64("affine coefficient")?,
                    reader.quantity("affine coefficient quantity")?,
                )?);
            }
            let relation = match reader.byte("constraint relation")? {
                0 => ConstraintRelation::Equal,
                1 => ConstraintRelation::LessOrEqual,
                2 => ConstraintRelation::GreaterOrEqual,
                tag => {
                    return Err(IdentifiabilityError::Canonical {
                        at: reader.at.saturating_sub(1),
                        detail: format!("unknown constraint relation tag {tag}"),
                    });
                }
            };
            JointConstraintKind::Affine {
                terms,
                relation,
                rhs_si: reader.f64("constraint RHS")?,
                residual_quantity: reader.quantity("constraint residual quantity")?,
            }
        }
        1 => JointConstraintKind::Simplex {
            members: decode_role_set(reader, "simplex members")?,
            total_si: reader.f64("simplex total")?,
            quantity: reader.quantity("simplex quantity")?,
        },
        2 => {
            let count = reader.count("ordered members")?;
            let mut members = Vec::with_capacity(count);
            for _ in 0..count {
                members.push(decode_role(reader)?);
            }
            JointConstraintKind::Ordered {
                members,
                strict: match reader.byte("strict ordering")? {
                    0 => false,
                    1 => true,
                    tag => {
                        return Err(IdentifiabilityError::Canonical {
                            at: reader.at.saturating_sub(1),
                            detail: format!("invalid strict-ordering tag {tag}"),
                        });
                    }
                },
            }
        }
        3 => JointConstraintKind::ExternalManifold {
            members: decode_role_set(reader, "manifold members")?,
            definition: decode_source_key(reader)?,
            codimension: reader.u32("manifold codimension")?,
        },
        4 => JointConstraintKind::StochasticCoupling {
            members: decode_role_set(reader, "stochastic members")?,
            distribution: decode_source_key(reader)?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown constraint kind tag {tag}"),
            });
        }
    };
    Ok(JointConstraint::new(id, kind))
}

fn encode_marginal_noise(
    writer: &mut CanonicalWriter,
    noise: &MarginalNoiseSpec,
) -> Result<(), IdentifiabilityError> {
    match noise {
        MarginalNoiseSpec::Gaussian { standard_deviation } => {
            writer.byte(0);
            writer.f64(*standard_deviation);
        }
        MarginalNoiseSpec::StudentT {
            scale,
            degrees_of_freedom,
        } => {
            writer.byte(1);
            writer.f64(*scale);
            writer.f64(*degrees_of_freedom);
        }
        MarginalNoiseSpec::Empirical {
            distribution,
            standard_deviation,
            finite_variance_model,
        } => {
            writer.byte(2);
            encode_source_key(writer, distribution)?;
            writer.f64(*standard_deviation);
            encode_source_key(writer, finite_variance_model)?;
        }
        MarginalNoiseSpec::Bounded { half_width } => {
            writer.byte(3);
            writer.f64(*half_width);
        }
        MarginalNoiseSpec::Unknown { reason } => {
            writer.byte(4);
            writer.text(reason, "unknown noise reason")?;
        }
    }
    Ok(())
}

fn decode_marginal_noise(
    reader: &mut CanonicalReader<'_>,
) -> Result<MarginalNoiseSpec, IdentifiabilityError> {
    match reader.byte("marginal noise")? {
        0 => Ok(MarginalNoiseSpec::Gaussian {
            standard_deviation: reader.f64("Gaussian standard deviation")?,
        }),
        1 => Ok(MarginalNoiseSpec::StudentT {
            scale: reader.f64("Student-t scale")?,
            degrees_of_freedom: reader.f64("Student-t degrees of freedom")?,
        }),
        2 => Ok(MarginalNoiseSpec::Empirical {
            distribution: decode_source_key(reader)?,
            standard_deviation: reader.f64("empirical standard deviation")?,
            finite_variance_model: decode_source_key(reader)?,
        }),
        3 => Ok(MarginalNoiseSpec::Bounded {
            half_width: reader.f64("bounded-noise half width")?,
        }),
        4 => Ok(MarginalNoiseSpec::Unknown {
            reason: reader.reason("unknown noise reason")?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown marginal-noise tag {tag}"),
        }),
    }
}

fn encode_missingness(
    writer: &mut CanonicalWriter,
    missingness: &MissingnessAssumption,
) -> Result<(), IdentifiabilityError> {
    match missingness {
        MissingnessAssumption::Complete { assumption } => {
            writer.byte(0);
            encode_source_key(writer, assumption)?;
        }
        MissingnessAssumption::Modeled { mechanism } => {
            writer.byte(1);
            encode_source_key(writer, mechanism)?;
        }
        MissingnessAssumption::Unknown { reason } => {
            writer.byte(2);
            writer.text(reason, "unknown missingness reason")?;
        }
    }
    Ok(())
}

fn decode_missingness(
    reader: &mut CanonicalReader<'_>,
) -> Result<MissingnessAssumption, IdentifiabilityError> {
    match reader.byte("missingness")? {
        0 => Ok(MissingnessAssumption::Complete {
            assumption: decode_source_key(reader)?,
        }),
        1 => Ok(MissingnessAssumption::Modeled {
            mechanism: decode_source_key(reader)?,
        }),
        2 => Ok(MissingnessAssumption::Unknown {
            reason: reader.reason("unknown missingness reason")?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown missingness tag {tag}"),
        }),
    }
}

fn encode_study_observation(
    writer: &mut CanonicalWriter,
    observation: &StudyObservation,
) -> Result<(), IdentifiabilityError> {
    encode_channel(writer, &observation.id)?;
    encode_qoi_id(writer, &observation.qoi)?;
    writer.text(observation.unit.as_str(), "observation unit")?;
    writer.quantity(observation.quantity);
    encode_frame(writer, &observation.frame)?;
    writer.text(&observation.graph_node, "observation graph node")?;
    writer.text(&observation.graph_port, "observation graph port")?;
    encode_source_key(writer, &observation.operator)?;
    encode_source_key(writer, &observation.aggregation)?;
    encode_source_key(writer, &observation.sensor)?;
    encode_artifact_id(writer, &observation.instrument)?;
    encode_artifact_id(writer, &observation.clock)?;
    writer.u32(observation.operator_version);
    encode_marginal_noise(writer, &observation.noise)?;
    encode_missingness(writer, &observation.missingness)?;
    match observation.saturation {
        Some(domain) => {
            writer.byte(1);
            encode_parameter_domain(writer, domain);
        }
        None => writer.byte(0),
    }
    writer.u32(observation.protocol_version);
    writer.u32(observation.refinement_version);
    match &observation.rows {
        ObservationRows::Prospective => writer.byte(0),
        ObservationRows::Retrospective(rows) => {
            writer.byte(1);
            writer.count(rows.len(), "observation rows")?;
            for row in rows {
                encode_observation_row_id(writer, row)?;
            }
        }
    }
    Ok(())
}

fn decode_study_observation(
    reader: &mut CanonicalReader<'_>,
) -> Result<StudyObservation, IdentifiabilityError> {
    let id = decode_channel(reader)?;
    let qoi = decode_qoi_id(reader)?;
    let unit = UnitId::try_new(reader.token("observation unit")?).map_err(|error| {
        IdentifiabilityError::Vv {
            detail: error.to_string(),
        }
    })?;
    let quantity = reader.quantity("observation quantity")?;
    let frame = decode_frame(reader)?;
    let graph_node = reader.token("observation graph node")?;
    let graph_port = reader.token("observation graph port")?;
    let operator = decode_source_key(reader)?;
    let aggregation = decode_source_key(reader)?;
    let sensor = decode_source_key(reader)?;
    let instrument = decode_artifact_id(reader)?;
    let clock = decode_artifact_id(reader)?;
    let operator_version = reader.u32("observation operator version")?;
    let noise = decode_marginal_noise(reader)?;
    let missingness = decode_missingness(reader)?;
    let saturation = match reader.byte("saturation option")? {
        0 => None,
        1 => Some(decode_parameter_domain(reader)?),
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("invalid saturation option tag {tag}"),
            });
        }
    };
    let protocol_version = reader.u32("observation protocol version")?;
    let refinement_version = reader.u32("observation refinement version")?;
    let rows = match reader.byte("observation rows")? {
        0 => ObservationRows::Prospective,
        1 => {
            let count = reader.count("observation rows")?;
            let mut rows = BTreeSet::new();
            for _ in 0..count {
                let row = decode_observation_row_id(reader)?;
                if !rows.insert(row.clone()) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "observation row",
                        id: row.as_str().to_string(),
                    });
                }
            }
            ObservationRows::Retrospective(rows)
        }
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("invalid observation-row tag {tag}"),
            });
        }
    };
    StudyObservation::try_new(
        id,
        qoi,
        unit,
        quantity,
        frame,
        graph_node,
        graph_port,
        operator,
        aggregation,
        sensor,
        instrument,
        clock,
        operator_version,
        noise,
        missingness,
        saturation,
        protocol_version,
        refinement_version,
        rows,
    )
}

fn encode_discrepancy(
    writer: &mut CanonicalWriter,
    discrepancy: &StudyDiscrepancy,
) -> Result<(), IdentifiabilityError> {
    match discrepancy {
        StudyDiscrepancy::Uncharacterized { reason } => {
            writer.byte(0);
            writer.text(reason, "uncharacterized discrepancy reason")?;
        }
        StudyDiscrepancy::NotApplicable { reason } => {
            writer.byte(1);
            writer.text(reason, "discrepancy not-applicable reason")?;
        }
        StudyDiscrepancy::AssumedZero { assumption } => {
            writer.byte(2);
            encode_source_key(writer, assumption)?;
        }
        StudyDiscrepancy::Modeled {
            family,
            parameters,
            support,
            confounding_guard,
        } => {
            writer.byte(3);
            encode_source_key(writer, family)?;
            writer.count(parameters.len(), "discrepancy parameters")?;
            for parameter in parameters {
                encode_role(writer, parameter)?;
            }
            encode_source_key(writer, support)?;
            encode_source_key(writer, confounding_guard)?;
        }
    }
    Ok(())
}

fn decode_discrepancy(
    reader: &mut CanonicalReader<'_>,
) -> Result<StudyDiscrepancy, IdentifiabilityError> {
    match reader.byte("discrepancy")? {
        0 => Ok(StudyDiscrepancy::Uncharacterized {
            reason: reader.reason("uncharacterized discrepancy reason")?,
        }),
        1 => Ok(StudyDiscrepancy::NotApplicable {
            reason: reader.reason("discrepancy not-applicable reason")?,
        }),
        2 => Ok(StudyDiscrepancy::AssumedZero {
            assumption: decode_source_key(reader)?,
        }),
        3 => Ok(StudyDiscrepancy::Modeled {
            family: decode_source_key(reader)?,
            parameters: decode_role_set(reader, "discrepancy parameters")?,
            support: decode_source_key(reader)?,
            confounding_guard: decode_source_key(reader)?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown discrepancy tag {tag}"),
        }),
    }
}

fn encode_case(
    writer: &mut CanonicalWriter,
    case: &StudyCaseDocument,
) -> Result<(), IdentifiabilityError> {
    encode_case_id(writer, &case.id)?;
    match &case.purpose {
        CasePurpose::Calibration => writer.byte(0),
        CasePurpose::SymmetryBreaking => writer.byte(1),
        CasePurpose::ValidationOnly => writer.byte(2),
        CasePurpose::BlindFalsification => writer.byte(3),
        CasePurpose::ProspectiveDesign => writer.byte(4),
        CasePurpose::Complementary { reason } => {
            writer.byte(5);
            writer.text(reason, "complementary case reason")?;
        }
    }
    encode_initial_state(writer, case.initial_state);
    encode_specimen(writer, &case.specimen)?;
    encode_protocol(writer, &case.protocol)?;
    encode_source_key(writer, &case.forward_model)?;
    match &case.data {
        CaseDataDeclaration::Prospective => writer.byte(0),
        CaseDataDeclaration::Retrospective {
            experiment,
            split,
            parser,
            preprocessing,
            parser_version,
            split_grouping,
        } => {
            writer.byte(1);
            encode_source_key(writer, experiment)?;
            encode_source_key(writer, split)?;
            encode_source_key(writer, parser)?;
            encode_source_key(writer, preprocessing)?;
            writer.u32(*parser_version);
            encode_artifact_id(writer, split_grouping)?;
        }
    }
    writer.count(case.observations.len(), "case observations")?;
    for observation in case.observations.values() {
        encode_study_observation(writer, observation)?;
    }
    writer.count(case.discrepancies.len(), "case discrepancies")?;
    for (channel, discrepancy) in &case.discrepancies {
        encode_channel(writer, channel)?;
        encode_discrepancy(writer, discrepancy)?;
    }
    Ok(())
}

fn decode_case(
    reader: &mut CanonicalReader<'_>,
) -> Result<StudyCaseDocument, IdentifiabilityError> {
    let id = decode_case_id(reader)?;
    let purpose = match reader.byte("case purpose")? {
        0 => CasePurpose::Calibration,
        1 => CasePurpose::SymmetryBreaking,
        2 => CasePurpose::ValidationOnly,
        3 => CasePurpose::BlindFalsification,
        4 => CasePurpose::ProspectiveDesign,
        5 => CasePurpose::Complementary {
            reason: reader.reason("complementary case reason")?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown case-purpose tag {tag}"),
            });
        }
    };
    let initial_state = decode_initial_state(reader)?;
    let specimen = decode_specimen(reader)?;
    let protocol = decode_protocol(reader)?;
    let forward_model = decode_source_key(reader)?;
    let data = match reader.byte("case data")? {
        0 => CaseDataDeclaration::Prospective,
        1 => CaseDataDeclaration::Retrospective {
            experiment: decode_source_key(reader)?,
            split: decode_source_key(reader)?,
            parser: decode_source_key(reader)?,
            preprocessing: decode_source_key(reader)?,
            parser_version: reader.u32("parser version")?,
            split_grouping: decode_artifact_id(reader)?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown case-data tag {tag}"),
            });
        }
    };
    let observation_count = reader.count("case observations")?;
    let mut observations = Vec::with_capacity(observation_count);
    for _ in 0..observation_count {
        observations.push(decode_study_observation(reader)?);
    }
    let discrepancy_count = reader.count("case discrepancies")?;
    let mut discrepancies = Vec::with_capacity(discrepancy_count);
    for _ in 0..discrepancy_count {
        discrepancies.push((decode_channel(reader)?, decode_discrepancy(reader)?));
    }
    StudyCaseDocument::try_new(
        id,
        purpose,
        initial_state,
        specimen,
        protocol,
        forward_model,
        data,
        observations,
        discrepancies,
    )
}

fn encode_functional(
    writer: &mut CanonicalWriter,
    functional: &DistributionFunctional,
) -> Result<(), IdentifiabilityError> {
    match functional {
        DistributionFunctional::Location { observation } => {
            writer.byte(0);
            encode_observation_key(writer, observation)?;
        }
        DistributionFunctional::LogScale { observation } => {
            writer.byte(1);
            encode_observation_key(writer, observation)?;
        }
        DistributionFunctional::Correlation { left, right } => {
            writer.byte(2);
            encode_observation_key(writer, left)?;
            encode_observation_key(writer, right)?;
        }
        DistributionFunctional::MissingnessLogit { observation } => {
            writer.byte(3);
            encode_observation_key(writer, observation)?;
        }
        DistributionFunctional::CensoringLogit { observation } => {
            writer.byte(4);
            encode_observation_key(writer, observation)?;
        }
    }
    Ok(())
}

fn decode_functional(
    reader: &mut CanonicalReader<'_>,
) -> Result<DistributionFunctional, IdentifiabilityError> {
    match reader.byte("distribution functional")? {
        0 => Ok(DistributionFunctional::Location {
            observation: decode_observation_key(reader)?,
        }),
        1 => Ok(DistributionFunctional::LogScale {
            observation: decode_observation_key(reader)?,
        }),
        2 => Ok(DistributionFunctional::Correlation {
            left: decode_observation_key(reader)?,
            right: decode_observation_key(reader)?,
        }),
        3 => Ok(DistributionFunctional::MissingnessLogit {
            observation: decode_observation_key(reader)?,
        }),
        4 => Ok(DistributionFunctional::CensoringLogit {
            observation: decode_observation_key(reader)?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown distribution-functional tag {tag}"),
        }),
    }
}

fn encode_influence(
    writer: &mut CanonicalWriter,
    influence: &InfluenceDeclaration,
) -> Result<(), IdentifiabilityError> {
    writer.text(influence.id.as_str(), "influence id")?;
    encode_role(writer, &influence.parameter)?;
    encode_functional(writer, &influence.functional)?;
    match &influence.representation {
        InfluenceRepresentation::Direct => writer.byte(0),
        InfluenceRepresentation::StateMediated { state_path } => {
            writer.byte(1);
            encode_source_key(writer, state_path)?;
        }
        InfluenceRepresentation::Composite { operator, inputs } => {
            writer.byte(2);
            encode_source_key(writer, operator)?;
            writer.count(inputs.len(), "composite influence inputs")?;
            for input in inputs {
                writer.text(input.as_str(), "influence id")?;
            }
        }
        InfluenceRepresentation::ExternalDefinition { definition } => {
            writer.byte(3);
            encode_source_key(writer, definition)?;
        }
    }
    Ok(())
}

fn decode_influence(
    reader: &mut CanonicalReader<'_>,
) -> Result<InfluenceDeclaration, IdentifiabilityError> {
    let id = InfluenceId::try_new(reader.token("influence id")?)?;
    let parameter = decode_role(reader)?;
    let functional = decode_functional(reader)?;
    let representation = match reader.byte("influence representation")? {
        0 => InfluenceRepresentation::Direct,
        1 => InfluenceRepresentation::StateMediated {
            state_path: decode_source_key(reader)?,
        },
        2 => {
            let operator = decode_source_key(reader)?;
            let count = reader.count("composite influence inputs")?;
            let mut inputs = BTreeSet::new();
            for _ in 0..count {
                let input = InfluenceId::try_new(reader.token("influence id")?)?;
                if !inputs.insert(input.clone()) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "composite influence input",
                        id: input.to_string(),
                    });
                }
            }
            InfluenceRepresentation::Composite { operator, inputs }
        }
        3 => InfluenceRepresentation::ExternalDefinition {
            definition: decode_source_key(reader)?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown influence-representation tag {tag}"),
            });
        }
    };
    Ok(InfluenceDeclaration::new(
        id,
        parameter,
        functional,
        representation,
    ))
}

fn encode_gauge(
    writer: &mut CanonicalWriter,
    gauge: &GaugeDeclaration,
) -> Result<(), IdentifiabilityError> {
    writer.text(gauge.id.as_str(), "gauge id")?;
    writer.count(gauge.members.len(), "gauge members")?;
    for member in &gauge.members {
        encode_role(writer, member)?;
    }
    encode_source_key(writer, &gauge.action)?;
    match &gauge.kind {
        GaugeKind::Continuous { dimension } => {
            writer.byte(0);
            writer.u32(*dimension);
        }
        GaugeKind::Discrete { group_order } => {
            writer.byte(1);
            writer.u64(*group_order);
        }
        GaugeKind::Mixed {
            continuous_dimension,
            discrete_order,
        } => {
            writer.byte(2);
            writer.u32(*continuous_dimension);
            writer.u64(*discrete_order);
        }
        GaugeKind::Stratified { strata } => {
            writer.byte(3);
            encode_source_key(writer, strata)?;
        }
        GaugeKind::Suspected { reason } => {
            writer.byte(4);
            writer.text(reason, "suspected gauge reason")?;
        }
    }
    match &gauge.handling {
        GaugeHandling::Quotient {
            quotient_map,
            local_sections,
        } => {
            writer.byte(0);
            encode_source_key(writer, quotient_map)?;
            encode_source_key(writer, local_sections)?;
        }
        GaugeHandling::Slice { constraint } => {
            writer.byte(1);
            writer.text(constraint.as_str(), "constraint id")?;
        }
        GaugeHandling::Retained { reason } => {
            writer.byte(2);
            writer.text(reason, "retained gauge reason")?;
        }
        GaugeHandling::Unresolved { reason } => {
            writer.byte(3);
            writer.text(reason, "unresolved gauge reason")?;
        }
    }
    Ok(())
}

fn decode_gauge(
    reader: &mut CanonicalReader<'_>,
) -> Result<GaugeDeclaration, IdentifiabilityError> {
    let id = GaugeClassId::try_new(reader.token("gauge id")?)?;
    let members = decode_role_set(reader, "gauge members")?;
    let action = decode_source_key(reader)?;
    let kind = match reader.byte("gauge kind")? {
        0 => GaugeKind::Continuous {
            dimension: reader.u32("gauge dimension")?,
        },
        1 => GaugeKind::Discrete {
            group_order: reader.u64("gauge group order")?,
        },
        2 => GaugeKind::Mixed {
            continuous_dimension: reader.u32("gauge continuous dimension")?,
            discrete_order: reader.u64("gauge discrete order")?,
        },
        3 => GaugeKind::Stratified {
            strata: decode_source_key(reader)?,
        },
        4 => GaugeKind::Suspected {
            reason: reader.reason("suspected gauge reason")?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown gauge-kind tag {tag}"),
            });
        }
    };
    let handling = match reader.byte("gauge handling")? {
        0 => GaugeHandling::Quotient {
            quotient_map: decode_source_key(reader)?,
            local_sections: decode_source_key(reader)?,
        },
        1 => GaugeHandling::Slice {
            constraint: ConstraintId::try_new(reader.token("constraint id")?)?,
        },
        2 => GaugeHandling::Retained {
            reason: reader.reason("retained gauge reason")?,
        },
        3 => GaugeHandling::Unresolved {
            reason: reader.reason("unresolved gauge reason")?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown gauge-handling tag {tag}"),
            });
        }
    };
    GaugeDeclaration::try_new(id, members, action, kind, handling)
}

fn encode_joint_noise(
    writer: &mut CanonicalWriter,
    noise: &JointNoiseModel,
) -> Result<(), IdentifiabilityError> {
    match noise {
        JointNoiseModel::Independent => writer.byte(0),
        JointNoiseModel::DenseCorrelation {
            order,
            correlation,
            model,
        } => {
            writer.byte(1);
            writer.count(order.len(), "joint-noise order")?;
            for key in order {
                encode_observation_key(writer, key)?;
            }
            writer.count(correlation.dimension(), "correlation dimension")?;
            writer.count(correlation.lower_triangle().len(), "correlation entries")?;
            for value in correlation.lower_triangle() {
                writer.f64(*value);
            }
            encode_source_key(writer, model)?;
        }
        JointNoiseModel::ExternalKernel { model } => {
            writer.byte(2);
            encode_source_key(writer, model)?;
        }
        JointNoiseModel::Unknown { reason } => {
            writer.byte(3);
            writer.text(reason, "unknown joint noise reason")?;
        }
    }
    Ok(())
}

fn decode_joint_noise(
    reader: &mut CanonicalReader<'_>,
) -> Result<JointNoiseModel, IdentifiabilityError> {
    match reader.byte("joint noise")? {
        0 => Ok(JointNoiseModel::Independent),
        1 => {
            let count = reader.count("joint-noise order")?;
            let mut order = Vec::with_capacity(count);
            for _ in 0..count {
                order.push(decode_observation_key(reader)?);
            }
            let dimension = reader.count("correlation dimension")?;
            let expected_entries = dimension
                .checked_mul(dimension.saturating_add(1))
                .and_then(|value| value.checked_div(2))
                .ok_or_else(|| IdentifiabilityError::Cardinality {
                    field: "correlation entries",
                    detail: "matrix entry count overflows address space".to_string(),
                })?;
            let encoded_entries =
                usize::try_from(reader.u32("correlation entries")?).map_err(|_| {
                    IdentifiabilityError::Cardinality {
                        field: "correlation entries",
                        detail: "matrix entry count exceeds address space".to_string(),
                    }
                })?;
            if encoded_entries != expected_entries {
                return Err(IdentifiabilityError::Cardinality {
                    field: "correlation entries",
                    detail: format!(
                        "dimension {dimension} requires exactly {expected_entries} entries, found {encoded_entries}"
                    ),
                });
            }
            let mut lower = Vec::with_capacity(expected_entries);
            for _ in 0..expected_entries {
                lower.push(reader.f64("correlation entry")?);
            }
            let correlation = CovarianceMatrix::try_new(dimension, lower).map_err(|error| {
                IdentifiabilityError::Vv {
                    detail: error.to_string(),
                }
            })?;
            Ok(JointNoiseModel::DenseCorrelation {
                order,
                correlation,
                model: decode_source_key(reader)?,
            })
        }
        2 => Ok(JointNoiseModel::ExternalKernel {
            model: decode_source_key(reader)?,
        }),
        3 => Ok(JointNoiseModel::Unknown {
            reason: reader.reason("unknown joint noise reason")?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown joint-noise tag {tag}"),
        }),
    }
}

fn encode_data_reuse(
    writer: &mut CanonicalWriter,
    policy: &DataReusePolicy,
) -> Result<(), IdentifiabilityError> {
    match policy {
        DataReusePolicy::Disjoint => writer.byte(0),
        DataReusePolicy::Shared { groups } => {
            writer.byte(1);
            writer.count(groups.len(), "data sharing groups")?;
            for group in groups {
                writer.count(group.cases.len(), "sharing-group cases")?;
                for case in &group.cases {
                    encode_case_id(writer, case)?;
                }
                encode_source_key(writer, &group.joint_likelihood)?;
                writer.text(&group.justification, "data sharing justification")?;
            }
        }
    }
    Ok(())
}

fn decode_data_reuse(
    reader: &mut CanonicalReader<'_>,
) -> Result<DataReusePolicy, IdentifiabilityError> {
    match reader.byte("data reuse policy")? {
        0 => Ok(DataReusePolicy::Disjoint),
        1 => {
            let count = reader.count("data sharing groups")?;
            let mut groups = Vec::with_capacity(count);
            for _ in 0..count {
                let case_count = reader.count("sharing-group cases")?;
                let mut cases = BTreeSet::new();
                for _ in 0..case_count {
                    let case = decode_case_id(reader)?;
                    if !cases.insert(case.clone()) {
                        return Err(IdentifiabilityError::Duplicate {
                            field: "sharing-group case",
                            id: case.to_string(),
                        });
                    }
                }
                groups.push(DataSharingGroup::try_new(
                    cases,
                    decode_source_key(reader)?,
                    reader.reason("data sharing justification")?,
                )?);
            }
            Ok(DataReusePolicy::Shared { groups })
        }
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown data-reuse tag {tag}"),
        }),
    }
}

fn encode_problem(
    document: &IdentifiabilityProblemDocument,
) -> Result<Vec<u8>, IdentifiabilityError> {
    check_problem_identity_version(document.schema_version)?;
    let mut writer = CanonicalWriter::new();
    writer.bytes.extend_from_slice(PROBLEM_MAGIC);
    writer.u32(document.schema_version);
    encode_source_key(&mut writer, &document.context_source)?;
    encode_source_key(&mut writer, &document.material_source)?;
    encode_source_key(&mut writer, &document.model_source)?;
    encode_source_key(&mut writer, &document.graph_source)?;
    writer.count(document.sources.len(), "source registry")?;
    for source in document.sources.values() {
        encode_source_ref(&mut writer, source)?;
    }
    writer.count(document.parameters.len(), "study parameters")?;
    for parameter in document.parameters.values() {
        encode_study_parameter(&mut writer, parameter)?;
    }
    writer.count(document.constraints.len(), "joint constraints")?;
    for constraint in document.constraints.values() {
        encode_constraint(&mut writer, constraint)?;
    }
    writer.count(document.cases.len(), "study cases")?;
    for case in document.cases.values() {
        encode_case(&mut writer, case)?;
    }
    writer.count(document.influences.len(), "influence declarations")?;
    for influence in document.influences.values() {
        encode_influence(&mut writer, influence)?;
    }
    writer.count(document.gauges.len(), "gauge declarations")?;
    for gauge in document.gauges.values() {
        encode_gauge(&mut writer, gauge)?;
    }
    encode_joint_noise(&mut writer, &document.joint_noise)?;
    encode_data_reuse(&mut writer, &document.data_reuse)?;
    writer.finish()
}

fn decode_problem(bytes: &[u8]) -> Result<IdentifiabilityProblemDocument, IdentifiabilityError> {
    let mut reader = CanonicalReader::new(bytes)?;
    let magic = reader.take(PROBLEM_MAGIC.len(), "problem magic")?;
    if magic != PROBLEM_MAGIC {
        return Err(IdentifiabilityError::Canonical {
            at: 0,
            detail: "wrong identifiability-problem magic".to_string(),
        });
    }
    let version = reader.u32("problem schema version")?;
    check_problem_identity_version(version)?;
    let context_source = decode_source_key(&mut reader)?;
    let material_source = decode_source_key(&mut reader)?;
    let model_source = decode_source_key(&mut reader)?;
    let graph_source = decode_source_key(&mut reader)?;
    let source_count = reader.count("source registry")?;
    let mut sources = Vec::with_capacity(source_count);
    for _ in 0..source_count {
        sources.push(decode_source_ref(&mut reader)?);
    }
    let parameter_count = reader.count("study parameters")?;
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        parameters.push(decode_study_parameter(&mut reader)?);
    }
    let constraint_count = reader.count("joint constraints")?;
    let mut constraints = Vec::with_capacity(constraint_count);
    for _ in 0..constraint_count {
        constraints.push(decode_constraint(&mut reader)?);
    }
    let case_count = reader.count("study cases")?;
    let mut cases = Vec::with_capacity(case_count);
    for _ in 0..case_count {
        cases.push(decode_case(&mut reader)?);
    }
    let influence_count = reader.count("influence declarations")?;
    let mut influences = Vec::with_capacity(influence_count);
    for _ in 0..influence_count {
        influences.push(decode_influence(&mut reader)?);
    }
    let gauge_count = reader.count("gauge declarations")?;
    let mut gauges = Vec::with_capacity(gauge_count);
    for _ in 0..gauge_count {
        gauges.push(decode_gauge(&mut reader)?);
    }
    let joint_noise = decode_joint_noise(&mut reader)?;
    let data_reuse = decode_data_reuse(&mut reader)?;
    reader.finish()?;
    let document = IdentifiabilityProblemDocument::try_new(
        context_source,
        material_source,
        model_source,
        graph_source,
        sources,
        parameters,
        constraints,
        cases,
        influences,
        gauges,
        joint_noise,
        data_reuse,
    )?;
    if document.canonical_bytes()?.as_slice() != bytes {
        return Err(IdentifiabilityError::Canonical {
            at: 0,
            detail: "non-canonical problem encoding".to_string(),
        });
    }
    Ok(document)
}

fn check_identity_version(declared: u32, supported: u32) -> Result<(), IdentifiabilityError> {
    if declared == supported {
        Ok(())
    } else {
        Err(IdentifiabilityError::UnsupportedSchemaVersion {
            declared,
            supported,
        })
    }
}

/// Fail closed on a stale/future umbrella API generation. Identity transports
/// must use their stage-specific checkers below.
pub fn check_authority_schema_version(declared: u32) -> Result<(), IdentifiabilityError> {
    check_identity_version(declared, IDENTIFIABILITY_AUTHORITY_SCHEMA_VERSION)
}

/// Fail closed on a stale/future physical-problem identity version.
pub fn check_problem_identity_version(declared: u32) -> Result<(), IdentifiabilityError> {
    check_identity_version(declared, IDENTIFIABILITY_PROBLEM_IDENTITY_VERSION)
}

/// Fail closed on a stale/future source-admission identity version.
pub fn check_source_admission_identity_version(declared: u32) -> Result<(), IdentifiabilityError> {
    check_identity_version(declared, IDENTIFIABILITY_SOURCE_ADMISSION_IDENTITY_VERSION)
}

/// Fail closed on a stale/future execution identity version.
pub fn check_execution_identity_version(declared: u32) -> Result<(), IdentifiabilityError> {
    check_identity_version(declared, IDENTIFIABILITY_EXECUTION_IDENTITY_VERSION)
}

/// Fail closed on a stale/future assessment identity version.
pub fn check_assessment_identity_version(declared: u32) -> Result<(), IdentifiabilityError> {
    check_identity_version(declared, IDENTIFIABILITY_ASSESSMENT_IDENTITY_VERSION)
}

fn encode_execution_action(
    writer: &mut CanonicalWriter,
    action: &ParameterExecutionAction,
) -> Result<(), IdentifiabilityError> {
    match action {
        ParameterExecutionAction::Optimize { coordinate } => {
            writer.byte(0);
            encode_coordinate(writer, coordinate)?;
        }
        ParameterExecutionAction::Profile { coordinate } => {
            writer.byte(1);
            encode_coordinate(writer, coordinate)?;
        }
        ParameterExecutionAction::Marginalize {
            coordinate,
            integrator,
        } => {
            writer.byte(2);
            encode_coordinate(writer, coordinate)?;
            encode_source_ref(writer, integrator)?;
        }
        ParameterExecutionAction::Conditioned => writer.byte(3),
        ParameterExecutionAction::Derived => writer.byte(4),
    }
    Ok(())
}

fn decode_execution_action(
    reader: &mut CanonicalReader<'_>,
) -> Result<ParameterExecutionAction, IdentifiabilityError> {
    match reader.byte("execution action")? {
        0 => Ok(ParameterExecutionAction::Optimize {
            coordinate: decode_coordinate(reader)?,
        }),
        1 => Ok(ParameterExecutionAction::Profile {
            coordinate: decode_coordinate(reader)?,
        }),
        2 => Ok(ParameterExecutionAction::Marginalize {
            coordinate: decode_coordinate(reader)?,
            integrator: decode_source_ref(reader)?,
        }),
        3 => Ok(ParameterExecutionAction::Conditioned),
        4 => Ok(ParameterExecutionAction::Derived),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown execution-action tag {tag}"),
        }),
    }
}

fn encode_execution_with_header_mode(
    plan: &IdentifiabilityExecutionPlan,
    exact_header: bool,
) -> Result<Vec<u8>, IdentifiabilityError> {
    check_execution_identity_version(plan.schema_version)?;
    let mut writer = CanonicalWriter::new();
    writer.bytes.extend_from_slice(EXECUTION_MAGIC);
    writer.u32(plan.schema_version);
    encode_header(&mut writer, &plan.header, exact_header)?;
    writer.hash(plan.problem_id.0);
    writer.hash(plan.source_admission_id.0);
    encode_source_ref(&mut writer, &plan.analyzer)?;
    encode_source_ref(&mut writer, &plan.build)?;
    match &plan.derivative_provider {
        Some(source) => {
            writer.byte(1);
            encode_source_ref(&mut writer, source)?;
        }
        None => writer.byte(0),
    }
    writer.count(plan.requested_axes.len(), "requested claim axes")?;
    for axis in &plan.requested_axes {
        writer.byte(match axis {
            RequestedClaimAxis::Structural => 0,
            RequestedClaimAxis::Local => 1,
            RequestedClaimAxis::Generic => 2,
            RequestedClaimAxis::Global => 3,
            RequestedClaimAxis::Practical => 4,
        });
    }
    writer.count(plan.actions.len(), "parameter actions")?;
    for (role, action) in &plan.actions {
        encode_role(&mut writer, role)?;
        encode_execution_action(&mut writer, action)?;
    }
    writer.f64(plan.numerical.rank_tolerance);
    writer.f64(plan.numerical.singular_value_floor);
    writer.f64(plan.numerical.maximum_condition_number);
    writer.byte(match plan.numerical.arithmetic {
        ArithmeticPolicy::ExactSymbolic => 0,
        ArithmeticPolicy::CertifiedInterval => 1,
        ArithmeticPolicy::DeterministicFloatingPoint => 2,
        ArithmeticPolicy::FastFloatingPoint => 3,
    });
    encode_source_ref(&mut writer, &plan.initialization)?;
    encode_source_ref(&mut writer, &plan.stopping)?;
    encode_source_ref(&mut writer, &plan.determinism_contract)?;
    encode_resolution_set(&mut writer, &plan.source_authority)?;
    writer.finish()
}

fn encode_execution(plan: &IdentifiabilityExecutionPlan) -> Result<Vec<u8>, IdentifiabilityError> {
    encode_execution_with_header_mode(plan, true)
}

fn encode_execution_identity(
    plan: &IdentifiabilityExecutionPlan,
) -> Result<Vec<u8>, IdentifiabilityError> {
    encode_execution_with_header_mode(plan, false)
}

fn execution_identity_hash(
    plan: &IdentifiabilityExecutionPlan,
) -> Result<ExecutionId, IdentifiabilityError> {
    Ok(ExecutionId(hash_domain(
        IDENTIFIABILITY_EXECUTION_IDENTITY_DOMAIN,
        &encode_execution_identity(plan)?,
    )))
}

fn decode_execution(
    bytes: &[u8],
    problem: &AdmittedIdentifiabilityProblem,
    verified_sources: &SourceResolutionSet,
) -> Result<IdentifiabilityExecutionPlan, IdentifiabilityError> {
    let mut reader = CanonicalReader::new(bytes)?;
    if reader.take(EXECUTION_MAGIC.len(), "execution magic")? != EXECUTION_MAGIC {
        return Err(IdentifiabilityError::Canonical {
            at: 0,
            detail: "wrong identifiability-execution magic".to_string(),
        });
    }
    check_execution_identity_version(reader.u32("execution schema version")?)?;
    let header = decode_header(&mut reader)?;
    let problem_id = ProblemId(reader.hash("execution problem id")?);
    let source_admission_id = SourceAdmissionId(reader.hash("execution source-admission id")?);
    if problem_id != problem.problem_id || source_admission_id != problem.source_admission_id {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "execution problem/source admission",
        });
    }
    let analyzer = decode_source_ref(&mut reader)?;
    let build = decode_source_ref(&mut reader)?;
    let derivative_provider = match reader.byte("derivative-provider option")? {
        0 => None,
        1 => Some(decode_source_ref(&mut reader)?),
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("invalid derivative-provider option tag {tag}"),
            });
        }
    };
    let axis_count = reader.count("requested claim axes")?;
    let mut requested_axes = BTreeSet::new();
    for _ in 0..axis_count {
        let axis = match reader.byte("requested claim axis")? {
            0 => RequestedClaimAxis::Structural,
            1 => RequestedClaimAxis::Local,
            2 => RequestedClaimAxis::Generic,
            3 => RequestedClaimAxis::Global,
            4 => RequestedClaimAxis::Practical,
            tag => {
                return Err(IdentifiabilityError::Canonical {
                    at: reader.at.saturating_sub(1),
                    detail: format!("unknown requested-axis tag {tag}"),
                });
            }
        };
        if !requested_axes.insert(axis) {
            return Err(IdentifiabilityError::Duplicate {
                field: "requested claim axis",
                id: format!("{axis:?}"),
            });
        }
    }
    let action_count = reader.count("parameter actions")?;
    let mut actions = Vec::with_capacity(action_count);
    for _ in 0..action_count {
        actions.push((
            decode_role(&mut reader)?,
            decode_execution_action(&mut reader)?,
        ));
    }
    let rank_tolerance = reader.f64("rank tolerance")?;
    let singular_value_floor = reader.f64("singular-value floor")?;
    let maximum_condition_number = reader.f64("maximum condition number")?;
    let arithmetic = match reader.byte("arithmetic policy")? {
        0 => ArithmeticPolicy::ExactSymbolic,
        1 => ArithmeticPolicy::CertifiedInterval,
        2 => ArithmeticPolicy::DeterministicFloatingPoint,
        3 => ArithmeticPolicy::FastFloatingPoint,
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown arithmetic-policy tag {tag}"),
            });
        }
    };
    let numerical = IdentifiabilityNumericalPolicy::try_new(
        rank_tolerance,
        singular_value_floor,
        maximum_condition_number,
        arithmetic,
    )?;
    let initialization = decode_source_ref(&mut reader)?;
    let stopping = decode_source_ref(&mut reader)?;
    let determinism_contract = decode_source_ref(&mut reader)?;
    let transported_sources = decode_resolution_set(&mut reader)?;
    reader.finish()?;
    if transported_sources != *verified_sources {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "execution source-resolution replay",
        });
    }
    let plan = IdentifiabilityExecutionPlan::try_new(
        header,
        problem,
        analyzer,
        build,
        derivative_provider,
        requested_axes,
        actions,
        numerical,
        initialization,
        stopping,
        determinism_contract,
        verified_sources.clone(),
    )?;
    if plan.canonical_bytes()?.as_slice() != bytes {
        return Err(IdentifiabilityError::Canonical {
            at: 0,
            detail: "non-canonical execution encoding".to_string(),
        });
    }
    Ok(plan)
}

fn encode_claim(
    writer: &mut CanonicalWriter,
    claim: &TypedIdentifiabilityClaim,
) -> Result<(), IdentifiabilityError> {
    writer.text(claim.id.as_str(), "claim id")?;
    writer.byte(match claim.information {
        InformationRegime::StructuralExactModel => 0,
        InformationRegime::ExactInputOutputMap => 1,
        InformationRegime::NoisyFiniteData => 2,
        InformationRegime::PosteriorUnderDeclaredPrior => 3,
    });
    writer.byte(match claim.extent {
        IdentifiabilityExtent::Local => 0,
        IdentifiabilityExtent::Global => 1,
        IdentifiabilityExtent::SetValued => 2,
    });
    match &claim.quantifier {
        ClaimQuantifier::AtRealization { realization } => {
            writer.byte(0);
            encode_source_ref(writer, realization)?;
        }
        ClaimQuantifier::AlmostEverywhere { measure } => {
            writer.byte(1);
            encode_source_ref(writer, measure)?;
        }
        ClaimQuantifier::ForAll { domain } => {
            writer.byte(2);
            encode_source_ref(writer, domain)?;
        }
        ClaimQuantifier::ProbabilityAtLeast {
            probability,
            measure,
        } => {
            writer.byte(3);
            writer.f64(*probability);
            encode_source_ref(writer, measure)?;
        }
    }
    writer.byte(match claim.scalar_domain {
        ScalarDomain::Real => 0,
        ScalarDomain::Complex => 1,
        ScalarDomain::MixedDiscreteContinuous => 2,
    });
    match &claim.subject {
        ClaimSubject::Parameter(role) => {
            writer.byte(0);
            encode_role(writer, role)?;
        }
        ClaimSubject::ParameterTuple(roles) => {
            writer.byte(1);
            writer.count(roles.len(), "claim parameter tuple")?;
            for role in roles {
                encode_role(writer, role)?;
            }
        }
        ClaimSubject::Influence(influence) => {
            writer.byte(2);
            writer.text(influence.as_str(), "influence id")?;
        }
        ClaimSubject::GaugeClass(gauge) => {
            writer.byte(3);
            writer.text(gauge.as_str(), "gauge id")?;
        }
        ClaimSubject::WholeProblem => writer.byte(4),
    }
    match &claim.scope {
        ClaimScope::WholeCampaign => writer.byte(0),
        ClaimScope::Cases(cases) => {
            writer.byte(1);
            writer.count(cases.len(), "claim case scope")?;
            for case in cases {
                encode_case_id(writer, case)?;
            }
        }
        ClaimScope::Stratum { definition } => {
            writer.byte(2);
            encode_source_key(writer, definition)?;
        }
    }
    Ok(())
}

fn decode_claim(
    reader: &mut CanonicalReader<'_>,
) -> Result<TypedIdentifiabilityClaim, IdentifiabilityError> {
    let id = ClaimId::try_new(reader.token("claim id")?)?;
    let information = match reader.byte("claim information regime")? {
        0 => InformationRegime::StructuralExactModel,
        1 => InformationRegime::ExactInputOutputMap,
        2 => InformationRegime::NoisyFiniteData,
        3 => InformationRegime::PosteriorUnderDeclaredPrior,
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown information-regime tag {tag}"),
            });
        }
    };
    let extent = match reader.byte("claim extent")? {
        0 => IdentifiabilityExtent::Local,
        1 => IdentifiabilityExtent::Global,
        2 => IdentifiabilityExtent::SetValued,
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown claim-extent tag {tag}"),
            });
        }
    };
    let quantifier = match reader.byte("claim quantifier")? {
        0 => ClaimQuantifier::AtRealization {
            realization: decode_source_ref(reader)?,
        },
        1 => ClaimQuantifier::AlmostEverywhere {
            measure: decode_source_ref(reader)?,
        },
        2 => ClaimQuantifier::ForAll {
            domain: decode_source_ref(reader)?,
        },
        3 => ClaimQuantifier::ProbabilityAtLeast {
            probability: reader.f64("claim probability")?,
            measure: decode_source_ref(reader)?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown claim-quantifier tag {tag}"),
            });
        }
    };
    let scalar_domain = match reader.byte("claim scalar domain")? {
        0 => ScalarDomain::Real,
        1 => ScalarDomain::Complex,
        2 => ScalarDomain::MixedDiscreteContinuous,
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown scalar-domain tag {tag}"),
            });
        }
    };
    let subject = match reader.byte("claim subject")? {
        0 => ClaimSubject::Parameter(decode_role(reader)?),
        1 => ClaimSubject::ParameterTuple(decode_role_set(reader, "claim parameter tuple")?),
        2 => ClaimSubject::Influence(InfluenceId::try_new(reader.token("influence id")?)?),
        3 => ClaimSubject::GaugeClass(GaugeClassId::try_new(reader.token("gauge id")?)?),
        4 => ClaimSubject::WholeProblem,
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown claim-subject tag {tag}"),
            });
        }
    };
    let scope = match reader.byte("claim scope")? {
        0 => ClaimScope::WholeCampaign,
        1 => {
            let count = reader.count("claim case scope")?;
            let mut cases = BTreeSet::new();
            for _ in 0..count {
                let case = decode_case_id(reader)?;
                if !cases.insert(case.clone()) {
                    return Err(IdentifiabilityError::Duplicate {
                        field: "claim case scope",
                        id: case.to_string(),
                    });
                }
            }
            ClaimScope::Cases(cases)
        }
        2 => ClaimScope::Stratum {
            definition: decode_source_key(reader)?,
        },
        tag => {
            return Err(IdentifiabilityError::Canonical {
                at: reader.at.saturating_sub(1),
                detail: format!("unknown claim-scope tag {tag}"),
            });
        }
    };
    Ok(TypedIdentifiabilityClaim::new(
        id,
        information,
        extent,
        quantifier,
        scalar_domain,
        subject,
        scope,
    ))
}

fn encode_claim_assessment(
    writer: &mut CanonicalWriter,
    assessment: &ClaimAssessment,
) -> Result<(), IdentifiabilityError> {
    match assessment {
        ClaimAssessment::ClaimedEstablished {
            method,
            receipt,
            tolerance,
        } => {
            writer.byte(0);
            encode_source_ref(writer, method)?;
            encode_source_ref(writer, receipt)?;
            writer.f64(*tolerance);
        }
        ClaimAssessment::ClaimedRefuted {
            method,
            receipt,
            tolerance,
        } => {
            writer.byte(1);
            encode_source_ref(writer, method)?;
            encode_source_ref(writer, receipt)?;
            writer.f64(*tolerance);
        }
        ClaimAssessment::ClaimedInconclusive {
            method,
            receipt,
            reason,
        } => {
            writer.byte(2);
            match method {
                Some(source) => {
                    writer.byte(1);
                    encode_source_ref(writer, source)?;
                }
                None => writer.byte(0),
            }
            match receipt {
                Some(source) => {
                    writer.byte(1);
                    encode_source_ref(writer, source)?;
                }
                None => writer.byte(0),
            }
            writer.text(reason, "inconclusive reason")?;
        }
        ClaimAssessment::NotAssessed { reason } => {
            writer.byte(3);
            writer.text(reason, "not-assessed reason")?;
        }
    }
    Ok(())
}

fn decode_optional_source_ref(
    reader: &mut CanonicalReader<'_>,
    field: &'static str,
) -> Result<Option<SourceRef>, IdentifiabilityError> {
    match reader.byte(field)? {
        0 => Ok(None),
        1 => Ok(Some(decode_source_ref(reader)?)),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("invalid {field} tag {tag}"),
        }),
    }
}

fn decode_claim_assessment(
    reader: &mut CanonicalReader<'_>,
) -> Result<ClaimAssessment, IdentifiabilityError> {
    match reader.byte("claim assessment")? {
        0 => Ok(ClaimAssessment::ClaimedEstablished {
            method: decode_source_ref(reader)?,
            receipt: decode_source_ref(reader)?,
            tolerance: reader.f64("claim tolerance")?,
        }),
        1 => Ok(ClaimAssessment::ClaimedRefuted {
            method: decode_source_ref(reader)?,
            receipt: decode_source_ref(reader)?,
            tolerance: reader.f64("claim tolerance")?,
        }),
        2 => Ok(ClaimAssessment::ClaimedInconclusive {
            method: decode_optional_source_ref(reader, "inconclusive method option")?,
            receipt: decode_optional_source_ref(reader, "inconclusive receipt option")?,
            reason: reader.reason("inconclusive reason")?,
        }),
        3 => Ok(ClaimAssessment::NotAssessed {
            reason: reader.reason("not-assessed reason")?,
        }),
        tag => Err(IdentifiabilityError::Canonical {
            at: reader.at.saturating_sub(1),
            detail: format!("unknown claim-assessment tag {tag}"),
        }),
    }
}

fn encode_assessment_with_header_mode(
    assessment: &IdentifiabilityAssessment,
    exact_header: bool,
) -> Result<Vec<u8>, IdentifiabilityError> {
    check_assessment_identity_version(assessment.schema_version)?;
    let mut writer = CanonicalWriter::new();
    writer.bytes.extend_from_slice(ASSESSMENT_MAGIC);
    writer.u32(assessment.schema_version);
    encode_header(&mut writer, &assessment.header, exact_header)?;
    writer.hash(assessment.problem_id.0);
    writer.hash(assessment.execution_id.0);
    writer.count(assessment.claims.len(), "identifiability claims")?;
    for claim in assessment.claims.values() {
        encode_claim(&mut writer, claim)?;
    }
    writer.count(assessment.evidence.len(), "claim assessments")?;
    for (id, conclusion) in &assessment.evidence {
        writer.text(id.as_str(), "claim id")?;
        encode_claim_assessment(&mut writer, conclusion)?;
    }
    encode_resolution_set(&mut writer, &assessment.source_authority)?;
    writer.finish()
}

fn encode_assessment(
    assessment: &IdentifiabilityAssessment,
) -> Result<Vec<u8>, IdentifiabilityError> {
    encode_assessment_with_header_mode(assessment, true)
}

fn encode_assessment_identity(
    assessment: &IdentifiabilityAssessment,
) -> Result<Vec<u8>, IdentifiabilityError> {
    encode_assessment_with_header_mode(assessment, false)
}

fn assessment_identity_hash(
    assessment: &IdentifiabilityAssessment,
) -> Result<AssessmentId, IdentifiabilityError> {
    Ok(AssessmentId(hash_domain(
        IDENTIFIABILITY_ASSESSMENT_IDENTITY_DOMAIN,
        &encode_assessment_identity(assessment)?,
    )))
}

fn decode_assessment(
    bytes: &[u8],
    problem: &AdmittedIdentifiabilityProblem,
    execution: &IdentifiabilityExecutionPlan,
    verified_sources: &SourceResolutionSet,
) -> Result<IdentifiabilityAssessment, IdentifiabilityError> {
    let mut reader = CanonicalReader::new(bytes)?;
    if reader.take(ASSESSMENT_MAGIC.len(), "assessment magic")? != ASSESSMENT_MAGIC {
        return Err(IdentifiabilityError::Canonical {
            at: 0,
            detail: "wrong identifiability-assessment magic".to_string(),
        });
    }
    check_assessment_identity_version(reader.u32("assessment schema version")?)?;
    let header = decode_header(&mut reader)?;
    let problem_id = ProblemId(reader.hash("assessment problem id")?);
    let execution_id = ExecutionId(reader.hash("assessment execution id")?);
    if problem_id != problem.problem_id || execution_id != execution.id()? {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "assessment problem/execution identity",
        });
    }
    let claim_count = reader.count("identifiability claims")?;
    let mut claims = Vec::with_capacity(claim_count);
    for _ in 0..claim_count {
        claims.push(decode_claim(&mut reader)?);
    }
    let evidence_count = reader.count("claim assessments")?;
    let mut evidence = Vec::with_capacity(evidence_count);
    for _ in 0..evidence_count {
        evidence.push((
            ClaimId::try_new(reader.token("claim id")?)?,
            decode_claim_assessment(&mut reader)?,
        ));
    }
    // Resolution evidence is retained in transport for identity/replay, but it
    // is not itself proof. Compare it to a caller-held, locally verified set;
    // never pass deserialized verification markers into the admitting
    // constructor.
    let transported_sources = decode_resolution_set(&mut reader)?;
    reader.finish()?;
    if transported_sources != *verified_sources {
        return Err(IdentifiabilityError::SourceMismatch {
            field: "assessment source-resolution replay",
        });
    }
    let assessment = IdentifiabilityAssessment::try_new(
        header,
        problem,
        execution,
        claims,
        evidence,
        verified_sources.clone(),
    )?;
    if assessment.canonical_bytes()?.as_slice() != bytes {
        return Err(IdentifiabilityError::Canonical {
            at: 0,
            detail: "non-canonical assessment encoding".to_string(),
        });
    }
    Ok(assessment)
}
