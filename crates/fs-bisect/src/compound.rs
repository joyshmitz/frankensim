//! compound — the Gauntlet failure-compounding workflow (bead 6nb.9).
//!
//! Every golden break, falsifier hit, guard failure, or property
//! counterexample should STRENGTHEN the permanent test surface instead of
//! being fixed and forgotten. This module is the v1 mechanism:
//!
//! 1. **Capture** the failure as a [`FailureCase`]: seed, typed input, the
//!    violated [`InvariantClass`], and the contract surface it broke.
//! 2. **Minimize** it ([`minimize`]): deterministic greedy descent through
//!    [`Shrink`] candidates, keeping the invariant violated at every step.
//!    A non-failing input is a typed refusal, never a silent no-op.
//! 3. **Probe the neighborhood** ([`probe_neighborhood`]): bounded, uniquely
//!    labeled perturbations around the minimum expose whether the failure is
//!    a point or a region.
//! 4. **Land a family** ([`RegressionFamily`]): the minimum plus its failing
//!    neighbors, with tracking-issue references and a recommended admission
//!    rule when the class is general.
//! 5. **Replay** ([`RegressionFamily::replay`]): the family is
//!    content-addressed ([`Canon`] bytes → domain-separated BLAKE3) and
//!    re-executable — a
//!    member that stops failing is REPORTED, because a regression family
//!    whose members silently pass is stale evidence.
//!
//! Everything is plain data and deterministic: same case + same predicate
//! ⇒ bitwise-identical minimum, probes, manifest, and content hash, on
//! every ISA and in every build mode (the canon encoding is integer bytes
//! and `f64::to_bits`, never formatted floats).
//!
//! What this module does NOT do (no-claims): it does not write to the
//! ledger or emit fs-obs events (recorded follow-up once the huq.16 schema
//! lands), and it does not itself change admission rules — the family
//! CARRIES the recommendation (as check-powi was born from the powi
//! incident); enacting it is the responding agent's task.

/// Semantic version of the canon encoding + content-hash assembly
/// (golden-couplings surface `fs-bisect:compound-canon`). Changing the
/// [`Canon`] byte layout, the tag values, the hash domain, or the
/// field order in [`RegressionFamily::content_hash`] changes every
/// family hash — bump this and deliberately re-freeze the dependents
/// listed in golden-couplings.json (docs/GOLDEN_POLICY.md).
pub const COMPOUND_CANON_VERSION: u32 = 2;

/// Domain separating regression-family identities from every other BLAKE3 use.
pub const COMPOUND_FAMILY_HASH_DOMAIN: &str = "org.frankensim.fs-bisect.compound-family.v2";

/// Maximum accepted minimizer descent steps.
pub const MAX_MINIMIZE_STEPS: usize = 65_536;
/// Maximum shrink candidates returned for one descent step.
pub const MAX_SHRINK_CANDIDATES_PER_STEP: usize = 4_096;
/// Maximum predicate evaluations across one minimization.
pub const MAX_MINIMIZE_EVALUATIONS: usize = 1_000_000;
/// Maximum neighboring inputs evaluated and retained for one family.
pub const MAX_NEIGHBOR_PROBES: usize = 4_096;
/// Maximum tracking references attached to one regression family.
pub const MAX_TRACKING_REFS: usize = 64;
/// Maximum bytes in a case/family/member/tracking identifier.
pub const MAX_IDENTIFIER_BYTES: usize = 256;
/// Maximum bytes in a contract or admission-rule description.
pub const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
/// Maximum canonical payload bytes retained for one regression member.
pub const MAX_CANONICAL_MEMBER_BYTES: usize = 1024 * 1024;
/// Maximum canonical payload bytes retained across one family.
pub const MAX_CANONICAL_FAMILY_BYTES: usize = 16 * 1024 * 1024;

const RESERVED_INVARIANT_NAMES: [&str; 7] = [
    "build-mode-determinism",
    "cross-isa-determinism",
    "golden-drift",
    "enclosure-violation",
    "certificate-forgery",
    "conservation-violation",
    "adjoint-inconsistency",
];

fn visible_identifier(value: &str) -> bool {
    value.bytes().all(|byte| (b'!'..=b'~').contains(&byte))
}

/// The invariant a failure violated — the classification axis that decides
/// which sibling surfaces the lesson propagates to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantClass {
    /// Bits differ between debug and release builds (e.g. the `f64::powi`
    /// incident, bead 4xnt).
    BuildModeDeterminism,
    /// Bits differ across ISAs where the contract claims they must not.
    CrossIsaDeterminism,
    /// A frozen golden hash no longer matches the observed bits.
    GoldenDrift,
    /// A certified enclosure excludes the true value.
    EnclosureViolation,
    /// A certificate accepted something its falsifier refutes.
    CertificateForgery,
    /// A conserved quantity drifted beyond its stated band.
    ConservationViolation,
    /// A gradient/adjoint disagrees with its independent check.
    AdjointInconsistency,
    /// Anything else — named, never silent.
    Other(String),
}

impl InvariantClass {
    /// Stable name for manifests and hashes.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            InvariantClass::BuildModeDeterminism => "build-mode-determinism",
            InvariantClass::CrossIsaDeterminism => "cross-isa-determinism",
            InvariantClass::GoldenDrift => "golden-drift",
            InvariantClass::EnclosureViolation => "enclosure-violation",
            InvariantClass::CertificateForgery => "certificate-forgery",
            InvariantClass::ConservationViolation => "conservation-violation",
            InvariantClass::AdjointInconsistency => "adjoint-inconsistency",
            InvariantClass::Other(s) => s,
        }
    }

    fn validate(&self) -> Result<(), CompoundError> {
        let Self::Other(name) = self else {
            return Ok(());
        };
        validate_identifier("invariant", name)?;
        if RESERVED_INVARIANT_NAMES.contains(&name.as_str()) {
            return Err(CompoundError::InvalidField {
                field: "invariant",
                problem: format!(
                    "custom invariant name {name:?} is reserved by a built-in variant"
                ),
            });
        }
        Ok(())
    }
}

/// A captured failure: everything needed to reproduce it deterministically.
#[derive(Debug, Clone)]
pub struct FailureCase<I> {
    /// Stable identifier (used in manifests and issue references).
    pub id: String,
    /// The seed that produced the input (0 when the input is explicit).
    pub seed: u64,
    /// The failing input itself.
    pub input: I,
    /// Which invariant broke.
    pub invariant: InvariantClass,
    /// The contract surface that broke, as `crate::surface` prose.
    pub contract: String,
    /// One-line human detail (observed vs expected).
    pub detail: String,
}

/// Immutable provenance bound into a regression family's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyProvenance {
    seed: u64,
    contract: String,
    detail: String,
}

impl FamilyProvenance {
    /// Construct bounded provenance for one captured failure.
    ///
    /// # Errors
    /// Empty or oversized contract/detail descriptions are refused.
    pub fn new(seed: u64, contract: String, detail: String) -> Result<Self, CompoundError> {
        validate_description("contract", &contract)?;
        validate_description("detail", &detail)?;
        Ok(Self {
            seed,
            contract,
            detail,
        })
    }

    /// Reproduction seed (`0` when the member was explicit).
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// Contract surface that was violated.
    #[must_use]
    pub fn contract(&self) -> &str {
        &self.contract
    }

    /// Captured expected/observed diagnosis.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Typed refusals — the workflow never silently does nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompoundError {
    /// The captured input does not fail the predicate: there is nothing to
    /// minimize, and pretending otherwise would produce a fake regression.
    NotFailing {
        /// The case id that was expected to fail.
        id: String,
    },
    /// A caller-controlled field violates the canonical family envelope.
    InvalidField {
        /// Stable field name.
        field: &'static str,
        /// Actionable diagnosis.
        problem: String,
    },
    /// A deterministic work or collection bound was exceeded.
    LimitExceeded {
        /// Bounded resource.
        resource: &'static str,
        /// Requested or observed value.
        requested: usize,
        /// Admitted maximum.
        max: usize,
    },
    /// Two labels or tracking references would make the manifest ambiguous.
    DuplicateIdentity {
        /// Collection containing the duplicate.
        field: &'static str,
        /// Repeated value.
        value: String,
    },
}

impl core::fmt::Display for CompoundError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFailing { id } => {
                write!(f, "captured case {id:?} does not fail its predicate")
            }
            Self::InvalidField { field, problem } => {
                write!(f, "invalid failure-family field {field}: {problem}")
            }
            Self::LimitExceeded {
                resource,
                requested,
                max,
            } => write!(
                f,
                "failure compounding {resource} request {requested} exceeds limit {max}"
            ),
            Self::DuplicateIdentity { field, value } => {
                write!(f, "duplicate {field} identity {value:?}")
            }
        }
    }
}

impl std::error::Error for CompoundError {}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), CompoundError> {
    if value.is_empty() {
        return Err(CompoundError::InvalidField {
            field,
            problem: "must not be empty".to_string(),
        });
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(CompoundError::LimitExceeded {
            resource: field,
            requested: value.len(),
            max: MAX_IDENTIFIER_BYTES,
        });
    }
    if !visible_identifier(value) {
        return Err(CompoundError::InvalidField {
            field,
            problem: "must contain visible ASCII bytes only".to_string(),
        });
    }
    Ok(())
}

fn validate_family_name(value: &str) -> Result<(), CompoundError> {
    validate_identifier("case_id", value)?;
    let mut bytes = value.bytes();
    let first = bytes.next().expect("validated non-empty");
    let last = value.as_bytes()[value.len() - 1];
    let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    if !alphanumeric(first)
        || !alphanumeric(last)
        || !value.bytes().all(|byte| alphanumeric(byte) || byte == b'-')
    {
        return Err(CompoundError::InvalidField {
            field: "case_id",
            problem: "must be lowercase kebab-case (ASCII letters/digits separated by '-')"
                .to_string(),
        });
    }
    Ok(())
}

fn validate_description(field: &'static str, value: &str) -> Result<(), CompoundError> {
    if value.trim().is_empty() {
        return Err(CompoundError::InvalidField {
            field,
            problem: "must not be empty or whitespace-only".to_string(),
        });
    }
    if value.len() > MAX_DESCRIPTION_BYTES {
        return Err(CompoundError::LimitExceeded {
            resource: field,
            requested: value.len(),
            max: MAX_DESCRIPTION_BYTES,
        });
    }
    Ok(())
}

fn validate_tracking_refs(tracking: &[String]) -> Result<(), CompoundError> {
    if tracking.is_empty() {
        return Err(CompoundError::InvalidField {
            field: "tracking",
            problem: "at least one Beads or issue reference is required".to_string(),
        });
    }
    if tracking.len() > MAX_TRACKING_REFS {
        return Err(CompoundError::LimitExceeded {
            resource: "tracking_refs",
            requested: tracking.len(),
            max: MAX_TRACKING_REFS,
        });
    }
    let mut tracking_refs = std::collections::BTreeSet::new();
    for reference in tracking {
        validate_identifier("tracking_ref", reference)?;
        if !tracking_refs.insert(reference.as_str()) {
            return Err(CompoundError::DuplicateIdentity {
                field: "tracking_ref",
                value: reference.clone(),
            });
        }
    }
    Ok(())
}

fn validate_admission_rule(rule: Option<&str>) -> Result<(), CompoundError> {
    if let Some(rule) = rule {
        validate_description("recommended_admission", rule)?;
    }
    Ok(())
}

/// Deterministic shrinking: candidates strictly "smaller" than `self`, in a
/// FIXED order. An empty vector means fully shrunk.
pub trait Shrink: Clone {
    /// Smaller candidate inputs, most aggressive first (convention).
    fn shrink_candidates(&self) -> Vec<Self>;
}

/// The result of [`minimize`].
#[derive(Debug, Clone)]
pub struct MinimizeReport<I> {
    /// The smallest input found that still fails.
    pub minimized: I,
    /// Accepted shrink steps.
    pub steps: usize,
    /// Total candidates evaluated.
    pub tried: usize,
    /// False when the step budget ran out before a fixpoint — the minimum
    /// is honest but possibly not minimal.
    pub converged: bool,
}

/// Greedy deterministic minimization: repeatedly take the FIRST failing
/// shrink candidate until none fails (fixpoint) or `max_steps` accepted
/// steps. Same input + same predicate ⇒ identical trajectory.
///
/// # Errors
/// [`CompoundError::NotFailing`] when `input` does not fail `fails`.
pub fn minimize<I: Shrink>(
    id: &str,
    input: &I,
    fails: &dyn Fn(&I) -> bool,
    max_steps: usize,
) -> Result<MinimizeReport<I>, CompoundError> {
    validate_family_name(id)?;
    if max_steps > MAX_MINIMIZE_STEPS {
        return Err(CompoundError::LimitExceeded {
            resource: "minimize_steps",
            requested: max_steps,
            max: MAX_MINIMIZE_STEPS,
        });
    }
    if !fails(input) {
        return Err(CompoundError::NotFailing { id: id.to_string() });
    }
    let mut current = input.clone();
    let mut steps = 0usize;
    let mut tried = 0usize;
    let mut converged = false;
    'outer: loop {
        if steps == max_steps {
            break;
        }
        let candidates = current.shrink_candidates();
        if candidates.len() > MAX_SHRINK_CANDIDATES_PER_STEP {
            return Err(CompoundError::LimitExceeded {
                resource: "shrink_candidates_per_step",
                requested: candidates.len(),
                max: MAX_SHRINK_CANDIDATES_PER_STEP,
            });
        }
        for cand in candidates {
            if tried == MAX_MINIMIZE_EVALUATIONS {
                return Err(CompoundError::LimitExceeded {
                    resource: "minimize_evaluations",
                    requested: tried.saturating_add(1),
                    max: MAX_MINIMIZE_EVALUATIONS,
                });
            }
            tried = tried.checked_add(1).ok_or(CompoundError::LimitExceeded {
                resource: "minimize_evaluations",
                requested: usize::MAX,
                max: MAX_MINIMIZE_EVALUATIONS,
            })?;
            if fails(&cand) {
                current = cand;
                steps += 1;
                continue 'outer;
            }
        }
        converged = true;
        break;
    }
    Ok(MinimizeReport {
        minimized: current,
        steps,
        tried,
        converged,
    })
}

/// One labeled neighborhood probe outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborProbe {
    /// The caller's label for this neighbor (e.g. `"k=5"`).
    pub label: String,
    /// Whether the invariant is violated there too.
    pub fails: bool,
}

/// The bounded neighborhood around a minimized failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborhoodReport {
    /// Every probe, in the caller's (deterministic) order.
    pub probes: Vec<NeighborProbe>,
    /// How many neighbors also fail (region vs point evidence).
    pub failing: usize,
}

/// Evaluate a bounded, labeled set of neighbors of a minimized failure.
/// The caller supplies the neighbors; this function validates the hard count
/// cap and unique canonical labels before making any predicate call. Its order
/// is the caller's order.
pub fn probe_neighborhood<I>(
    neighbors: &[(String, I)],
    fails: &dyn Fn(&I) -> bool,
) -> Result<NeighborhoodReport, CompoundError> {
    if neighbors.len() > MAX_NEIGHBOR_PROBES {
        return Err(CompoundError::LimitExceeded {
            resource: "neighbor_probes",
            requested: neighbors.len(),
            max: MAX_NEIGHBOR_PROBES,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for (label, _) in neighbors {
        validate_identifier("neighbor_label", label)?;
        if !seen.insert(label.as_str()) {
            return Err(CompoundError::DuplicateIdentity {
                field: "neighbor_label",
                value: label.clone(),
            });
        }
    }
    let probes: Vec<NeighborProbe> = neighbors
        .iter()
        .map(|(label, input)| NeighborProbe {
            label: label.clone(),
            fails: fails(input),
        })
        .collect();
    let failing = probes.iter().filter(|p| p.fails).count();
    Ok(NeighborhoodReport { probes, failing })
}

/// Canonical bytes for content addressing. Tagged and length-prefixed so
/// distinct structures cannot collide by concatenation; floats canonicalize
/// through `to_bits`, never through formatting.
pub trait Canon {
    /// Append this value's canonical bytes.
    fn canon(&self, out: &mut Vec<u8>);
}

impl Canon for u64 {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(1);
        out.extend_from_slice(&self.to_le_bytes());
    }
}
impl Canon for u32 {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(11);
        out.extend_from_slice(&self.to_le_bytes());
    }
}
impl Canon for i64 {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(2);
        out.extend_from_slice(&self.to_le_bytes());
    }
}
impl Canon for i32 {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(3);
        out.extend_from_slice(&self.to_le_bytes());
    }
}
impl Canon for f64 {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(4);
        out.extend_from_slice(&self.to_bits().to_le_bytes());
    }
}
impl Canon for bool {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(5);
        out.push(u8::from(*self));
    }
}
impl Canon for str {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(6);
        out.extend_from_slice(&(self.len() as u64).to_le_bytes());
        out.extend_from_slice(self.as_bytes());
    }
}
impl Canon for String {
    fn canon(&self, out: &mut Vec<u8>) {
        self.as_str().canon(out);
    }
}
impl<T: Canon> Canon for Vec<T> {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(7);
        out.extend_from_slice(&(self.len() as u64).to_le_bytes());
        for item in self {
            item.canon(out);
        }
    }
}
impl<A: Canon, B: Canon> Canon for (A, B) {
    fn canon(&self, out: &mut Vec<u8>) {
        out.push(8);
        self.0.canon(out);
        self.1.canon(out);
    }
}

impl Canon for InvariantClass {
    fn canon(&self, out: &mut Vec<u8>) {
        match self {
            Self::BuildModeDeterminism => out.push(0),
            Self::CrossIsaDeterminism => out.push(1),
            Self::GoldenDrift => out.push(2),
            Self::EnclosureViolation => out.push(3),
            Self::CertificateForgery => out.push(4),
            Self::ConservationViolation => out.push(5),
            Self::AdjointInconsistency => out.push(6),
            Self::Other(name) => {
                out.push(7);
                name.canon(out);
            }
        }
    }
}

/// A permanent regression family: the minimized case plus its failing
/// neighbors, with provenance. This is the artifact a failure leaves
/// behind — MORE than one example, linked to its tracking issue, carrying
/// the admission-rule lesson when one generalizes.
#[derive(Debug, Clone)]
pub struct RegressionFamily<I> {
    /// Family name (stable, kebab-case).
    name: String,
    /// The invariant every member violates.
    invariant: InvariantClass,
    /// Reproduction seed and violated contract context.
    provenance: FamilyProvenance,
    /// Labeled members; `members[0]` is the minimized case by convention.
    members: Vec<(String, I)>,
    /// Construction-time canonical snapshots paired one-for-one with members.
    /// Hashes and manifests never re-run a stateful caller implementation.
    member_canon: Vec<Vec<u8>>,
    /// Tracking references (bead ids / issue ids) — never empty for a
    /// landed family; a failure without a paper trail cannot compound.
    tracking: Vec<String>,
    /// The generalized lesson, when there is one (e.g. "lint variable-
    /// exponent powi out of deterministic paths").
    recommended_admission: Option<String>,
}

/// A replayed family: which members still fail (live) and which now pass
/// (stale — the regression they pinned was fixed or the predicate moved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    /// Labels of members that still violate the invariant.
    pub still_failing: Vec<String>,
    /// Labels of members that no longer violate it.
    pub now_passing: Vec<String>,
}

fn write_json_string(out: &mut String, value: &str) {
    use std::fmt::Write as _;

    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            control if control <= '\u{1f}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            ordinary => out.push(ordinary),
        }
    }
    out.push('"');
}

impl<I: Canon> RegressionFamily<I> {
    /// Build a canonical, bounded regression family.
    ///
    /// # Errors
    /// Refuses empty/duplicate/oversized identifiers, an empty tracking set,
    /// an empty member set, or a malformed custom invariant.
    pub fn new(
        name: String,
        invariant: InvariantClass,
        members: Vec<(String, I)>,
        tracking: Vec<String>,
        recommended_admission: Option<String>,
        provenance: FamilyProvenance,
    ) -> Result<Self, CompoundError> {
        validate_family_name(&name)?;
        invariant.validate()?;
        validate_tracking_refs(&tracking)?;
        validate_admission_rule(recommended_admission.as_deref())?;
        if members.is_empty() {
            return Err(CompoundError::InvalidField {
                field: "members",
                problem: "a regression family must retain at least its minimized case".to_string(),
            });
        }
        let max_members = MAX_NEIGHBOR_PROBES + 1;
        if members.len() > max_members {
            return Err(CompoundError::LimitExceeded {
                resource: "family_members",
                requested: members.len(),
                max: max_members,
            });
        }
        if members[0].0 != "minimized" {
            return Err(CompoundError::InvalidField {
                field: "members",
                problem: "the first member must be labeled \"minimized\"".to_string(),
            });
        }
        let mut member_labels = std::collections::BTreeSet::new();
        let mut member_canon = Vec::with_capacity(members.len());
        let mut canonical_family_bytes = 0usize;
        for (label, input) in &members {
            validate_identifier("member_label", label)?;
            if !member_labels.insert(label.as_str()) {
                return Err(CompoundError::DuplicateIdentity {
                    field: "member_label",
                    value: label.clone(),
                });
            }
            let mut canonical = Vec::new();
            input.canon(&mut canonical);
            if canonical.is_empty() {
                return Err(CompoundError::InvalidField {
                    field: "member_canon",
                    problem: "Canon implementations must emit a non-empty tagged value".to_string(),
                });
            }
            if canonical.len() > MAX_CANONICAL_MEMBER_BYTES {
                return Err(CompoundError::LimitExceeded {
                    resource: "canonical_member_bytes",
                    requested: canonical.len(),
                    max: MAX_CANONICAL_MEMBER_BYTES,
                });
            }
            canonical_family_bytes = canonical_family_bytes.checked_add(canonical.len()).ok_or(
                CompoundError::LimitExceeded {
                    resource: "canonical_family_bytes",
                    requested: usize::MAX,
                    max: MAX_CANONICAL_FAMILY_BYTES,
                },
            )?;
            if canonical_family_bytes > MAX_CANONICAL_FAMILY_BYTES {
                return Err(CompoundError::LimitExceeded {
                    resource: "canonical_family_bytes",
                    requested: canonical_family_bytes,
                    max: MAX_CANONICAL_FAMILY_BYTES,
                });
            }
            member_canon.push(canonical);
        }
        Ok(Self {
            name,
            invariant,
            provenance,
            members,
            member_canon,
            tracking,
            recommended_admission,
        })
    }

    /// Stable family name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Classified invariant.
    #[must_use]
    pub fn invariant(&self) -> &InvariantClass {
        &self.invariant
    }

    /// Captured seed and violated contract context.
    #[must_use]
    pub fn provenance(&self) -> &FamilyProvenance {
        &self.provenance
    }

    /// Minimized case followed by failing neighbors.
    #[must_use]
    pub fn members(&self) -> &[(String, I)] {
        &self.members
    }

    /// Beads or issue references that own the family.
    #[must_use]
    pub fn tracking(&self) -> &[String] {
        &self.tracking
    }

    /// Generalized admission recommendation, if one was justified.
    #[must_use]
    pub fn recommended_admission(&self) -> Option<&str> {
        self.recommended_admission.as_deref()
    }

    /// Content hash over the full canonical encoding: name, invariant,
    /// members (labels + inputs), tracking, admission. Deterministic across
    /// runs, build modes, and ISAs.
    #[must_use]
    pub fn content_hash(&self) -> fs_blake3::ContentHash {
        let mut bytes = Vec::new();
        COMPOUND_CANON_VERSION.canon(&mut bytes);
        self.name.canon(&mut bytes);
        self.invariant.canon(&mut bytes);
        self.provenance.seed.canon(&mut bytes);
        self.provenance.contract.canon(&mut bytes);
        self.provenance.detail.canon(&mut bytes);
        bytes.push(7);
        bytes.extend_from_slice(&(self.members.len() as u64).to_le_bytes());
        for ((label, _), canonical) in self.members.iter().zip(&self.member_canon) {
            label.canon(&mut bytes);
            bytes.push(12);
            bytes.extend_from_slice(
                &u64::try_from(canonical.len())
                    .expect("bounded canonical member length fits u64")
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(canonical);
        }
        self.tracking.canon(&mut bytes);
        match &self.recommended_admission {
            Some(a) => {
                bytes.push(9);
                a.canon(&mut bytes);
            }
            None => bytes.push(10),
        }
        fs_blake3::hash_domain(COMPOUND_FAMILY_HASH_DOMAIN, &bytes)
    }

    /// The canonical capture manifest: JSON-lines, one header, one line per
    /// member (canonical bytes hex-encoded), one trailer with the content hash.
    /// Decoding arbitrary caller-defined member types remains the family
    /// owner's responsibility.
    #[must_use]
    pub fn manifest(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = write!(
            out,
            "{{\"canon_version\":{COMPOUND_CANON_VERSION},\"family\":"
        );
        write_json_string(&mut out, &self.name);
        let _ = write!(out, ",\"invariant\":");
        write_json_string(&mut out, self.invariant.name());
        let _ = write!(out, ",\"seed\":{},\"contract\":", self.provenance.seed);
        write_json_string(&mut out, &self.provenance.contract);
        let _ = write!(out, ",\"detail\":");
        write_json_string(&mut out, &self.provenance.detail);
        let _ = write!(out, ",\"members\":{},\"tracking\":[", self.members.len());
        for (index, reference) in self.tracking.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            write_json_string(&mut out, reference);
        }
        let _ = write!(out, "],\"recommended_admission\":");
        if let Some(rule) = &self.recommended_admission {
            write_json_string(&mut out, rule);
        } else {
            out.push_str("null");
        }
        out.push_str("}\n");
        for ((label, _), canonical) in self.members.iter().zip(&self.member_canon) {
            let hex: String = canonical.iter().fold(String::new(), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            });
            out.push_str("{\"member\":");
            write_json_string(&mut out, label);
            let _ = writeln!(out, ",\"canon\":\"{hex}\"}}");
        }
        let _ = writeln!(out, "{{\"content_hash\":\"{}\"}}", self.content_hash());
        out
    }

    /// Re-execute every member against the predicate. A live family has
    /// `now_passing` empty; anything else is stale evidence to act on.
    #[must_use]
    pub fn replay(&self, fails: &dyn Fn(&I) -> bool) -> ReplayReport {
        let mut still_failing = Vec::new();
        let mut now_passing = Vec::new();
        for (label, input) in &self.members {
            if fails(input) {
                still_failing.push(label.clone());
            } else {
                now_passing.push(label.clone());
            }
        }
        ReplayReport {
            still_failing,
            now_passing,
        }
    }
}

/// The full workflow output: minimized case, neighborhood, family, hash.
#[derive(Debug, Clone)]
pub struct CompoundReport<I> {
    /// The captured case with its input replaced by the minimum.
    pub case: FailureCase<I>,
    /// Minimization statistics.
    pub steps: usize,
    /// Whether minimization reached a fixpoint.
    pub converged: bool,
    /// The bounded neighborhood around the minimum.
    pub neighborhood: NeighborhoodReport,
    /// The landed family (minimum first, then failing neighbors).
    pub family: RegressionFamily<I>,
    /// The family's content hash (also in the manifest trailer).
    pub content_hash: fs_blake3::ContentHash,
}

/// The v2 workflow driver: validate → minimize → probe → seal the family.
///
/// `neighbors_of` receives the MINIMIZED input and returns a deterministically
/// ordered, labeled neighbor set. Its callback work is caller-owned; the
/// returned set is count/identity-validated before any neighbor predicate is
/// evaluated. Failing neighbors join the family behind the minimum.
///
/// # Errors
/// [`CompoundError::NotFailing`] when the captured input does not fail, plus
/// structured field, identity, and deterministic work-limit refusals.
pub fn compound<I: Shrink + Canon>(
    case: FailureCase<I>,
    fails: &dyn Fn(&I) -> bool,
    neighbors_of: &dyn Fn(&I) -> Vec<(String, I)>,
    tracking: Vec<String>,
    recommended_admission: Option<String>,
    max_steps: usize,
) -> Result<CompoundReport<I>, CompoundError> {
    validate_family_name(&case.id)?;
    case.invariant.validate()?;
    let provenance = FamilyProvenance::new(case.seed, case.contract.clone(), case.detail.clone())?;
    validate_tracking_refs(&tracking)?;
    validate_admission_rule(recommended_admission.as_deref())?;
    let report = minimize(&case.id, &case.input, fails, max_steps)?;
    let neighbors = neighbors_of(&report.minimized);
    let neighborhood = probe_neighborhood(&neighbors, fails)?;
    let mut members: Vec<(String, I)> = vec![("minimized".to_string(), report.minimized.clone())];
    for ((label, input), probe) in neighbors.into_iter().zip(&neighborhood.probes) {
        if probe.fails {
            members.push((label, input));
        }
    }
    let family = RegressionFamily::new(
        case.id.clone(),
        case.invariant.clone(),
        members,
        tracking,
        recommended_admission,
        provenance,
    )?;
    let content_hash = family.content_hash();
    Ok(CompoundReport {
        case: FailureCase {
            input: report.minimized,
            ..case
        },
        steps: report.steps,
        converged: report.converged,
        neighborhood,
        family,
        content_hash,
    })
}
