//! fs-neuroshape-e2e — NeuroShapeCert: certified facts about a neural implicit.
//! Layer: L5 (LUMEN).
//!
//! # The campaign
//!
//! A learned neural SDF renders a shape, but gives no guarantees: how far can a
//! sphere-tracing ray step without tunneling through a thin feature, and which
//! topology facts are actually certified? This campaign proves a safe step and
//! the existence of at least one enclosed negative component. It deliberately
//! makes no exact component-count claim, composing crates never designed to meet:
//!
//! - **The field** ([`fs_rep_neural`]): a small `tanh`-MLP SDF whose
//!   spectral-normalized effective form is `≈ 2.12·Σ tanh(3(±coord − 0.7)) + 6.5`
//!   — provably negative near the origin, provably positive on a surrounding ring.
//! - **A certified Lipschitz constant** — `L = Π Uᵢ` (product of outward
//!   spectral-norm upper bounds). A degenerate IBP enclosure at the origin
//!   supplies a certified lower sign margin; dividing it downward by `L`
//!   yields a sphere-trace step that CANNOT tunnel through the surface.
//! - **A topology certificate by interval arithmetic** — the network's sound
//!   Interval Bound Propagation (`eval_interval`) proves a central box is
//!   strictly inside (`hi < 0`) and that the FOUR edge strips of a bounding box
//!   are strictly outside (`lo > 0`). Those strips tile the box boundary into a
//!   CLOSED frame (corners overlap), so the component meeting the negative
//!   central box cannot cross it: at least one component is proven to exist and
//!   be ENCLOSED — a proof, not a mesh. (Discrete ring boxes would leave angular
//!   gaps and prove no enclosure theorem.)
//! - **Typed topology evidence**: the negative central box and positive closed
//!   frame construct a [`CertifiedEnclosedComponentExists`]. Its public
//!   [`ComponentCountEvidence`] reports only the global lower bound `>= 1`;
//!   disconnected interior wells or negative exterior regions remain possible.
//! - **A curvature cross-check** ([`fs_viz`]): the origin has a positive-definite
//!   finite-difference Hessian. Without a certified zero gradient this is not a
//!   critical-point or minimum theorem, and never a component-count proof.
//!   `isocontour_crossings` separately localizes the sampled zero set.
//! Same-build bit-deterministic replay; retained cross-ISA evidence is required
//! before attaching a portable G5 receipt.

use fs_rep_neural::{
    Layer, MLP_ACTIVATION_SEMANTICS, MLP_ACTIVATION_SEMANTICS_VERSION, MLP_ACTIVATION_ULP_BUDGET,
    MlpSdf, NeuralFieldIdentity, SAFE_STEP_POLICY, SAFE_STEP_POLICY_VERSION, SafeStepDerivation,
    derive_safe_step,
};
use fs_viz::{CriticalKind, Grid2, Vec2, classify_hessian};
use std::fmt;

/// Version of the public component-evidence semantics carried by
/// [`NeuroShapeReport`].
///
/// Version 1 means that enclosed-component evidence carries only a global
/// lower bound, while an exact component count remains unavailable. Adapters
/// serializing these fields must carry this value so consumers can refuse
/// layouts whose topology semantics they do not understand.
pub const NEUROSHAPE_COMPONENT_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Public campaign input named by a structured admission refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignParameter {
    /// Outer half-width used by the frame and visualization grid.
    RingRadius,
    /// Half-width of the central interval-negative square.
    InnerHalfWidth,
}

impl fmt::Display for CampaignParameter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RingRadius => "ring_r",
            Self::InnerHalfWidth => "inner",
        })
    }
}

/// Structured campaign admission refusal for untrusted callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignError {
    /// NeuroShapeCert is currently a two-dimensional theorem/campaign.
    InputDimension {
        /// Input dimension the campaign requires (always `2` in this tranche).
        expected: usize,
        /// Input dimension the supplied network actually declares.
        actual: usize,
    },
    /// A geometric parameter was NaN or infinite.
    NonFiniteParameter(CampaignParameter),
    /// `ring_r` was non-positive or `inner` was negative.
    OutOfRangeParameter(CampaignParameter),
}

impl fmt::Display for CampaignError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputDimension { expected, actual } => write!(
                formatter,
                "NeuroShape input dimension mismatch: expected {expected}, got {actual}"
            ),
            Self::NonFiniteParameter(parameter) => {
                write!(formatter, "NeuroShape parameter {parameter} must be finite")
            }
            Self::OutOfRangeParameter(CampaignParameter::RingRadius) => {
                formatter.write_str("NeuroShape parameter ring_r must be positive")
            }
            Self::OutOfRangeParameter(CampaignParameter::InnerHalfWidth) => {
                formatter.write_str("NeuroShape parameter inner must be non-negative")
            }
        }
    }
}

impl std::error::Error for CampaignError {}

/// The blob SDF network. `MlpSdf::new` spectral-normalizes every layer to
/// exactly `bound`, so with `bound = √18` the effective hidden slope is
/// `√18/√2 = 3` (a wall at `|coord| = 0.7`) and the effective output weight is
/// `√18/2 ≈ 2.12`. With the biases below the effective field is
/// `f ≈ 2.12·Σ tanh(3(±coord − 0.7)) + 6.5`: negative near the origin, positive
/// on a surrounding ring. `L = bound² = 18`.
#[must_use]
pub fn blob_sdf_net() -> MlpSdf {
    // Hidden layer: one tanh wall per ±axis direction (bias −2.1 ⇒ wall at 0.7).
    let l1 = Layer::new(
        vec![
            vec![3.0, 0.0],
            vec![-3.0, 0.0],
            vec![0.0, 3.0],
            vec![0.0, -3.0],
        ],
        vec![-2.1, -2.1, -2.1, -2.1],
    );
    // Linear output: sum the walls, lift by +6.5 (bias is not normalized).
    let l2 = Layer::new(vec![vec![1.0, 1.0, 1.0, 1.0]], vec![6.5]);
    MlpSdf::new(vec![l1, l2], (18.0_f64).sqrt())
}

fn is_finite_ordered_interval((lo, hi): (f64, f64)) -> bool {
    lo.is_finite() && hi.is_finite() && lo <= hi
}

/// A constructor-sealed, campaign-local witness that at least one connected
/// component of `{f < 0}` exists and is enclosed by the certified-positive
/// boundary frame.
///
/// `MlpSdf` is a continuous composition of affine maps and `tanh`: the connected
/// negative central square therefore lies in one negative component, and any
/// path from that component to the exterior must cross the positive frame.
///
/// The private fields are important: callers can inspect or clone a witness
/// produced by [`run_campaign`], but cannot manufacture one through safe public
/// constructors from booleans or a sampled contour. This value has no field,
/// source, unit, budget, or receipt identity and therefore is not portable
/// authority. It proves neither that the full negative set is bounded nor that
/// its global component count is exactly one.
#[derive(Debug, Clone, PartialEq)]
pub struct CertifiedEnclosedComponentExists {
    central_box_half_width: f64,
    central_box_interval: (f64, f64),
    boundary_frame_outer_half_width: f64,
    boundary_frame_inner_half_width: f64,
    boundary_strip_intervals: [(f64, f64); 4],
}

impl CertifiedEnclosedComponentExists {
    fn from_interval_frame(
        central_box_half_width: f64,
        central_box_interval: (f64, f64),
        boundary_frame_outer_half_width: f64,
        boundary_frame_width: f64,
        boundary_strip_intervals: [(f64, f64); 4],
    ) -> Option<Self> {
        let boundary_frame_inner_half_width =
            boundary_frame_outer_half_width - boundary_frame_width;
        if !central_box_half_width.is_finite()
            || central_box_half_width < 0.0
            || !boundary_frame_outer_half_width.is_finite()
            || !boundary_frame_width.is_finite()
            || boundary_frame_width <= 0.0
            || !boundary_frame_inner_half_width.is_finite()
            || boundary_frame_inner_half_width <= central_box_half_width
            || !is_finite_ordered_interval(central_box_interval)
            || central_box_interval.1 >= 0.0
            || boundary_strip_intervals
                .iter()
                .any(|&interval| !is_finite_ordered_interval(interval) || interval.0 <= 0.0)
        {
            return None;
        }

        Some(Self {
            central_box_half_width,
            central_box_interval,
            boundary_frame_outer_half_width,
            boundary_frame_inner_half_width,
            boundary_strip_intervals,
        })
    }

    /// Half-width of the central square certified strictly negative.
    #[must_use]
    pub const fn central_box_half_width(&self) -> f64 {
        self.central_box_half_width
    }

    /// Sound IBP enclosure over the central square.
    #[must_use]
    pub const fn central_box_interval(&self) -> (f64, f64) {
        self.central_box_interval
    }

    /// Outer half-width of the square boundary frame.
    #[must_use]
    pub const fn boundary_frame_outer_half_width(&self) -> f64 {
        self.boundary_frame_outer_half_width
    }

    /// Inner half-width of the square boundary frame.
    #[must_use]
    pub const fn boundary_frame_inner_half_width(&self) -> f64 {
        self.boundary_frame_inner_half_width
    }

    /// Sound IBP enclosures for the top, bottom, left, and right frame strips.
    #[must_use]
    pub const fn boundary_strip_intervals(&self) -> &[(f64, f64); 4] {
        &self.boundary_strip_intervals
    }
}

/// What the campaign can state about the global number of negative components.
///
/// This enum is non-exhaustive so a future global topology certificate can add
/// an exact-count state without turning today's lower-bound witness into one.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ComponentCountEvidence {
    /// No positive global component-count statement is certified.
    Unknown,
    /// The closed interval frame certifies that at least one enclosed component
    /// exists. The upper bound remains unknown.
    LowerBound(CertifiedEnclosedComponentExists),
}

impl ComponentCountEvidence {
    /// Certified lower bound on the global component count.
    #[must_use]
    pub const fn lower_bound(&self) -> usize {
        match self {
            Self::Unknown => 0,
            Self::LowerBound(_) => 1,
        }
    }

    /// Certified exact global component count, when available.
    ///
    /// Phase 0 exposes no exact-count certificate, so this is always `None`.
    #[must_use]
    pub const fn exact_count(&self) -> Option<usize> {
        match self {
            Self::Unknown | Self::LowerBound(_) => None,
        }
    }
}

/// The campaign report.
#[derive(Debug, Clone)]
pub struct NeuroShapeReport {
    /// Content identity of the normalized MLP and certificate arithmetic.
    pub field_identity: NeuralFieldIdentity,
    /// Governed hidden-activation semantic version.
    pub activation_semantics_version: u32,
    /// Governed hidden-activation semantic name.
    pub activation_semantics: &'static str,
    /// ULP budget used by the interval activation enclosure.
    pub activation_ulp_budget: u64,
    /// Safe-step derivation semantic version.
    pub safe_step_policy_version: u32,
    /// Safe-step derivation semantic name.
    pub safe_step_policy: &'static str,
    /// The certified global Lipschitz constant `L`.
    pub lipschitz: f64,
    /// Nominal field value at the origin, retained for visualization only.
    pub origin_value: f64,
    /// Interval-derived sign margin and downward-rounded no-tunnel step.
    pub safe_step: SafeStepDerivation,
    /// IBP enclosure of `f` over the central box.
    pub inside_interval: (f64, f64),
    /// Is the central box certified strictly inside (`hi < 0`)?
    pub certified_inside: bool,
    /// How many of the box-boundary strips are certified strictly outside.
    pub boundary_certified: usize,
    /// Total boundary strips (4 — a CLOSED frame around the box).
    pub boundary_segments: usize,
    /// Is every strip in the closed boundary frame certified strictly positive?
    /// This is a local frame fact, not a claim that the full negative set is
    /// bounded.
    pub boundary_frame_certified: bool,
    /// Typed component-count evidence. A certified frame yields only a lower
    /// bound of one and never an exact count in this tranche.
    pub component_count_evidence: ComponentCountEvidence,
    /// Is the origin's finite-difference Hessian positive definite under the
    /// classifier tolerance? This is curvature corroboration only: without a
    /// certified zero gradient it does not establish a critical point or local
    /// minimum.
    pub origin_hessian_positive_definite: bool,
    /// Number of zero-set crossings found on the visualization grid.
    /// Zero can also accompany rejected localization evidence; in that case
    /// both reported crossing radii are `NaN` rather than the valid-empty
    /// sentinels `0` and `+inf`.
    pub surface_crossings: usize,
    /// The largest radius at which a crossing was found (must be inside the
    /// ring), `0` for a valid empty result, or `NaN` when localization was
    /// rejected.
    pub max_crossing_radius: f64,
    /// The smallest radius at which a crossing was found — the NEAREST surface
    /// point; the safe step radius must under-estimate it (no-tunnel soundness).
    /// A valid empty result is `+inf`; rejected localization evidence is `NaN`.
    pub nearest_surface_radius: f64,
}

fn radius(p: Vec2) -> f64 {
    p[0].hypot(p[1])
}

/// Run the NeuroShapeCert campaign on `net` with a bounding box of half-width
/// `ring_r` (its four edge strips form the closed barrier) and a central
/// certified-inside box of half-width `inner`.
///
/// # Panics
///
/// Panics on invalid campaign inputs. Use [`try_run_campaign`] at untrusted
/// boundaries that need a structured non-trapping refusal.
#[must_use]
pub fn run_campaign(net: &MlpSdf, ring_r: f64, inner: f64) -> NeuroShapeReport {
    try_run_campaign(net, ring_r, inner).unwrap_or_else(|error| panic!("{error}"))
}

/// Fallible NeuroShape campaign admission and execution.
pub fn try_run_campaign(
    net: &MlpSdf,
    ring_r: f64,
    inner: f64,
) -> Result<NeuroShapeReport, CampaignError> {
    if net.input_dim() != 2 {
        return Err(CampaignError::InputDimension {
            expected: 2,
            actual: net.input_dim(),
        });
    }
    for (parameter, value) in [
        (CampaignParameter::RingRadius, ring_r),
        (CampaignParameter::InnerHalfWidth, inner),
    ] {
        if !value.is_finite() {
            return Err(CampaignError::NonFiniteParameter(parameter));
        }
    }
    if ring_r <= 0.0 {
        return Err(CampaignError::OutOfRangeParameter(
            CampaignParameter::RingRadius,
        ));
    }
    if inner < 0.0 {
        return Err(CampaignError::OutOfRangeParameter(
            CampaignParameter::InnerHalfWidth,
        ));
    }

    let lipschitz = net.lipschitz();
    let origin_value = net.eval(&[0.0, 0.0]);
    let origin_interval = net.eval_interval(&[0.0, 0.0], &[0.0, 0.0]);
    let safe_step = derive_safe_step(origin_interval, lipschitz);

    // Interval topology certificate.
    let inside_interval = net.eval_interval(&[-inner, -inner], &[inner, inner]);
    let certified_inside = is_finite_ordered_interval(inside_interval) && inside_interval.1 < 0.0;

    // A CLOSED barrier: the four edge strips of the box [−R, R]² tile the whole
    // boundary frame (corners overlap), so certifying every strip strictly
    // outside (lo > 0) RIGOROUSLY traps the negative component meeting the
    // central box. It does not exclude other interior or exterior components.
    // Eight discrete boxes would leave angular gaps and prove no enclosure.
    let r = ring_r;
    let w = 0.4;
    let strips = [
        ([-r, r - w], [r, r]),   // top
        ([-r, -r], [r, -r + w]), // bottom
        ([-r, -r], [-r + w, r]), // left
        ([r - w, -r], [r, r]),   // right
    ];
    let boundary_segments = strips.len();
    let boundary_strip_intervals = strips.map(|(lo_pt, hi_pt)| net.eval_interval(&lo_pt, &hi_pt));
    let boundary_certified = boundary_strip_intervals
        .iter()
        .filter(|&&interval| is_finite_ordered_interval(interval) && interval.0 > 0.0)
        .count();
    let boundary_frame_certified = boundary_certified == boundary_segments;
    let component_count_evidence = CertifiedEnclosedComponentExists::from_interval_frame(
        inner,
        inside_interval,
        ring_r,
        w,
        boundary_strip_intervals,
    )
    .map_or(
        ComponentCountEvidence::Unknown,
        ComponentCountEvidence::LowerBound,
    );

    // Curvature cross-check at the origin (Hessian by finite difference). This
    // does not establish criticality because the gradient is not certified zero.
    let h = 1e-3;
    let f00 = origin_value;
    let fxx = (net.eval(&[h, 0.0]) - 2.0 * f00 + net.eval(&[-h, 0.0])) / (h * h);
    let fyy = (net.eval(&[0.0, h]) - 2.0 * f00 + net.eval(&[0.0, -h])) / (h * h);
    let fxy = (net.eval(&[h, h]) - net.eval(&[h, -h]) - net.eval(&[-h, h]) + net.eval(&[-h, -h]))
        / (4.0 * h * h);
    let crit = classify_hessian([[fxx, fxy], [fxy, fyy]], 1e-6);
    let origin_hessian_positive_definite = crit.kind == CriticalKind::Minimum;

    // Localize the zero set on a visualization grid.
    const GRID_N: usize = 81;
    const CROSSING_LIMIT: usize = 2 * GRID_N * (GRID_N - 1);
    let crossings = Grid2::from_fn(
        GRID_N,
        GRID_N,
        [-ring_r - 0.5, -ring_r - 0.5],
        [ring_r + 0.5, ring_r + 0.5],
        GRID_N * GRID_N,
        |p| net.eval(&[p[0], p[1]]),
    )
    .ok()
    .and_then(|grid| grid.isocontour_crossings(0.0, CROSSING_LIMIT).ok());
    let (surface_crossings, max_crossing_radius, nearest_surface_radius) =
        if let Some(crossings) = crossings {
            (
                crossings.len(),
                crossings.iter().copied().map(radius).fold(0.0, f64::max),
                crossings
                    .iter()
                    .copied()
                    .map(radius)
                    .fold(f64::INFINITY, f64::min),
            )
        } else {
            // The interval topology certificate below is independent of this
            // sampled visualization. NaN radii distinguish rejected contour
            // evidence from a valid grid with no crossings (0 / +inf).
            (0, f64::NAN, f64::NAN)
        };

    Ok(NeuroShapeReport {
        field_identity: net.identity(),
        activation_semantics_version: MLP_ACTIVATION_SEMANTICS_VERSION,
        activation_semantics: MLP_ACTIVATION_SEMANTICS,
        activation_ulp_budget: MLP_ACTIVATION_ULP_BUDGET,
        safe_step_policy_version: SAFE_STEP_POLICY_VERSION,
        safe_step_policy: SAFE_STEP_POLICY,
        lipschitz,
        origin_value,
        safe_step,
        inside_interval,
        certified_inside,
        boundary_certified,
        boundary_segments,
        boundary_frame_certified,
        component_count_evidence,
        origin_hessian_positive_definite,
        surface_crossings,
        max_crossing_radius,
        nearest_surface_radius,
    })
}
