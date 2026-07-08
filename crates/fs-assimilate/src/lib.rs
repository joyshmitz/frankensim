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
//! reading + its instrument noise) via the sequential Kalman fusion, which
//! provably REDUCES the model-data [`misfit`]. Two honest properties:
//! - POINT SENSORS ([`point_sensor`]) are the REGISTRATION-FREE path (the R8
//!   fallback): their observation operator picks a state component directly, so
//!   they work even where full-field scan integration is premature. Scan
//!   observations ([`scan_observation`]) carry the registration variance too.
//! - The posterior is colored **validated**, anchored to the calibrated
//!   instrument ([`assimilate_colored`]).
//!
//! Deterministic; depends only on `fs-evidence`.

pub use fs_evidence::{Color, ValidityDomain};

/// A Gaussian belief over an `n`-dimensional state.
#[derive(Debug, Clone, PartialEq)]
pub struct Belief {
    /// The state mean.
    pub mean: Vec<f64>,
    /// The state covariance (`n × n`, symmetric).
    pub cov: Vec<Vec<f64>>,
}

impl Belief {
    /// A 1-D belief `N(mean, var)`.
    #[must_use]
    pub fn scalar(mean: f64, var: f64) -> Belief {
        Belief {
            mean: vec![mean],
            cov: vec![vec![var]],
        }
    }

    /// An independent (diagonal-covariance) belief.
    #[must_use]
    pub fn diagonal(means: Vec<f64>, vars: &[f64]) -> Belief {
        let n = means.len();
        let mut cov = vec![vec![0.0; n]; n];
        for (i, &v) in vars.iter().enumerate().take(n) {
            cov[i][i] = v;
        }
        Belief { mean: means, cov }
    }

    /// The state dimension.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.mean.len()
    }

    /// The variance of state component `i`.
    #[must_use]
    pub fn variance(&self, i: usize) -> f64 {
        self.cov[i][i]
    }
}

/// One scalar observation: `value = operator · state + noise`, where `operator`
/// is the restriction-map row (the sensor's trace) and `noise_var` is the
/// instrument (+ registration) variance.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// The restriction-map row (observation operator) `h`.
    pub operator: Vec<f64>,
    /// The measured value `y`.
    pub value: f64,
    /// The observation noise variance `r` (> 0).
    pub noise_var: f64,
    /// The instrument that produced it (provenance / anchor).
    pub instrument: String,
}

/// A registration-FREE point-sensor observation of state component `component`
/// (a strain gauge / thermocouple): its operator is the unit row `e_component`.
#[must_use]
pub fn point_sensor(
    component: usize,
    dim: usize,
    value: f64,
    instrument_noise: f64,
    instrument: impl Into<String>,
) -> Observation {
    let mut operator = vec![0.0; dim];
    if component < dim {
        operator[component] = 1.0;
    }
    Observation {
        operator,
        value,
        noise_var: instrument_noise,
        instrument: instrument.into(),
    }
}

/// A full-field SCAN observation: same as a sensor reading but its noise carries
/// the REGISTRATION variance on top of the instrument variance (R8).
#[must_use]
pub fn scan_observation(
    operator: Vec<f64>,
    value: f64,
    instrument_noise: f64,
    registration_var: f64,
    instrument: impl Into<String>,
) -> Observation {
    Observation {
        operator,
        value,
        noise_var: instrument_noise + registration_var,
        instrument: instrument.into(),
    }
}

/// A structured assimilation failure.
#[derive(Debug, Clone, PartialEq)]
pub enum AssimError {
    /// An observation operator's length ≠ the state dimension.
    DimMismatch {
        /// State dimension.
        state: usize,
        /// Operator length.
        operator: usize,
    },
    /// A non-positive observation noise variance.
    NonPositiveNoise,
    /// The innovation covariance was non-positive (degenerate).
    SingularInnovation,
}

/// The model-data misfit `Σⱼ (hⱼ·mean − yⱼ)² / rⱼ` — the weighted squared
/// residual assimilation drives down.
#[must_use]
pub fn misfit(belief: &Belief, observations: &[Observation]) -> f64 {
    observations
        .iter()
        .map(|o| {
            let predicted = dot(&o.operator, &belief.mean);
            let resid = predicted - o.value;
            resid * resid / o.noise_var
        })
        .sum()
}

/// Fuse one observation into the belief by the scalar Kalman update. The
/// posterior variance of every state component is `≤` the prior's (information
/// only increases).
///
/// # Errors
/// [`AssimError`] on a dimension mismatch, non-positive noise, or a degenerate
/// innovation.
pub fn assimilate(prior: &Belief, obs: &Observation) -> Result<Belief, AssimError> {
    let n = prior.dim();
    if obs.operator.len() != n {
        return Err(AssimError::DimMismatch {
            state: n,
            operator: obs.operator.len(),
        });
    }
    if obs.noise_var <= 0.0 || !obs.noise_var.is_finite() {
        return Err(AssimError::NonPositiveNoise);
    }
    let h = &obs.operator;
    // Ph = P·h  (n-vector).
    let ph: Vec<f64> = (0..n).map(|i| dot(&prior.cov[i], h)).collect();
    // innovation covariance S = hᵀ P h + r.
    let s = dot(h, &ph) + obs.noise_var;
    if s <= 0.0 || !s.is_finite() {
        return Err(AssimError::SingularInnovation);
    }
    // Kalman gain K = Ph / S; innovation d = y − h·mean.
    let d = obs.value - dot(h, &prior.mean);
    let mean: Vec<f64> = (0..n).map(|i| prior.mean[i] + ph[i] / s * d).collect();
    // P⁺ = P − K (Ph)ᵀ = P − (Ph)(Ph)ᵀ / S (stays symmetric).
    let mut cov = prior.cov.clone();
    for i in 0..n {
        for j in 0..n {
            cov[i][j] -= ph[i] * ph[j] / s;
        }
    }
    Ok(Belief { mean, cov })
}

/// Sequentially fuse all observations (order-independent for the
/// linear-Gaussian posterior).
///
/// # Errors
/// See [`assimilate`].
pub fn assimilate_all(prior: &Belief, observations: &[Observation]) -> Result<Belief, AssimError> {
    let mut belief = prior.clone();
    for obs in observations {
        belief = assimilate(&belief, obs)?;
    }
    Ok(belief)
}

/// The colored, regime-tagged assimilated posterior.
#[derive(Debug, Clone, PartialEq)]
pub struct AssimilatedPosterior {
    /// The updated belief.
    pub belief: Belief,
    /// Validated color, anchored to the calibrated instrument(s).
    pub color: Color,
    /// The misfit before assimilation.
    pub misfit_before: f64,
    /// The misfit after (≤ before).
    pub misfit_after: f64,
}

/// Assimilate all observations and return a VALIDATED, instrument-anchored
/// posterior for a named regime — Proposal 3's living belief update.
///
/// # Errors
/// See [`assimilate`].
pub fn assimilate_colored(
    prior: &Belief,
    observations: &[Observation],
    regime_param: &str,
    regime_lo: f64,
    regime_hi: f64,
) -> Result<AssimilatedPosterior, AssimError> {
    let misfit_before = misfit(prior, observations);
    let belief = assimilate_all(prior, observations)?;
    let misfit_after = misfit(&belief, observations);
    // anchor to the instruments that produced the readings.
    let mut anchors: Vec<&str> = observations.iter().map(|o| o.instrument.as_str()).collect();
    anchors.sort_unstable();
    anchors.dedup();
    let dataset = anchors.join("+");
    Ok(AssimilatedPosterior {
        belief,
        color: Color::Validated {
            regime: ValidityDomain::unconstrained().with(regime_param, regime_lo, regime_hi),
            dataset,
        },
        misfit_before,
        misfit_after,
    })
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
