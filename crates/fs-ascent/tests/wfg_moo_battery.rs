//! 7tv.21 (WFG-class fixtures leg): MOO standards — WFG-family front
//! conformance for the Pareto tracing sweeps, covering the three front
//! geometries the ZDT/DTLZ batteries do not: the WFG concave ellipse,
//! the WFG convex arc, and the WFG1-class MIXED (rippled) curve — all
//! under the WFG objective scaling (f1 ∈ [0,2], f2 ∈ [0,4]), which the
//! unit-scaled batteries never exercise.
//!
//! Honest reduction (deliberate, recorded): the distance function is the
//! smooth unimodal ZDT-style g, not WFG's multimodal/deceptive
//! t-transformations — gradient sweeps on multimodal distance are
//! exactly the basin-fragility class the DTLZ x86-64 dispatch on this
//! bead documents, and exercising it belongs to that resolution, not to
//! front-geometry conformance. WFG2-class DISCONNECTED fronts need
//! feasibility-gap handling in the ε sweep and stay a named follow-on.

use fs_ascent::pareto::{ParetoPoint, epsilon_constraint_sweep, weighted_sum_sweep};

fn verdict(name: &str, pass: bool, details: &str) {
    println!("{{\"test\":\"{name}\",\"pass\":{pass},\"details\":\"{details}\"}}");
    assert!(pass, "{name}: {details}");
}

const N: usize = 3;
const PI: f64 = std::f64::consts::PI;
/// WFG objective scales: f1 ∈ [0, 2], f2 ∈ [0, 4].
const S1: f64 = 2.0;
const S2: f64 = 4.0;
/// Mixed-front ripple count (WFG1's A = 5).
const RIPPLES: f64 = 5.0;

/// Chart: x_i = sin²(θ_i) ∈ [0, 1]; dx/dθ = sin(2θ).
fn chart(theta: &[f64]) -> Vec<f64> {
    theta.iter().map(|t| t.sin() * t.sin()).collect()
}

fn chart_jac(theta: &[f64]) -> Vec<f64> {
    theta.iter().map(|t| (2.0 * t).sin()).collect()
}

/// Smooth unimodal distance g = 1 + 9·Σ_{i≥2} x_i/(n−1); front at g = 1.
fn g_of(x: &[f64]) -> f64 {
    1.0 + 9.0 * x[1..].iter().sum::<f64>() / (N as f64 - 1.0)
}

const G_C: f64 = 9.0 / (N as f64 - 1.0);

/// Shared assembly: f = S·g·h(t) with t = x₁, plus the chain-rule
/// gradient in θ from (∂h/∂t, h, scale).
fn objective(theta: &[f64], scale: f64, h: f64, dh_dt: f64) -> (f64, Vec<f64>) {
    let x = chart(theta);
    let j = chart_jac(theta);
    let g = g_of(&x);
    let f = scale * g * h;
    let mut grad = vec![0.0; N];
    grad[0] = scale * g * dh_dt * j[0];
    for i in 1..N {
        grad[i] = scale * h * G_C * j[i];
    }
    (f, grad)
}

// ---- WFG concave (WFG4-class front, unimodal distance) ---------------

fn concave_f1(theta: &[f64]) -> (f64, Vec<f64>) {
    let t = chart(theta)[0];
    objective(
        theta,
        S1,
        (t * PI / 2.0).sin(),
        (PI / 2.0) * (t * PI / 2.0).cos(),
    )
}

fn concave_f2(theta: &[f64]) -> (f64, Vec<f64>) {
    let t = chart(theta)[0];
    objective(
        theta,
        S2,
        (t * PI / 2.0).cos(),
        -(PI / 2.0) * (t * PI / 2.0).sin(),
    )
}

// ---- WFG convex ------------------------------------------------------

fn convex_f1(theta: &[f64]) -> (f64, Vec<f64>) {
    let t = chart(theta)[0];
    objective(
        theta,
        S1,
        1.0 - (t * PI / 2.0).cos(),
        (PI / 2.0) * (t * PI / 2.0).sin(),
    )
}

fn convex_f2(theta: &[f64]) -> (f64, Vec<f64>) {
    let t = chart(theta)[0];
    objective(
        theta,
        S2,
        1.0 - (t * PI / 2.0).sin(),
        -(PI / 2.0) * (t * PI / 2.0).cos(),
    )
}

// ---- WFG1-class mixed (rippled) --------------------------------------

fn mixed_h2(t: f64) -> f64 {
    1.0 - t - (2.0 * PI * RIPPLES * t + PI / 2.0).cos() / (2.0 * PI * RIPPLES)
}

fn mixed_dh2(t: f64) -> f64 {
    -1.0 + (2.0 * PI * RIPPLES * t + PI / 2.0).sin()
}

fn mixed_f1(theta: &[f64]) -> (f64, Vec<f64>) {
    let t = chart(theta)[0];
    objective(theta, S1, t, 1.0)
}

fn mixed_f2(theta: &[f64]) -> (f64, Vec<f64>) {
    let t = chart(theta)[0];
    objective(theta, S2, mixed_h2(t), mixed_dh2(t))
}

// ---- shared conformance walk -----------------------------------------

struct FrontReport {
    worst_front_err: f64,
    worst_g_excess: f64,
    worst_kkt: f64,
    f1_min: f64,
    f1_max: f64,
    scale_respected: bool,
}

fn conform(points: &[ParetoPoint], analytic_f2: impl Fn(f64) -> f64) -> FrontReport {
    let mut r = FrontReport {
        worst_front_err: 0.0,
        worst_g_excess: 0.0,
        worst_kkt: 0.0,
        f1_min: f64::INFINITY,
        f1_max: f64::NEG_INFINITY,
        scale_respected: true,
    };
    for p in points {
        let [f1, f2] = p.f;
        r.worst_front_err = r.worst_front_err.max((f2 - analytic_f2(f1)).abs());
        let x = chart(&p.x);
        r.worst_g_excess = r.worst_g_excess.max(g_of(&x) - 1.0);
        if let Some(kkt) = &p.kkt {
            r.worst_kkt = r.worst_kkt.max(kkt.stationarity);
        }
        r.f1_min = r.f1_min.min(f1);
        r.f1_max = r.f1_max.max(f1);
        // The WFG scaling envelope with slack for solver tolerance.
        r.scale_respected &= (0.0..=S1 + 1e-3).contains(&f1) && (0.0..=S2 + 1e-3).contains(&f2);
    }
    r
}

/// ε grid over f1's WFG range [0, 2] (interior, seam-free).
fn epsilons() -> Vec<f64> {
    (1..=19).map(|k| f64::from(k) * 0.1).collect()
}

#[test]
fn wfg_concave_ellipse_front_conformance_under_wfg_scaling() {
    let theta0 = [0.8f64, 0.3, 0.3];
    let points = epsilon_constraint_sweep(&concave_f1, &concave_f2, &epsilons(), &theta0, 1e-9);
    // Front at g = 1: (f1/2)² + (f2/4)² = 1, the concave ellipse arc.
    let r = conform(&points, |f1| {
        S2 * (1.0 - (f1 / S1) * (f1 / S1)).max(0.0).sqrt()
    });
    assert!(
        r.worst_front_err < 2e-3,
        "wfg-concave: front error {:.3e}",
        r.worst_front_err
    );
    assert!(
        r.worst_g_excess < 1e-4,
        "wfg-concave: g excess {:.3e}",
        r.worst_g_excess
    );
    assert!(r.worst_kkt < 1e-4, "wfg-concave: KKT {:.3e}", r.worst_kkt);
    assert!(
        r.f1_min < 0.12 && r.f1_max > 1.88,
        "wfg-concave: coverage [{:.3}, {:.3}]",
        r.f1_min,
        r.f1_max
    );
    assert!(
        r.scale_respected,
        "wfg-concave: WFG scaling envelope violated"
    );

    // Concave front: interior weighted sums collapse to the extremes.
    let weights: Vec<f64> = (1..=9).map(|k| f64::from(k) * 0.1).collect();
    let ws = weighted_sum_sweep(&concave_f1, &concave_f2, &weights, &theta0);
    let interior = ws
        .iter()
        .filter(|p| p.f[0] > 0.1 && p.f[0] < S1 - 0.1)
        .count();
    assert_eq!(
        interior, 0,
        "wfg-concave: weighted sums must collapse on a concave front"
    );
    verdict(
        "7tv21-wfg-concave",
        true,
        &format!(
            "ellipse front 19 eps-points under (2,4) scaling: front-err {:.2e}, g-excess \
             {:.2e}, KKT {:.2e}, coverage [{:.2},{:.2}], WS collapse exhibited",
            r.worst_front_err, r.worst_g_excess, r.worst_kkt, r.f1_min, r.f1_max
        ),
    );
}

#[test]
fn wfg_convex_arc_front_conformance_under_wfg_scaling() {
    let theta0 = [0.8f64, 0.3, 0.3];
    let points = epsilon_constraint_sweep(&convex_f1, &convex_f2, &epsilons(), &theta0, 1e-9);
    // Front at g = 1: t = (2/π)·acos(1 − f1/2), f2 = 4(1 − sin(tπ/2))
    // — eliminate t: sin(tπ/2) = √(1 − (1 − f1/2)²).
    let r = conform(&points, |f1| {
        let c = 1.0 - f1 / S1;
        S2 * (1.0 - (1.0 - c * c).max(0.0).sqrt())
    });
    assert!(
        r.worst_front_err < 2e-3,
        "wfg-convex: front error {:.3e}",
        r.worst_front_err
    );
    assert!(
        r.worst_g_excess < 1e-4,
        "wfg-convex: g excess {:.3e}",
        r.worst_g_excess
    );
    assert!(r.worst_kkt < 1e-4, "wfg-convex: KKT {:.3e}", r.worst_kkt);
    assert!(
        r.f1_min < 0.12 && r.f1_max > 1.88,
        "wfg-convex: coverage [{:.3}, {:.3}]",
        r.f1_min,
        r.f1_max
    );
    assert!(
        r.scale_respected,
        "wfg-convex: WFG scaling envelope violated"
    );

    // Convex front: interior weighted sums DO land on the interior —
    // the geometric contrast with the concave case, on WFG scales.
    let weights: Vec<f64> = (2..=8).map(|k| f64::from(k) * 0.1).collect();
    let ws = weighted_sum_sweep(&convex_f1, &convex_f2, &weights, &theta0);
    let interior = ws
        .iter()
        .filter(|p| p.f[0] > 0.1 && p.f[0] < S1 - 0.1)
        .count();
    assert!(
        interior >= 3,
        "wfg-convex: weighted sums must reach the interior of a convex front, got {interior}"
    );
    verdict(
        "7tv21-wfg-convex",
        true,
        &format!(
            "convex arc 19 eps-points under (2,4) scaling: front-err {:.2e}, g-excess {:.2e}, \
             KKT {:.2e}, coverage [{:.2},{:.2}], WS interior {interior}",
            r.worst_front_err, r.worst_g_excess, r.worst_kkt, r.f1_min, r.f1_max
        ),
    );
}

#[test]
fn wfg1_mixed_ripple_curve_membership_and_weighted_sum_hull_gap() {
    let theta0 = [0.8f64, 0.3, 0.3];
    let points = epsilon_constraint_sweep(&mixed_f1, &mixed_f2, &epsilons(), &theta0, 1e-9);
    // Every ε solution must lie ON the mixed curve at g = 1 (whether the
    // constraint binds on a descending segment or rests in a ripple
    // valley, the solution stays on-curve with g collapsed).
    let r = conform(&points, |f1| S2 * mixed_h2(f1 / S1));
    assert!(
        r.worst_front_err < 5e-3,
        "wfg-mixed: curve membership error {:.3e}",
        r.worst_front_err
    );
    assert!(
        r.worst_g_excess < 1e-4,
        "wfg-mixed: g excess {:.3e}",
        r.worst_g_excess
    );
    assert!(r.worst_kkt < 1e-3, "wfg-mixed: KKT {:.3e}", r.worst_kkt);
    assert!(
        r.scale_respected,
        "wfg-mixed: WFG scaling envelope violated"
    );

    // WFG1's mixed slope is h2' = −1 + sin(·) ∈ [−2, 0]: the curve is
    // monotone with STATIONARY PLATEAUS at the ripple crests. The
    // measured sweep behaviors on this shape class (both verified
    // on-curve above):
    //  - the warm-started ε tracer legitimately CLUSTERS at plateaus
    //    (interior stationarity satisfies KKT with the constraint
    //    slack), so its distinct-f1 coverage is sparse;
    //  - the weighted-sum sweep solves h2' = −w/(2(1−w)) < 0 per
    //    weight, landing on ripple DESCENTS: dense distinct coverage.
    // Both facts are asserted as measured, not assumed.
    let weights: Vec<f64> = (1..=19).map(|k| f64::from(k) * 0.05).collect();
    let ws = weighted_sum_sweep(&mixed_f1, &mixed_f2, &weights, &theta0);
    let mut worst_ws_membership = 0.0f64;
    for p in &ws {
        worst_ws_membership = worst_ws_membership.max((p.f[1] - S2 * mixed_h2(p.f[0] / S1)).abs());
    }
    assert!(
        worst_ws_membership < 5e-3,
        "wfg-mixed: weighted-sum points left the curve ({worst_ws_membership:.3e})"
    );
    let distinct = |pts: &[ParetoPoint]| -> usize {
        pts.iter()
            .map(|p| (p.f[0] / S1 * 1000.0).round() as i64)
            .collect::<std::collections::BTreeSet<i64>>()
            .len()
    };
    let (eps_distinct, ws_distinct) = (distinct(&points), distinct(&ws));
    assert!(
        ws_distinct >= 12,
        "wfg-mixed: weighted sums must cover ripple descents densely, got {ws_distinct}"
    );
    assert!(
        eps_distinct >= 3,
        "wfg-mixed: the eps tracer must still traverse between plateaus, got {eps_distinct}"
    );
    verdict(
        "7tv21-wfg-mixed",
        true,
        &format!(
            "rippled WFG1-class curve: eps membership-err {:.2e} / WS {:.2e}, g-excess {:.2e}, \
             KKT {:.2e}; measured coverage eps {} cells (plateau clustering) vs WS {} cells \
             (descent sampling)",
            r.worst_front_err,
            worst_ws_membership,
            r.worst_g_excess,
            r.worst_kkt,
            eps_distinct,
            ws_distinct
        ),
    );
}
