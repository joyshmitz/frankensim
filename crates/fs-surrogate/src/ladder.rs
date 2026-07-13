//! THE ABSTRACTION LADDER (addendum Proposal A, bead knh1.4; [F] —
//! behind the `abstraction-ladder` feature): reduced-order VIEWS at
//! multiple abstraction levels whose fidelity to the level below is a
//! QUANTIFIED, leak-alarmed estimator — the operator reasons at the concept
//! level until the estimator says the abstraction is no longer faithful
//! HERE, and drills down only there.
//!
//! THE BEACHHEAD: REDUCED-BASIS for the affine-parametric elliptic
//! family `−(a(x;μ) u′)′ = f` with `a = 1 + μ·χ` — offline snapshots +
//! energy-orthonormal basis + online k×k Galerkin, and the TEXTBOOK
//! a-posteriori bound `‖u−u_rb‖_a ≤ ‖r‖_{V′}/√α_LB` with the residual
//! dual norm assembled offline and the coercivity floor exact for the
//! affine family. The compliance QoI estimator retains both the classic
//! squared energy estimator and the floating reduced solve's computable
//! Galerkin defect.
//!
//! EVIDENCE HONESTY (bead y6yv): every color this module emits is
//! ESTIMATED. The RB bound is real mathematics over the reals, but it
//! is COMPUTED here in round-to-nearest f64 with no outward rounding,
//! no Riesz-solve error control, and no linear-solve certificate — a
//! textbook bound evaluated in floating point is NOT an executable
//! enclosure, so it cannot mint `Color::Verified`. Likewise level 0 is
//! the DECLARED FE truth semantics, but its Thomas solve is plain
//! floating point: a point value under declared semantics, not a
//! verified interval. The certified-ladder DESTINATION (outward-rounded
//! residual/Riesz/solve certificates upgrading these colors to
//! Verified) is recorded in the CONTRACT as future scope; the type
//! system keeps today's estimates from impersonating it.

use core::{fmt, mem::size_of};
use fs_evidence::{Color, validate_color_payload};

/// Largest full-order dimension a ladder will allocate (n vectors of n
/// f64 during training: 1<<20 keeps worst-case snapshots well bounded).
pub const MAX_TRUTH_NODES: usize = 1 << 20;
/// Largest RB basis size per rung.
pub const MAX_RB_DIM: usize = 512;
/// Largest concept grid / probe count.
pub const MAX_CONCEPT_POINTS: usize = 4096;
/// Largest number of RB rungs in one ladder.
pub const MAX_RB_LEVELS: usize = 16;
/// Maximum aggregate basis/scratch/dense storage estimated for one
/// training plan (512 MiB).
pub const MAX_TRAINING_BYTES: usize = 512 * 1024 * 1024;
/// Maximum scalar work estimate admitted for one training plan.
pub const MAX_TRAINING_WORK_UNITS: usize = 1_000_000_000;
/// Maximum points on either axis of an RB-coverage battery.
pub const MAX_COVERAGE_AXIS_POINTS: usize = 4096;
/// Maximum Cartesian-product size of an RB-coverage battery.
pub const MAX_COVERAGE_QUERIES: usize = 1_000_000;
/// Maximum conservative scalar work admitted for one synchronous coverage
/// battery, including every possible RB descent and one truth fallback per
/// parameter plus every tolerance comparison.
pub const MAX_COVERAGE_WORK_UNITS: usize = 1_000_000_000;

const DEFAULT_CONCEPT_GRID_POINTS: usize = 5;
const DEFAULT_CONCEPT_PROBES: usize = 7;

const CONCEPT_ESTIMATOR_ID: &str = "fs-surrogate.concept-cross-rung-v1";
const RB_ESTIMATOR_ID: &str = "fs-surrogate.rb-a-posteriori-f64-v1";
const TRUTH_ESTIMATOR_ID: &str = "fs-surrogate.fe-truth-f64-no-certificate-v1";

/// A structured ladder refusal (Decalogue P10): every invalid input or
/// non-finite derived quantity is a named error, never a panic, NaN, or
/// silently wrong color.
#[derive(Debug, Clone, PartialEq)]
pub enum SurrogateError {
    /// Full-order dimension outside `1..=MAX_TRUTH_NODES`.
    InvalidDimension {
        /// The rejected n.
        n: usize,
    },
    /// A μ-range that is non-finite, empty, or reversed.
    InvalidRange {
        /// The rejected range.
        lo: f64,
        /// Upper end.
        hi: f64,
    },
    /// A parameter that breaks coercivity (`1 + μ ≤ 0`) or is
    /// non-finite — the bilinear form is no longer elliptic and every
    /// bound downstream is meaningless.
    InvalidCoercivity {
        /// The rejected μ.
        mu: f64,
    },
    /// A query parameter outside the rung's declared training range —
    /// estimator constants were calibrated on the range, so extrapolated
    /// ladder claims are refused.
    OutOfRange {
        /// The rejected μ.
        mu: f64,
        /// The declared range.
        range: (f64, f64),
    },
    /// A tolerance that is non-finite or non-positive.
    InvalidTolerance {
        /// The rejected tolerance.
        tol: f64,
    },
    /// Basis/grid/probe geometry outside its operation's allowed bounds.
    InvalidGeometry {
        /// Which knob.
        what: &'static str,
        /// The rejected value.
        got: usize,
    },
    /// A vector supplied to a public numerical operation has the wrong shape.
    InvalidVectorLength {
        /// Operation/operand identity.
        what: &'static str,
        /// Required length.
        expected: usize,
        /// Supplied length.
        got: usize,
    },
    /// An input vector contains a NaN or infinity.
    NonFiniteInput {
        /// Operation/operand identity.
        what: &'static str,
        /// First invalid element.
        index: usize,
    },
    /// A requested start rung does not exist.
    InvalidLevel {
        /// Requested rung.
        requested: usize,
        /// Highest declared rung.
        top: usize,
    },
    /// A rung is not bound to the ladder's truth-space/range identity.
    FamilyMismatch {
        /// Mismatched component.
        what: &'static str,
    },
    /// An RB ladder has too many rungs.
    TooManyRbLevels {
        /// Requested rung count.
        got: usize,
        /// Configured cap.
        max: usize,
    },
    /// One requested RB dimension is empty, exceeds its cap, or exceeds
    /// the truth-space dimension.
    InvalidRbDimension {
        /// Zero-based rung position.
        rung: usize,
        /// Requested basis dimension.
        dimension: usize,
        /// Maximum admitted at this rung.
        maximum: usize,
    },
    /// Rungs must become strictly coarser as their index increases.
    NonDecreasingFidelity {
        /// Zero-based position of the offending rung.
        rung: usize,
        /// Previous (finer) requested dimension.
        previous: usize,
        /// Current requested dimension.
        current: usize,
    },
    /// Orthogonalization retained a rung that is not strictly coarser
    /// than the prior rung, despite decreasing requested dimensions.
    NonDecreasingRetainedFidelity {
        /// Zero-based position of the offending rung.
        rung: usize,
        /// Prior retained basis dimension.
        previous: usize,
        /// Current retained basis dimension.
        current: usize,
        /// Current requested basis dimension.
        requested: usize,
    },
    /// Floating-point interpolation could not represent a strictly
    /// increasing grid.
    UnrepresentableGrid {
        /// Grid identity.
        what: &'static str,
        /// Index of the point that failed strict ordering.
        index: usize,
        /// Previous representable point.
        previous: f64,
        /// Candidate next point.
        next: f64,
    },
    /// Checked integer arithmetic overflowed while sizing work.
    BudgetArithmeticOverflow {
        /// Budget being computed.
        resource: &'static str,
    },
    /// A preflighted training resource exceeds its hard cap.
    BudgetExceeded {
        /// Resource identity.
        resource: &'static str,
        /// Requested amount.
        requested: usize,
        /// Configured cap.
        limit: usize,
    },
    /// A coverage axis is empty.
    EmptyCoverageBattery {
        /// Empty axis (`mu` or `tolerance`).
        axis: &'static str,
    },
    /// A coverage axis exceeds its independent cap.
    CoverageAxisTooLarge {
        /// Oversized axis.
        axis: &'static str,
        /// Requested count.
        requested: usize,
        /// Configured cap.
        limit: usize,
    },
    /// The coverage Cartesian product exceeds its cap.
    CoverageProductTooLarge {
        /// Requested query count.
        requested: usize,
        /// Configured cap.
        limit: usize,
    },
    /// Gram–Schmidt retained no basis vector (degenerate snapshots).
    EmptyBasis,
    /// A dense solve hit a zero or non-finite pivot.
    SingularSystem {
        /// Pivot column.
        column: usize,
    },
    /// A derived quantity (bound, QoI, solution entry) left the finite
    /// domain.
    NonFiniteDerived {
        /// What was being derived.
        what: &'static str,
    },
    /// A quantity that must be nonnegative was computed as negative.
    NegativeEnergy {
        /// Energy identity.
        what: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// An emitted evidence color violated the shared payload grammar.
    InvalidEvidencePayload {
        /// Shared validator diagnostic.
        reason: String,
    },
}

impl fmt::Display for SurrogateError {
    #[allow(clippy::too_many_lines)] // exhaustive, one-to-one structured refusal rendering
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SurrogateError::InvalidDimension { n } => {
                write!(f, "full-order dimension {n} outside 1..={MAX_TRUTH_NODES}")
            }
            SurrogateError::InvalidRange { lo, hi } => {
                write!(f, "mu range [{lo}, {hi}] must be finite with lo < hi")
            }
            SurrogateError::InvalidCoercivity { mu } => write!(
                f,
                "mu = {mu} breaks coercivity (need finite mu with 1 + mu > 0)"
            ),
            SurrogateError::OutOfRange { mu, range } => write!(
                f,
                "mu = {mu} outside the rung's declared training range [{}, {}] — ladder \
                 estimators are never extrapolated",
                range.0, range.1
            ),
            SurrogateError::InvalidTolerance { tol } => {
                write!(f, "tolerance {tol} must be finite and positive")
            }
            SurrogateError::InvalidGeometry { what, got } => {
                write!(f, "{what} = {got} is outside its allowed bounds")
            }
            SurrogateError::InvalidVectorLength {
                what,
                expected,
                got,
            } => write!(
                f,
                "{what} length {got} does not match required length {expected}"
            ),
            SurrogateError::NonFiniteInput { what, index } => {
                write!(f, "{what} contains a non-finite value at index {index}")
            }
            SurrogateError::InvalidLevel { requested, top } => {
                write!(
                    f,
                    "ladder level {requested} exceeds declared top level {top}"
                )
            }
            SurrogateError::FamilyMismatch { what } => {
                write!(f, "{what} does not match the ladder family identity")
            }
            SurrogateError::TooManyRbLevels { got, max } => {
                write!(f, "RB rung count {got} exceeds the cap {max}")
            }
            SurrogateError::InvalidRbDimension {
                rung,
                dimension,
                maximum,
            } => write!(
                f,
                "RB rung {rung} dimension {dimension} is outside 1..={maximum}"
            ),
            SurrogateError::NonDecreasingFidelity {
                rung,
                previous,
                current,
            } => write!(
                f,
                "RB rung {rung} dimension {current} must be strictly below prior dimension {previous}"
            ),
            SurrogateError::NonDecreasingRetainedFidelity {
                rung,
                previous,
                current,
                requested,
            } => write!(
                f,
                "RB rung {rung} requested dimension {requested} retained {current}, which is not \
                 strictly below the prior retained dimension {previous}"
            ),
            SurrogateError::UnrepresentableGrid {
                what,
                index,
                previous,
                next,
            } => write!(
                f,
                "{what} point {index} = {next} is not representably above prior point {previous}"
            ),
            SurrogateError::BudgetArithmeticOverflow { resource } => {
                write!(f, "{resource} budget arithmetic overflowed")
            }
            SurrogateError::BudgetExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} request {requested} exceeds hard cap {limit}"),
            SurrogateError::EmptyCoverageBattery { axis } => {
                write!(f, "RB coverage {axis} axis must not be empty")
            }
            SurrogateError::CoverageAxisTooLarge {
                axis,
                requested,
                limit,
            } => write!(
                f,
                "RB coverage {axis} axis count {requested} exceeds cap {limit}"
            ),
            SurrogateError::CoverageProductTooLarge { requested, limit } => write!(
                f,
                "RB coverage Cartesian product {requested} exceeds cap {limit}"
            ),
            SurrogateError::EmptyBasis => {
                write!(
                    f,
                    "Gram–Schmidt retained no basis vector (degenerate snapshots)"
                )
            }
            SurrogateError::SingularSystem { column } => {
                write!(
                    f,
                    "linear solve hit an inadmissible/non-finite pivot at column {column}"
                )
            }
            SurrogateError::NonFiniteDerived { what } => {
                write!(
                    f,
                    "{what} left the finite domain — refusing to color a NaN/Inf"
                )
            }
            SurrogateError::NegativeEnergy { what, value } => {
                write!(f, "{what} computed inadmissible negative energy {value}")
            }
            SurrogateError::InvalidEvidencePayload { reason } => {
                write!(f, "ladder evidence payload is invalid: {reason}")
            }
        }
    }
}

impl core::error::Error for SurrogateError {}

fn require_coercive(mu: f64) -> Result<(), SurrogateError> {
    if !mu.is_finite() || 1.0 + mu <= 0.0 {
        return Err(SurrogateError::InvalidCoercivity { mu });
    }
    Ok(())
}

fn require_range(lo: f64, hi: f64) -> Result<(), SurrogateError> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SurrogateError::InvalidRange { lo, hi });
    }
    require_coercive(lo)?;
    require_coercive(hi)?;
    let width = hi - lo;
    if !width.is_finite() || width <= 0.0 {
        return Err(SurrogateError::InvalidRange { lo, hi });
    }
    let mid = f64::midpoint(lo, hi);
    if !(mid.is_finite() && lo < mid && mid < hi) {
        return Err(SurrogateError::UnrepresentableGrid {
            what: "mu range",
            index: 1,
            previous: lo,
            next: mid,
        });
    }
    Ok(())
}

fn require_query_mu(mu: f64, range: (f64, f64)) -> Result<(), SurrogateError> {
    require_coercive(mu)?;
    if mu < range.0 || mu > range.1 {
        return Err(SurrogateError::OutOfRange { mu, range });
    }
    Ok(())
}

fn validate_vector(
    values: &[f64],
    expected: usize,
    what: &'static str,
) -> Result<(), SurrogateError> {
    if values.len() != expected {
        return Err(SurrogateError::InvalidVectorLength {
            what,
            expected,
            got: values.len(),
        });
    }
    if let Some(index) = values.iter().position(|value| !value.is_finite()) {
        return Err(SurrogateError::NonFiniteInput { what, index });
    }
    Ok(())
}

fn strictly_spaced_grid(
    range: (f64, f64),
    points: usize,
    what: &'static str,
) -> Result<Vec<f64>, SurrogateError> {
    require_range(range.0, range.1)?;
    let mut grid = Vec::with_capacity(points);
    for index in 0..points {
        let value = if points == 1 {
            f64::midpoint(range.0, range.1)
        } else if index == 0 {
            range.0
        } else if index + 1 == points {
            range.1
        } else {
            #[allow(clippy::cast_precision_loss)]
            let t = index as f64 / (points - 1) as f64;
            range.0 + (range.1 - range.0) * t
        };
        if !value.is_finite() {
            return Err(SurrogateError::NonFiniteDerived { what });
        }
        if let Some(&previous) = grid.last()
            && value <= previous
        {
            return Err(SurrogateError::UnrepresentableGrid {
                what,
                index,
                previous,
                next: value,
            });
        }
        grid.push(value);
    }
    Ok(grid)
}

fn strictly_spaced_probe_grid(
    range: (f64, f64),
    points: usize,
) -> Result<Vec<f64>, SurrogateError> {
    require_range(range.0, range.1)?;
    let denominator = points
        .checked_mul(2)
        .ok_or(SurrogateError::BudgetArithmeticOverflow {
            resource: "concept probe grid",
        })?;
    let mut grid = Vec::with_capacity(points);
    for index in 0..points {
        #[allow(clippy::cast_precision_loss)]
        let t = (2 * index + 1) as f64 / denominator as f64;
        let value = range.0 + (range.1 - range.0) * t;
        let previous = grid.last().copied().unwrap_or(range.0);
        if !(value.is_finite() && previous < value && value < range.1) {
            return Err(SurrogateError::UnrepresentableGrid {
                what: "concept probe grid",
                index,
                previous,
                next: value,
            });
        }
        grid.push(value);
    }
    Ok(grid)
}

fn estimated_color(estimator: &'static str, dispersion: f64) -> Result<Color, SurrogateError> {
    let color = Color::Estimated {
        estimator: estimator.to_string(),
        dispersion,
    };
    validate_color_payload(&color).map_err(|error| SurrogateError::InvalidEvidencePayload {
        reason: error.to_string(),
    })?;
    Ok(color)
}

/// Immutable identity binding all rungs to one truth space and μ range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LadderFamily {
    truth_nodes: usize,
    mu_lo_bits: u64,
    mu_hi_bits: u64,
}

impl LadderFamily {
    fn new(truth: &TruthModel, range: (f64, f64)) -> Self {
        LadderFamily {
            truth_nodes: truth.n(),
            mu_lo_bits: range.0.to_bits(),
            mu_hi_bits: range.1.to_bits(),
        }
    }

    /// Full-order interior-node count.
    #[must_use]
    pub fn truth_nodes(self) -> usize {
        self.truth_nodes
    }

    /// Exact floating-point training range encoded by this identity.
    #[must_use]
    pub fn mu_range(self) -> (f64, f64) {
        (
            f64::from_bits(self.mu_lo_bits),
            f64::from_bits(self.mu_hi_bits),
        )
    }
}

/// The full-order "truth" model: P1 finite elements on a uniform grid
/// for `−(a u′)′ = 1`, `u(0) = u(1) = 0`, `a(x;μ) = 1 + μ·χ_{[½,1]}`.
/// The FE model IS level 0's DECLARED semantics — discretization
/// honesty lives in the CONTRACT; its floating-point solve carries no
/// numeric certificate (bead y6yv), so level-0 answers are Estimated.
///
/// Dimensions are SEALED: a `TruthModel` exists only through
/// [`TruthModel::new`], which bounds `n` before any allocation.
#[derive(Debug, Clone)]
pub struct TruthModel {
    n: usize,
}

impl TruthModel {
    /// A bounded truth model.
    ///
    /// # Errors
    /// [`SurrogateError::InvalidDimension`] outside `1..=MAX_TRUTH_NODES`.
    pub fn new(n: usize) -> Result<TruthModel, SurrogateError> {
        if n == 0 || n > MAX_TRUTH_NODES {
            return Err(SurrogateError::InvalidDimension { n });
        }
        Ok(TruthModel { n })
    }

    /// Interior node count.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Assemble and solve at parameter `mu` (Thomas algorithm).
    /// Returns interior nodal values.
    ///
    /// # Errors
    /// [`SurrogateError::InvalidCoercivity`] for non-finite or
    /// non-coercive `mu`; [`SurrogateError::NonFiniteDerived`] if the
    /// solve leaves the finite domain.
    pub fn solve(&self, mu: f64) -> Result<Vec<f64>, SurrogateError> {
        require_coercive(mu)?;
        let n = self.n;
        #[allow(clippy::cast_precision_loss)]
        let h = 1.0 / (n as f64 + 1.0);
        let mut diag = vec![0.0f64; n];
        let mut off = vec![0.0f64; n.saturating_sub(1)];
        let rhs = vec![h; n];
        for i in 0..=n {
            #[allow(clippy::cast_precision_loss)]
            let mid = (i as f64 + 0.5) * h;
            let coefficient = if mid >= 0.5 { 1.0 + mu } else { 1.0 };
            let a = coefficient / h;
            if !(mid.is_finite() && coefficient.is_finite() && coefficient > 0.0 && a.is_finite()) {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "full-order stiffness assembly",
                });
            }
            if i < n {
                diag[i] += a;
                if !diag[i].is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "full-order stiffness diagonal",
                    });
                }
            }
            if i > 0 {
                diag[i - 1] += a;
                if !diag[i - 1].is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "full-order stiffness diagonal",
                    });
                }
            }
            if i > 0 && i < n {
                off[i - 1] -= a;
                if !off[i - 1].is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "full-order stiffness off-diagonal",
                    });
                }
            }
        }
        // Thomas solve (diagonally dominant for coercive a; pivots
        // stay positive, but the finiteness of the RESULT is still
        // checked — refusal beats a colored NaN).
        let mut c = off.clone();
        let mut d = rhs;
        let first_pivot = diag[0];
        if !first_pivot.is_finite() || first_pivot <= 0.0 {
            return Err(SurrogateError::SingularSystem { column: 0 });
        }
        if let Some(c0) = c.first_mut() {
            *c0 /= first_pivot;
            if !c0.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "full-order Thomas coefficient",
                });
            }
        }
        d[0] /= first_pivot;
        if !d[0].is_finite() {
            return Err(SurrogateError::NonFiniteDerived {
                what: "full-order Thomas right-hand side",
            });
        }
        for i in 1..n {
            let m = diag[i] - off[i - 1] * c[i - 1];
            if !m.is_finite() || m <= 0.0 {
                return Err(SurrogateError::SingularSystem { column: i });
            }
            if i < n - 1 {
                c[i] = off[i] / m;
                if !c[i].is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "full-order Thomas coefficient",
                    });
                }
            }
            d[i] = (d[i] - off[i - 1] * d[i - 1]) / m;
            if !d[i].is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "full-order Thomas right-hand side",
                });
            }
        }
        let mut u = d;
        for i in (0..n.saturating_sub(1)).rev() {
            u[i] -= c[i] * u[i + 1];
            if !u[i].is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "full-order back-substitution",
                });
            }
        }
        if u.iter().any(|v| !v.is_finite()) {
            return Err(SurrogateError::NonFiniteDerived {
                what: "full-order solution",
            });
        }
        Ok(u)
    }

    /// The energy inner product `a(u, v; mu)`.
    ///
    /// # Errors
    /// Refuses invalid μ, shape mismatches, non-finite operands, and any
    /// non-finite derived term or accumulation.
    pub fn energy(&self, u: &[f64], v: &[f64], mu: f64) -> Result<f64, SurrogateError> {
        require_coercive(mu)?;
        let n = self.n;
        validate_vector(u, n, "energy left operand")?;
        validate_vector(v, n, "energy right operand")?;
        #[allow(clippy::cast_precision_loss)]
        let h = 1.0 / (n as f64 + 1.0);
        let mut acc = 0.0f64;
        for i in 0..=n {
            #[allow(clippy::cast_precision_loss)]
            let mid = (i as f64 + 0.5) * h;
            let a = if mid >= 0.5 { 1.0 + mu } else { 1.0 };
            let du = if i == 0 {
                u[0]
            } else if i == n {
                -u[n - 1]
            } else {
                u[i] - u[i - 1]
            };
            let dv = if i == 0 {
                v[0]
            } else if i == n {
                -v[n - 1]
            } else {
                v[i] - v[i - 1]
            };
            let term = a * du * dv / h;
            if !term.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "energy element contribution",
                });
            }
            acc += term;
            if !acc.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "energy accumulation",
                });
            }
        }
        Ok(acc)
    }

    /// The compliance QoI `∫ f u = h·Σu` (f = 1).
    ///
    /// # Errors
    /// Refuses a shape mismatch, a non-finite input, or non-finite sum.
    pub fn compliance(&self, u: &[f64]) -> Result<f64, SurrogateError> {
        validate_vector(u, self.n, "compliance operand")?;
        #[allow(clippy::cast_precision_loss)]
        let h = 1.0 / (self.n as f64 + 1.0);
        let mut sum = 0.0;
        for value in u {
            sum += value;
            if !sum.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "compliance accumulation",
                });
            }
        }
        let compliance = h * sum;
        if !compliance.is_finite() {
            return Err(SurrogateError::NonFiniteDerived {
                what: "compliance result",
            });
        }
        Ok(compliance)
    }
}

/// One RB rung: an energy-orthonormal basis with the offline residual
/// machinery for the online dual-norm bound. The bound is honest
/// mathematics evaluated in f64 — its answers are ESTIMATED (module
/// docs).
#[derive(Debug, Clone)]
pub struct RbLevel {
    truth: TruthModel,
    basis: Vec<Vec<f64>>,
    mu_range: (f64, f64),
    family: LadderFamily,
}

impl RbLevel {
    /// OFFLINE: snapshots at `k` training parameters spread over
    /// `mu_range`, Gram–Schmidt in the reference (μ = mid) energy.
    ///
    /// # Errors
    /// Geometry/range refusals; [`SurrogateError::EmptyBasis`] when no
    /// snapshot survives orthonormalization.
    pub fn train(
        truth: &TruthModel,
        mu_range: (f64, f64),
        k: usize,
    ) -> Result<RbLevel, SurrogateError> {
        require_range(mu_range.0, mu_range.1)?;
        validate_training_plan(truth, &[k], false)?;
        let training_grid = strictly_spaced_grid(mu_range, k, "RB training grid")?;
        let mid = f64::midpoint(mu_range.0, mu_range.1);
        let mut basis: Vec<Vec<f64>> = Vec::with_capacity(k);
        for mu in training_grid {
            let mut snap = truth.solve(mu)?;
            for b in &basis {
                let proj = truth.energy(&snap, b, mid)?;
                for (s, bi) in snap.iter_mut().zip(b) {
                    *s -= proj * bi;
                    if !s.is_finite() {
                        return Err(SurrogateError::NonFiniteDerived {
                            what: "RB orthogonalization",
                        });
                    }
                }
            }
            let norm_squared = truth.energy(&snap, &snap, mid)?;
            if norm_squared < 0.0 {
                return Err(SurrogateError::NegativeEnergy {
                    what: "RB snapshot norm",
                    value: norm_squared,
                });
            }
            let norm = norm_squared.sqrt();
            if !norm.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "RB snapshot norm",
                });
            }
            if norm > 1e-10 {
                for s in &mut snap {
                    *s /= norm;
                    if !s.is_finite() {
                        return Err(SurrogateError::NonFiniteDerived {
                            what: "RB basis normalization",
                        });
                    }
                }
                basis.push(snap);
            }
        }
        if basis.is_empty() {
            return Err(SurrogateError::EmptyBasis);
        }
        Ok(RbLevel {
            truth: truth.clone(),
            basis,
            mu_range,
            family: LadderFamily::new(truth, mu_range),
        })
    }

    /// Basis size.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.basis.len()
    }

    /// Declared training/query range.
    #[must_use]
    pub fn mu_range(&self) -> (f64, f64) {
        self.mu_range
    }

    /// Immutable truth-space/range identity.
    #[must_use]
    pub fn family(&self) -> LadderFamily {
        self.family
    }

    /// ONLINE: Galerkin solve in the basis + the energy and QoI bounds
    /// (f64-evaluated — ESTIMATED authority). Returns (`u_rb` in nodal
    /// form, compliance, `energy_bound`, `qoi_bound`).
    ///
    /// # Errors
    /// [`SurrogateError::OutOfRange`] outside the declared training
    /// range (bounds are never extrapolated);
    /// [`SurrogateError::SingularSystem`] /
    /// [`SurrogateError::NonFiniteDerived`] from the dense solve.
    pub fn query(&self, mu: f64) -> Result<(Vec<f64>, f64, f64, f64), SurrogateError> {
        require_query_mu(mu, self.mu_range)?;
        let k = self.basis.len();
        let mut a = vec![vec![0.0f64; k]; k];
        let mut b = vec![0.0f64; k];
        for i in 0..k {
            for (j, aij) in a[i].iter_mut().enumerate() {
                *aij = self.truth.energy(&self.basis[j], &self.basis[i], mu)?;
            }
            b[i] = self.truth.compliance(&self.basis[i])?;
        }
        let coef = solve_dense(&mut a, &mut b)?;
        let n = self.truth.n();
        let mut u = vec![0.0f64; n];
        for (c, basis_vec) in coef.iter().zip(&self.basis) {
            for (ui, bi) in u.iter_mut().zip(basis_vec) {
                *ui += c * bi;
                if !ui.is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "RB solution reconstruction",
                    });
                }
            }
        }
        let s_rb = self.truth.compliance(&u)?;
        let riesz = self.residual_riesz(&u, mu)?;
        let dual_energy = self.truth.energy(&riesz, &riesz, 0.0)?;
        if dual_energy < 0.0 {
            return Err(SurrogateError::NegativeEnergy {
                what: "residual dual norm",
                value: dual_energy,
            });
        }
        let dual_norm = dual_energy.sqrt();
        let alpha_lb = 1.0f64.min(1.0 + mu);
        if !(dual_norm.is_finite() && alpha_lb.is_finite() && alpha_lb > 0.0) {
            return Err(SurrogateError::NonFiniteDerived {
                what: "RB residual norm or coercivity floor",
            });
        }
        let energy_bound = dual_norm / alpha_lb.sqrt();
        // Exact Galerkin orthogonality would make the compliance error equal
        // the energy error squared. Floating elimination leaves a computable
        // defect r(u_rb) = f(u_rb) - a(u_rb,u_rb), which must be retained.
        let reduced_energy = self.truth.energy(&u, &u, mu)?;
        let galerkin_defect = (s_rb - reduced_energy).abs();
        let qoi_bound = energy_bound.mul_add(energy_bound, galerkin_defect);
        if !(energy_bound.is_finite()
            && galerkin_defect.is_finite()
            && qoi_bound.is_finite()
            && qoi_bound >= 0.0)
        {
            return Err(SurrogateError::NonFiniteDerived {
                what: "rb bound arithmetic",
            });
        }
        Ok((u, s_rb, energy_bound, qoi_bound))
    }

    /// Solve the reference-Laplacian Riesz problem for the residual.
    fn residual_riesz(&self, u_rb: &[f64], mu: f64) -> Result<Vec<f64>, SurrogateError> {
        require_query_mu(mu, self.mu_range)?;
        let n = self.truth.n();
        validate_vector(u_rb, n, "RB residual state")?;
        #[allow(clippy::cast_precision_loss)]
        let h = 1.0 / (n as f64 + 1.0);
        let mut rhs = vec![h; n];
        for (i, ri) in rhs.iter_mut().enumerate() {
            let mut acc = 0.0f64;
            for e in [i, i + 1] {
                #[allow(clippy::cast_precision_loss)]
                let mid = (e as f64 + 0.5) * h;
                let a = if mid >= 0.5 { 1.0 + mu } else { 1.0 };
                let du = if e == 0 {
                    u_rb[0]
                } else if e == n {
                    -u_rb[n - 1]
                } else {
                    u_rb[e] - u_rb[e - 1]
                };
                let dphi = if e == i { 1.0 } else { -1.0 };
                let term = a * du * dphi / h;
                if !term.is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "RB residual element contribution",
                    });
                }
                acc += term;
                if !acc.is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "RB residual accumulation",
                    });
                }
            }
            *ri -= acc;
            if !ri.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "RB residual right-hand side",
                });
            }
        }
        let diag = vec![2.0 / h; n];
        let off = vec![-1.0 / h; n.saturating_sub(1)];
        let mut c = off.clone();
        let mut d = rhs;
        let first_pivot = diag[0];
        if !first_pivot.is_finite() || first_pivot <= 0.0 {
            return Err(SurrogateError::SingularSystem { column: 0 });
        }
        if let Some(c0) = c.first_mut() {
            *c0 /= first_pivot;
            if !c0.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "Riesz Thomas coefficient",
                });
            }
        }
        d[0] /= first_pivot;
        if !d[0].is_finite() {
            return Err(SurrogateError::NonFiniteDerived {
                what: "Riesz Thomas right-hand side",
            });
        }
        for i in 1..n {
            let m = diag[i] - off[i - 1] * c[i - 1];
            if !m.is_finite() || m <= 0.0 {
                return Err(SurrogateError::SingularSystem { column: i });
            }
            if i < n - 1 {
                c[i] = off[i] / m;
                if !c[i].is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "Riesz Thomas coefficient",
                    });
                }
            }
            d[i] = (d[i] - off[i - 1] * d[i - 1]) / m;
            if !d[i].is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "Riesz Thomas right-hand side",
                });
            }
        }
        for i in (0..n.saturating_sub(1)).rev() {
            d[i] -= c[i] * d[i + 1];
            if !d[i].is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "Riesz back-substitution",
                });
            }
        }
        Ok(d)
    }
}

fn solve_dense(a: &mut [Vec<f64>], b: &mut [f64]) -> Result<Vec<f64>, SurrogateError> {
    let n = b.len();
    if n == 0 {
        return Err(SurrogateError::InvalidGeometry {
            what: "dense system dimension",
            got: 0,
        });
    }
    if a.len() != n {
        return Err(SurrogateError::InvalidVectorLength {
            what: "dense matrix row count",
            expected: n,
            got: a.len(),
        });
    }
    validate_vector(b, n, "dense right-hand side")?;
    for row in a.iter() {
        validate_vector(row, n, "dense matrix row")?;
    }
    for col in 0..n {
        let mut piv = col;
        for r in col + 1..n {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        a.swap(col, piv);
        b.swap(col, piv);
        let p = a[col][col];
        if p == 0.0 || !p.is_finite() {
            return Err(SurrogateError::SingularSystem { column: col });
        }
        let pivot_rhs = b[col];
        for (r, rhs) in b.iter_mut().enumerate().skip(col + 1) {
            let (rows_before, rows_from_r) = a.split_at_mut(r);
            let pivot_row = &rows_before[col];
            let row = &mut rows_from_r[0];
            let f = row[col] / p;
            if !f.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "dense elimination factor",
                });
            }
            for (arc, pc) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *arc -= f * pc;
                if !arc.is_finite() {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "dense elimination matrix update",
                    });
                }
            }
            *rhs -= f * pivot_rhs;
            if !rhs.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "dense elimination right-hand side",
                });
            }
        }
    }
    let mut x = vec![0.0f64; n];
    for r in (0..n).rev() {
        let mut acc = b[r];
        for c in r + 1..n {
            acc -= a[r][c] * x[c];
            if !acc.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "dense back-substitution accumulation",
                });
            }
        }
        let pivot = a[r][r];
        if pivot == 0.0 || !pivot.is_finite() {
            return Err(SurrogateError::SingularSystem { column: r });
        }
        x[r] = acc / pivot;
        if !x[r].is_finite() {
            return Err(SurrogateError::NonFiniteDerived {
                what: "dense back-substitution",
            });
        }
    }
    Ok(x)
}

/// A NON-RB concept level: QoI lookup by linear interpolation over a
/// training grid, total dispersion calibrated by CROSS-RUNG PROBES as
/// `|concept − lower RB| + lower RB QoI estimator`.
#[derive(Debug, Clone)]
pub struct ConceptLevel {
    grid: Vec<(f64, f64)>,
    dispersion: f64,
    mu_range: (f64, f64),
    family: LadderFamily,
}

impl ConceptLevel {
    /// Build from a training grid and calibrate against a lower rung.
    ///
    /// # Errors
    /// Geometry refusals (`grid_points < 2` would divide by zero;
    /// `probes < 1` calibrates nothing; both capped) and every rung
    /// query refusal.
    pub fn train(
        rb: &RbLevel,
        grid_points: usize,
        probes: usize,
    ) -> Result<ConceptLevel, SurrogateError> {
        if !(2..=MAX_CONCEPT_POINTS).contains(&grid_points) {
            return Err(SurrogateError::InvalidGeometry {
                what: "concept grid points",
                got: grid_points,
            });
        }
        if probes == 0 || probes > MAX_CONCEPT_POINTS {
            return Err(SurrogateError::InvalidGeometry {
                what: "concept probes",
                got: probes,
            });
        }
        let evaluations =
            grid_points
                .checked_add(probes)
                .ok_or(SurrogateError::BudgetArithmeticOverflow {
                    resource: "concept training work",
                })?;
        let work_units = checked_budget_product(
            "concept training work",
            &[rb.truth.n(), rb.dim(), rb.dim(), evaluations],
        )?;
        if work_units > MAX_TRAINING_WORK_UNITS {
            return Err(SurrogateError::BudgetExceeded {
                resource: "concept training work",
                requested: work_units,
                limit: MAX_TRAINING_WORK_UNITS,
            });
        }
        let range = rb.mu_range();
        let training_grid = strictly_spaced_grid(range, grid_points, "concept training grid")?;
        let probe_grid = strictly_spaced_probe_grid(range, probes)?;
        let mut grid = Vec::with_capacity(grid_points);
        for mu in training_grid {
            grid.push((mu, rb.query(mu)?.1));
        }
        let mut level = ConceptLevel {
            grid,
            dispersion: f64::INFINITY,
            mu_range: range,
            family: rb.family(),
        };
        let mut disp = 0.0f64;
        for mu in probe_grid {
            let (_, rb_value, _, rb_qoi_estimator) = rb.query(mu)?;
            let discrepancy = (level.lookup(mu)? - rb_value).abs();
            if !discrepancy.is_finite() {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "concept discrepancy probe",
                });
            }
            let total_dispersion = discrepancy + rb_qoi_estimator;
            if !(rb_qoi_estimator.is_finite()
                && rb_qoi_estimator >= 0.0
                && total_dispersion.is_finite())
            {
                return Err(SurrogateError::NonFiniteDerived {
                    what: "concept total dispersion",
                });
            }
            disp = disp.max(total_dispersion);
        }
        level.dispersion = disp;
        Ok(level)
    }

    /// Linear interpolation lookup (grid length ≥ 2 by construction).
    ///
    /// # Errors
    /// Refuses non-finite, non-coercive, or out-of-range queries before
    /// inspecting the lookup grid; also refuses every non-finite
    /// interpolation result.
    pub fn lookup(&self, mu: f64) -> Result<f64, SurrogateError> {
        require_query_mu(mu, self.mu_range)?;
        let g = &self.grid;
        let pos = g.partition_point(|(m, _)| *m < mu).clamp(1, g.len() - 1);
        let (m0, v0) = g[pos - 1];
        let (m1, v1) = g[pos];
        let width = m1 - m0;
        if !(width.is_finite() && width > 0.0) {
            return Err(SurrogateError::UnrepresentableGrid {
                what: "concept lookup grid",
                index: pos,
                previous: m0,
                next: m1,
            });
        }
        let t = (mu - m0) / width;
        if !(t.is_finite() && (0.0..=1.0).contains(&t)) {
            return Err(SurrogateError::NonFiniteDerived {
                what: "concept interpolation coordinate",
            });
        }
        let value = v0 + t * (v1 - v0);
        if !value.is_finite() {
            return Err(SurrogateError::NonFiniteDerived {
                what: "concept interpolation result",
            });
        }
        Ok(value)
    }

    /// Calibrated maximum cross-rung discrepancy over the probe grid.
    #[must_use]
    pub fn dispersion(&self) -> f64 {
        self.dispersion
    }

    /// Declared training/query range.
    #[must_use]
    pub fn mu_range(&self) -> (f64, f64) {
        self.mu_range
    }

    /// Immutable truth-space/range identity inherited from the RB rung.
    #[must_use]
    pub fn family(&self) -> LadderFamily {
        self.family
    }
}

/// A ladder answer: the value, its (always ESTIMATED — module docs)
/// color, and the drill-down forensics.
#[derive(Debug, Clone)]
pub struct LadderAnswer {
    value: f64,
    color: Color,
    level_used: usize,
    leaks: Vec<usize>,
}

impl LadderAnswer {
    /// QoI value.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Evidence color. Every color emitted by this module is Estimated;
    /// level 0 carries infinite dispersion because it has no numerical
    /// enclosure.
    #[must_use]
    pub fn color(&self) -> &Color {
        &self.color
    }

    /// Level that ultimately answered (0 = full order).
    #[must_use]
    pub fn level_used(&self) -> usize {
        self.level_used
    }

    /// Levels whose estimator exceeded tolerance during descent.
    #[must_use]
    pub fn leaks(&self) -> &[usize] {
        &self.leaks
    }
}

/// The assembled ladder: level 0 = full order (declared truth), higher
/// = more abstract. `at_level(k)` starts at rung k and DESCENDS
/// AUTOMATICALLY on leak — invisible until it leaks.
#[derive(Debug, Clone)]
pub struct Ladder {
    truth: TruthModel,
    rb_levels: Vec<RbLevel>,
    concept: Option<ConceptLevel>,
    family: LadderFamily,
}

/// A view of the ladder starting at a rung.
pub struct LevelView<'a> {
    ladder: &'a Ladder,
    start: usize,
}

fn checked_budget_product(
    resource: &'static str,
    factors: &[usize],
) -> Result<usize, SurrogateError> {
    factors.iter().try_fold(1usize, |product, factor| {
        product
            .checked_mul(*factor)
            .ok_or(SurrogateError::BudgetArithmeticOverflow { resource })
    })
}

#[allow(clippy::too_many_lines)] // one atomic preflight; no allocation may precede any check
fn validate_training_plan(
    truth: &TruthModel,
    rb_dims: &[usize],
    concept: bool,
) -> Result<(), SurrogateError> {
    if rb_dims.is_empty() {
        return Err(SurrogateError::InvalidGeometry {
            what: "rb rung count",
            got: 0,
        });
    }
    if rb_dims.len() > MAX_RB_LEVELS {
        return Err(SurrogateError::TooManyRbLevels {
            got: rb_dims.len(),
            max: MAX_RB_LEVELS,
        });
    }

    let maximum = MAX_RB_DIM.min(truth.n());
    let mut previous = None;
    let mut memory_bytes = 0usize;
    let mut largest_dimension = 0usize;
    let mut work_units = 0usize;
    for (rung, &dimension) in rb_dims.iter().enumerate() {
        if dimension == 0 || dimension > maximum {
            return Err(SurrogateError::InvalidRbDimension {
                rung,
                dimension,
                maximum,
            });
        }
        if let Some(previous) = previous
            && dimension >= previous
        {
            return Err(SurrogateError::NonDecreasingFidelity {
                rung,
                previous,
                current: dimension,
            });
        }
        previous = Some(dimension);
        largest_dimension = largest_dimension.max(dimension);

        let rung_bytes = checked_budget_product(
            "aggregate ladder training memory",
            &[truth.n(), dimension, size_of::<f64>()],
        )?;
        memory_bytes = memory_bytes.checked_add(rung_bytes).ok_or(
            SurrogateError::BudgetArithmeticOverflow {
                resource: "aggregate ladder training memory",
            },
        )?;
        let rung_work =
            checked_budget_product("RB training work", &[truth.n(), dimension, dimension])?;
        work_units =
            work_units
                .checked_add(rung_work)
                .ok_or(SurrogateError::BudgetArithmeticOverflow {
                    resource: "RB training work",
                })?;
    }

    // Conservative peak scratch: one state plus the tridiagonal solve's
    // diagonal/off-diagonal/right-hand-side/work vectors. Dense online
    // training queries additionally hold one k×k matrix.
    let scratch_bytes = checked_budget_product(
        "aggregate ladder training memory",
        &[truth.n(), 6, size_of::<f64>()],
    )?;
    let dense_bytes = checked_budget_product(
        "aggregate ladder training memory",
        &[largest_dimension, largest_dimension, size_of::<f64>()],
    )?;
    memory_bytes = memory_bytes
        .checked_add(scratch_bytes)
        .and_then(|bytes| bytes.checked_add(dense_bytes))
        .ok_or(SurrogateError::BudgetArithmeticOverflow {
            resource: "aggregate ladder training memory",
        })?;

    if concept {
        let coarsest = *rb_dims.last().ok_or(SurrogateError::InvalidGeometry {
            what: "rb rung count",
            got: 0,
        })?;
        let concept_queries = DEFAULT_CONCEPT_GRID_POINTS
            .checked_add(DEFAULT_CONCEPT_PROBES)
            .ok_or(SurrogateError::BudgetArithmeticOverflow {
                resource: "concept training work",
            })?;
        let concept_work = checked_budget_product(
            "concept training work",
            &[truth.n(), coarsest, coarsest, concept_queries],
        )?;
        work_units = work_units.checked_add(concept_work).ok_or(
            SurrogateError::BudgetArithmeticOverflow {
                resource: "aggregate ladder training work",
            },
        )?;
    }

    if memory_bytes > MAX_TRAINING_BYTES {
        return Err(SurrogateError::BudgetExceeded {
            resource: "aggregate ladder training memory",
            requested: memory_bytes,
            limit: MAX_TRAINING_BYTES,
        });
    }
    if work_units > MAX_TRAINING_WORK_UNITS {
        return Err(SurrogateError::BudgetExceeded {
            resource: "aggregate ladder training work",
            requested: work_units,
            limit: MAX_TRAINING_WORK_UNITS,
        });
    }
    Ok(())
}

impl Ladder {
    /// Build: truth + RB rungs of the given basis sizes (descending
    /// fidelity) + a concept rung calibrated against the last RB rung.
    ///
    /// # Errors
    /// Every constructor/training refusal, plus
    /// [`SurrogateError::InvalidGeometry`] for an empty `rb_dims`.
    pub fn build(
        n: usize,
        mu_range: (f64, f64),
        rb_dims: &[usize],
        concept: bool,
    ) -> Result<Ladder, SurrogateError> {
        require_range(mu_range.0, mu_range.1)?;
        let truth = TruthModel::new(n)?;
        validate_training_plan(&truth, rb_dims, concept)?;
        let family = LadderFamily::new(&truth, mu_range);
        let mut rb_levels = Vec::with_capacity(rb_dims.len());
        let mut previous_retained = None;
        for (rung, &k) in rb_dims.iter().enumerate() {
            let level = RbLevel::train(&truth, mu_range, k)?;
            if level.family() != family {
                return Err(SurrogateError::FamilyMismatch { what: "RB rung" });
            }
            if let Some(previous) = previous_retained
                && level.dim() >= previous
            {
                return Err(SurrogateError::NonDecreasingRetainedFidelity {
                    rung,
                    previous,
                    current: level.dim(),
                    requested: k,
                });
            }
            previous_retained = Some(level.dim());
            rb_levels.push(level);
        }
        let concept = if concept {
            let rb = rb_levels.last().ok_or(SurrogateError::InvalidGeometry {
                what: "rb rung count",
                got: 0,
            })?;
            let level =
                ConceptLevel::train(rb, DEFAULT_CONCEPT_GRID_POINTS, DEFAULT_CONCEPT_PROBES)?;
            if level.family() != family {
                return Err(SurrogateError::FamilyMismatch {
                    what: "concept rung",
                });
            }
            Some(level)
        } else {
            None
        };
        Ok(Ladder {
            truth,
            rb_levels,
            concept,
            family,
        })
    }

    /// The view starting at rung `k` (0 = full order, 1.. = RB rungs,
    /// rb_levels.len()+1 = concept rung if present).
    ///
    /// # Errors
    /// [`SurrogateError::InvalidLevel`] when `k` exceeds [`Self::top`].
    pub fn at_level(&self, k: usize) -> Result<LevelView<'_>, SurrogateError> {
        if k > self.top() {
            return Err(SurrogateError::InvalidLevel {
                requested: k,
                top: self.top(),
            });
        }
        Ok(LevelView {
            ladder: self,
            start: k,
        })
    }

    /// Highest rung index.
    #[must_use]
    pub fn top(&self) -> usize {
        self.rb_levels.len() + usize::from(self.concept.is_some())
    }

    /// Number of RB rungs (excluding truth and concept).
    #[must_use]
    pub fn rb_level_count(&self) -> usize {
        self.rb_levels.len()
    }

    /// Immutable RB rungs, finest first.
    #[must_use]
    pub fn rb_levels(&self) -> &[RbLevel] {
        &self.rb_levels
    }

    /// Optional immutable concept rung.
    #[must_use]
    pub fn concept(&self) -> Option<&ConceptLevel> {
        self.concept.as_ref()
    }

    /// Immutable truth-space/range identity shared by every rung.
    #[must_use]
    pub fn family(&self) -> LadderFamily {
        self.family
    }

    /// Declared μ range shared by every ladder query.
    #[must_use]
    pub fn mu_range(&self) -> (f64, f64) {
        self.family.mu_range()
    }
}

impl LevelView<'_> {
    /// Query with AUTOMATIC ESTIMATOR-GUARDED DESCENT: each rung answers only
    /// if its estimator meets `tol`; a leaking rung is recorded and the query
    /// drills down. Level 0 answers unconditionally (it IS the declared
    /// truth semantics — and its color still says ESTIMATED, because a
    /// floating solve carries no certificate; bead y6yv).
    ///
    /// # Errors
    /// [`SurrogateError::InvalidTolerance`] /
    /// [`SurrogateError::InvalidCoercivity`] /
    /// [`SurrogateError::OutOfRange`] before any rung runs; rung solve
    /// refusals propagate.
    pub fn query(&self, mu: f64, tol: f64) -> Result<LadderAnswer, SurrogateError> {
        if !tol.is_finite() || tol <= 0.0 {
            return Err(SurrogateError::InvalidTolerance { tol });
        }
        require_query_mu(mu, self.ladder.mu_range())?;
        let mut leaks = Vec::new();
        let mut level = self.start;
        loop {
            if level > self.ladder.rb_levels.len() {
                let c = self
                    .ladder
                    .concept
                    .as_ref()
                    .ok_or(SurrogateError::InvalidLevel {
                        requested: level,
                        top: self.ladder.top(),
                    })?;
                if c.dispersion() > tol {
                    leaks.push(level);
                    level -= 1;
                    continue;
                }
                let concept_value = c.lookup(mu)?;
                let rb = self
                    .ladder
                    .rb_levels
                    .last()
                    .ok_or(SurrogateError::InvalidGeometry {
                        what: "concept lower RB rung",
                        got: 0,
                    })?;
                let (_, rb_value, _, rb_qoi_estimator) = rb.query(mu)?;
                let local_cross_rung = (concept_value - rb_value).abs();
                let local_dispersion = local_cross_rung + rb_qoi_estimator;
                if !(local_cross_rung.is_finite()
                    && rb_qoi_estimator.is_finite()
                    && rb_qoi_estimator >= 0.0
                    && local_dispersion.is_finite())
                {
                    return Err(SurrogateError::NonFiniteDerived {
                        what: "concept query-local dispersion",
                    });
                }
                let dispersion = c.dispersion().max(local_dispersion);
                if dispersion <= tol {
                    return Ok(LadderAnswer {
                        value: concept_value,
                        color: estimated_color(CONCEPT_ESTIMATOR_ID, dispersion)?,
                        level_used: level,
                        leaks,
                    });
                }
                leaks.push(level);
                level -= 1;
                continue;
            }
            if level >= 1 {
                let rb = &self.ladder.rb_levels[level - 1];
                let (_, s_rb, _, qoi_bound) = rb.query(mu)?;
                if qoi_bound <= tol {
                    return Ok(LadderAnswer {
                        value: s_rb,
                        // DEMOTED from Verified (bead y6yv): the
                        // a-posteriori bound is evaluated in
                        // round-to-nearest f64 with no outward rounding
                        // — honest mathematics, not an executable
                        // enclosure. The bound travels as dispersion.
                        color: estimated_color(RB_ESTIMATOR_ID, qoi_bound)?,
                        level_used: level,
                        leaks,
                    });
                }
                leaks.push(level);
                level -= 1;
                continue;
            }
            let u = self.ladder.truth.solve(mu)?;
            let s = self.ladder.truth.compliance(&u)?;
            return Ok(LadderAnswer {
                value: s,
                // DEMOTED from Verified{[s,s]} (bead y6yv): a plain
                // floating solve is the DECLARED truth semantics, not a
                // verified enclosure. Infinite dispersion is the
                // explicit no-spread-claim payload.
                color: estimated_color(TRUTH_ESTIMATOR_ID, f64::INFINITY)?,
                level_used: 0,
                leaks,
            });
        }
    }
}

fn conservative_coverage_query_work(ladder: &Ladder) -> Result<usize, SurrogateError> {
    let truth_nodes = ladder.truth.n();
    let mut work = checked_budget_product("RB coverage truth fallback", &[truth_nodes, 8])?;
    for rb in &ladder.rb_levels {
        let dimension = rb.dim();
        let matrix = checked_budget_product(
            "RB coverage reduced matrix",
            &[truth_nodes, dimension, dimension],
        )?;
        let projections = checked_budget_product(
            "RB coverage reduced projections",
            &[truth_nodes, dimension, 2],
        )?;
        let residual = checked_budget_product("RB coverage residual", &[truth_nodes, 8])?;
        let dense = checked_budget_product(
            "RB coverage dense solve",
            &[dimension, dimension, dimension],
        )?;
        work = work
            .checked_add(matrix)
            .and_then(|value| value.checked_add(projections))
            .and_then(|value| value.checked_add(residual))
            .and_then(|value| value.checked_add(dense))
            .ok_or(SurrogateError::BudgetArithmeticOverflow {
                resource: "RB coverage query work",
            })?;
    }
    Ok(work)
}

/// THE KILL MEASUREMENT (Proposal A): the fraction of a query battery
/// answerable at an RB rung (f64 estimator ≤ tol) WITHOUT drilling to full
/// order. Below 0.2 the beachhead is too narrow — park certification.
///
/// # Errors
/// Propagates every query refusal (an invalid battery point refuses the
/// whole measurement rather than silently skewing the fraction). Cartesian
/// count and conservative worst-case work are both admitted before the first
/// query. Each parameter's RB rungs are evaluated at most once; their bounds
/// classify every requested tolerance without repeating a reduced solve.
#[allow(clippy::cast_precision_loss)]
pub fn rb_coverage(ladder: &Ladder, mus: &[f64], tols: &[f64]) -> Result<f64, SurrogateError> {
    if mus.is_empty() {
        return Err(SurrogateError::EmptyCoverageBattery { axis: "mu" });
    }
    if tols.is_empty() {
        return Err(SurrogateError::EmptyCoverageBattery { axis: "tolerance" });
    }
    for (axis, count) in [("mu", mus.len()), ("tolerance", tols.len())] {
        if count > MAX_COVERAGE_AXIS_POINTS {
            return Err(SurrogateError::CoverageAxisTooLarge {
                axis,
                requested: count,
                limit: MAX_COVERAGE_AXIS_POINTS,
            });
        }
    }
    let total =
        mus.len()
            .checked_mul(tols.len())
            .ok_or(SurrogateError::BudgetArithmeticOverflow {
                resource: "RB coverage Cartesian product",
            })?;
    if total > MAX_COVERAGE_QUERIES {
        return Err(SurrogateError::CoverageProductTooLarge {
            requested: total,
            limit: MAX_COVERAGE_QUERIES,
        });
    }
    let per_mu = conservative_coverage_query_work(ladder)?;
    let solver_work =
        per_mu
            .checked_mul(mus.len())
            .ok_or(SurrogateError::BudgetArithmeticOverflow {
                resource: "RB coverage total work",
            })?;
    let coverage_work =
        solver_work
            .checked_add(total)
            .ok_or(SurrogateError::BudgetArithmeticOverflow {
                resource: "RB coverage total work",
            })?;
    if coverage_work > MAX_COVERAGE_WORK_UNITS {
        return Err(SurrogateError::BudgetExceeded {
            resource: "RB coverage total work",
            requested: coverage_work,
            limit: MAX_COVERAGE_WORK_UNITS,
        });
    }
    for &mu in mus {
        require_query_mu(mu, ladder.mu_range())?;
    }
    for &tol in tols {
        if !tol.is_finite() || tol <= 0.0 {
            return Err(SurrogateError::InvalidTolerance { tol });
        }
    }

    let strictest_tol = tols.iter().copied().fold(f64::INFINITY, f64::min);
    let mut covered = 0usize;
    for &mu in mus {
        let mut best_bound = f64::INFINITY;
        for rb in ladder.rb_levels.iter().rev() {
            let (_, _, _, qoi_bound) = rb.query(mu)?;
            best_bound = best_bound.min(qoi_bound);
            if best_bound <= strictest_tol {
                break;
            }
        }
        if best_bound > strictest_tol {
            let truth_state = ladder.truth.solve(mu)?;
            let _ = ladder.truth.compliance(&truth_state)?;
        }
        for &tol in tols {
            if best_bound <= tol {
                covered += 1;
            }
        }
    }
    let coverage = covered as f64 / total as f64;
    if !coverage.is_finite() {
        return Err(SurrogateError::NonFiniteDerived {
            what: "RB coverage fraction",
        });
    }
    Ok(coverage)
}

#[cfg(test)]
mod tests {
    use super::{SurrogateError, solve_dense};

    #[test]
    fn dense_solver_refuses_bad_shapes_inputs_and_pivots() {
        let mut missing_row = vec![vec![1.0]];
        let mut rhs = vec![1.0, 2.0];
        assert!(matches!(
            solve_dense(&mut missing_row, &mut rhs),
            Err(SurrogateError::InvalidVectorLength { .. })
        ));

        let mut non_finite = vec![vec![f64::NAN]];
        let mut rhs = vec![1.0];
        assert!(matches!(
            solve_dense(&mut non_finite, &mut rhs),
            Err(SurrogateError::NonFiniteInput { .. })
        ));

        let mut singular = vec![vec![1.0, 1.0], vec![2.0, 2.0]];
        let mut rhs = vec![1.0, 2.0];
        assert!(matches!(
            solve_dense(&mut singular, &mut rhs),
            Err(SurrogateError::SingularSystem { column: 1 })
        ));
    }
}
