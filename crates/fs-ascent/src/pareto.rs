//! Gradient-based Pareto TRACING: scalarization sweeps that produce
//! fronts of certificate-grade points — warm-started L-BFGS along a
//! weighted-sum schedule (convex fronts), and warm-started augmented-
//! Lagrangian ε-constraint continuation (the form that also covers
//! CONCAVE fronts, where weighted sums provably collapse to the
//! extremes — exhibited, not cited, in the battery). Every
//! ε-constraint point carries its KKT residual certificate.

use crate::auglag::{ConstrainedProblem, KktResidual, augmented_lagrangian};
use crate::lbfgs::LbfgsState;
use crate::stop::StopRule;

/// One traced Pareto point.
#[derive(Debug, Clone)]
pub struct ParetoPoint {
    /// Decision vector.
    pub x: Vec<f64>,
    /// Objective values (f₁, f₂).
    pub f: [f64; 2],
    /// KKT certificate (ε-constraint path; None for weighted sums,
    /// whose certificate is the scalarized gradient norm).
    pub kkt: Option<KktResidual>,
    /// Scalarized gradient ∞-norm at the solution (weighted-sum path).
    pub grad_norm: f64,
}

/// Objective callback: x ↦ (f, ∇f). `Fn` (not FnMut) so the sweep can
/// wrap it for the constrained solver's split borrows.
pub type Objective<'a> = &'a dyn Fn(&[f64]) -> (f64, Vec<f64>);

fn assert_decision(x: &[f64]) {
    assert!(
        !x.is_empty() && x.iter().all(|v| v.is_finite()),
        "Pareto decision vectors must be non-empty and finite"
    );
}

fn eval_objective(label: &str, f: Objective<'_>, x: &[f64]) -> (f64, Vec<f64>) {
    assert_decision(x);
    let (value, grad) = f(x);
    assert!(value.is_finite(), "{label} objective value must be finite");
    assert_eq!(
        grad.len(),
        x.len(),
        "{label} gradient length must match the decision dimension"
    );
    assert!(
        grad.iter().all(|v| v.is_finite()),
        "{label} gradient entries must be finite"
    );
    (value, grad)
}

/// Weighted-sum sweep: for each w in `weights` (processed in order),
/// minimize w·f₁ + (1−w)·f₂ by L-BFGS, WARM-STARTED from the previous
/// solution (continuation along the front). Exact on convex fronts;
/// on concave fronts this collapses to extremes — use
/// [`epsilon_constraint_sweep`] there.
#[must_use]
pub fn weighted_sum_sweep(
    f1: Objective<'_>,
    f2: Objective<'_>,
    weights: &[f64],
    x0: &[f64],
) -> Vec<ParetoPoint> {
    assert_decision(x0);
    assert!(
        weights
            .iter()
            .all(|w| w.is_finite() && (0.0..=1.0).contains(w)),
        "Pareto weights must be finite and inside [0, 1]"
    );
    let mut x = x0.to_vec();
    let mut out = Vec::with_capacity(weights.len());
    for &w in weights {
        let mut fg = |xv: &[f64]| -> (f64, Vec<f64>) {
            let (a, ga) = eval_objective("f1", f1, xv);
            let (b, gb) = eval_objective("f2", f2, xv);
            let val = w.mul_add(a, (1.0 - w) * b);
            let g: Vec<f64> = ga
                .iter()
                .zip(&gb)
                .map(|(p, q)| w.mul_add(*p, (1.0 - w) * q))
                .collect();
            (val, g)
        };
        let mut st = LbfgsState::new(&x, 10, &mut fg);
        let rep = st.run(&mut fg, &StopRule::GradNorm(1e-10), 500);
        x = st.x.clone();
        let (a, _) = eval_objective("f1", f1, &x);
        let (b, _) = eval_objective("f2", f2, &x);
        out.push(ParetoPoint {
            x: x.clone(),
            f: [a, b],
            kkt: None,
            grad_norm: rep.grad_norm,
        });
    }
    out
}

/// ε-constraint sweep: for each ε in `epsilons` (in order), solve
/// min f₂ s.t. f₁ ≤ ε by the augmented Lagrangian, WARM-STARTED from
/// the previous solution. Covers concave fronts; every point carries
/// its KKT certificate.
#[must_use]
pub fn epsilon_constraint_sweep(
    f1: Objective<'_>,
    f2: Objective<'_>,
    epsilons: &[f64],
    x0: &[f64],
    tol: f64,
) -> Vec<ParetoPoint> {
    assert_decision(x0);
    assert!(
        epsilons.iter().all(|eps| eps.is_finite()),
        "Pareto epsilon constraints must be finite"
    );
    assert!(
        tol.is_finite() && tol > 0.0,
        "Pareto epsilon-constraint tolerance must be finite and positive"
    );
    let mut x = x0.to_vec();
    let mut out = Vec::with_capacity(epsilons.len());
    for &eps in epsilons {
        let mut fg = |xv: &[f64]| eval_objective("f2", f2, xv);
        let ci = |xv: &[f64]| -> Vec<f64> {
            let (a, _) = eval_objective("f1", f1, xv);
            vec![a - eps]
        };
        let ci_jt = |xv: &[f64], wv: &[f64]| -> Vec<f64> {
            assert_eq!(
                wv.len(),
                1,
                "epsilon-constraint multiplier dimension must be one"
            );
            let (_, ga) = eval_objective("f1", f1, xv);
            ga.iter().map(|g| g * wv[0]).collect()
        };
        let ce = |_: &[f64]| Vec::new();
        let ce_jt = |xv: &[f64], _: &[f64]| vec![0.0f64; xv.len()];
        let mut problem = ConstrainedProblem {
            fg: &mut fg,
            ce: &ce,
            ce_jt: &ce_jt,
            ci: &ci,
            ci_jt: &ci_jt,
        };
        let rep = augmented_lagrangian(&mut problem, &x, tol, 40);
        x.clone_from(&rep.x);
        let (a, _) = eval_objective("f1", f1, &x);
        let (b, _) = eval_objective("f2", f2, &x);
        out.push(ParetoPoint {
            x: x.clone(),
            f: [a, b],
            grad_norm: rep.kkt.stationarity,
            kkt: Some(rep.kkt),
        });
    }
    out
}
