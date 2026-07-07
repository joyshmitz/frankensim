//! 1D Gauss–Legendre quadrature and Legendre/Lobatto polynomial
//! evaluation — the scalar bedrock of the tensor-product high-order
//! families (tfz.6). Deterministic: nodes come from Newton iteration
//! with fixed starting guesses and a fixed iteration count, all
//! arithmetic through fs-math strict kernels.

use fs_math::det;

/// Legendre polynomial L_n(x) and its derivative, by the three-term
/// recurrence (numerically stable on [-1, 1]).
#[must_use]
pub fn legendre(n: usize, x: f64) -> (f64, f64) {
    if n == 0 {
        return (1.0, 0.0);
    }
    let (mut pm1, mut p) = (1.0f64, x);
    for k in 1..n {
        let kf = k as f64;
        let next = ((2.0 * kf + 1.0) * x * p - kf * pm1) / (kf + 1.0);
        pm1 = p;
        p = next;
    }
    // L'_n from the standard identity (guarded at |x| = 1 where the
    // denominator vanishes; endpoint derivative is n(n+1)/2 · (±1)ⁿ⁺¹).
    let denom = x.mul_add(-x, 1.0);
    let dp = if denom.abs() < 1e-14 {
        let nf = n as f64;
        let sign = if x > 0.0 || (n + 1).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        sign * nf * (nf + 1.0) / 2.0
    } else {
        (n as f64) * x.mul_add(p, -pm1) / -denom
    };
    (p, dp)
}

/// Gauss–Legendre nodes and weights on [-1, 1]: `n` points integrate
/// polynomials of degree ≤ 2n−1 exactly. Chebyshev starting guesses +
/// 25 Newton steps (converged far past f64 by then; FIXED count keeps
/// the bit pattern independent of convergence-test vagaries).
#[must_use]
pub fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    assert!(n >= 1, "quadrature needs at least one point");
    let mut nodes = vec![0.0f64; n];
    let mut weights = vec![0.0f64; n];
    for i in 0..n {
        // Chebyshev-angle initial guess for root i (descending order).
        let theta = std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5);
        let mut x = det::cos(theta);
        for _ in 0..25 {
            let (p, dp) = legendre(n, x);
            x -= p / dp;
        }
        let (_, dp) = legendre(n, x);
        nodes[i] = x;
        weights[i] = 2.0 / (x.mul_add(-x, 1.0) * dp * dp);
    }
    // Ascending order (Newton from descending guesses gives descending
    // roots); fixed deterministic reorder.
    nodes.reverse();
    weights.reverse();
    (nodes, weights)
}

/// Lobatto hierarchical shape functions on [-1, 1] and derivatives:
/// N_0 = (1−x)/2, N_1 = (1+x)/2 (the vertex pair), and for k ≥ 2 the
/// integrated-Legendre bubbles
/// N_k = (L_k(x) − L_{k−2}(x)) / √(2(2k−1)), which vanish at ±1.
/// Returns (values, derivatives), length `order + 1`.
#[must_use]
pub fn lobatto_shapes(order: usize, x: f64) -> (Vec<f64>, Vec<f64>) {
    assert!(order >= 1, "continuous H1 basis needs order >= 1");
    let mut vals = Vec::with_capacity(order + 1);
    let mut ders = Vec::with_capacity(order + 1);
    vals.push(f64::midpoint(1.0, -x));
    ders.push(-0.5);
    vals.push(f64::midpoint(1.0, x));
    ders.push(0.5);
    for k in 2..=order {
        let (lk, dlk) = legendre(k, x);
        let (lk2, dlk2) = legendre(k - 2, x);
        let scale = 1.0 / det::sqrt(2.0 * (2.0 * k as f64 - 1.0));
        vals.push((lk - lk2) * scale);
        ders.push((dlk - dlk2) * scale);
    }
    (vals, ders)
}

/// 1D element mass and stiffness matrices for the order-r Lobatto
/// basis on a physical cell of width `h` (reference [-1, 1] scaled by
/// the affine map): M_e = (h/2)·∫N_i N_j, K_e = (2/h)·∫N'_i N'_j.
/// Quadrature with r+2 points (exact: integrands have degree ≤ 2r).
#[must_use]
pub fn element_matrices(order: usize, h: f64) -> (Vec<f64>, Vec<f64>) {
    let n = order + 1;
    let (qx, qw) = gauss_legendre(order + 2);
    let mut mass = vec![0.0f64; n * n];
    let mut stiff = vec![0.0f64; n * n];
    for (&x, &w) in qx.iter().zip(&qw) {
        let (vals, ders) = lobatto_shapes(order, x);
        for i in 0..n {
            for j in 0..n {
                mass[i * n + j] = (w * vals[i]).mul_add(vals[j], mass[i * n + j]);
                stiff[i * n + j] = (w * ders[i]).mul_add(ders[j], stiff[i * n + j]);
            }
        }
    }
    for v in &mut mass {
        *v *= h / 2.0;
    }
    for v in &mut stiff {
        *v *= 2.0 / h;
    }
    (mass, stiff)
}
