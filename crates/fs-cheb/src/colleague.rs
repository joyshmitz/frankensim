//! Colleague-matrix rootfinding (bead kw89): the Chebyshev companion.
//! The v1 subdivision scanner (`Cheb1::roots`) provably MISSES
//! even-multiplicity roots (no sign change to see — a documented
//! no-claim); the colleague matrix sees every root as an eigenvalue.
//! Eigenvalues come from the fs-la complex nonsymmetric stack, real
//! in-domain roots are filtered by a DOCUMENTED tolerance policy, and
//! `certified_roots` upgrades simple roots to rigorous fs-ivl
//! interval-Newton enclosures (Clenshaw evaluated in interval
//! arithmetic — the enclosure is a proof, not a float).

use crate::Cheb1;
use fs_ivl::{Interval, RootBox, newton_roots};
use fs_la::eigen_complex::eig;
use fs_math::c64::C64;

/// Root-filter policy (all tolerances RELATIVE to the domain size
/// where lengths are involved).
#[derive(Debug, Clone, Copy)]
pub struct ColleaguePolicy {
    /// Trailing coefficients below `trim_rel`·max|c| are trimmed
    /// before building the matrix (degree deflation).
    pub trim_rel: f64,
    /// Eigenvalues with |Im| above this are discarded as complex.
    pub im_tol: f64,
    /// Reference-domain slack outside [−1, 1] still accepted (roots
    /// on the boundary jitter by rounding), clamped back in.
    pub domain_slack: f64,
    /// Roots closer than this (reference domain) merge into one
    /// reported position (multiple eigenvalues of a multiple root).
    pub cluster_tol: f64,
}

impl Default for ColleaguePolicy {
    fn default() -> Self {
        ColleaguePolicy {
            trim_rel: 1e-13,
            im_tol: 1e-8,
            domain_slack: 1e-8,
            // A double root's eigenvalue pair splits at the √ε scale
            // (measured 5e-9 on the battery's fixture); the cluster
            // width must sit above it or the pair reports twice.
            cluster_tol: 1e-6,
        }
    }
}

fn assert_policy(policy: ColleaguePolicy) {
    assert!(
        policy.trim_rel.is_finite() && policy.trim_rel >= 0.0,
        "colleague trim tolerance must be finite and non-negative"
    );
    assert!(
        policy.im_tol.is_finite() && policy.im_tol >= 0.0,
        "colleague imaginary tolerance must be finite and non-negative"
    );
    assert!(
        policy.domain_slack.is_finite() && policy.domain_slack >= 0.0,
        "colleague domain slack must be finite and non-negative"
    );
    assert!(
        policy.cluster_tol.is_finite() && policy.cluster_tol > 0.0,
        "colleague cluster tolerance must be finite and positive"
    );
}

/// All real roots of the interpolant in its domain by the colleague
/// matrix, deduplicated per the policy. Even-multiplicity roots ARE
/// found (each appears once after clustering).
///
/// # Panics
/// If the trimmed polynomial is constant (no roots to define) — a
/// caller contract, or if the eigensolver fails to converge (typed
/// failure surfaced as a panic at fixture scale).
#[must_use]
pub fn colleague_roots(p: &Cheb1, policy: ColleaguePolicy) -> Vec<f64> {
    assert_policy(policy);
    // Cheb1 stores the Σ′ convention (c₀ un-halved: f = c₀/2 + Σ c_k T_k);
    // the colleague algebra wants the TRUE constant term.
    let mut coeffs = p.coeffs().to_vec();
    coeffs[0] *= 0.5;
    let cmax = coeffs.iter().fold(0.0f64, |m, &c| m.max(c.abs()));
    assert!(
        cmax.is_finite() && cmax > 0.0,
        "finite non-zero polynomial required for colleague matrix"
    );
    let mut n = coeffs.len() - 1;
    while n > 0 && coeffs[n].abs() <= policy.trim_rel * cmax {
        n -= 1;
    }
    assert!(n >= 1, "constant polynomial has no roots to define");
    // Colleague matrix (n×n) for Σ_{k=0}^{n} a_k T_k:
    // row 0:      x T_0 = T_1                → [0, 1, 0, ...]
    // row k:      x T_k = (T_{k−1}+T_{k+1})/2 → [.., ½, 0, ½, ..]
    // row n−1:    coefficient-loaded: −a_j/(2 a_n) + ½·δ_{j,n−2}.
    let mut m = vec![C64::ZERO; n * n];
    let set = |m: &mut Vec<C64>, r: usize, c: usize, v: f64| {
        m[r * n + c] = C64::new(v, 0.0);
    };
    if n == 1 {
        // a_0 + a_1 T_1 = 0 → x = −a_0/a_1.
        set(&mut m, 0, 0, -coeffs[0] / coeffs[1]);
    } else {
        set(&mut m, 0, 1, 1.0);
        for r in 1..n - 1 {
            set(&mut m, r, r - 1, 0.5);
            set(&mut m, r, r + 1, 0.5);
        }
        let an = coeffs[n];
        for (j, &coeff) in coeffs.iter().enumerate().take(n) {
            let mut v = -coeff / (2.0 * an);
            if j == n - 2 {
                v += 0.5;
            }
            set(&mut m, n - 1, j, v);
        }
    }
    let eigs = eig(&m, n).expect("colleague eigensolve converges");
    let mut roots: Vec<f64> = eigs
        .into_iter()
        .filter(|l| l.im.abs() <= policy.im_tol)
        .map(|l| l.re)
        .filter(|&t| t >= -1.0 - policy.domain_slack && t <= 1.0 + policy.domain_slack)
        .map(|t| t.clamp(-1.0, 1.0))
        .collect();
    roots.sort_by(f64::total_cmp);
    // Cluster dedupe (multiple roots surface as eigenvalue clusters).
    let mut out: Vec<f64> = Vec::new();
    for t in roots {
        if out.last().is_none_or(|&prev| t - prev > policy.cluster_tol) {
            out.push(t);
        }
    }
    // Map reference → domain.
    let (a, b) = p.domain();
    out.iter()
        .map(|&t| f64::midpoint(a, b) + t * (b - a) / 2.0)
        .collect()
}

/// Clenshaw evaluation of a Chebyshev series in INTERVAL arithmetic —
/// a rigorous enclosure of p over the reference-domain interval `t`.
#[must_use]
pub fn clenshaw_interval(coeffs: &[f64], t: Interval) -> Interval {
    assert!(
        !coeffs.is_empty() && coeffs.iter().all(|c| c.is_finite()),
        "interval Clenshaw needs finite coefficients"
    );
    let two_t = Interval::point(2.0) * t;
    let mut b1 = Interval::point(0.0);
    let mut b2 = Interval::point(0.0);
    for &c in coeffs.iter().rev() {
        let b0 = Interval::point(c) + two_t * b1 - b2;
        b2 = b1;
        b1 = b0;
    }
    // p(t) = b0 − t·b1_old = b1 − t·b2 after the loop shuffle.
    b1 - t * b2
}

/// Certified root enclosures over the interpolant's domain via
/// interval Newton (fs-ivl): every returned [`RootBox::Certified`]
/// PROVES a unique root inside; [`RootBox::Possible`] boxes are
/// honest ambiguity at `min_width` (multiple roots land here — the
/// derivative's enclosure contains zero, as it must).
#[must_use]
pub fn certified_roots(p: &Cheb1, min_width: f64) -> Vec<RootBox> {
    assert!(
        min_width.is_finite() && min_width > 0.0,
        "certified root min_width must be finite and positive"
    );
    let d = p.differentiate();
    // Σ′ convention: halve the stored c₀ of both series (see
    // colleague_roots) — clenshaw_interval sums plain Σ a_k T_k.
    let mut coeffs = p.coeffs().to_vec();
    coeffs[0] *= 0.5;
    // The derivative Cheb1 lives on the same domain; its coefficients
    // are with respect to the reference variable SCALED by 2/(b−a).
    // Work entirely in the reference domain: p_ref(t) = p(x(t)), whose
    // derivative series equals d's coefficients times (b−a)/2.
    let (a, b) = p.domain();
    let scale = (b - a) / 2.0;
    let mut dcoeffs: Vec<f64> = d.coeffs().iter().map(|&c| c * scale).collect();
    dcoeffs[0] *= 0.5;
    let f = move |t: Interval| clenshaw_interval(&coeffs, t);
    let fp = move |t: Interval| clenshaw_interval(&dcoeffs, t);
    newton_roots(&f, &fp, Interval::new(-1.0, 1.0), min_width)
        .into_iter()
        .map(|root| map_root_box_to_domain(root, a, b))
        .collect()
}

fn map_interval_to_domain(iv: Interval, a: f64, b: f64) -> Interval {
    let mid = f64::midpoint(a, b);
    let half = (b - a) / 2.0;
    Interval::point(mid) + Interval::point(half) * iv
}

fn map_root_box_to_domain(root: RootBox, a: f64, b: f64) -> RootBox {
    match root {
        RootBox::Certified(iv) => RootBox::Certified(map_interval_to_domain(iv, a, b)),
        RootBox::Possible(iv) => RootBox::Possible(map_interval_to_domain(iv, a, b)),
    }
}
