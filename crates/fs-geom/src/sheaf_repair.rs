//! SHEAF REPAIR (patch Rev L, bead wqd.14; [M] — behind the
//! `sheaf-repair` feature until certifier trials pass): upgrade the sheaf
//! machinery from diagnosis to explicit GAUGE-CORRECTION PLANNING. The current routine
//! sequentially fits the interface mismatch to the patch-coboundary image,
//! then fits that residual to the retained triangle-coboundary image, and
//! retains the final remainder. The fixed-iteration results are Hodge-inspired
//! diagnostics, not a per-result certified orthogonal decomposition. Each
//! output has an INTERPRETATION CONTRACT:
//!
//! - EXACT (`δ⁰c`): algebraically removable from the sampled mismatch by a
//!   patch 0-cochain — a candidate chart/gauge adjustment bounded by each
//!   chart's declared error budget;
//! - COEXACT (`δ¹ᵀw`): circulation-like inconsistency around retained
//!   triple cells. Converter orientation/trace errors are one hypothesis, but
//!   chart/model, junction, sampling, and numerical errors can produce the
//!   same algebraic signature; the decomposition alone does not assign cause;
//! - HARMONIC (the remainder): the part left by the current deterministic
//!   patch-potential and triple-junction projections. Because those numerical
//!   solves have no per-result convergence certificate, a generic remainder is
//!   only a candidate. Calling it H¹ or ruling out gauge repair additionally
//!   requires a retained closed, non-exact witness. It does not by itself prove
//!   that geometry topology must change.
//!
//! Repairs are PROPOSALS. `apply_gauge` only corrects the retained mismatch
//! cochain; it does not mutate or re-evaluate a chart, publish geometry, or
//! prove that a chart-level edit realizes the algebraic correction. Only the
//! algebraic gauge proposal currently has a
//! directly evaluated post-repair seam norm; other proposal kinds retain an
//! unavailable (`+∞`) prediction rather than comparing unlike quantities.
//! Optional Rep-Router reroute costs remain cost estimates, and repairs apply
//! only under an explicit budget.

use crate::router::{CostOracle, RoutePlanError, RouteRequest, Router};
use crate::sheaf::{
    SHEAF_MAX_CHARTS, SHEAF_MAX_PAIR_CANDIDATES, SHEAF_MAX_TRIPLE_CANDIDATES, SheafComplex,
};
use std::fmt::Write as _;

/// The complex skeleton the decomposition runs over (extractable from a
/// [`SheafComplex`] or built directly for controlled fixtures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheafSkeleton {
    /// Patch count.
    pub n_patches: usize,
    /// Interfaces as (u, v) with u < v (edge k orients u → v).
    pub edges: Vec<(usize, usize)>,
    /// Triple junctions (a, b, c) sorted; boundary = +e_ab + e_bc − e_ac.
    pub triangles: Vec<(usize, usize, usize)>,
}

/// Structurally admitted repair skeleton.
///
/// Unlike [`SheafSkeleton`], this type cannot be assembled with unchecked
/// public fields. Its canonical incidence is validated once and then retained
/// immutably, so the fallible incidence operators below do not need to rescan
/// topology or rely on indexing assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedSheafSkeleton {
    n_patches: usize,
    edges: Vec<(usize, usize)>,
    triangles: Vec<(usize, usize, usize)>,
}

/// Failure to extract a repair skeleton from a public complex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheafSkeletonError {
    /// The public complex violates ordering, incidence, range, sample, or
    /// sampling-domain invariants required by the incidence operators.
    MalformedComplex,
    /// At least one patch is required by the repair algebra.
    EmptyComplex,
    /// A canonical skeleton cardinality exceeds its defensive ceiling.
    WorkLimit {
        /// Stable validation stage.
        stage: &'static str,
        /// Caller-supplied cardinality.
        requested: usize,
        /// Defensive ceiling.
        cap: usize,
    },
    /// Edges must be strictly increasing canonical pairs in range.
    InvalidEdge {
        /// Position of the first invalid edge.
        index: usize,
    },
    /// Triangles must be strictly increasing canonical triples in range and
    /// every oriented boundary edge must be retained.
    InvalidTriangle {
        /// Position of the first invalid triangle.
        index: usize,
    },
    /// A cochain does not match the admitted incidence space.
    CochainLength {
        /// Stable cochain role.
        role: &'static str,
        /// Required number of scalars.
        expected: usize,
        /// Supplied number of scalars.
        actual: usize,
    },
    /// A cochain scalar is not finite.
    NonFiniteCochain {
        /// Stable cochain role.
        role: &'static str,
        /// First non-finite scalar.
        index: usize,
    },
    /// Finite inputs overflowed an incidence arithmetic operation.
    NumericalOverflow {
        /// Stable arithmetic stage.
        stage: &'static str,
    },
    /// A bounded output allocation could not be reserved.
    ResourceExhausted {
        /// Stable allocation stage.
        stage: &'static str,
    },
}

impl core::fmt::Display for SheafSkeletonError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedComplex => write!(
                f,
                "cannot extract a skeleton from a malformed sheaf complex"
            ),
            Self::EmptyComplex => write!(f, "repair skeleton requires at least one patch"),
            Self::WorkLimit {
                stage,
                requested,
                cap,
            } => write!(
                f,
                "repair skeleton stage {stage} requests {requested} items above cap {cap}"
            ),
            Self::InvalidEdge { index } => write!(
                f,
                "repair skeleton edge {index} is non-canonical, duplicated, or out of range"
            ),
            Self::InvalidTriangle { index } => write!(
                f,
                "repair skeleton triangle {index} is non-canonical, duplicated, out of range, or missing a boundary edge"
            ),
            Self::CochainLength {
                role,
                expected,
                actual,
            } => write!(
                f,
                "repair {role} cochain requires {expected} scalars, got {actual}"
            ),
            Self::NonFiniteCochain { role, index } => {
                write!(f, "repair {role} cochain scalar {index} is not finite")
            }
            Self::NumericalOverflow { stage } => {
                write!(f, "repair incidence arithmetic overflowed during {stage}")
            }
            Self::ResourceExhausted { stage } => {
                write!(f, "repair incidence could not reserve storage for {stage}")
            }
        }
    }
}

impl std::error::Error for SheafSkeletonError {}

fn validate_finite_cochain(
    values: &[f64],
    expected: usize,
    role: &'static str,
) -> Result<(), SheafSkeletonError> {
    if values.len() != expected {
        return Err(SheafSkeletonError::CochainLength {
            role,
            expected,
            actual: values.len(),
        });
    }
    if let Some(index) = values.iter().position(|value| !value.is_finite()) {
        return Err(SheafSkeletonError::NonFiniteCochain { role, index });
    }
    Ok(())
}

fn zeroed_output(len: usize, stage: &'static str) -> Result<Vec<f64>, SheafSkeletonError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| SheafSkeletonError::ResourceExhausted { stage })?;
    values.resize(len, 0.0);
    Ok(values)
}

impl AdmittedSheafSkeleton {
    /// Validate and seal canonical repair incidence supplied by a caller.
    ///
    /// Validation order is deterministic: cardinalities, edges in input
    /// order, then triangles in input order. Edges and triangles must already
    /// be strictly lexicographically ordered, making duplicate and orientation
    /// errors unambiguous and every later lookup bounded.
    pub fn try_new(
        n_patches: usize,
        edges: Vec<(usize, usize)>,
        triangles: Vec<(usize, usize, usize)>,
    ) -> Result<Self, SheafSkeletonError> {
        if n_patches == 0 {
            return Err(SheafSkeletonError::EmptyComplex);
        }
        for (stage, requested, cap) in [
            ("patches", n_patches, SHEAF_MAX_CHARTS),
            ("edges", edges.len(), SHEAF_MAX_PAIR_CANDIDATES),
            ("triangles", triangles.len(), SHEAF_MAX_TRIPLE_CANDIDATES),
        ] {
            if requested > cap {
                return Err(SheafSkeletonError::WorkLimit {
                    stage,
                    requested,
                    cap,
                });
            }
        }

        let mut previous_edge = None;
        for (index, &(u, v)) in edges.iter().enumerate() {
            if u >= v || v >= n_patches || previous_edge.is_some_and(|previous| previous >= (u, v))
            {
                return Err(SheafSkeletonError::InvalidEdge { index });
            }
            previous_edge = Some((u, v));
        }

        let mut previous_triangle = None;
        for (index, &(a, b, c)) in triangles.iter().enumerate() {
            if a >= b
                || b >= c
                || c >= n_patches
                || previous_triangle.is_some_and(|previous| previous >= (a, b, c))
                || edges.binary_search(&(a, b)).is_err()
                || edges.binary_search(&(a, c)).is_err()
                || edges.binary_search(&(b, c)).is_err()
            {
                return Err(SheafSkeletonError::InvalidTriangle { index });
            }
            previous_triangle = Some((a, b, c));
        }

        Ok(Self {
            n_patches,
            edges,
            triangles,
        })
    }

    /// Validate and seal a raw repair skeleton without copying its storage.
    pub fn admit(raw: SheafSkeleton) -> Result<Self, SheafSkeletonError> {
        Self::try_new(raw.n_patches, raw.edges, raw.triangles)
    }

    /// Structurally validate a public complex and retain its canonical edge
    /// incidence. Candidate clique triples remain omitted because the base
    /// builder has not verified a common triple overlap.
    pub fn of(complex: &SheafComplex) -> Result<Self, SheafSkeletonError> {
        if !complex.structure_is_valid() {
            return Err(SheafSkeletonError::MalformedComplex);
        }
        let mut edges = Vec::new();
        edges
            .try_reserve_exact(complex.interfaces.len())
            .map_err(|_| SheafSkeletonError::ResourceExhausted {
                stage: "complex-edges",
            })?;
        edges.extend(complex.interfaces.iter().map(|interface| interface.patches));
        Self::try_new(complex.n_patches, edges, Vec::new())
    }

    /// Number of retained patches.
    #[must_use]
    pub const fn n_patches(&self) -> usize {
        self.n_patches
    }

    /// Canonically ordered retained interfaces.
    #[must_use]
    pub fn edges(&self) -> &[(usize, usize)] {
        &self.edges
    }

    /// Canonically ordered retained triangles.
    #[must_use]
    pub fn triangles(&self) -> &[(usize, usize, usize)] {
        &self.triangles
    }

    /// Apply `delta^0` to a finite vertex cochain.
    pub fn d0(&self, c: &[f64]) -> Result<Vec<f64>, SheafSkeletonError> {
        validate_finite_cochain(c, self.n_patches, "vertex")?;
        let mut out = zeroed_output(self.edges.len(), "d0-output")?;
        for (value, &(u, v)) in out.iter_mut().zip(&self.edges) {
            *value = c[v] - c[u];
            if !value.is_finite() {
                return Err(SheafSkeletonError::NumericalOverflow { stage: "d0" });
            }
        }
        Ok(out)
    }

    /// Apply `delta^0` transpose to a finite edge cochain.
    pub fn d0t(&self, m: &[f64]) -> Result<Vec<f64>, SheafSkeletonError> {
        validate_finite_cochain(m, self.edges.len(), "edge")?;
        let mut out = zeroed_output(self.n_patches, "d0t-output")?;
        for (k, &(u, v)) in self.edges.iter().enumerate() {
            out[u] -= m[k];
            out[v] += m[k];
            if !(out[u].is_finite() && out[v].is_finite()) {
                return Err(SheafSkeletonError::NumericalOverflow { stage: "d0t" });
            }
        }
        Ok(out)
    }

    /// Apply `delta^1` to a finite edge cochain.
    pub fn d1(&self, m: &[f64]) -> Result<Vec<f64>, SheafSkeletonError> {
        validate_finite_cochain(m, self.edges.len(), "edge")?;
        let mut out = zeroed_output(self.triangles.len(), "d1-output")?;
        for (triangle, (value, &(a, b, c))) in out.iter_mut().zip(&self.triangles).enumerate() {
            let eab = self
                .edges
                .binary_search(&(a, b))
                .map_err(|_| SheafSkeletonError::InvalidTriangle { index: triangle })?;
            let ebc = self
                .edges
                .binary_search(&(b, c))
                .map_err(|_| SheafSkeletonError::InvalidTriangle { index: triangle })?;
            let eac = self
                .edges
                .binary_search(&(a, c))
                .map_err(|_| SheafSkeletonError::InvalidTriangle { index: triangle })?;
            *value = (m[eab] + m[ebc]) - m[eac];
            if !value.is_finite() {
                return Err(SheafSkeletonError::NumericalOverflow { stage: "d1" });
            }
        }
        Ok(out)
    }

    /// Apply `delta^1` transpose to a finite triangle cochain.
    pub fn d1t(&self, w: &[f64]) -> Result<Vec<f64>, SheafSkeletonError> {
        validate_finite_cochain(w, self.triangles.len(), "triangle")?;
        let mut out = zeroed_output(self.edges.len(), "d1t-output")?;
        for (triangle, &(a, b, c)) in self.triangles.iter().enumerate() {
            let eab = self
                .edges
                .binary_search(&(a, b))
                .map_err(|_| SheafSkeletonError::InvalidTriangle { index: triangle })?;
            let ebc = self
                .edges
                .binary_search(&(b, c))
                .map_err(|_| SheafSkeletonError::InvalidTriangle { index: triangle })?;
            let eac = self
                .edges
                .binary_search(&(a, c))
                .map_err(|_| SheafSkeletonError::InvalidTriangle { index: triangle })?;
            out[eab] += w[triangle];
            out[ebc] += w[triangle];
            out[eac] -= w[triangle];
            if !(out[eab].is_finite() && out[ebc].is_finite() && out[eac].is_finite()) {
                return Err(SheafSkeletonError::NumericalOverflow { stage: "d1t" });
            }
        }
        Ok(out)
    }
}

impl SheafSkeleton {
    /// Structurally validate and extract caller-supplied adjacency for
    /// diagnostic repair algebra. This accepts a raw public complex and does not
    /// authenticate chart-sampling origin or confer topology authority. The base
    /// builder's `TripleCell`s are pairwise-interface clique completions rather
    /// than verified common triple overlaps, so they are deliberately omitted.
    ///
    /// # Errors
    /// Returns [`SheafSkeletonError::MalformedComplex`] rather than copying
    /// unchecked public indices into later panicking incidence operations.
    pub fn of(complex: &SheafComplex) -> Result<SheafSkeleton, SheafSkeletonError> {
        if !complex.structure_is_valid() {
            return Err(SheafSkeletonError::MalformedComplex);
        }
        Ok(SheafSkeleton {
            n_patches: complex.n_patches,
            edges: complex.interfaces.iter().map(|i| i.patches).collect(),
            triangles: Vec::new(),
        })
    }

    fn edge_index(&self, a: usize, b: usize) -> Option<usize> {
        let key = (a.min(b), a.max(b));
        self.edges.iter().position(|&e| e == key)
    }

    /// Apply δ⁰ to a vertex cochain: `(δ⁰c)_e = c_v − c_u`.
    #[must_use]
    pub fn d0(&self, c: &[f64]) -> Vec<f64> {
        assert_eq!(c.len(), self.n_patches, "one vertex value per patch");
        self.edges.iter().map(|&(u, v)| c[v] - c[u]).collect()
    }

    /// Apply δ⁰ᵀ to an edge cochain.
    #[must_use]
    pub fn d0t(&self, m: &[f64]) -> Vec<f64> {
        assert_eq!(m.len(), self.edges.len(), "one edge value per interface");
        let mut out = vec![0.0f64; self.n_patches];
        for (k, &(u, v)) in self.edges.iter().enumerate() {
            out[u] -= m[k];
            out[v] += m[k];
        }
        out
    }

    /// Apply δ¹ to an edge cochain: signed sum around each triangle.
    #[must_use]
    pub fn d1(&self, m: &[f64]) -> Vec<f64> {
        assert_eq!(m.len(), self.edges.len(), "one edge value per interface");
        self.triangles
            .iter()
            .map(|&(a, b, c)| {
                let eab = self.edge_index(a, b).expect("triangle implies edge");
                let ebc = self.edge_index(b, c).expect("triangle implies edge");
                let eac = self.edge_index(a, c).expect("triangle implies edge");
                m[eab] + m[ebc] - m[eac]
            })
            .collect()
    }

    /// Apply δ¹ᵀ to a triangle cochain.
    #[must_use]
    pub fn d1t(&self, w: &[f64]) -> Vec<f64> {
        assert_eq!(
            w.len(),
            self.triangles.len(),
            "one face value per retained triangle"
        );
        let mut out = vec![0.0f64; self.edges.len()];
        for (t, &(a, b, c)) in self.triangles.iter().enumerate() {
            let eab = self.edge_index(a, b).expect("triangle implies edge");
            let ebc = self.edge_index(b, c).expect("triangle implies edge");
            let eac = self.edge_index(a, c).expect("triangle implies edge");
            out[eab] += w[t];
            out[ebc] += w[t];
            out[eac] -= w[t];
        }
        out
    }
}

/// The Hodge-inspired sequential diagnostic split of an edge mismatch cochain.
#[derive(Debug, Clone, PartialEq)]
pub struct HodgeSplit {
    /// The fitted exact (coboundary) component `δ⁰c`.
    pub exact: Vec<f64>,
    /// The vertex potential `c` (gauge offsets; `c[0]` pinned to 0).
    pub potential: Vec<f64>,
    /// The fitted coexact component `δ¹ᵀw` of the first residual.
    pub coexact: Vec<f64>,
    /// The remainder retained after both fixed-iteration fits.
    pub harmonic: Vec<f64>,
    /// Separate squared-norm ratios (exact, coexact, remainder) over ‖m‖².
    /// Without certified orthogonality these diagnostic ratios need not sum to
    /// one.
    pub fractions: (f64, f64, f64),
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm2(a: &[f64]) -> f64 {
    dot(a, a)
}

/// Least squares `min ‖m − A x‖²` via Gauss–Seidel on the normal
/// equations, with `apply`/`apply_t` as the operator (small complexes;
/// deterministic sweep order; component 0 optionally pinned).
fn least_squares(
    m: &[f64],
    n_unknowns: usize,
    apply: impl Fn(&[f64]) -> Vec<f64>,
    apply_t: impl Fn(&[f64]) -> Vec<f64>,
    pin_first: bool,
) -> Vec<f64> {
    let mut x = vec![0.0f64; n_unknowns];
    let rhs = apply_t(m);
    // Diagonal of AᵀA via unit vectors (small n — fine and exact).
    let mut diag = vec![0.0f64; n_unknowns];
    for (i, d) in diag.iter_mut().enumerate() {
        let mut e = vec![0.0f64; n_unknowns];
        e[i] = 1.0;
        *d = norm2(&apply(&e));
    }
    for _ in 0..400 {
        for i in 0..n_unknowns {
            if pin_first && i == 0 {
                continue;
            }
            if diag[i] <= 0.0 {
                continue;
            }
            // Residual of the normal equations at coordinate i.
            let ax = apply(&x);
            let grad_i = {
                let atax = apply_t(&ax);
                atax[i] - rhs[i]
            };
            x[i] -= grad_i / diag[i];
        }
    }
    x
}

/// Sequentially fit an edge cochain over a skeleton. A retained fixture checks
/// the first fit against an independent dense reference, but this fixed-count
/// solver returns no convergence or orthogonality certificate. Consumers must
/// verify residual identities such as `d0t(remainder) ≈ 0` and
/// `d1(remainder) ≈ 0` before assigning stronger meaning to a result.
#[must_use]
pub fn hodge_decompose(skeleton: &SheafSkeleton, m: &[f64]) -> HodgeSplit {
    assert_eq!(m.len(), skeleton.edges.len(), "cochain size");
    // Exact: project onto im δ⁰.
    let c = least_squares(
        m,
        skeleton.n_patches,
        |x| skeleton.d0(x),
        |y| skeleton.d0t(y),
        true,
    );
    let exact = skeleton.d0(&c);
    let r1: Vec<f64> = m.iter().zip(&exact).map(|(a, b)| a - b).collect();
    // Coexact: project the remainder onto im δ¹ᵀ.
    let coexact = if skeleton.triangles.is_empty() {
        vec![0.0; m.len()]
    } else {
        let w = least_squares(
            &r1,
            skeleton.triangles.len(),
            |x| skeleton.d1t(x),
            |y| skeleton.d1(y),
            false,
        );
        skeleton.d1t(&w)
    };
    let harmonic: Vec<f64> = r1.iter().zip(&coexact).map(|(a, b)| a - b).collect();
    let total = norm2(m).max(f64::MIN_POSITIVE);
    HodgeSplit {
        fractions: (
            norm2(&exact) / total,
            norm2(&coexact) / total,
            norm2(&harmonic) / total,
        ),
        exact,
        potential: c,
        coexact,
        harmonic,
    }
}

/// One ranked repair proposal (the agent-facing format).
#[derive(Debug, Clone, PartialEq)]
pub struct RepairProposal {
    /// What to do, concretely.
    pub action: String,
    /// Expected post-repair worst interface mismatch. `+∞` means this proposal
    /// has no comparable constructive seam-norm prediction yet.
    pub expected_post_norm: f64,
    /// Cost estimate in seconds (router-modeled where applicable).
    pub cost_s: f64,
}

/// The repair verdict for one model.
#[derive(Debug, Clone, PartialEq)]
pub struct RepairPlan {
    /// The decomposition driving the plan.
    pub split: HodgeSplit,
    /// Ranked proposals (best first).
    pub proposals: Vec<RepairProposal>,
    /// Gauge offsets the eligible exact-component step would apply (per patch).
    pub gauge: Vec<f64>,
    /// True when the exact-component gauge step fits EVERY patch budget.
    /// This does not claim the complete repair is automatic when coexact or
    /// retained harmonic components remain.
    pub gauge_step_eligible: bool,
    /// Interfaces in the retained harmonic support with their magnitudes.
    /// This is not a graph-theoretic minimal cut-set.
    pub harmonic_support: Vec<((usize, usize), f64)>,
    /// Structured reason an optional router alternative could not be planned.
    /// `None` means no reroute was requested or a proposal was produced.
    pub reroute_error: Option<RoutePlanError>,
}

/// Threshold below which a component is treated as absent (fractions).
pub const COMPONENT_FLOOR: f64 = 1e-6;

/// Choose a deterministic maximum-slack midpoint of the feasible constant-shift
/// interval independently on each connected component (or its finite boundary
/// when the interval is half-unbounded). Adding such a constant leaves `δ⁰c`
/// unchanged mathematically; the returned gauge is the representative that the
/// planner will actually apply.
fn gauge_representative_within_budgets(
    skeleton: &SheafSkeleton,
    potential: &[f64],
    budgets: &[f64],
) -> Option<Vec<f64>> {
    if potential.len() != skeleton.n_patches
        || budgets.len() != skeleton.n_patches
        || potential.iter().any(|value| !value.is_finite())
        || budgets
            .iter()
            .any(|budget| budget.is_nan() || *budget < 0.0)
    {
        return None;
    }

    let mut adjacency = vec![Vec::new(); skeleton.n_patches];
    for &(u, v) in &skeleton.edges {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }

    let mut gauge = potential.to_vec();
    let mut seen = vec![false; skeleton.n_patches];
    for root in 0..skeleton.n_patches {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut stack = vec![root];
        let mut component = Vec::new();
        while let Some(patch) = stack.pop() {
            component.push(patch);
            for &neighbor in &adjacency[patch] {
                if !seen[neighbor] {
                    seen[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }

        let mut lower = f64::NEG_INFINITY;
        let mut upper = f64::INFINITY;
        for &patch in &component {
            let budget = budgets[patch];
            if budget.is_finite() {
                lower = lower.max(-budget - potential[patch]);
                upper = upper.min(budget - potential[patch]);
            }
        }
        if lower > upper {
            return None;
        }
        let shift = match (lower.is_finite(), upper.is_finite()) {
            (true, true) => f64::midpoint(lower, upper),
            (true, false) => lower,
            (false, true) => upper,
            (false, false) => 0.0,
        };
        if !shift.is_finite() {
            return None;
        }
        for patch in component {
            let shifted = potential[patch] + shift;
            if !shifted.is_finite() || shifted.abs() > budgets[patch] {
                return None;
            }
            gauge[patch] = shifted;
        }
    }
    Some(gauge)
}

/// Build the repair plan: decompose, interpret, rank. `budgets` is each
/// patch's declared error budget — the exact-component gauge repair is
/// only auto-appliable when |offset| stays within it for EVERY patch
/// (a repair must never silently distort geometry beyond budget).
/// `reroute` optionally consults the Rep Router for a conversion-based
/// alternative for the worst-offending patch.
#[must_use]
pub fn plan_repair(
    skeleton: &SheafSkeleton,
    mismatch: &[f64],
    budgets: &[f64],
    reroute: Option<(&Router, &dyn CostOracle, &RouteRequest)>,
) -> RepairPlan {
    // One gauge budget per patch. Without this, the per-patch budget check below
    // (`potential.iter().zip(budgets)`) would silently TRUNCATE to the shorter
    // length: a short `budgets` leaves the trailing patches unchecked, so
    // `gauge_step_eligible` could report true while an unchecked patch's gauge
    // offset exceeds its budget — silently distorting geometry beyond budget,
    // the one thing this planner promises never to do. Fail closed, matching
    // `hodge_decompose`'s cochain-size assertion.
    assert_eq!(
        budgets.len(),
        skeleton.n_patches,
        "one gauge budget per patch"
    );
    let split = hodge_decompose(skeleton, mismatch);
    let feasible_gauge = gauge_representative_within_budgets(skeleton, &split.potential, budgets);
    let gauge_step_is_feasible = feasible_gauge.is_some();
    let gauge = feasible_gauge.unwrap_or_else(|| split.potential.clone());
    let residual_after_exact = apply_gauge(skeleton, mismatch, &gauge);
    let expected_after_gauge = residual_after_exact
        .iter()
        .fold(0.0f64, |a, &b| a.max(b.abs()));
    let gauge_step_eligible = split.fractions.0 > COMPONENT_FLOOR && gauge_step_is_feasible;
    let mut proposals = Vec::new();
    if split.fractions.0 > COMPONENT_FLOOR {
        proposals.push(gauge_proposal(
            &gauge,
            gauge_step_eligible,
            expected_after_gauge,
        ));
    }
    if split.fractions.1 > COMPONENT_FLOOR {
        proposals.push(coexact_proposal(skeleton, mismatch));
    }
    // First require the whole retained component to be significant relative
    // to the input mismatch. Otherwise scaling a localization threshold by the
    // remainder's own maximum guarantees that even roundoff residue promotes
    // itself into scary-looking support and a +inf proposal. Once admitted,
    // use a within-component relative amplitude floor to localize it without a
    // unit-dependent absolute threshold. The raw split always retains the
    // sub-floor remainder for diagnosis.
    let mut harmonic_support: Vec<((usize, usize), f64)> = if split.fractions.2 > COMPONENT_FLOOR {
        let harmonic_scale = split
            .harmonic
            .iter()
            .fold(0.0f64, |scale, value| scale.max(value.abs()));
        let support_floor = harmonic_scale * COMPONENT_FLOOR.sqrt();
        skeleton
            .edges
            .iter()
            .zip(&split.harmonic)
            .filter(|(_, h)| h.abs() > support_floor)
            .map(|(&e, &h)| (e, h.abs()))
            .collect()
    } else {
        Vec::new()
    };
    harmonic_support.sort_by(|a, b| b.1.total_cmp(&a.1));
    if !harmonic_support.is_empty() {
        proposals.push(RepairProposal {
            action: format!(
                "retained harmonic remainder after deterministic gauge projection; no \
                 generic exactness or topology claim; inspect interface support {:?}",
                harmonic_support.iter().map(|(e, _)| *e).collect::<Vec<_>>()
            ),
            expected_post_norm: f64::INFINITY,
            cost_s: f64::INFINITY,
        });
    }
    let mut reroute_error = None;
    if let Some((router, oracle, req)) = reroute {
        match router.plan(req, oracle) {
            Ok(route) => proposals.push(RepairProposal {
                action: format!(
                    "reroute worst patch {} -> {} via [{}] (router-planned alternative chart)",
                    req.from,
                    req.to,
                    route.edges().join(", ")
                ),
                expected_post_norm: f64::INFINITY,
                cost_s: route.predicted_cost_s(),
            }),
            Err(error) => reroute_error = Some(error),
        }
    }
    proposals.sort_by(|a, b| {
        a.expected_post_norm
            .total_cmp(&b.expected_post_norm)
            .then(a.cost_s.total_cmp(&b.cost_s))
    });
    RepairPlan {
        gauge,
        split,
        proposals,
        gauge_step_eligible,
        harmonic_support,
        reroute_error,
    }
}

/// The exact-component proposal: the concrete per-patch gauge projection.
fn gauge_proposal(gauge: &[f64], gauge_step_eligible: bool, expected: f64) -> RepairProposal {
    let worst = gauge
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map_or(0, |(i, _)| i);
    let mut action = format!(
        "project patch P{worst} gauge by {:+.3e} (exact-component section \
         projection; offsets per patch: [",
        gauge[worst]
    );
    for (i, off) in gauge.iter().enumerate() {
        if i > 0 {
            action.push_str(", ");
        }
        let _ = write!(action, "{off:+.3e}");
    }
    action.push_str("])");
    if !gauge_step_eligible {
        let _ = write!(action, " — EXCEEDS a patch budget; needs acceptance");
    }
    RepairProposal {
        action,
        expected_post_norm: expected,
        cost_s: 0.001, // local gauge arithmetic
    }
}

/// The coexact-component proposal: a non-causal diagnostic localized to the
/// retained triangle with the largest circulation residual.
fn coexact_proposal(skeleton: &SheafSkeleton, mismatch: &[f64]) -> RepairProposal {
    let d1m = skeleton.d1(mismatch);
    let worst_tri = skeleton
        .triangles
        .iter()
        .enumerate()
        .max_by(|a, b| d1m[a.0].abs().total_cmp(&d1m[b.0].abs()))
        .map(|(_, t)| *t);
    RepairProposal {
        action: format!(
            "coexact circulation candidate around retained triangle {worst_tri:?}: inspect \
             chart/model/junction/sampling evidence and converter orientation/trace \
             conventions; algebra alone does not assign cause"
        ),
        expected_post_norm: f64::INFINITY,
        cost_s: 0.0,
    }
}

/// Apply one algebraic gauge correction to an edge cochain:
/// `m ← m − δ⁰c`. Re-planning a converged repaired model can yield a zero
/// follow-up gauge; applying the same nonzero gauge twice is not idempotent.
/// This does not mutate or re-evaluate any source chart.
#[must_use]
pub fn apply_gauge(skeleton: &SheafSkeleton, mismatch: &[f64], gauge: &[f64]) -> Vec<f64> {
    assert_eq!(
        mismatch.len(),
        skeleton.edges.len(),
        "one mismatch value per interface"
    );
    assert_eq!(gauge.len(), skeleton.n_patches, "one gauge value per patch");
    let correction = skeleton.d0(gauge);
    mismatch
        .iter()
        .zip(&correction)
        .map(|(m, c)| m - c)
        .collect()
}
