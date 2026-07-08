//! fs-voi — value-of-information and active validation. Layer: L4.
//!
//! The strategic layer deciding WHAT INFORMATION TO ACQUIRE NEXT. The whole
//! point is the ESTIMATOR-vs-DECISION distinction: spend compute where it can
//! CHANGE A DECISION (flip which design ranks first), not merely where an
//! estimator is uncertain. A design that is wildly uncertain but clearly last
//! is not worth another cent.
//!
//! Each design's objective is an Evidence-carrying estimate: a mean plus an
//! [`Uncertainty`] with NUMERICAL, STATISTICAL, and MODEL-form components. The
//! decision is "which design minimizes the objective". Over that:
//! - [`ranking_flip_probability`] — `P(a runner-up is actually better)`;
//! - [`evpi`] — the expected opportunity loss of the current decision (the
//!   ceiling on what any information is worth);
//! - [`action_value`] — how much a menu action (surrogate / simulate / sample /
//!   test / refine) REDUCES that EVPI, per unit cost. An action on a
//!   decision-IRRELEVANT design is worth ~0;
//! - [`recommend`] — the best action, or STOP when the decision is already
//!   robust;
//! - [`heuristic_choice`] — the uncertainty-proportional baseline VOI must beat
//!   ([M] discipline).
//!
//! Deterministic; no dependencies (Gaussian decision algebra with an in-house
//! normal CDF).

/// The three uncertainty components of an estimate (they compose in quadrature).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Uncertainty {
    /// Numerical / discretization uncertainty.
    pub numerical: f64,
    /// Statistical (sampling) uncertainty.
    pub statistical: f64,
    /// Model-form uncertainty.
    pub model: f64,
}

/// Which uncertainty component an action reduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    /// Numerical / discretization.
    Numerical,
    /// Statistical / sampling.
    Statistical,
    /// Model-form.
    Model,
}

impl Uncertainty {
    /// The total standard deviation `√(num² + stat² + model²)`.
    #[must_use]
    pub fn total_std(&self) -> f64 {
        (self.numerical * self.numerical
            + self.statistical * self.statistical
            + self.model * self.model)
            .sqrt()
    }

    /// The dominant uncertainty component.
    #[must_use]
    pub fn dominant(&self) -> Component {
        let (n, s, m) = (
            self.numerical.abs(),
            self.statistical.abs(),
            self.model.abs(),
        );
        if m >= n && m >= s {
            Component::Model
        } else if s >= n {
            Component::Statistical
        } else {
            Component::Numerical
        }
    }

    fn reduced(&self, component: Component, factor: f64) -> Uncertainty {
        let f = (1.0 - factor).clamp(0.0, 1.0);
        let mut u = *self;
        match component {
            Component::Numerical => u.numerical *= f,
            Component::Statistical => u.statistical *= f,
            Component::Model => u.model *= f,
        }
        u
    }
}

/// A design with an Evidence-carrying objective estimate (minimizing).
#[derive(Debug, Clone, PartialEq)]
pub struct DesignEstimate {
    /// The design id.
    pub name: String,
    /// The estimated objective (lower is better).
    pub mean: f64,
    /// Its decomposed uncertainty.
    pub uncertainty: Uncertainty,
}

impl DesignEstimate {
    /// A design estimate.
    #[must_use]
    pub fn new(name: impl Into<String>, mean: f64, uncertainty: Uncertainty) -> DesignEstimate {
        DesignEstimate {
            name: name.into(),
            mean,
            uncertainty,
        }
    }
    fn std(&self) -> f64 {
        self.uncertainty.total_std()
    }
}

/// `P(other is actually better than chosen)` — the ranking-flip probability
/// under Gaussian objectives (minimizing).
#[must_use]
pub fn ranking_flip_probability(chosen: &DesignEstimate, other: &DesignEstimate) -> f64 {
    let sigma = (chosen.std().powi(2) + other.std().powi(2)).sqrt();
    if sigma <= 0.0 {
        return f64::from(u8::from(other.mean < chosen.mean));
    }
    // P(obj_other < obj_chosen) = Φ((mean_chosen − mean_other) / σ).
    normal_cdf((chosen.mean - other.mean) / sigma)
}

/// The current decision posture: the best design, the runner-up, and the
/// probability the ranking flips between them.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionPosture {
    /// The current best design (lowest mean).
    pub best: String,
    /// The closest competitor.
    pub runner_up: String,
    /// `P(runner-up actually better)`.
    pub flip_probability: f64,
}

fn top_two(designs: &[DesignEstimate]) -> Option<(&DesignEstimate, &DesignEstimate)> {
    if designs.len() < 2 {
        return None;
    }
    let mut best: Option<(usize, &DesignEstimate)> = None;
    let mut runner: Option<(usize, &DesignEstimate)> = None;
    for (idx, design) in designs.iter().enumerate() {
        if !design.mean.is_finite() {
            continue;
        }
        match best {
            None => best = Some((idx, design)),
            Some(current_best) if estimate_precedes((idx, design), current_best) => {
                runner = best;
                best = Some((idx, design));
            }
            Some(_) => match runner {
                None => runner = Some((idx, design)),
                Some(current_runner) if estimate_precedes((idx, design), current_runner) => {
                    runner = Some((idx, design));
                }
                Some(_) => {}
            },
        }
    }
    let (_, best) = best?;
    let (_, runner) = runner?;
    Some((best, runner))
}

fn estimate_precedes(
    (a_idx, a): (usize, &DesignEstimate),
    (b_idx, b): (usize, &DesignEstimate),
) -> bool {
    match a.mean.total_cmp(&b.mean) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => a_idx < b_idx,
        std::cmp::Ordering::Greater => false,
    }
}

/// The decision posture over a set of designs.
#[must_use]
pub fn decision_posture(designs: &[DesignEstimate]) -> Option<DecisionPosture> {
    let (best, runner) = top_two(designs)?;
    Some(DecisionPosture {
        best: best.name.clone(),
        runner_up: runner.name.clone(),
        flip_probability: ranking_flip_probability(best, runner),
    })
}

/// Pairwise expected opportunity loss `E[(obj_chosen − obj_other)⁺]` for the
/// two closest designs — the expected regret of the current top-two decision.
fn pairwise_evpi(chosen: &DesignEstimate, other: &DesignEstimate) -> f64 {
    let sigma = (chosen.std().powi(2) + other.std().powi(2)).sqrt();
    let delta = chosen.mean - other.mean; // ≤ 0 when `chosen` is best
    if sigma <= 0.0 {
        return (-delta).max(0.0) * 0.0; // no uncertainty → no opportunity loss
    }
    let z = delta / sigma;
    sigma * normal_pdf(z) + delta * normal_cdf(z)
}

/// The expected value of PERFECT information for the current decision — the
/// expected opportunity loss of choosing the current best over the runner-up.
/// Near zero means the decision is already robust.
#[must_use]
pub fn evpi(designs: &[DesignEstimate]) -> f64 {
    match top_two(designs) {
        Some((best, runner)) => pairwise_evpi(best, runner),
        None => 0.0,
    }
}

/// A menu action's kind (which uncertainty it reduces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Run a cheaper surrogate — reduces numerical uncertainty.
    Surrogate,
    /// Run a higher-resolution simulation — reduces numerical uncertainty.
    Simulate,
    /// Refine the mesh near the decision boundary — reduces numerical.
    Refine,
    /// Sample more scenarios — reduces statistical uncertainty.
    Sample,
    /// Build / test a prototype — reduces model-form uncertainty.
    Test,
}

impl ActionKind {
    /// The uncertainty component this action reduces.
    #[must_use]
    pub fn component(self) -> Component {
        match self {
            ActionKind::Surrogate | ActionKind::Simulate | ActionKind::Refine => {
                Component::Numerical
            }
            ActionKind::Sample => Component::Statistical,
            ActionKind::Test => Component::Model,
        }
    }
}

/// A candidate information-acquisition action.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    /// The action id.
    pub name: String,
    /// What it does.
    pub kind: ActionKind,
    /// Which design it informs.
    pub target_design: String,
    /// The fraction by which it reduces the target component (`0..=1`).
    pub reduction: f64,
    /// Its cost.
    pub cost: f64,
}

/// The decision value of an action.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionValue {
    /// The action id.
    pub action: String,
    /// The EVPI reduction it buys (its decision value).
    pub value: f64,
    /// Its cost.
    pub cost: f64,
    /// Decision value per unit cost.
    pub value_per_cost: f64,
}

/// The decision value of `action`: how much it reduces the top-two EVPI (per
/// cost). An action on a design outside the decision boundary reduces the EVPI
/// by ~0 — worthless however uncertain that design is.
#[must_use]
pub fn action_value(designs: &[DesignEstimate], action: &Action) -> ActionValue {
    let before = evpi(designs);
    let after_designs: Vec<DesignEstimate> = designs
        .iter()
        .map(|d| {
            if d.name == action.target_design {
                DesignEstimate {
                    uncertainty: d
                        .uncertainty
                        .reduced(action.kind.component(), action.reduction),
                    ..d.clone()
                }
            } else {
                d.clone()
            }
        })
        .collect();
    let after = evpi(&after_designs);
    let value = (before - after).max(0.0);
    let value_per_cost = if action.cost.is_finite() && action.cost > 0.0 {
        value / action.cost
    } else if action.cost.is_finite()
        && action.cost >= 0.0
        && action.cost <= f64::EPSILON
        && value > 0.0
    {
        f64::INFINITY
    } else {
        0.0
    };
    ActionValue {
        action: action.name.clone(),
        value,
        cost: action.cost,
        value_per_cost,
    }
}

/// The VOI recommendation.
#[derive(Debug, Clone, PartialEq)]
pub enum Recommendation {
    /// Take this action (best decision value per cost).
    Act {
        /// The action id.
        action: String,
        /// Its decision value per cost.
        value_per_cost: f64,
    },
    /// Stop — the decision is already robust (EVPI below the threshold).
    Stop {
        /// Why.
        reason: String,
    },
}

/// Recommend the highest decision-value-per-cost action, or STOP when the
/// decision is already robust (current EVPI `<= stop_threshold`).
#[must_use]
pub fn recommend(
    designs: &[DesignEstimate],
    actions: &[Action],
    stop_threshold: f64,
) -> Recommendation {
    let current = evpi(designs);
    if current <= stop_threshold {
        return Recommendation::Stop {
            reason: format!("decision robust: EVPI {current:.3e} <= {stop_threshold:.3e}"),
        };
    }
    let best = actions
        .iter()
        .map(|a| action_value(designs, a))
        .filter(|v| v.value > 0.0 && v.value_per_cost > 0.0)
        .max_by(|a, b| {
            a.value_per_cost
                .partial_cmp(&b.value_per_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    match best {
        Some(v) => Recommendation::Act {
            action: v.action,
            value_per_cost: v.value_per_cost,
        },
        None => Recommendation::Stop {
            reason: "no action changes the decision".to_string(),
        },
    }
}

/// The [M] baseline VOI must beat: uncertainty-proportional allocation — pick
/// the action informing the design with the LARGEST total uncertainty,
/// regardless of decision relevance.
#[must_use]
pub fn heuristic_choice<'a>(
    designs: &[DesignEstimate],
    actions: &'a [Action],
) -> Option<&'a Action> {
    let most_uncertain = designs.iter().max_by(|a, b| {
        a.std()
            .partial_cmp(&b.std())
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    actions
        .iter()
        .find(|a| a.target_design == most_uncertain.name)
}

// -- Gaussian helpers (in-house; no external special functions) -------------

fn normal_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / std::f64::consts::TAU.sqrt()
}

fn normal_cdf(z: f64) -> f64 {
    f64::midpoint(1.0, erf(z / std::f64::consts::SQRT_2))
}

/// `erf` via Abramowitz & Stegun 7.1.26 (published constants, ≤ 1.5e-7 error).
#[allow(clippy::unreadable_literal)]
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    sign * y
}
