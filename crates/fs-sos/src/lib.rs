//! fs-sos — proof-carrying optimization (sum-of-squares certificates). Layer: L4.
//!
//! A lower bound proven by SAMPLING can be wrong (it can miss the true minimum).
//! A SUM-OF-SQUARES certificate cannot: if `p(x) − γ = Σ qᵢ(x)²` as an identity
//! of polynomials, then `p(x) ≥ γ` for EVERY `x`, because a square is
//! nonnegative. This crate makes that certificate executable — [`SosCertificate`]
//! is verified by matching polynomial coefficients, so a claimed bound ABOVE the
//! true minimum simply fails to verify. Certificates over vibes; zero false
//! certificates.
//!
//! Included: univariate [`Poly`] arithmetic; [`certify_quadratic`] (an exact
//! global optimum + its SOS certificate); [`is_psd`] (the SDP-feasibility core,
//! by an in-house Jacobi eigensolver); and [`lyapunov_certifies_stability`] (an
//! SOS/quadratic Lyapunov stability certificate for a linear system).
//!
//! Deterministic; no dependencies.

/// A univariate polynomial, coefficients ascending (`coeffs[i]` multiplies `xⁱ`).
#[derive(Debug, Clone, PartialEq)]
pub struct Poly {
    coeffs: Vec<f64>,
}

impl Poly {
    /// A polynomial from ascending coefficients (trailing near-zeros trimmed).
    #[must_use]
    pub fn new(coeffs: Vec<f64>) -> Poly {
        let mut p = Poly { coeffs };
        p.trim();
        p
    }

    /// A constant polynomial.
    #[must_use]
    pub fn constant(c: f64) -> Poly {
        Poly::new(vec![c])
    }

    fn trim(&mut self) {
        while self.coeffs.len() > 1 && self.coeffs.last().copied().unwrap_or(0.0).abs() < 1e-15 {
            self.coeffs.pop();
        }
    }

    /// The coefficients (ascending).
    #[must_use]
    pub fn coeffs(&self) -> &[f64] {
        &self.coeffs
    }

    /// The degree.
    #[must_use]
    pub fn degree(&self) -> usize {
        self.coeffs.len().saturating_sub(1)
    }

    /// Evaluate at `x` (Horner).
    #[must_use]
    pub fn eval(&self, x: f64) -> f64 {
        self.coeffs.iter().rev().fold(0.0, |acc, &c| acc * x + c)
    }

    /// Sum.
    #[must_use]
    pub fn add(&self, other: &Poly) -> Poly {
        let n = self.coeffs.len().max(other.coeffs.len());
        let coeffs = (0..n)
            .map(|i| {
                self.coeffs.get(i).copied().unwrap_or(0.0)
                    + other.coeffs.get(i).copied().unwrap_or(0.0)
            })
            .collect();
        Poly::new(coeffs)
    }

    /// Difference.
    #[must_use]
    pub fn sub(&self, other: &Poly) -> Poly {
        let n = self.coeffs.len().max(other.coeffs.len());
        let coeffs = (0..n)
            .map(|i| {
                self.coeffs.get(i).copied().unwrap_or(0.0)
                    - other.coeffs.get(i).copied().unwrap_or(0.0)
            })
            .collect();
        Poly::new(coeffs)
    }

    /// Product.
    #[must_use]
    pub fn mul(&self, other: &Poly) -> Poly {
        if self.coeffs.is_empty() || other.coeffs.is_empty() {
            return Poly::constant(0.0);
        }
        let mut coeffs = vec![0.0; self.coeffs.len() + other.coeffs.len() - 1];
        for (i, &a) in self.coeffs.iter().enumerate() {
            for (j, &b) in other.coeffs.iter().enumerate() {
                coeffs[i + j] += a * b;
            }
        }
        Poly::new(coeffs)
    }

    /// The largest absolute coefficient (a supremum-style norm; `0` for the zero
    /// polynomial).
    #[must_use]
    pub fn max_abs_coeff(&self) -> f64 {
        self.coeffs.iter().map(|c| c.abs()).fold(0.0, f64::max)
    }
}

/// The square `q·q`.
#[must_use]
pub fn square(q: &Poly) -> Poly {
    q.mul(q)
}

/// A sum-of-squares certificate: the claim `p(x) − lower_bound = Σ squaresᵢ(x)²`,
/// which (if it holds as a polynomial identity) PROVES `p(x) ≥ lower_bound`.
#[derive(Debug, Clone, PartialEq)]
pub struct SosCertificate {
    /// The polynomials whose squares sum to `p − lower_bound`.
    pub squares: Vec<Poly>,
    /// The certified lower bound.
    pub lower_bound: f64,
}

impl SosCertificate {
    /// The certificate residual: the largest absolute coefficient of
    /// `p − lower_bound − Σ squaresᵢ²` (nominally `0`).
    #[must_use]
    pub fn residual(&self, p: &Poly) -> f64 {
        let mut acc = p.sub(&Poly::constant(self.lower_bound));
        for q in &self.squares {
            acc = acc.sub(&square(q));
        }
        acc.max_abs_coeff()
    }

    /// Is the certificate valid (the identity holds within `tol`)?
    #[must_use]
    pub fn verify(&self, p: &Poly, tol: f64) -> bool {
        self.residual(p) <= tol
    }

    /// The PROVEN lower bound if the certificate verifies, else `None` — so a
    /// bound above the true minimum is never returned.
    #[must_use]
    pub fn certified_bound(&self, p: &Poly, tol: f64) -> Option<f64> {
        self.verify(p, tol).then_some(self.lower_bound)
    }
}

/// The exact global minimum of `a·x² + b·x + c` (`a > 0`) with its SOS
/// certificate: `p(x) − (c − b²/4a) = (√a·x + b/2√a)²`.
///
/// Returns `None` when `a <= 0` (not bounded below by a square).
#[must_use]
pub fn certify_quadratic(a: f64, b: f64, c: f64) -> Option<SosCertificate> {
    if a <= 0.0 {
        return None;
    }
    let lower_bound = c - b * b / (4.0 * a);
    let root_a = a.sqrt();
    // q(x) = √a·x + b/(2√a).
    let q = Poly::new(vec![b / (2.0 * root_a), root_a]);
    Some(SosCertificate {
        squares: vec![q],
        lower_bound,
    })
}

/// Is the symmetric matrix positive semidefinite (min eigenvalue `>= −tol`)?
/// The feasibility core of the SDP the full Lasserre hierarchy solves.
#[must_use]
pub fn is_psd(matrix: &[Vec<f64>], tol: f64) -> bool {
    min_eigenvalue(matrix) >= -tol
}

/// Does the quadratic Lyapunov function `V(x) = xᵀPx` certify asymptotic
/// stability of the 2-D linear system `ẋ = Ax`? True iff `P ≻ 0` and
/// `−(AᵀP + PA) ≻ 0` (Lyapunov's theorem) — a sound SOS/quadratic stability
/// certificate. Finding such a `P` is the SDP (staged); this VERIFIES a candidate.
#[must_use]
pub fn lyapunov_certifies_stability(a: [[f64; 2]; 2], p: [[f64; 2]; 2]) -> bool {
    let pm = vec![vec![p[0][0], p[0][1]], vec![p[1][0], p[1][1]]];
    // Aᵀ P + P A.
    let at = [[a[0][0], a[1][0]], [a[0][1], a[1][1]]];
    let atp = matmul2(at, p);
    let pa = matmul2(p, a);
    let q = [
        [-(atp[0][0] + pa[0][0]), -(atp[0][1] + pa[0][1])],
        [-(atp[1][0] + pa[1][0]), -(atp[1][1] + pa[1][1])],
    ];
    let qm = vec![vec![q[0][0], q[0][1]], vec![q[1][0], q[1][1]]];
    // strict definiteness: min eigenvalue > 0 (small positive threshold).
    min_eigenvalue(&pm) > 1e-9 && min_eigenvalue(&qm) > 1e-9
}

fn matmul2(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [
        [
            a[0][0] * b[0][0] + a[0][1] * b[1][0],
            a[0][0] * b[0][1] + a[0][1] * b[1][1],
        ],
        [
            a[1][0] * b[0][0] + a[1][1] * b[1][0],
            a[1][0] * b[0][1] + a[1][1] * b[1][1],
        ],
    ]
}

/// The smallest eigenvalue of a symmetric matrix, by cyclic Jacobi rotations.
// A dense symmetric eigen-kernel: `m[i][j]` is inherently 2D-indexed, so the
// index loops are the correct, readable form.
#[allow(clippy::needless_range_loop)]
fn min_eigenvalue(a: &[Vec<f64>]) -> f64 {
    let n = a.len();
    if n == 0 {
        return 0.0;
    }
    let mut m: Vec<Vec<f64>> = a.to_vec();
    for _ in 0..100 {
        let mut off = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                off += m[i][j] * m[i][j];
            }
        }
        if off <= 1e-28 {
            break;
        }
        for p in 0..n {
            for q in (p + 1)..n {
                if m[p][q].abs() <= 1e-20 {
                    continue;
                }
                let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (mkp, mkq) = (m[k][p], m[k][q]);
                    m[k][p] = c * mkp - s * mkq;
                    m[k][q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = c * mpk - s * mqk;
                    m[q][k] = s * mpk + c * mqk;
                }
            }
        }
    }
    (0..n).map(|i| m[i][i]).fold(f64::INFINITY, f64::min)
}
