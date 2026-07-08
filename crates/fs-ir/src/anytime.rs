//! ANYTIME + REFUSAL SEMANTICS (addendum Proposal 8, bead lmp4.17;
//! ships behind `ladder-planner` because it drives the planner, but the
//! CONTRACT here survives even if the planner is frozen by its kill
//! criterion — this is the product win): every query returns
//! IMMEDIATELY with a wide certified interval and tightens
//! monotonically as budget is spent; every interval carries its color
//! and a "WHAT WOULD TIGHTEN THIS" hint that prices the next move; and
//! when the budget cannot discharge the query, the system SAYS SO with
//! the interval it DID achieve and the price of the gap — never a
//! silent best-effort number dressed as an answer.
//!
//! Determinism (G5): the planner underneath is deterministic, so a
//! replayed query reproduces the same interval trajectory.

use crate::planner::{AnswerCache, CostTable, PlanOp, PlanOutcome, ProblemFamily, plan};
use fs_evidence::Color;

/// One point on the anytime trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalStep {
    /// The budget this step was allowed (cells).
    pub budget: f64,
    /// The certified half-width achieved.
    pub bound: f64,
    /// The Proposal-3 color of the interval (equilibrated enclosures
    /// are VERIFIED; the operator always knows what they hold).
    pub color: Color,
    /// The "what would tighten this" hint, priced.
    pub hint: String,
    /// True when the query discharged at this budget.
    pub discharged: bool,
}

/// The anytime result: the trajectory plus the final verdict.
#[derive(Debug, Clone)]
pub struct AnytimeReport {
    /// The interval trajectory, one entry per budget rung.
    pub trajectory: Vec<IntervalStep>,
    /// The refusal note when the final budget could not discharge —
    /// the achieved interval AND the price of the gap, teaching.
    pub refusal: Option<String>,
}

impl AnytimeReport {
    /// The final certified bound.
    #[must_use]
    pub fn final_bound(&self) -> f64 {
        self.trajectory.last().map_or(f64::INFINITY, |s| s.bound)
    }

    /// Did the query discharge within the final budget?
    #[must_use]
    pub fn discharged(&self) -> bool {
        self.trajectory.last().is_some_and(|s| s.discharged)
    }
}

/// The "what would tighten this" hint: extrapolate the price of closing
/// the gap from the achieved bound and spend (O(h) energy convergence:
/// cells scale like bound/tol), and NAME where the money goes (the
/// residual's hot region when the planner exposes it, else the
/// operator menu's next move). Cold telemetry degrades to a generic
/// but still-priced hint.
#[must_use]
pub fn tighten_hint(
    bound: f64,
    tol: f64,
    cells_spent: f64,
    costs: &CostTable,
    hot_region: Option<(f64, f64)>,
) -> String {
    if bound <= tol {
        return "already at tolerance — spend nothing".to_string();
    }
    let factor = (bound / tol).max(1.0);
    let projected = cells_spent * factor;
    let extra = (projected - cells_spent).max(1.0);
    let next_op = if costs.predict(PlanOp::DwrRefine) <= costs.predict(PlanOp::Climb) {
        PlanOp::DwrRefine
    } else {
        PlanOp::Climb
    };
    match hot_region {
        Some((lo, hi)) => format!(
            "closing ±{bound:.3e} to ±{tol:.3e} needs ~{extra:.0} more cells via \
             {}, mostly on the region x ∈ [{lo:.2}, {hi:.2}]",
            next_op.name()
        ),
        None => format!(
            "closing ±{bound:.3e} to ±{tol:.3e} needs ~{extra:.0} more cells via {}",
            next_op.name()
        ),
    }
}

/// Run the query ANYTIME-style over an increasing budget ladder: each
/// rung re-plans deterministically (the shared cache makes later rungs
/// cheaper, never different), recording the certified interval, its
/// color, and the priced hint at every step. The FIRST rung is the
/// immediate answer; the LAST rung's failure produces the teaching
/// refusal.
#[must_use]
pub fn run_anytime(
    family: &ProblemFamily,
    theta: f64,
    tol: f64,
    budget_ladder: &[f64],
    rung_cells: &[usize],
    cache: &mut dyn AnswerCache,
    costs: &mut CostTable,
) -> AnytimeReport {
    let mut trajectory = Vec::with_capacity(budget_ladder.len());
    let mut refusal = None;
    for (i, &budget) in budget_ladder.iter().enumerate() {
        let outcome = plan(family, theta, tol, budget, rung_cells, cache, costs);
        let last = i + 1 == budget_ladder.len();
        match outcome {
            PlanOutcome::Discharged {
                bound, cost, mesh, ..
            } => {
                let hot = hot_region_of(&mesh);
                trajectory.push(IntervalStep {
                    budget,
                    bound,
                    color: Color::Verified { lo: 0.0, hi: bound },
                    hint: tighten_hint(bound, tol, cost, costs, hot),
                    discharged: true,
                });
                // Discharged: later budgets would only repeat the cache
                // hit — the trajectory is complete.
                break;
            }
            PlanOutcome::RefusedWithBest {
                best_bound,
                best_mesh,
                cost,
                ..
            } => {
                let hot = hot_region_of(&best_mesh);
                let hint = tighten_hint(best_bound, tol, cost, costs, hot);
                trajectory.push(IntervalStep {
                    budget,
                    bound: best_bound,
                    color: Color::Verified {
                        lo: 0.0,
                        hi: best_bound,
                    },
                    hint: hint.clone(),
                    discharged: false,
                });
                if last {
                    refusal = Some(format!(
                        "REFUSED at the requested tolerance: achieved a certified \
                         ±{best_bound:.3e} (verified) within {budget:.0} cells; {hint}. \
                         No best-effort point estimate is returned."
                    ));
                }
            }
        }
    }
    AnytimeReport {
        trajectory,
        refusal,
    }
}

/// The densest-mesh window (where refinement concentrated): the hint's
/// "where the money goes". `None` on uniform meshes.
fn hot_region_of(mesh: &[f64]) -> Option<(f64, f64)> {
    if mesh.len() < 3 {
        return None;
    }
    let mut min_h = f64::INFINITY;
    let mut max_h = 0.0f64;
    for e in 0..mesh.len() - 1 {
        let h = mesh[e + 1] - mesh[e];
        min_h = min_h.min(h);
        max_h = max_h.max(h);
    }
    if max_h < 2.0 * min_h {
        return None; // effectively uniform: no hot region to name
    }
    // The window spanned by the finest quartile of elements.
    let cutoff = 2.0 * min_h;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for e in 0..mesh.len() - 1 {
        if mesh[e + 1] - mesh[e] <= cutoff {
            lo = lo.min(mesh[e]);
            hi = hi.max(mesh[e + 1]);
        }
    }
    (lo < hi).then_some((lo, hi))
}
