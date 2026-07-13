//! Battery for neural implicit charts (fs-rep-neural). The load-bearing tests
//! are the CERTIFICATE ones: the spectral norm is correct, the certified
//! Lipschitz constant is never violated by sampling, interval bound propagation
//! is sound over a box, the gradient is bounded by L, and a sphere-trace step
//! of |f|/L never tunnels through the surface.

use fs_rep_neural::{
    Layer, MlpSdf, TopologyHint, safe_step_radius, spectral_norm, spectral_normalize,
};

// A deterministic pseudo-random point stream in [-1, 1)^2 (no rand crate).
fn points(n: usize, seed: u64) -> Vec<[f64; 2]> {
    let mut s = seed;
    let mut next = || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // `s >> 32` ∈ [0, 2^32); /2^31 ∈ [0, 2); −1 ∈ [-1, 1). (The former
        // `s >> 33` gave [0, 2^31)/2^31 − 1 = [-1, 0) — only the third
        // quadrant, so the certificate tests silently probed one octant of the
        // domain. `L` is a GLOBAL Lipschitz bound, so full-domain coverage is
        // still sound.)
        ((s >> 32) as f64 / f64::from(1u32 << 31)) - 1.0
    };
    (0..n).map(|_| [next(), next()]).collect()
}

fn sample_mlp() -> MlpSdf {
    let l1 = Layer::new(
        vec![vec![0.5, -0.3], vec![0.2, 0.7], vec![-0.4, 0.1]],
        vec![0.1, -0.2, 0.05],
    );
    let l2 = Layer::new(vec![vec![0.6, -0.5, 0.3]], vec![0.0]);
    MlpSdf::new(vec![l1, l2], 1.0)
}

#[test]
fn spectral_norm_matches_known_values() {
    // diagonal: σ_max is the largest |diagonal|.
    assert!((spectral_norm(&[vec![3.0, 0.0], vec![0.0, 1.0]]) - 3.0).abs() < 1e-9);
    // [[1,2],[3,4]] has σ_max = sqrt((30 + sqrt(884))/2) ≈ 5.464986.
    let s = spectral_norm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    assert!((s - 5.464_985_704_219_04).abs() < 1e-6, "σ = {s}");
}

#[test]
fn spectral_normalization_sets_the_norm_to_the_bound() {
    let layer = Layer::new(vec![vec![1.0, 2.0], vec![3.0, 4.0]], vec![0.0, 0.0]);
    let normed = spectral_normalize(layer, 2.0);
    assert!((spectral_norm(&normed.weights) - 2.0).abs() < 1e-6);
}

#[test]
fn the_lipschitz_certificate_is_never_violated() {
    let net = sample_mlp();
    let l = net.lipschitz();
    assert!(l > 0.0 && l <= 1.0 + 1e-6, "L = {l}"); // bound 1, 1-Lipschitz tanh
    let pts = points(300, 0x1234_5678);
    let mut max_ratio = 0.0_f64;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dist = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        if dist < 1e-9 {
            continue;
        }
        let ratio = (net.eval(&a) - net.eval(&b)).abs() / dist;
        max_ratio = max_ratio.max(ratio);
    }
    // NO sampled violation of the certified bound.
    assert!(
        max_ratio <= l + 1e-6,
        "sampled ratio {max_ratio} exceeds L {l}"
    );
}

#[test]
fn interval_bound_propagation_is_sound() {
    let net = sample_mlp();
    let (lo, hi) = ([-0.5, -0.5], [0.5, 0.5]);
    let (blo, bhi) = net.eval_interval(&lo, &hi);
    assert!(blo <= bhi);
    // every point in the box has f(x) inside the guaranteed enclosure.
    for p in points(400, 0xBEEF) {
        let x = [p[0] * 0.5, p[1] * 0.5]; // remap into the box
        let v = net.eval(&x);
        assert!(
            v >= blo - 1e-12 && v <= bhi + 1e-12,
            "f={v} escaped [{blo}, {bhi}]"
        );
    }
    // a degenerate box collapses to the point value.
    let x = [0.2, -0.1];
    let (dlo, dhi) = net.eval_interval(&x, &x);
    assert!((dlo - net.eval(&x)).abs() < 1e-12 && (dhi - net.eval(&x)).abs() < 1e-12);
}

#[test]
fn the_gradient_is_bounded_by_the_lipschitz_constant() {
    let net = sample_mlp();
    let l = net.lipschitz();
    for p in points(200, 0xC0FFEE) {
        let g = net.eval_grad(&p);
        let gnorm = (g[0] * g[0] + g[1] * g[1]).sqrt();
        assert!(gnorm <= l + 1e-4, "‖∇f‖ = {gnorm} exceeds L = {l}");
    }
}

#[test]
fn a_certified_sphere_trace_step_never_tunnels() {
    let net = sample_mlp();
    let l = net.lipschitz();
    let x0 = [0.3, 0.2];
    let v = net.eval(&x0);
    let r = safe_step_radius(v, l);
    // f cannot change sign anywhere within radius r of x0 -> no tunneling.
    for k in 0..64 {
        let theta = f64::from(k) * std::f64::consts::TAU / 64.0;
        let y = [
            x0[0] + 0.999 * r * theta.cos(),
            x0[1] + 0.999 * r * theta.sin(),
        ];
        assert!(
            net.eval(&y).signum() == v.signum(),
            "tunneled at angle {theta}"
        );
    }
}

#[test]
fn topology_is_honestly_unknown() {
    assert_eq!(sample_mlp().topology_hint(), TopologyHint::Unknown);
}

#[test]
fn evaluation_is_deterministic() {
    let a = sample_mlp();
    let b = sample_mlp();
    let x = [0.4, -0.6];
    assert_eq!(a.eval(&x).to_bits(), b.eval(&x).to_bits());
    assert_eq!(a.lipschitz().to_bits(), b.lipschitz().to_bits());
}
