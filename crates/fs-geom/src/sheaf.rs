//! SHEAF CERTIFICATES (plan §7.3, Bet 11): cellular-sheaf sampled
//! interface-agreement evidence for multi-representation models. A model
//! whose patches live in different charts is a cellular sheaf over the
//! patch-adjacency complex: stalks are per-patch sample spaces,
//! restriction maps are selection operators onto shared interface
//! samples, and GLOBAL CONSISTENCY is the existence of a section. The
//! current positive certificate is an INTERVAL-VERIFIED bound
//! `‖δs‖∞ ≤ tol` on sampled interface mismatches. When a mismatch
//! enclosure lies entirely above tolerance, the offending interface cells are
//! reported as proven interface violations. This base certificate does not
//! establish between-sample coverage, continuum watertightness, cocycle
//! membership, or non-exactness and therefore makes no global or H¹ claim.
//!
//! The construction is finite linear algebra: the edge-level
//! [`SheafComplex::delta0_edges`] and [`SheafComplex::delta1`] maps assemble as
//! sparse matrices with entries in {−1, 0, +1}, so their `δ¹·δ⁰ = 0` identity
//! holds BITWISE — small-integer f64 arithmetic is exact. The separate
//! sample-row restriction incidence is [`SheafComplex::delta0`]. The
//! least-squares section solve (per-patch gauge offsets over the adjacency
//! Laplacian) reports the fractional reduction in uncentered sample-level
//! midpoint-mismatch mean-square energy. That graph-gauge diagnostic is not a cohomology
//! certificate; the feature-gated repair classifier owns exact/coexact/harmonic
//! claims.

use crate::{Aabb, Chart, ChartSample, Point3, SamplingDomain, SamplingDomainError};
use fs_evidence::{
    Evidence, ModelEvidence, NumericalCertificate, NumericalKind, ProvenanceHash,
    SensitivitySummary, StatisticalCertificate,
};
use fs_exec::Cx;
use fs_ivl::Interval;
use fs_sparse::{Coo, Csr};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Samples drawn per pairwise interface.
pub const SAMPLES_PER_INTERFACE: usize = 32;

/// Zero-band half-width as a fraction of the overlap-box diagonal:
/// a point belongs to the shared surface region when BOTH charts place
/// it within this band of their zero set.
pub const BAND_FRACTION: f64 = 0.05;

/// Maximum number of chart evaluations admitted by one ray-parity probe.
///
/// The legacy sign-sequence diagnostic is deliberately bounded rather than
/// hiding an unbounded marching workload behind a sample-count argument.
pub const RAY_PARITY_MAX_EVALUATIONS: usize = 1_048_576;

/// Maximum number of neighbor-membership probes admitted while discovering
/// fully connected triples during one sheaf build.
pub const SHEAF_MAX_TRIPLE_CANDIDATES: usize = 1_048_576;

/// Allocation-free writer for the legacy FNV provenance stream.
///
/// `watertightness` historically materialized its complete canonical transcript
/// in one `String` before hashing it.  A public complex can contain many samples,
/// so that duplicated all evidence bytes without an admission budget.  Streaming
/// the identical bytes preserves the legacy fingerprint while keeping auxiliary
/// memory constant.  This remains a non-cryptographic fingerprint; strong
/// identity migration is tracked separately.
struct LegacyProvenanceWriter(u64);

impl LegacyProvenanceWriter {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

impl std::fmt::Write for LegacyProvenanceWriter {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        for byte in value.bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(())
    }
}

/// Endpoint of a ray named by a structured parity refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RayEndpoint {
    /// Segment start.
    Start,
    /// Segment end.
    End,
}

/// Why the legacy ray-parity diagnostic could not return a result.
#[derive(Debug, Clone, PartialEq)]
pub enum RayParityError {
    /// A union-of-charts model needs at least one presentation.
    EmptyCharts,
    /// A diagnostic run with no rays gathers no evidence.
    EmptyRays,
    /// At least one interval is required to define a segment march.
    InvalidSteps {
        /// Offending interval count.
        steps: usize,
    },
    /// The requested diagnostic exceeds its public deterministic work cap.
    WorkLimitExceeded {
        /// Requested chart evaluations.
        requested: u128,
        /// Public chart-evaluation cap.
        cap: usize,
    },
    /// A ray endpoint was NaN or infinite.
    NonFiniteEndpoint {
        /// Ray index.
        ray: usize,
        /// Which endpoint was invalid.
        endpoint: RayEndpoint,
        /// Offending point.
        point: Point3,
    },
    /// Finite endpoints did not yield a representable convex interpolation.
    NonRepresentableSamplePoint {
        /// Ray index.
        ray: usize,
        /// Sample index in `0..=steps`.
        step: usize,
        /// Offending interpolated point.
        point: Point3,
    },
    /// A chart returned a NaN or infinite nominal field value.
    NonFiniteSample {
        /// Ray index.
        ray: usize,
        /// Sample index in `0..=steps`.
        step: usize,
        /// Chart index.
        chart: usize,
        /// Offending value.
        value: f64,
    },
    /// The parity theorem requires both endpoints to be strictly outside the
    /// union model.
    EndpointNotOutside {
        /// Ray index.
        ray: usize,
        /// Which endpoint violated the precondition.
        endpoint: RayEndpoint,
        /// Minimum signed field across the charts.
        min_signed_distance: f64,
    },
    /// Cancellation was observed before a verdict could be published.
    Cancelled {
        /// Rays fully classified before cancellation.
        completed_rays: usize,
        /// Ray points fully evaluated before cancellation.
        completed_points: usize,
        /// Individual chart evaluations completed before cancellation.
        completed_chart_evaluations: usize,
    },
}

impl core::fmt::Display for RayParityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyCharts => write!(f, "ray parity refused: the chart set is empty"),
            Self::EmptyRays => write!(f, "ray parity refused: the ray set is empty"),
            Self::InvalidSteps { steps } => {
                write!(f, "ray parity refused: steps must be positive, got {steps}")
            }
            Self::WorkLimitExceeded { requested, cap } => write!(
                f,
                "ray parity refused: {requested} chart evaluations exceed the public cap {cap}"
            ),
            Self::NonFiniteEndpoint {
                ray,
                endpoint,
                point,
            } => write!(
                f,
                "ray parity refused: ray {ray} {endpoint:?} endpoint is non-finite: {point:?}"
            ),
            Self::NonRepresentableSamplePoint { ray, step, point } => write!(
                f,
                "ray parity refused: ray {ray} sample {step} is not representable: {point:?}"
            ),
            Self::NonFiniteSample {
                ray,
                step,
                chart,
                value,
            } => write!(
                f,
                "ray parity refused: chart {chart} returned non-finite value {value} at ray {ray} sample {step}"
            ),
            Self::EndpointNotOutside {
                ray,
                endpoint,
                min_signed_distance,
            } => write!(
                f,
                "ray parity refused: ray {ray} {endpoint:?} endpoint is not strictly outside (minimum field {min_signed_distance})"
            ),
            Self::Cancelled {
                completed_rays,
                completed_points,
                completed_chart_evaluations,
            } => write!(
                f,
                "ray parity cancelled after {completed_rays} rays, {completed_points} points, and {completed_chart_evaluations} chart evaluations"
            ),
        }
    }
}

impl core::error::Error for RayParityError {}

/// One shared interface sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterfaceSample {
    /// The world-space point.
    pub point: Point3,
    /// Enclosures of the two charts' signed distances at the point
    /// (outward bounds from each chart's own error certificate).
    pub values: [Interval; 2],
}

/// One interface (edge of the adjacency complex).
#[derive(Debug, Clone)]
pub struct Interface {
    /// Patch indices (u < v; the edge is oriented u → v).
    pub patches: (usize, usize),
    /// Shared samples.
    pub samples: Vec<InterfaceSample>,
}

/// An unverified pairwise-interface clique completion (candidate 2-cell).
/// Pairwise sampled overlaps do not by themselves prove a common triple
/// overlap or aligned restriction samples, so this carries no Čech/topology
/// authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripleCell {
    /// Patch indices (sorted).
    pub patches: (usize, usize, usize),
    /// Minimum of the three independent pairwise sample counts. This is not a
    /// count of common aligned triple samples.
    pub samples: usize,
}

/// The patch-adjacency sheaf complex.
#[derive(Debug)]
pub struct SheafComplex {
    /// Explicit finite scope used to gather this evidence. `None` means the
    /// admitted global supports were sampled.
    pub sampling_clip: Option<Aabb>,
    /// Patch count.
    pub n_patches: usize,
    /// Pairwise interfaces (sorted by patch pair — deterministic).
    pub interfaces: Vec<Interface>,
    /// Unverified pairwise-interface clique completions.
    pub triples: Vec<TripleCell>,
}

/// Immutable complex produced by the chart-sampling admission path. Only this
/// wrapper may publish positive or negative sampled-interface evidence. It
/// dereferences immutably for incidence algebra and diagnostics, but exposes no
/// `DerefMut` or ownership escape that could mutate retained evidence after
/// admission.
#[derive(Debug)]
pub struct AdmittedSheafComplex {
    inner: SheafComplex,
}

impl core::ops::Deref for AdmittedSheafComplex {
    type Target = SheafComplex;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AdmittedSheafComplex {
    /// Assess builder-retained sampled interfaces. This authority says only
    /// that the declared chart certificates were sampled through the admitted
    /// path and then kept immutable; it does not independently prove a chart
    /// implementation truthful.
    #[must_use]
    pub fn watertightness(&self, tol: f64) -> Evidence<SheafVerdict> {
        self.inner.assess_sampled_agreement(tol, true)
    }
}

/// One interface's assessed mismatch. Verdict bits are predicate-sound and
/// reported magnitudes come directly from outward interval endpoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterfaceBound {
    /// The patch pair.
    pub patches: (usize, usize),
    /// Every sample's |mismatch| enclosure lies inside `[0, tol]`.
    pub all_within: bool,
    /// Some sample's enclosure lies ENTIRELY above tol (proven leak).
    pub proven_leak: bool,
    /// The interface has ordered in-range patch indices and at least one sample,
    /// every sample point and mismatch-interval endpoint is finite, and the
    /// supplied tolerance is finite and non-negative.
    pub determinate: bool,
    /// Reported worst lower bound.
    pub lo_report: f64,
    /// Reported worst upper bound.
    pub hi_report: f64,
}

/// The certificate verdict.
#[derive(Debug, Clone, PartialEq)]
pub enum SheafVerdict {
    /// Every retained sample proves `‖δs‖∞ ≤ tol`, with per-interface
    /// margins attached. This is sampled agreement, not by itself a continuum
    /// covering or global watertightness theorem.
    Pass {
        /// Upper bound of the worst interface mismatch.
        worst_mismatch: f64,
        /// Per-interface (patch pair, mismatch upper bound).
        margins: Vec<((usize, usize), f64)>,
    },
    /// Some interface's mismatch enclosure lies entirely above tolerance —
    /// an interval-proven interface violation, localized without a topology
    /// claim.
    Fail {
        /// Offending interfaces: (patch pair, mismatch lower bound).
        interface_violations: Vec<((usize, usize), f64)>,
        /// Fractional reduction in uncentered midpoint-mismatch mean-square
        /// energy from per-patch graph gauge offsets, in `[0, 1]`. Near 1
        /// means a constant re-gauge fits the sampled edge means; it does not
        /// prove exactness or classify the residual topologically. `None`
        /// refuses the diagnostic when its least-squares arithmetic is not
        /// representable.
        gauge_fit_share: Option<f64>,
    },
    /// No sound aggregate claim: enclosures may straddle the tolerance, or the
    /// retained structure/scope/tolerance may be absent or malformed.
    Unknown {
        /// Non-authoritative interface bounds: (patch pair, lower, upper).
        /// This may be empty when the structure or interface set is absent.
        straddling: Vec<((usize, usize), f64, f64)>,
    },
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Geometry-derived deterministic seed: identical boxes give identical
/// samples regardless of patch indexing (re-index invariance is exact).
fn box_seed(b: &Aabb) -> u64 {
    let mut bytes = Vec::with_capacity(48);
    for v in [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z] {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fnv(&bytes)
}

fn lcg(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 11) as f64) / (1u64 << 53) as f64
}

/// Why sheaf interface discovery could not safely sample a chart pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheafBuildError {
    /// Cooperative cancellation was observed before publishing a complex.
    Cancelled {
        /// Stable build stage at which cancellation was observed.
        stage: &'static str,
        /// Pair being sampled, when cancellation occurred inside an interface.
        patches: Option<(usize, usize)>,
        /// Candidate points fully evaluated for that pair, or neighbor
        /// membership probes inspected when `patches` is `None`.
        completed_draws: usize,
    },
    /// The requested finite sampling clip itself was not admissible. This is
    /// checked even when there are fewer than two charts or every chart pair
    /// is disjoint, so malformed caller input cannot silently succeed.
    SamplingClip {
        /// Exact clip admission failure.
        source: SamplingDomainError,
    },
    /// Pair-attributed finite-domain admission refusal.
    SamplingDomain {
        /// Chart indices in deterministic ascending order.
        patches: (usize, usize),
        /// Exact shared admission failure.
        source: SamplingDomainError,
    },
    /// A chart returned a non-finite signed-distance sample while an
    /// interface was being discovered. Such a producer cannot be treated as
    /// merely outside the sampled zero band.
    NonFiniteSample {
        /// Chart pair being sampled.
        patches: (usize, usize),
        /// Index of the chart that returned the malformed value.
        chart: usize,
        /// Exact sampled point coordinates, encoded as IEEE-754 bits.
        point_bits: [u64; 3],
        /// Exact malformed signed-distance bits.
        value_bits: u64,
        /// Pair draws fully evaluated before the refusal.
        completed_draws: usize,
    },
    /// Triple discovery exceeded its deterministic membership-probe cap.
    TripleWorkLimit {
        /// Neighbor membership probes encountered.
        candidates: usize,
        /// Public work cap.
        cap: usize,
    },
}

impl core::fmt::Display for SheafBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cancelled {
                stage,
                patches,
                completed_draws,
            } => write!(
                f,
                "sheaf build cancelled during {stage} for patches {patches:?} after \
                 {completed_draws} completed work units; no complex was published"
            ),
            Self::SamplingClip { source } => {
                write!(f, "sheaf explicit sampling clip {source}")
            }
            Self::SamplingDomain { patches, source } => {
                write!(f, "sheaf interface ({}, {}) {source}", patches.0, patches.1)
            }
            Self::NonFiniteSample {
                patches,
                chart,
                point_bits,
                value_bits,
                completed_draws,
            } => write!(
                f,
                "sheaf interface ({}, {}) chart {chart} returned non-finite signed-distance \
                 bits {value_bits:#018x} at point bits [{:#018x}, {:#018x}, {:#018x}] after \
                 {completed_draws} completed draws",
                patches.0, patches.1, point_bits[0], point_bits[1], point_bits[2]
            ),
            Self::TripleWorkLimit { candidates, cap } => write!(
                f,
                "sheaf triple discovery refused after {candidates} neighbor probes; the deterministic cap is {cap}"
            ),
        }
    }
}

impl core::error::Error for SheafBuildError {}

/// Outward enclosure of a chart sample's signed distance from its own error
/// certificate. Only well-formed rigorous claims are usable: estimates,
/// no-claims, and malformed rigorous certificates poison that sample into the
/// whole extended real line. It cannot contribute positive authority, though
/// it may coexist with an independently proven violation and aggregate `Fail`.
fn sample_interval(s: &ChartSample) -> Interval {
    match s.error.kind {
        NumericalKind::Exact
            if s.signed_distance.is_finite()
                && s.error.lo.is_finite()
                && s.error.hi.is_finite()
                && s.error.lo.to_bits() == s.signed_distance.to_bits()
                && s.error.hi.to_bits() == s.signed_distance.to_bits() =>
        {
            Interval::point(s.signed_distance)
        }
        NumericalKind::Enclosure
            if s.signed_distance.is_finite()
                && s.error.lo.is_finite()
                && s.error.hi.is_finite()
                && s.error.lo <= s.signed_distance
                && s.signed_distance <= s.error.hi =>
        {
            Interval::new(s.error.lo, s.error.hi)
        }
        NumericalKind::Exact
        | NumericalKind::Enclosure
        | NumericalKind::Estimate
        | NumericalKind::NoClaim => Interval::WHOLE,
    }
}

fn discover_triples(
    interfaces: &[Interface],
    cx: &Cx<'_>,
) -> Result<Vec<TripleCell>, SheafBuildError> {
    let mut adjacency: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut edge_samples = BTreeMap::new();
    for (completed_edges, interface) in interfaces.iter().enumerate() {
        if completed_edges.is_multiple_of(256) {
            cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
                stage: "triple-discovery",
                patches: None,
                completed_draws: completed_edges,
            })?;
        }
        let (a, b) = interface.patches;
        adjacency.entry(a).or_default().insert(b);
        adjacency.entry(b).or_default().insert(a);
        edge_samples.insert((a, b), interface.samples.len());
    }

    let mut triples = Vec::new();
    let mut inspected = 0usize;
    for (&(a, b), &ab_samples) in &edge_samples {
        let (Some(a_neighbors), Some(b_neighbors)) = (adjacency.get(&a), adjacency.get(&b)) else {
            continue;
        };
        // Iterate the smaller neighbor set explicitly so every membership
        // probe is counted and cancellation-checkable. `BTreeSet::intersection`
        // hides the work spent skipping non-common neighbors, allowing a dense
        // triangle-free graph to consume substantial unmetered work.
        let (probe, lookup) = if a_neighbors.len() <= b_neighbors.len() {
            (a_neighbors, b_neighbors)
        } else {
            (b_neighbors, a_neighbors)
        };
        for &c in probe {
            if inspected >= SHEAF_MAX_TRIPLE_CANDIDATES {
                return Err(SheafBuildError::TripleWorkLimit {
                    candidates: inspected.saturating_add(1),
                    cap: SHEAF_MAX_TRIPLE_CANDIDATES,
                });
            }
            if inspected.is_multiple_of(256) {
                cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
                    stage: "triple-discovery",
                    patches: None,
                    completed_draws: inspected,
                })?;
            }
            inspected += 1;
            if c <= b || !lookup.contains(&c) {
                continue;
            }
            let Some(&bc_samples) = edge_samples.get(&(b, c)) else {
                continue;
            };
            let Some(&ac_samples) = edge_samples.get(&(a, c)) else {
                continue;
            };
            triples.push(TripleCell {
                patches: (a, b, c),
                samples: ab_samples.min(bc_samples).min(ac_samples),
            });
        }
    }
    cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
        stage: "triple-discovery",
        patches: None,
        completed_draws: inspected,
    })?;
    Ok(triples)
}

impl SheafComplex {
    /// Build the complex from charts: interface discovery via
    /// support-overlap, shared-surface samples via the zero band of BOTH
    /// charts, plus deterministic pairwise-interface clique completions. The
    /// latter are algebraic candidate cells, not verified common triple
    /// overlaps.
    pub fn from_charts(
        charts: &[&dyn Chart],
        cx: &Cx<'_>,
    ) -> Result<AdmittedSheafComplex, SheafBuildError> {
        Self::from_charts_with_clip(charts, None, cx)
    }

    /// Build interface evidence inside an explicit finite clip. Pairs outside
    /// the clip are skipped; invalid supports and unresolved unbounded shared
    /// domains are structured refusals.
    pub fn from_charts_clipped(
        charts: &[&dyn Chart],
        clip: Aabb,
        cx: &Cx<'_>,
    ) -> Result<AdmittedSheafComplex, SheafBuildError> {
        Self::from_charts_with_clip(charts, Some(clip), cx)
    }

    #[allow(clippy::too_many_lines)] // One ordered discovery, sampling, cancellation, and finalize transaction.
    fn from_charts_with_clip(
        charts: &[&dyn Chart],
        clip: Option<Aabb>,
        cx: &Cx<'_>,
    ) -> Result<AdmittedSheafComplex, SheafBuildError> {
        cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
            stage: "admission",
            patches: None,
            completed_draws: 0,
        })?;
        let n = charts.len();
        if let Some(explicit_clip) = clip {
            SamplingDomain::resolve(Aabb::WHOLE_SPACE, Some(explicit_clip))
                .map_err(|source| SheafBuildError::SamplingClip { source })?;
        }
        let mut interfaces = Vec::new();
        for u in 0..n {
            for v in (u + 1)..n {
                cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
                    stage: "interface-discovery",
                    patches: Some((u, v)),
                    completed_draws: 0,
                })?;
                let support_u = charts[u].support();
                let support_v = charts[v].support();
                SamplingDomain::validate_support(support_u).map_err(|source| {
                    SheafBuildError::SamplingDomain {
                        patches: (u, v),
                        source,
                    }
                })?;
                SamplingDomain::validate_support(support_v).map_err(|source| {
                    SheafBuildError::SamplingDomain {
                        patches: (u, v),
                        source,
                    }
                })?;
                let Some(shared_support) = support_u.intersection(&support_v) else {
                    continue;
                };
                let domain = match SamplingDomain::resolve(shared_support, clip) {
                    Ok(domain) => domain,
                    Err(
                        SamplingDomainError::EmptyIntersection
                        | SamplingDomainError::DegenerateDomain { .. },
                    ) => continue,
                    Err(source) => {
                        return Err(SheafBuildError::SamplingDomain {
                            patches: (u, v),
                            source,
                        });
                    }
                };
                let shared = domain.bounds();
                let spans = domain.spans();
                let diag = domain.diagonal();
                let band = BAND_FRACTION * diag;
                let mut state = box_seed(&shared);
                let mut samples = Vec::new();
                // Rejection-sample the shared zero band (bounded draws).
                for draw_index in 0..SAMPLES_PER_INTERFACE * 64 {
                    if samples.len() >= SAMPLES_PER_INTERFACE {
                        break;
                    }
                    let completed_draws = draw_index;
                    cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
                        stage: "interface-sampling",
                        patches: Some((u, v)),
                        completed_draws,
                    })?;
                    let p = Point3::new(
                        shared.min.x + lcg(&mut state) * spans.x,
                        shared.min.y + lcg(&mut state) * spans.y,
                        shared.min.z + lcg(&mut state) * spans.z,
                    );
                    let su = charts[u].eval(p, cx);
                    cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
                        stage: "interface-sampling",
                        patches: Some((u, v)),
                        completed_draws,
                    })?;
                    if !su.signed_distance.is_finite() {
                        return Err(SheafBuildError::NonFiniteSample {
                            patches: (u, v),
                            chart: u,
                            point_bits: [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()],
                            value_bits: su.signed_distance.to_bits(),
                            completed_draws,
                        });
                    }
                    let sv = charts[v].eval(p, cx);
                    let completed_draws = draw_index + 1;
                    cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
                        stage: "interface-sampling",
                        patches: Some((u, v)),
                        completed_draws,
                    })?;
                    if !sv.signed_distance.is_finite() {
                        return Err(SheafBuildError::NonFiniteSample {
                            patches: (u, v),
                            chart: v,
                            point_bits: [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()],
                            value_bits: sv.signed_distance.to_bits(),
                            completed_draws,
                        });
                    }
                    if su.signed_distance.abs() <= band && sv.signed_distance.abs() <= band {
                        samples.push(InterfaceSample {
                            point: p,
                            values: [sample_interval(&su), sample_interval(&sv)],
                        });
                    }
                }
                if !samples.is_empty() {
                    interfaces.push(Interface {
                        patches: (u, v),
                        samples,
                    });
                }
            }
        }
        let triples = discover_triples(&interfaces, cx)?;
        cx.checkpoint().map_err(|_| SheafBuildError::Cancelled {
            stage: "finalize",
            patches: None,
            completed_draws: triples.len(),
        })?;
        Ok(AdmittedSheafComplex {
            inner: SheafComplex {
                sampling_clip: clip,
                n_patches: n,
                interfaces,
                triples,
            },
        })
    }

    /// Assemble the sampled restriction incidence (samples × patches) with
    /// ±1 entries: one row per interface sample, `+1` on patch v's slot and
    /// `−1` on patch u's. These sample rows are not dimension-compatible with
    /// [`Self::delta1`]; use [`Self::delta0_edges`] for the edge-level cochain
    /// map in the bitwise `δ¹δ⁰ = 0` identity.
    #[must_use]
    pub fn delta0(&self) -> Csr {
        let rows: usize = self.interfaces.iter().map(|i| i.samples.len()).sum();
        let mut coo = Coo::new(rows, self.n_patches);
        let mut r = 0usize;
        for iface in &self.interfaces {
            for _ in &iface.samples {
                coo.push(r, iface.patches.0, -1.0);
                coo.push(r, iface.patches.1, 1.0);
                r += 1;
            }
        }
        coo.assemble()
    }

    /// Assemble the edge-level δ¹ (triples × interfaces) with ±1 entries
    /// per the oriented
    /// triangle boundary: for triple (a,b,c) with edges e_ab, e_bc, e_ac:
    /// `+e_ab + e_bc − e_ac` (edge-level stalks: one column per edge).
    #[must_use]
    pub fn delta1(&self) -> Csr {
        let edge_index = |a: usize, b: usize| {
            self.interfaces
                .iter()
                .position(|i| i.patches == (a.min(b), a.max(b)))
                .expect("triple implies edges")
        };
        let mut coo = Coo::new(self.triples.len(), self.interfaces.len());
        for (t, triple) in self.triples.iter().enumerate() {
            let (a, b, c) = triple.patches;
            coo.push(t, edge_index(a, b), 1.0);
            coo.push(t, edge_index(b, c), 1.0);
            coo.push(t, edge_index(a, c), -1.0);
        }
        coo.assemble()
    }

    /// Edge-level δ⁰ (edges × patches, one row per INTERFACE): the
    /// companion of [`Self::delta1`] for the bitwise δδ = 0 identity.
    #[must_use]
    pub fn delta0_edges(&self) -> Csr {
        let mut coo = Coo::new(self.interfaces.len(), self.n_patches);
        for (r, iface) in self.interfaces.iter().enumerate() {
            coo.push(r, iface.patches.0, -1.0);
            coo.push(r, iface.patches.1, 1.0);
        }
        coo.assemble()
    }

    pub(crate) fn structure_is_valid(&self) -> bool {
        if self.n_patches == 0 {
            return false;
        }
        if self
            .sampling_clip
            .is_some_and(|clip| SamplingDomain::resolve(Aabb::WHOLE_SPACE, Some(clip)).is_err())
        {
            return false;
        }
        let mut previous_edge = None;
        for interface in &self.interfaces {
            let (u, v) = interface.patches;
            if u >= v
                || v >= self.n_patches
                || interface.samples.is_empty()
                || previous_edge.is_some_and(|previous| previous >= (u, v))
                || interface.samples.iter().any(|sample| {
                    !finite_point(sample.point)
                        || self
                            .sampling_clip
                            .is_some_and(|clip| !clip.contains(sample.point))
                })
            {
                return false;
            }
            previous_edge = Some((u, v));
        }

        let mut previous_triple = None;
        for triple in &self.triples {
            let (a, b, c) = triple.patches;
            if a >= b
                || b >= c
                || c >= self.n_patches
                || triple.samples == 0
                || previous_triple.is_some_and(|previous| previous >= (a, b, c))
            {
                return false;
            }
            // The interface loop above has already established strict edge
            // ordering, so each lookup is logarithmic rather than rescanning
            // every interface three times for every triple.
            let edge_samples = |u: usize, v: usize| {
                self.interfaces
                    .binary_search_by_key(&(u, v), |interface| interface.patches)
                    .ok()
                    .map(|index| self.interfaces[index].samples.len())
            };
            let expected = match (edge_samples(a, b), edge_samples(a, c), edge_samples(b, c)) {
                (Some(ab), Some(ac), Some(bc)) => ab.min(ac).min(bc),
                _ => return false,
            };
            if triple.samples != expected {
                return false;
            }
            previous_triple = Some((a, b, c));
        }
        true
    }

    /// Per-interface mismatch assessment. The VERDICT bits come from
    /// fs-ivl's sound predicates (`encloses`/`contains`). Reported magnitudes
    /// are aggregated directly from the intervals' outward endpoints; an
    /// indeterminate interval retains an infinite upper report and cannot
    /// authorize numerical evidence.
    #[must_use]
    pub fn mismatch_bounds(&self, tol: f64) -> Vec<InterfaceBound> {
        let valid_tolerance = tol.is_finite() && tol >= 0.0;
        let ok_band = valid_tolerance.then(|| Interval::new(0.0, tol));
        self.interfaces
            .iter()
            .map(|iface| {
                let (u, v) = iface.patches;
                let valid_interface = u < v
                    && v < self.n_patches
                    && !iface.samples.is_empty()
                    && iface
                        .samples
                        .iter()
                        .all(|sample| finite_point(sample.point));
                let mut all_within = valid_tolerance && valid_interface;
                let mut proven_leak = false;
                let mut determinate = valid_tolerance && valid_interface;
                let mut lo = 0.0f64;
                let mut hi = if valid_interface { 0.0 } else { f64::INFINITY };
                for s in &iface.samples {
                    let d = (s.values[1] - s.values[0]).abs();
                    if !(d.lo().is_finite() && d.hi().is_finite()) {
                        determinate = false;
                        all_within = false;
                        hi = f64::INFINITY;
                        continue;
                    }
                    let within = ok_band.is_some_and(|band| band.encloses(d));
                    all_within &= within;
                    // |mismatch| enclosure entirely above tol: a proven
                    // violation (sound: the true value is inside d).
                    if valid_tolerance && valid_interface && d.lo() > tol {
                        proven_leak = true;
                    }
                    lo = lo.max(d.lo());
                    hi = hi.max(d.hi());
                }
                InterfaceBound {
                    patches: iface.patches,
                    all_within,
                    proven_leak,
                    determinate,
                    lo_report: lo,
                    hi_report: hi,
                }
            })
            .collect()
    }

    /// Least-squares section solve: per-patch gauge offsets minimizing
    /// the mean-square mismatch (graph-Laplacian normal equations, gauge
    /// fixed by pinning patch 0; deterministic Gauss–Seidel). Returns
    /// (offsets, raw ms mismatch, residual ms mismatch).
    #[must_use]
    pub fn section_solve(&self) -> (Vec<f64>, f64, f64) {
        let n = self.n_patches;
        let mut offsets = vec![0.0f64; n];
        // Edge means of the midpoint mismatch.
        let edges: Vec<((usize, usize), f64, usize)> = self
            .interfaces
            .iter()
            .map(|iface| {
                let mut sum = 0.0;
                for s in &iface.samples {
                    sum += s.values[1].midpoint() - s.values[0].midpoint();
                }
                (iface.patches, sum, s_len(iface))
            })
            .collect();
        let raw_ms = sample_mean_square(&self.interfaces, &offsets);
        for _ in 0..200 {
            for p in 1..n {
                // Optimal c_p given the rest: weighted average balance.
                let mut num = 0.0f64;
                let mut den = 0.0f64;
                for ((u, v), sum, count) in &edges {
                    #[allow(clippy::cast_precision_loss)]
                    let w = *count as f64;
                    if *u == p {
                        num += sum + w * offsets[*v];
                        den += w;
                    } else if *v == p {
                        num += -sum + w * offsets[*u];
                        den += w;
                    }
                }
                if den > 0.0 {
                    offsets[p] = num / den;
                }
            }
        }
        let residual_ms = sample_mean_square(&self.interfaces, &offsets);
        (offsets, raw_ms, residual_ms)
    }

    /// Assess raw public parts as non-authoritative diagnostics. Raw complexes
    /// can exercise incidence and mismatch algebra, but callers can construct
    /// every field themselves, so neither `Pass` nor `Fail` would be evidence.
    /// Use [`Self::from_charts`] to obtain an immutable
    /// [`AdmittedSheafComplex`] when sampled-interface authority is required.
    #[must_use]
    pub fn watertightness(&self, tol: f64) -> Evidence<SheafVerdict> {
        self.assess_sampled_agreement(tol, false)
    }

    fn assess_sampled_agreement(
        &self,
        tol: f64,
        admitted_builder_origin: bool,
    ) -> Evidence<SheafVerdict> {
        let bounds = self.mismatch_bounds(tol);
        let structure_is_valid = self.structure_is_valid();
        let worst_hi = bounds.iter().map(|b| b.hi_report).fold(0.0f64, f64::max);
        let worst_lo = bounds.iter().map(|b| b.lo_report).fold(0.0f64, f64::max);
        let all_determinate = structure_is_valid
            && !bounds.is_empty()
            && bounds.iter().all(|bound| bound.determinate);
        // PASS requires at least one DISCOVERED interface whose samples all lie
        // inside [0, tol]. An empty `bounds` means NO interface was found (charts
        // are disjoint/gapped/near-tangent, or the geometry is empty) — the
        // interface-agreement check gathered NO evidence, so the honest verdict
        // is `Unknown`, not a positive `worst_mismatch = 0` PASS. `all()` on an
        // empty set is vacuously true; guarding on non-emptiness closes the
        // vacuous-truth false certificate (bead obnw).
        let all_pass = all_determinate && bounds.iter().all(|b| b.all_within);
        let interface_violations: Vec<((usize, usize), f64)> = bounds
            .iter()
            .filter(|b| b.proven_leak)
            .map(|b| (b.patches, b.lo_report))
            .collect();
        let verdict = if !admitted_builder_origin {
            SheafVerdict::Unknown {
                straddling: bounds
                    .iter()
                    .map(|b| (b.patches, b.lo_report, b.hi_report))
                    .collect(),
            }
        } else if all_pass {
            SheafVerdict::Pass {
                worst_mismatch: worst_hi,
                margins: bounds.iter().map(|b| (b.patches, b.hi_report)).collect(),
            }
        } else if structure_is_valid && !interface_violations.is_empty() {
            let share = if all_determinate {
                let (_, raw, residual) = self.section_solve();
                if raw.is_finite() && residual.is_finite() && raw > 0.0 && residual >= 0.0 {
                    Some((1.0 - residual / raw).clamp(0.0, 1.0))
                } else if raw == 0.0 && residual == 0.0 {
                    Some(0.0)
                } else {
                    None
                }
            } else {
                None
            };
            SheafVerdict::Fail {
                interface_violations,
                gauge_fit_share: share,
            }
        } else {
            SheafVerdict::Unknown {
                straddling: bounds
                    .iter()
                    .filter(|b| !b.determinate || !b.all_within)
                    .map(|b| (b.patches, b.lo_report, b.hi_report))
                    .collect(),
            }
        };
        let mut canon = LegacyProvenanceWriter::new();
        let _ = write!(
            canon,
            "sheaf-sampled-agreement;schema=3;origin={};patches={};interfaces={};triples={};tol={:016x};structure_valid={structure_is_valid}",
            if admitted_builder_origin { "chart-sampling-builder" } else { "raw-public-parts" },
            self.n_patches,
            self.interfaces.len(),
            self.triples.len(),
            tol.to_bits(),
        );
        match self.sampling_clip {
            None => {
                let _ = canon.write_str(";sampling_clip=none");
            }
            Some(clip) => {
                let _ = write!(
                    canon,
                    ";sampling_clip=some:{:016x},{:016x},{:016x},{:016x},{:016x},{:016x}",
                    clip.min.x.to_bits(),
                    clip.min.y.to_bits(),
                    clip.min.z.to_bits(),
                    clip.max.x.to_bits(),
                    clip.max.y.to_bits(),
                    clip.max.z.to_bits()
                );
            }
        }
        for interface in &self.interfaces {
            let _ = write!(
                canon,
                ";interface={}-{};samples={}",
                interface.patches.0,
                interface.patches.1,
                interface.samples.len()
            );
            for sample in &interface.samples {
                let _ = write!(
                    canon,
                    ";sample={:016x},{:016x},{:016x}:{:016x},{:016x}:{:016x},{:016x}",
                    sample.point.x.to_bits(),
                    sample.point.y.to_bits(),
                    sample.point.z.to_bits(),
                    sample.values[0].lo().to_bits(),
                    sample.values[0].hi().to_bits(),
                    sample.values[1].lo().to_bits(),
                    sample.values[1].hi().to_bits(),
                );
            }
        }
        for triple in &self.triples {
            let _ = write!(
                canon,
                ";triple={}-{}-{}:{}",
                triple.patches.0, triple.patches.1, triple.patches.2, triple.samples
            );
        }
        for b in &bounds {
            let _ = write!(
                canon,
                ";bound={}-{}:{:016x}:{:016x}:within={}:leak={}:determinate={}",
                b.patches.0,
                b.patches.1,
                b.lo_report.to_bits(),
                b.hi_report.to_bits(),
                b.all_within,
                b.proven_leak,
                b.determinate,
            );
        }
        match &verdict {
            SheafVerdict::Pass {
                worst_mismatch,
                margins,
            } => {
                let _ = write!(canon, ";verdict=pass:{:016x}", worst_mismatch.to_bits());
                for (patches, margin) in margins {
                    let _ = write!(
                        canon,
                        ";margin={}-{}:{:016x}",
                        patches.0,
                        patches.1,
                        margin.to_bits()
                    );
                }
            }
            SheafVerdict::Fail {
                interface_violations,
                gauge_fit_share,
            } => {
                let _ = canon.write_str(";verdict=fail");
                for (patches, lower) in interface_violations {
                    let _ = write!(
                        canon,
                        ";violation={}-{}:{:016x}",
                        patches.0,
                        patches.1,
                        lower.to_bits()
                    );
                }
                match gauge_fit_share {
                    Some(share) => {
                        let _ = write!(canon, ";gauge_fit_share={:016x}", share.to_bits());
                    }
                    None => {
                        let _ = canon.write_str(";gauge_fit_share=none");
                    }
                }
            }
            SheafVerdict::Unknown { straddling } => {
                let _ = canon.write_str(";verdict=unknown");
                for (patches, lower, upper) in straddling {
                    let _ = write!(
                        canon,
                        ";straddling={}-{}:{:016x}:{:016x}",
                        patches.0,
                        patches.1,
                        lower.to_bits(),
                        upper.to_bits()
                    );
                }
            }
        }
        let numerical = if admitted_builder_origin
            && all_determinate
            && !matches!(&verdict, SheafVerdict::Unknown { .. })
            && worst_lo.is_finite()
            && worst_hi.is_finite()
        {
            NumericalCertificate::enclosure(worst_lo, worst_hi)
        } else {
            NumericalCertificate::no_claim()
        };
        Evidence {
            qoi: worst_hi,
            numerical,
            statistical: StatisticalCertificate::None,
            model: ModelEvidence::none(),
            sensitivity: SensitivitySummary::default(),
            provenance: ProvenanceHash(canon.finish()),
            adjoint_ref: None,
            value: verdict,
        }
    }
}

fn s_len(iface: &Interface) -> usize {
    iface.samples.len()
}

fn sample_mean_square(interfaces: &[Interface], offsets: &[f64]) -> f64 {
    let mut num = 0.0f64;
    let mut den = 0.0f64;
    for interface in interfaces {
        let (u, v) = interface.patches;
        for sample in &interface.samples {
            let mismatch = sample.values[1].midpoint() - sample.values[0].midpoint();
            let gauged = mismatch + offsets[v] - offsets[u];
            num += gauged * gauged;
            den += 1.0;
        }
    }
    if den > 0.0 { num / den } else { 0.0 }
}

/// Legacy sign-sequence diagnostic retained for input validation and replay.
/// It is NOT an independent topology falsifier: because both endpoints are
/// required to be strictly outside, the sampled boolean sequence begins and
/// ends with the same sign and therefore has an even number of toggles by
/// construction. Authentic cross-examination needs certified oriented
/// intersections or winding/degree evidence. The sample count remains
/// work-capped, every endpoint and field sample is validated, convex
/// interpolation avoids an overflowing endpoint subtraction, and cancellation
/// is observed after every chart evaluation.
///
/// # Errors
/// [`RayParityError`] when the inputs do not satisfy the finite outside-ray
/// preconditions, the public work cap would be exceeded, a chart produces a
/// non-finite value, or cancellation is observed.
pub fn ray_parity_falsifier(
    charts: &[&dyn Chart],
    rays: &[(Point3, Point3)],
    steps: usize,
    cx: &Cx<'_>,
) -> Result<Option<usize>, RayParityError> {
    if charts.is_empty() {
        return Err(RayParityError::EmptyCharts);
    }
    if rays.is_empty() {
        return Err(RayParityError::EmptyRays);
    }
    if steps == 0 {
        return Err(RayParityError::InvalidSteps { steps });
    }
    let requested = (rays.len() as u128)
        .saturating_mul((steps as u128).saturating_add(1))
        .saturating_mul(charts.len() as u128);
    if requested > RAY_PARITY_MAX_EVALUATIONS as u128 {
        return Err(RayParityError::WorkLimitExceeded {
            requested,
            cap: RAY_PARITY_MAX_EVALUATIONS,
        });
    }

    for (ray, (start, end)) in rays.iter().copied().enumerate() {
        for (endpoint, point) in [(RayEndpoint::Start, start), (RayEndpoint::End, end)] {
            if !finite_point(point) {
                return Err(RayParityError::NonFiniteEndpoint {
                    ray,
                    endpoint,
                    point,
                });
            }
        }
    }

    let mut completed_points = 0usize;
    let mut completed_chart_evaluations = 0usize;
    for (ri, (a, b)) in rays.iter().enumerate() {
        let mut crossings = 0usize;
        let mut prev_sign = None;
        for k in 0..=steps {
            checkpoint_ray_parity(cx, ri, completed_points, completed_chart_evaluations)?;
            let p = ray_sample_point(*a, *b, k, steps);
            if !finite_point(p) {
                return Err(RayParityError::NonRepresentableSamplePoint {
                    ray: ri,
                    step: k,
                    point: p,
                });
            }
            let mut d = f64::INFINITY;
            for (chart_index, chart) in charts.iter().enumerate() {
                let value = chart.eval(p, cx).signed_distance;
                completed_chart_evaluations = completed_chart_evaluations.saturating_add(1);
                checkpoint_ray_parity(cx, ri, completed_points, completed_chart_evaluations)?;
                if !value.is_finite() {
                    return Err(RayParityError::NonFiniteSample {
                        ray: ri,
                        step: k,
                        chart: chart_index,
                        value,
                    });
                }
                d = d.min(value);
            }
            if k == 0 && d <= 0.0 {
                return Err(RayParityError::EndpointNotOutside {
                    ray: ri,
                    endpoint: RayEndpoint::Start,
                    min_signed_distance: d,
                });
            }
            if k == steps && d <= 0.0 {
                return Err(RayParityError::EndpointNotOutside {
                    ray: ri,
                    endpoint: RayEndpoint::End,
                    min_signed_distance: d,
                });
            }
            let sign = d < 0.0;
            if let Some(ps) = prev_sign
                && ps != sign
            {
                crossings += 1;
            }
            prev_sign = Some(sign);
            completed_points = completed_points.saturating_add(1);
        }
        if !crossings.is_multiple_of(2) {
            return Ok(Some(ri));
        }
    }
    checkpoint_ray_parity(
        cx,
        rays.len(),
        completed_points,
        completed_chart_evaluations,
    )?;
    Ok(None)
}

fn checkpoint_ray_parity(
    cx: &Cx<'_>,
    completed_rays: usize,
    completed_points: usize,
    completed_chart_evaluations: usize,
) -> Result<(), RayParityError> {
    cx.checkpoint().map_err(|_| RayParityError::Cancelled {
        completed_rays,
        completed_points,
        completed_chart_evaluations,
    })
}

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn ray_sample_point(start: Point3, end: Point3, step: usize, steps: usize) -> Point3 {
    if step == 0 {
        return start;
    }
    if step == steps {
        return end;
    }
    #[allow(clippy::cast_precision_loss)]
    let t = step as f64 / steps as f64;
    let lerp = |a: f64, b: f64| a.mul_add(1.0 - t, b * t);
    Point3::new(
        lerp(start.x, end.x),
        lerp(start.y, end.y),
        lerp(start.z, end.z),
    )
}
