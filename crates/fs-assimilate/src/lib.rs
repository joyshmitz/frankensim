//! fs-assimilate — validation as a living belief (plan addendum, Proposal 11).
//! Layer: L4.
//!
//! Strain-gauge and wind-tunnel data update the MODEL-FORM POSTERIOR that
//! Proposal 3 tracks per regime, so "validated" stops being a one-time stamp
//! and becomes a living belief state. A sensor readout is a TRACE of the field
//! onto the sensor's support — an observation operator expressed in the same
//! restriction-map algebra as the sheaf.
//!
//! This crate is the linear-Gaussian core of that assimilation: a [`Belief`]
//! (Gaussian state) is updated by [`Observation`]s (a restriction-map row + a
//! reading + its instrument noise) via sequential Kalman fusion. Two honest
//! properties:
//! - POINT SENSORS ([`point_sensor`]) are the REGISTRATION-FREE path (the R8
//!   fallback): their observation operator picks a state component directly, so
//!   they work even where full-field scan integration is premature. Scan
//!   observations ([`scan_observation`]) carry the registration variance too.
//! - Assimilation produces an **estimated candidate** tied to a proposed regime
//!   ([`assimilate_colored`]). Experimental validation is a separate admission
//!   act requiring calibrated data and an external authenticated authority.
//!
//! Deterministic; depends only on `fs-evidence` and the in-tree `fs-blake3`.

use core::fmt;

pub use fs_evidence::{Color, ValidityDomain};

const CANDIDATE_ID_DOMAIN: &str = "org.frankensim.fs-assimilate.candidate.v1";
const CANDIDATE_ID_PREFIX: &str = "assimilation-candidate:v1:";
/// Maximum state dimension admitted by the synchronous dense v0 core.
///
/// The Joseph update is `O(n^3)` and owns several `n x n` work matrices. Larger
/// states belong on a sparse or matrix-free, cancellable assimilation path.
pub const MAX_DENSE_STATE_DIM: usize = 256;
/// Maximum observations admitted by one synchronous dense aggregate call.
///
/// This also bounds canonical-order sorting and candidate-identity materialization
/// for low-dimensional campaigns. High-rate streams belong in a cancellable,
/// incremental assimilation session rather than one monolithic call.
pub const MAX_DENSE_OBSERVATIONS: usize = 4_096;
/// Maximum `observation_count * state_dimension^3` work proxy admitted by one
/// dense aggregate update.
///
/// The Joseph covariance update is cubic in the state dimension. A count cap by
/// itself would still admit tens of billions of dense operations at the largest
/// state, so aggregate admission must bound the multiplicative workload too.
pub const MAX_DENSE_UPDATE_CUBIC_WORK: u128 = 4 * 256_u128 * 256_u128 * 256_u128;

/// A Gaussian belief over an `n`-dimensional state.
///
/// Construction is checked so the mean is finite and the covariance is finite,
/// square, symmetric, and positive semidefinite. Fields stay private so a
/// checked belief cannot later be made ragged or non-finite.
#[derive(Debug, Clone, PartialEq)]
pub struct Belief {
    mean: Vec<f64>,
    cov: Vec<Vec<f64>>,
}

impl Belief {
    /// Construct a checked belief from a mean and full covariance matrix.
    ///
    /// # Errors
    /// Returns [`AssimError`] when the state is empty or any covariance
    /// invariant is violated.
    pub fn new(mut mean: Vec<f64>, mut cov: Vec<Vec<f64>>) -> Result<Self, AssimError> {
        validate_belief_parts(&mean, &cov)?;
        canonicalize_belief_zeros(&mut mean, &mut cov);
        Ok(Self { mean, cov })
    }

    /// Construct a checked 1-D belief `N(mean, var)`.
    ///
    /// # Errors
    /// Returns [`AssimError`] when `mean` is non-finite or `var` is non-finite
    /// or negative.
    pub fn scalar(mean: f64, var: f64) -> Result<Self, AssimError> {
        Self::new(vec![mean], vec![vec![var]])
    }

    /// Construct a checked independent (diagonal-covariance) belief.
    ///
    /// # Errors
    /// Returns [`AssimError`] when the vectors have different lengths, are
    /// empty, contain non-finite values, or contain a negative variance.
    pub fn diagonal(means: Vec<f64>, vars: &[f64]) -> Result<Self, AssimError> {
        if means.len() != vars.len() {
            return Err(AssimError::DiagonalDimensionMismatch {
                means: means.len(),
                variances: vars.len(),
            });
        }
        validate_state_dimension(means.len())?;
        let mut cov = vec![vec![0.0; means.len()]; means.len()];
        for (i, &variance) in vars.iter().enumerate() {
            cov[i][i] = variance;
        }
        Self::new(means, cov)
    }

    /// Recheck every structural and numerical belief invariant.
    ///
    /// # Errors
    /// Returns the first violated invariant.
    pub fn validate(&self) -> Result<(), AssimError> {
        validate_belief_parts(&self.mean, &self.cov)
    }

    fn from_covariance_preserving_update(
        mean: Vec<f64>,
        cov: Vec<Vec<f64>>,
    ) -> Result<Self, AssimError> {
        // Floating-point evaluation does not inherit the exact-arithmetic PSD
        // closure law automatically. Route every computed posterior through
        // the same fail-closed boundary as an externally supplied belief.
        Self::new(mean, cov)
    }

    /// The state dimension.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.mean.len()
    }

    /// Read-only view of the state mean.
    #[must_use]
    pub fn mean(&self) -> &[f64] {
        &self.mean
    }

    /// Read-only view of the covariance matrix.
    #[must_use]
    pub fn covariance(&self) -> &[Vec<f64>] {
        &self.cov
    }

    /// The mean of state component `component`.
    ///
    /// # Errors
    /// Returns [`AssimError::ComponentOutOfRange`] for an invalid component.
    pub fn component_mean(&self, component: usize) -> Result<f64, AssimError> {
        self.mean
            .get(component)
            .copied()
            .ok_or(AssimError::ComponentOutOfRange {
                component,
                dim: self.dim(),
            })
    }

    /// The variance of state component `component`.
    ///
    /// # Errors
    /// Returns [`AssimError::ComponentOutOfRange`] for an invalid component.
    pub fn variance(&self, component: usize) -> Result<f64, AssimError> {
        self.cov
            .get(component)
            .and_then(|row| row.get(component))
            .copied()
            .ok_or(AssimError::ComponentOutOfRange {
                component,
                dim: self.dim(),
            })
    }
}

/// One scalar observation: `value = operator · state + noise`, where `operator`
/// is the restriction-map row (the sensor's trace) and `noise_var` is the
/// instrument (+ registration) variance.
///
/// Construction is checked and fields stay private, preventing a valid
/// observation from being mutated into an empty, non-finite, or unanchored one.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    operator: Vec<f64>,
    value: f64,
    noise_var: f64,
    instrument: String,
}

impl Observation {
    /// Construct a checked scalar observation.
    ///
    /// # Errors
    /// Returns [`AssimError`] for an empty, oversized, zero, or non-finite
    /// operator; a non-finite reading; non-positive noise; or an unusable
    /// instrument identity.
    pub fn new(
        operator: Vec<f64>,
        value: f64,
        noise_var: f64,
        instrument: impl Into<String>,
    ) -> Result<Self, AssimError> {
        let observation = Self {
            operator: operator.into_iter().map(canonicalize_zero).collect(),
            value: canonicalize_zero(value),
            noise_var: canonicalize_zero(noise_var),
            instrument: instrument.into(),
        };
        observation.validate()?;
        Ok(observation)
    }

    /// Recheck every observation invariant except equality with a particular
    /// belief dimension.
    ///
    /// # Errors
    /// Returns the first violated invariant.
    pub fn validate(&self) -> Result<(), AssimError> {
        if self.operator.is_empty() {
            return Err(AssimError::EmptyObservationOperator);
        }
        validate_state_dimension(self.operator.len())?;
        for (index, coefficient) in self.operator.iter().enumerate() {
            if !coefficient.is_finite() {
                return Err(AssimError::NonFiniteObservationOperator { index });
            }
        }
        if self
            .operator
            .iter()
            .all(|coefficient| canonical_f64_bits(*coefficient) == 0)
        {
            return Err(AssimError::ZeroObservationOperator);
        }
        if !self.value.is_finite() {
            return Err(AssimError::NonFiniteObservationValue);
        }
        validate_noise(self.noise_var)?;
        validate_leaf_identity("instrument", &self.instrument)
    }

    /// Read-only view of the observation operator.
    #[must_use]
    pub fn operator(&self) -> &[f64] {
        &self.operator
    }

    /// The observed scalar value.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The total observation noise variance.
    #[must_use]
    pub fn noise_var(&self) -> f64 {
        self.noise_var
    }

    /// The calibrated instrument identity.
    #[must_use]
    pub fn instrument(&self) -> &str {
        &self.instrument
    }

    fn validate_for_dim(&self, state_dim: usize) -> Result<(), AssimError> {
        self.validate()?;
        if self.operator.len() != state_dim {
            return Err(AssimError::DimMismatch {
                state: state_dim,
                operator: self.operator.len(),
            });
        }
        Ok(())
    }
}

/// A registration-free point-sensor observation of state component `component`
/// (a strain gauge / thermocouple): its operator is the unit row `e_component`.
///
/// # Errors
/// Returns [`AssimError`] for a zero or oversized dimension, an out-of-range
/// component, or any malformed reading, noise, or instrument identity.
pub fn point_sensor(
    component: usize,
    dim: usize,
    value: f64,
    instrument_noise: f64,
    instrument: impl Into<String>,
) -> Result<Observation, AssimError> {
    if dim == 0 {
        return Err(AssimError::EmptyStateDimension);
    }
    validate_state_dimension(dim)?;
    if component >= dim {
        return Err(AssimError::ComponentOutOfRange { component, dim });
    }
    let mut operator = vec![0.0; dim];
    operator[component] = 1.0;
    Observation::new(operator, value, instrument_noise, instrument)
}

/// A full-field scan observation whose noise carries registration variance on
/// top of the strictly positive instrument variance (R8).
///
/// # Errors
/// Returns [`AssimError`] for malformed observation data, non-positive
/// instrument noise, negative registration variance, or an overflowing total.
pub fn scan_observation(
    operator: Vec<f64>,
    value: f64,
    instrument_noise: f64,
    registration_var: f64,
    instrument: impl Into<String>,
) -> Result<Observation, AssimError> {
    validate_noise(instrument_noise)?;
    if !registration_var.is_finite() {
        return Err(AssimError::NonFiniteRegistrationVariance);
    }
    if registration_var < 0.0 {
        return Err(AssimError::NegativeRegistrationVariance);
    }
    let noise_var = instrument_noise + registration_var;
    if !noise_var.is_finite() {
        return Err(AssimError::NonFiniteNoise);
    }
    Observation::new(operator, value, noise_var, instrument)
}

/// A structured assimilation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssimError {
    /// A belief cannot have zero state dimensions.
    EmptyBelief,
    /// A sensor cannot declare a zero state dimension.
    EmptyStateDimension,
    /// The dense synchronous v0 core refuses a state beyond its declared
    /// memory/compute envelope.
    StateDimensionLimit {
        /// Requested dimension.
        dim: usize,
        /// Maximum admitted dense dimension.
        max: usize,
    },
    /// An aggregate supplied too many observations for synchronous sorting,
    /// hashing, and iteration.
    ObservationCountLimit {
        /// Requested observation count.
        count: usize,
        /// Maximum admitted count.
        max: usize,
    },
    /// The observation-count/state-size product exceeds the bounded dense
    /// Joseph-update work envelope.
    AssimilationWorkLimit {
        /// Requested `observation_count * state_dimension^3` proxy units.
        requested: u128,
        /// Maximum admitted proxy units.
        max: u128,
    },
    /// The diagonal constructor received a different count of means and
    /// variances.
    DiagonalDimensionMismatch {
        /// Number of means.
        means: usize,
        /// Number of variances.
        variances: usize,
    },
    /// The covariance row count differs from the mean dimension.
    CovarianceDimensionMismatch {
        /// State dimension from the mean.
        state: usize,
        /// Covariance row count.
        rows: usize,
    },
    /// A covariance row is ragged.
    CovarianceRowDimensionMismatch {
        /// Offending row.
        row: usize,
        /// Required column count.
        expected: usize,
        /// Actual column count.
        actual: usize,
    },
    /// A mean component is NaN or infinite.
    NonFiniteMean {
        /// Offending component.
        index: usize,
    },
    /// A covariance entry is NaN or infinite.
    NonFiniteCovariance {
        /// Offending row.
        row: usize,
        /// Offending column.
        column: usize,
    },
    /// A diagonal covariance entry is negative.
    NegativeVariance {
        /// Offending component.
        index: usize,
    },
    /// A covariance pair is not exactly symmetric.
    NonSymmetricCovariance {
        /// Row of the upper-triangular entry.
        row: usize,
        /// Column of the upper-triangular entry.
        column: usize,
    },
    /// The symmetric covariance is not positive semidefinite.
    CovarianceNotPositiveSemidefinite,
    /// An observation operator has no coefficients.
    EmptyObservationOperator,
    /// An observation operator contains no state sensitivity.
    ZeroObservationOperator,
    /// An observation-operator coefficient is NaN or infinite.
    NonFiniteObservationOperator {
        /// Offending coefficient.
        index: usize,
    },
    /// The observed scalar value is NaN or infinite.
    NonFiniteObservationValue,
    /// An observation operator's length differs from the state dimension.
    DimMismatch {
        /// State dimension.
        state: usize,
        /// Operator length.
        operator: usize,
    },
    /// A requested state component is outside the declared dimension.
    ComponentOutOfRange {
        /// Requested component.
        component: usize,
        /// State dimension.
        dim: usize,
    },
    /// Observation noise is zero or negative.
    NonPositiveNoise,
    /// Observation noise is NaN or infinite, including overflow while combining
    /// instrument and registration variances.
    NonFiniteNoise,
    /// Registration variance is negative.
    NegativeRegistrationVariance,
    /// Registration variance is NaN or infinite.
    NonFiniteRegistrationVariance,
    /// An instrument identity is blank.
    EmptyInstrument,
    /// A regime-axis identity is blank.
    EmptyRegime,
    /// A machine-readable identity violates the shared evidence grammar.
    InvalidIdentity {
        /// Identity role (`instrument` or `regime_param`).
        field: &'static str,
        /// Stable rejection reason from `fs-evidence`.
        reason: &'static str,
    },
    /// An aggregate operation requires at least one observation.
    EmptyObservations,
    /// A regime bound is NaN or infinite.
    NonFiniteRegimeBounds,
    /// The regime lower bound exceeds its upper bound.
    InvertedRegimeBounds,
    /// The innovation covariance was non-positive (degenerate).
    SingularInnovation,
    /// Finite inputs overflowed or otherwise produced a non-finite intermediate.
    NonFiniteComputation {
        /// Stable computation stage.
        stage: &'static str,
    },
}

impl fmt::Display for AssimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBelief => write!(f, "belief state dimension must be non-zero"),
            Self::EmptyStateDimension => write!(f, "sensor state dimension must be non-zero"),
            Self::StateDimensionLimit { dim, max } => {
                write!(f, "dense assimilation dimension {dim} exceeds limit {max}")
            }
            Self::ObservationCountLimit { count, max } => write!(
                f,
                "dense assimilation observation count {count} exceeds limit {max}"
            ),
            Self::AssimilationWorkLimit { requested, max } => write!(
                f,
                "dense assimilation work proxy {requested} exceeds limit {max}"
            ),
            Self::DiagonalDimensionMismatch { means, variances } => write!(
                f,
                "diagonal belief has {means} means but {variances} variances"
            ),
            Self::CovarianceDimensionMismatch { state, rows } => write!(
                f,
                "belief dimension is {state} but covariance has {rows} rows"
            ),
            Self::CovarianceRowDimensionMismatch {
                row,
                expected,
                actual,
            } => write!(
                f,
                "covariance row {row} has {actual} columns; expected {expected}"
            ),
            Self::NonFiniteMean { index } => {
                write!(f, "belief mean component {index} is non-finite")
            }
            Self::NonFiniteCovariance { row, column } => {
                write!(f, "covariance entry ({row}, {column}) is non-finite")
            }
            Self::NegativeVariance { index } => {
                write!(f, "covariance diagonal {index} is negative")
            }
            Self::NonSymmetricCovariance { row, column } => write!(
                f,
                "covariance entries ({row}, {column}) and ({column}, {row}) differ"
            ),
            Self::CovarianceNotPositiveSemidefinite => {
                write!(f, "covariance is not positive semidefinite")
            }
            Self::EmptyObservationOperator => write!(f, "observation operator must not be empty"),
            Self::ZeroObservationOperator => {
                write!(
                    f,
                    "observation operator must contain a non-zero coefficient"
                )
            }
            Self::NonFiniteObservationOperator { index } => {
                write!(f, "observation operator coefficient {index} is non-finite")
            }
            Self::NonFiniteObservationValue => write!(f, "observation value is non-finite"),
            Self::DimMismatch { state, operator } => write!(
                f,
                "state dimension is {state} but observation operator length is {operator}"
            ),
            Self::ComponentOutOfRange { component, dim } => {
                write!(f, "component {component} is outside state dimension {dim}")
            }
            Self::NonPositiveNoise => write!(f, "observation noise must be strictly positive"),
            Self::NonFiniteNoise => write!(f, "observation noise is non-finite"),
            Self::NegativeRegistrationVariance => {
                write!(f, "registration variance must be non-negative")
            }
            Self::NonFiniteRegistrationVariance => {
                write!(f, "registration variance is non-finite")
            }
            Self::EmptyInstrument => write!(f, "instrument identity must not be blank"),
            Self::EmptyRegime => write!(f, "regime axis identity must not be blank"),
            Self::InvalidIdentity { field, reason } => {
                write!(f, "invalid {field} identity: {reason}")
            }
            Self::EmptyObservations => write!(f, "at least one observation is required"),
            Self::NonFiniteRegimeBounds => write!(f, "regime bounds must be finite"),
            Self::InvertedRegimeBounds => {
                write!(f, "regime lower bound must not exceed its upper bound")
            }
            Self::SingularInnovation => {
                write!(f, "innovation covariance is non-positive")
            }
            Self::NonFiniteComputation { stage } => {
                write!(f, "assimilation produced a non-finite value during {stage}")
            }
        }
    }
}

impl std::error::Error for AssimError {}

/// The model-data misfit `Σⱼ (hⱼ·mean − yⱼ)² / rⱼ` — the weighted squared
/// residual assimilation seeks to reduce.
///
/// # Errors
/// Returns [`AssimError`] for an empty observation set, malformed input, a
/// dimension mismatch, or a non-finite computed term or sum.
pub fn misfit(belief: &Belief, observations: &[Observation]) -> Result<f64, AssimError> {
    let observations = validated_canonical_observations(observations, belief.dim())?;
    misfit_canonical(belief, &observations)
}

fn misfit_canonical(belief: &Belief, observations: &[&Observation]) -> Result<f64, AssimError> {
    let mut total = 0.0;
    for observation in observations {
        let predicted = checked_dot(&observation.operator, &belief.mean, "misfit prediction")?;
        let residual = predicted - observation.value;
        if !residual.is_finite() {
            return Err(AssimError::NonFiniteComputation {
                stage: "misfit residual",
            });
        }
        let term = residual * residual / observation.noise_var;
        if !term.is_finite() {
            return Err(AssimError::NonFiniteComputation {
                stage: "misfit term",
            });
        }
        total += term;
        if !total.is_finite() {
            return Err(AssimError::NonFiniteComputation {
                stage: "misfit sum",
            });
        }
    }
    Ok(total)
}

/// Fuse one observation into the belief by the scalar Kalman update. For a
/// valid covariance, every posterior component variance is at most its prior
/// value (information only increases).
///
/// # Errors
/// Returns [`AssimError`] for malformed input, a dimension mismatch, a
/// degenerate innovation, or a non-finite computed intermediate.
pub fn assimilate(prior: &Belief, obs: &Observation) -> Result<Belief, AssimError> {
    obs.validate_for_dim(prior.dim())?;
    assimilate_checked(prior, obs)
}

/// Fuse all observations in their canonical content order. The mathematical
/// linear-Gaussian posterior is order-independent; canonical evaluation also
/// makes the floating-point result bit-stable across input permutations.
///
/// # Errors
/// Returns [`AssimError`] for an empty observation set or any error described by
/// [`assimilate`].
pub fn assimilate_all(prior: &Belief, observations: &[Observation]) -> Result<Belief, AssimError> {
    validate_assimilation_work(prior.dim(), observations.len())?;
    let observations = validated_canonical_observations(observations, prior.dim())?;
    assimilate_canonical(prior, &observations)
}

fn assimilate_canonical(
    prior: &Belief,
    observations: &[&Observation],
) -> Result<Belief, AssimError> {
    validate_assimilation_work(prior.dim(), observations.len())?;
    let mut belief = prior.clone();
    for observation in observations {
        belief = assimilate_checked(&belief, observation)?;
    }
    Ok(belief)
}

/// An estimated, regime-tagged assimilated-posterior candidate.
///
/// The fields are read-only so this crate cannot accidentally expose a mutable
/// route from its honest estimated output to a stronger evidence color.
#[derive(Debug, Clone, PartialEq)]
pub struct AssimilatedPosterior {
    belief: Belief,
    color: Color,
    regime: ValidityDomain,
    misfit_before: f64,
    misfit_after: f64,
}

impl AssimilatedPosterior {
    /// The updated belief.
    #[must_use]
    pub fn belief(&self) -> &Belief {
        &self.belief
    }

    /// The honest estimated color of this candidate.
    #[must_use]
    pub fn color(&self) -> &Color {
        &self.color
    }

    /// The proposed regime for later experimental validation.
    #[must_use]
    pub fn regime(&self) -> &ValidityDomain {
        &self.regime
    }

    /// Model-data misfit before assimilation.
    #[must_use]
    pub fn misfit_before(&self) -> f64 {
        self.misfit_before
    }

    /// Model-data misfit after assimilation.
    #[must_use]
    pub fn misfit_after(&self) -> f64 {
        self.misfit_after
    }
}

/// Assimilate observations and return an instrument-bound **estimated**
/// candidate for a named regime — Proposal 3's living-belief update.
///
/// The candidate identity is a bounded domain-separated BLAKE3 digest over the
/// complete prior, the observation multiset (canonicalized independent of
/// input ordering), and the proposed regime. This function does not claim that
/// seeing data is itself validation. Promotion to [`Color::Validated`] belongs
/// at an external admission boundary that authenticates calibrated dataset
/// provenance and validation authority.
///
/// # Errors
/// Returns [`AssimError`] for an invalid regime, an empty observation set, or
/// any malformed/non-finite assimilation input or result.
pub fn assimilate_colored(
    prior: &Belief,
    observations: &[Observation],
    regime_param: &str,
    regime_lo: f64,
    regime_hi: f64,
) -> Result<AssimilatedPosterior, AssimError> {
    validate_regime(regime_param, regime_lo, regime_hi)?;
    validate_assimilation_work(prior.dim(), observations.len())?;
    let regime_lo = canonicalize_zero(regime_lo);
    let regime_hi = canonicalize_zero(regime_hi);
    let observations = validated_canonical_observations(observations, prior.dim())?;
    let misfit_before = misfit_canonical(prior, &observations)?;
    let belief = assimilate_canonical(prior, &observations)?;
    let misfit_after = misfit_canonical(&belief, &observations)?;
    let estimator = candidate_identity(prior, &observations, regime_param, regime_lo, regime_hi);
    debug_assert!(fs_evidence::color_leaf_identity_reason(&estimator).is_none());

    Ok(AssimilatedPosterior {
        belief,
        color: Color::Estimated {
            estimator,
            dispersion: f64::INFINITY,
        },
        regime: ValidityDomain::unconstrained().with(regime_param, regime_lo, regime_hi),
        misfit_before,
        misfit_after,
    })
}

fn validate_belief_parts(mean: &[f64], cov: &[Vec<f64>]) -> Result<(), AssimError> {
    validate_belief_structure(mean, cov)?;
    if !covariance_is_positive_semidefinite(cov) {
        return Err(AssimError::CovarianceNotPositiveSemidefinite);
    }
    Ok(())
}

fn validate_belief_structure(mean: &[f64], cov: &[Vec<f64>]) -> Result<(), AssimError> {
    let n = mean.len();
    if n == 0 {
        return Err(AssimError::EmptyBelief);
    }
    validate_state_dimension(n)?;
    if cov.len() != n {
        return Err(AssimError::CovarianceDimensionMismatch {
            state: n,
            rows: cov.len(),
        });
    }
    for (index, value) in mean.iter().enumerate() {
        if !value.is_finite() {
            return Err(AssimError::NonFiniteMean { index });
        }
    }
    for (row_index, row) in cov.iter().enumerate() {
        if row.len() != n {
            return Err(AssimError::CovarianceRowDimensionMismatch {
                row: row_index,
                expected: n,
                actual: row.len(),
            });
        }
        for (column_index, value) in row.iter().enumerate() {
            if !value.is_finite() {
                return Err(AssimError::NonFiniteCovariance {
                    row: row_index,
                    column: column_index,
                });
            }
        }
        if row[row_index] < 0.0 {
            return Err(AssimError::NegativeVariance { index: row_index });
        }
    }
    for (row_index, row) in cov.iter().enumerate() {
        for (column_index, column) in cov.iter().enumerate().skip(row_index + 1) {
            if canonical_f64_bits(row[column_index]) != canonical_f64_bits(column[row_index]) {
                return Err(AssimError::NonSymmetricCovariance {
                    row: row_index,
                    column: column_index,
                });
            }
        }
    }
    Ok(())
}

fn validate_state_dimension(dim: usize) -> Result<(), AssimError> {
    if dim > MAX_DENSE_STATE_DIM {
        Err(AssimError::StateDimensionLimit {
            dim,
            max: MAX_DENSE_STATE_DIM,
        })
    } else {
        Ok(())
    }
}

fn validate_observation_count(count: usize) -> Result<(), AssimError> {
    if count == 0 {
        Err(AssimError::EmptyObservations)
    } else if count > MAX_DENSE_OBSERVATIONS {
        Err(AssimError::ObservationCountLimit {
            count,
            max: MAX_DENSE_OBSERVATIONS,
        })
    } else {
        Ok(())
    }
}

fn validate_assimilation_work(dim: usize, observation_count: usize) -> Result<(), AssimError> {
    validate_observation_count(observation_count)?;
    let dim = dim as u128;
    let requested = dim
        .checked_mul(dim)
        .and_then(|value| value.checked_mul(dim))
        .and_then(|value| value.checked_mul(observation_count as u128))
        .unwrap_or(u128::MAX);
    if requested > MAX_DENSE_UPDATE_CUBIC_WORK {
        Err(AssimError::AssimilationWorkLimit {
            requested,
            max: MAX_DENSE_UPDATE_CUBIC_WORK,
        })
    } else {
        Ok(())
    }
}

fn covariance_is_positive_semidefinite(cov: &[Vec<f64>]) -> bool {
    // Scaling to a correlation matrix makes the Schur-complement test
    // dimensionless. Without this step, one enormous variance can hide an
    // invalid correlation involving a much smaller variance. This boundary is
    // deliberately fail-closed: unlike a solver convergence test, no negative
    // pivot is a harmless tolerance event. Ambiguous roundoff is rejected
    // rather than silently relabelled as zero curvature.
    let mut active = Vec::with_capacity(cov.len());
    for (index, row) in cov.iter().enumerate() {
        if canonical_f64_bits(row[index]) == 0 {
            // A PSD matrix with zero variance has an exactly zero row/column.
            if row.iter().any(|entry| canonical_f64_bits(*entry) != 0) {
                return false;
            }
        } else {
            active.push(index);
        }
    }
    let n = active.len();
    if n == 0 {
        return true;
    }

    let mut scaled = vec![vec![0.0; n]; n];
    for (scaled_row, &source_row) in active.iter().enumerate() {
        scaled[scaled_row][scaled_row] = 1.0;
        for (scaled_column, &source_column) in active.iter().enumerate().skip(scaled_row + 1) {
            if !square_is_at_most_product(
                cov[source_row][source_column].abs(),
                cov[source_row][source_row],
                cov[source_column][source_column],
            ) {
                // This exact binary-rational comparison enforces every 2x2
                // principal minor before square roots and divisions can round
                // an invalid correlation back onto the unit boundary.
                return false;
            }
            let row_scale = cov[source_row][source_row].sqrt();
            let column_scale = cov[source_column][source_column].sqrt();
            let (first_divisor, second_divisor) = if row_scale >= column_scale {
                (row_scale, column_scale)
            } else {
                (column_scale, row_scale)
            };
            let correlation = cov[source_row][source_column] / first_divisor / second_divisor;
            if !correlation.is_finite() || correlation.abs() > 1.0 {
                return false;
            }
            scaled[scaled_row][scaled_column] = correlation;
            scaled[scaled_column][scaled_row] = correlation;
        }
    }

    // Symmetric diagonal pivoting avoids dividing by a small Schur pivot when
    // a better-conditioned one remains. The transformation is a sequence of
    // congruences, so a negative diagonal in any Schur complement is direct
    // evidence of negative curvature.
    for pivot_index in 0..n {
        let mut selected = pivot_index;
        for candidate in (pivot_index + 1)..n {
            if scaled[candidate][candidate] > scaled[selected][selected] {
                selected = candidate;
            }
        }
        if selected != pivot_index {
            scaled.swap(selected, pivot_index);
            for row in &mut scaled {
                row.swap(selected, pivot_index);
            }
        }

        let pivot = scaled[pivot_index][pivot_index];
        if !pivot.is_finite() || pivot < 0.0 {
            return false;
        }
        if canonical_f64_bits(pivot) == 0 {
            // A PSD matrix with a zero diagonal has an exactly zero row and
            // column. Accept exact singular structure, but never manufacture
            // it by tolerance-clamping a negative pivot.
            if scaled[pivot_index][(pivot_index + 1)..]
                .iter()
                .any(|entry| canonical_f64_bits(*entry) != 0)
            {
                return false;
            }
            continue;
        }

        let pivot_column = (0..n)
            .map(|row| scaled[row][pivot_index])
            .collect::<Vec<_>>();
        for row in (pivot_index + 1)..n {
            let multiplier = pivot_column[row] / pivot;
            if !multiplier.is_finite() {
                return false;
            }
            for (column, column_pivot) in pivot_column.iter().enumerate().skip(row) {
                let updated = (-multiplier).mul_add(*column_pivot, scaled[column][row]);
                if !updated.is_finite() || (row == column && updated < 0.0) {
                    return false;
                }
                scaled[column][row] = updated;
                scaled[row][column] = updated;
            }
        }
    }
    true
}

fn square_is_at_most_product(value: f64, left: f64, right: f64) -> bool {
    let square = binary_product(value, value);
    let diagonal_product = binary_product(left, right);
    compare_binary_products(square, diagonal_product) != core::cmp::Ordering::Greater
}

fn binary_product(left: f64, right: f64) -> (u128, i32) {
    let (left_significand, left_exponent) = binary_significand_and_exponent(left);
    let (right_significand, right_exponent) = binary_significand_and_exponent(right);
    (
        u128::from(left_significand) * u128::from(right_significand),
        left_exponent + right_exponent,
    )
}

fn binary_significand_and_exponent(value: f64) -> (u64, i32) {
    debug_assert!(value.is_finite() && value >= 0.0);
    let bits = value.to_bits();
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32; // Masked to eleven bits.
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent_bits == 0 {
        (fraction, -1074)
    } else {
        ((1_u64 << 52) | fraction, exponent_bits - 1023 - 52)
    }
}

fn compare_binary_products(left: (u128, i32), right: (u128, i32)) -> core::cmp::Ordering {
    let ((left_significand, left_exponent), (right_significand, right_exponent)) = (left, right);
    match (left_significand == 0, right_significand == 0) {
        (true, true) => return core::cmp::Ordering::Equal,
        (true, false) => return core::cmp::Ordering::Less,
        (false, true) => return core::cmp::Ordering::Greater,
        (false, false) => {}
    }

    let left_top_bit = i64::from(left_significand.ilog2()) + i64::from(left_exponent);
    let right_top_bit = i64::from(right_significand.ilog2()) + i64::from(right_exponent);
    match left_top_bit.cmp(&right_top_bit) {
        core::cmp::Ordering::Equal => {
            if left_exponent >= right_exponent {
                (left_significand << (left_exponent - right_exponent).unsigned_abs())
                    .cmp(&right_significand)
            } else {
                left_significand
                    .cmp(&(right_significand << (right_exponent - left_exponent).unsigned_abs()))
            }
        }
        ordering => ordering,
    }
}

fn validate_noise(noise_var: f64) -> Result<(), AssimError> {
    if !noise_var.is_finite() {
        Err(AssimError::NonFiniteNoise)
    } else if noise_var <= 0.0 {
        Err(AssimError::NonPositiveNoise)
    } else {
        Ok(())
    }
}

fn validate_leaf_identity(field: &'static str, identity: &str) -> Result<(), AssimError> {
    if identity.trim().is_empty() {
        return match field {
            "instrument" => Err(AssimError::EmptyInstrument),
            "regime_param" => Err(AssimError::EmptyRegime),
            _ => Err(AssimError::InvalidIdentity {
                field,
                reason: "blank",
            }),
        };
    }
    if let Some(reason) = fs_evidence::color_leaf_identity_reason(identity) {
        return Err(AssimError::InvalidIdentity { field, reason });
    }
    Ok(())
}

fn validate_regime(regime_param: &str, lo: f64, hi: f64) -> Result<(), AssimError> {
    validate_leaf_identity("regime_param", regime_param)?;
    if !lo.is_finite() || !hi.is_finite() {
        return Err(AssimError::NonFiniteRegimeBounds);
    }
    if lo > hi {
        return Err(AssimError::InvertedRegimeBounds);
    }
    Ok(())
}

fn validated_canonical_observations(
    observations: &[Observation],
    state_dim: usize,
) -> Result<Vec<&Observation>, AssimError> {
    validate_observation_count(observations.len())?;
    let observations = canonical_observations(observations);
    for observation in &observations {
        observation.validate_for_dim(state_dim)?;
    }
    Ok(observations)
}

fn assimilate_checked(prior: &Belief, obs: &Observation) -> Result<Belief, AssimError> {
    let n = prior.dim();
    let h = &obs.operator;

    let mut ph = Vec::with_capacity(n);
    for row in &prior.cov {
        ph.push(checked_dot(row, h, "covariance-times-operator")?);
    }
    let innovation_variance = checked_dot(h, &ph, "innovation variance")? + obs.noise_var;
    if !innovation_variance.is_finite() {
        return Err(AssimError::NonFiniteComputation {
            stage: "innovation variance",
        });
    }
    if innovation_variance <= 0.0 {
        return Err(AssimError::SingularInnovation);
    }
    let gain = ph
        .iter()
        .map(|entry| entry / innovation_variance)
        .collect::<Vec<_>>();
    if gain.iter().any(|entry| !entry.is_finite()) {
        return Err(AssimError::NonFiniteComputation {
            stage: "Kalman gain",
        });
    }

    let predicted = checked_dot(h, &prior.mean, "observation prediction")?;
    let innovation = obs.value - predicted;
    if !innovation.is_finite() {
        return Err(AssimError::NonFiniteComputation {
            stage: "observation innovation",
        });
    }

    let mut mean = Vec::with_capacity(n);
    for (prior_mean, gain_entry) in prior.mean.iter().zip(&gain) {
        let updated = prior_mean + gain_entry * innovation;
        if !updated.is_finite() {
            return Err(AssimError::NonFiniteComputation {
                stage: "posterior mean",
            });
        }
        mean.push(updated);
    }

    let cov = joseph_covariance(prior, h, obs.noise_var, &gain)?;
    Belief::from_covariance_preserving_update(mean, cov)
}

fn joseph_covariance(
    prior: &Belief,
    observation_operator: &[f64],
    noise_variance: f64,
    gain: &[f64],
) -> Result<Vec<Vec<f64>>, AssimError> {
    let n = prior.dim();
    // Joseph form, P' = (I-KH)P(I-KH)^T + KRK^T, retains both PSD terms
    // instead of relying on a cancellation-prone rank-one subtraction. The
    // final matrix is mirrored from one computed triangle for exact symmetry
    // and then passes through the full public Belief validator.
    let mut transform = vec![vec![0.0; n]; n];
    for (row, transform_row) in transform.iter_mut().enumerate() {
        for (column, entry) in transform_row.iter_mut().enumerate() {
            let identity = if row == column { 1.0 } else { 0.0 };
            *entry = (-gain[row]).mul_add(observation_operator[column], identity);
            if !entry.is_finite() {
                return Err(AssimError::NonFiniteComputation {
                    stage: "Joseph transform",
                });
            }
        }
    }

    let mut transformed_prior = vec![vec![0.0; n]; n];
    for (row, transformed_row) in transformed_prior.iter_mut().enumerate() {
        for (column, transformed_entry) in transformed_row.iter_mut().enumerate() {
            let mut entry = 0.0;
            for (transform_entry, prior_row) in transform[row].iter().zip(&prior.cov) {
                entry = transform_entry.mul_add(prior_row[column], entry);
                if !entry.is_finite() {
                    return Err(AssimError::NonFiniteComputation {
                        stage: "Joseph left product",
                    });
                }
            }
            *transformed_entry = entry;
        }
    }

    let noise_scale = noise_variance.sqrt();
    let mut noise_factor = Vec::with_capacity(n);
    for gain_entry in gain {
        let factor = gain_entry * noise_scale;
        if !factor.is_finite() {
            return Err(AssimError::NonFiniteComputation {
                stage: "Joseph noise factor",
            });
        }
        noise_factor.push(factor);
    }

    let mut cov = vec![vec![0.0; n]; n];
    for row in 0..n {
        for column in row..n {
            let propagated = checked_dot_fma(
                &transformed_prior[row],
                &transform[column],
                "Joseph propagated covariance",
            )?;
            let updated = noise_factor[row].mul_add(noise_factor[column], propagated);
            if !updated.is_finite() {
                return Err(AssimError::NonFiniteComputation {
                    stage: "posterior covariance",
                });
            }
            cov[row][column] = updated;
            cov[column][row] = updated;
        }
    }
    Ok(cov)
}

fn checked_dot_fma(a: &[f64], b: &[f64], stage: &'static str) -> Result<f64, AssimError> {
    debug_assert_eq!(a.len(), b.len());
    let mut total = 0.0;
    for (left, right) in a.iter().zip(b) {
        total = left.mul_add(*right, total);
        if !total.is_finite() {
            return Err(AssimError::NonFiniteComputation { stage });
        }
    }
    Ok(total)
}

fn checked_dot(a: &[f64], b: &[f64], stage: &'static str) -> Result<f64, AssimError> {
    debug_assert_eq!(a.len(), b.len());
    let mut total = 0.0;
    for (left, right) in a.iter().zip(b) {
        let product = left * right;
        if !product.is_finite() {
            return Err(AssimError::NonFiniteComputation { stage });
        }
        total += product;
        if !total.is_finite() {
            return Err(AssimError::NonFiniteComputation { stage });
        }
    }
    Ok(total)
}

fn candidate_identity(
    prior: &Belief,
    observations: &[&Observation],
    regime_param: &str,
    regime_lo: f64,
    regime_hi: f64,
) -> String {
    let mut canonical = Vec::new();
    push_atom(
        &mut canonical,
        b"state-dimension",
        &usize_bytes(prior.dim()),
    );
    for value in &prior.mean {
        push_atom(
            &mut canonical,
            b"prior-mean",
            &canonical_f64_bits(*value).to_le_bytes(),
        );
    }
    for row in &prior.cov {
        for value in row {
            push_atom(
                &mut canonical,
                b"prior-covariance",
                &canonical_f64_bits(*value).to_le_bytes(),
            );
        }
    }

    for observation in observations {
        let record = canonical_observation_bytes(observation);
        push_atom(&mut canonical, b"observation", &record);
    }
    push_atom(&mut canonical, b"regime-axis", regime_param.as_bytes());
    push_atom(
        &mut canonical,
        b"regime-lo",
        &canonical_f64_bits(regime_lo).to_le_bytes(),
    );
    push_atom(
        &mut canonical,
        b"regime-hi",
        &canonical_f64_bits(regime_hi).to_le_bytes(),
    );

    format!(
        "{CANDIDATE_ID_PREFIX}{}",
        fs_blake3::hash_domain(CANDIDATE_ID_DOMAIN, &canonical)
    )
}

fn canonical_observation_bytes(observation: &Observation) -> Vec<u8> {
    let mut record = Vec::new();
    push_atom(
        &mut record,
        b"operator-length",
        &usize_bytes(observation.operator.len()),
    );
    for coefficient in &observation.operator {
        push_atom(
            &mut record,
            b"operator-coefficient",
            &canonical_f64_bits(*coefficient).to_le_bytes(),
        );
    }
    push_atom(
        &mut record,
        b"value",
        &canonical_f64_bits(observation.value).to_le_bytes(),
    );
    push_atom(
        &mut record,
        b"noise-variance",
        &canonical_f64_bits(observation.noise_var).to_le_bytes(),
    );
    push_atom(
        &mut record,
        b"instrument",
        observation.instrument.as_bytes(),
    );
    record
}

fn canonical_observations(observations: &[Observation]) -> Vec<&Observation> {
    let mut keyed = observations
        .iter()
        .map(|observation| (canonical_observation_bytes(observation), observation))
        .collect::<Vec<_>>();
    keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    keyed
        .into_iter()
        .map(|(_, observation)| observation)
        .collect()
}

fn push_atom(buffer: &mut Vec<u8>, label: &[u8], value: &[u8]) {
    buffer.extend_from_slice(&usize_bytes(label.len()));
    buffer.extend_from_slice(label);
    buffer.extend_from_slice(&usize_bytes(value.len()));
    buffer.extend_from_slice(value);
}

fn usize_bytes(value: usize) -> [u8; 16] {
    (value as u128).to_le_bytes()
}

fn canonical_f64_bits(value: f64) -> u64 {
    const SIGN_BIT: u64 = 1_u64 << 63;
    match value.to_bits() {
        SIGN_BIT => 0,
        bits => bits,
    }
}

fn canonicalize_zero(value: f64) -> f64 {
    f64::from_bits(canonical_f64_bits(value))
}

fn canonicalize_belief_zeros(mean: &mut [f64], cov: &mut [Vec<f64>]) {
    for value in mean {
        *value = canonicalize_zero(*value);
    }
    for value in cov.iter_mut().flatten() {
        *value = canonicalize_zero(*value);
    }
}
