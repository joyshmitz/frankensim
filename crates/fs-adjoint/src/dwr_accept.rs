//! DWR GOAL-ORIENTED ACCEPT TEST (addendum Proposal 9, bead lmp4.4;
//! [F] — behind the `dwr-accept` feature): dual-weighted-residual
//! estimates target the QUERY's actual quantity of interest, which
//! sharpens the accept test enormously — but DWR constants are NOT
//! guaranteed, so a DWR-only accept carries ESTIMATED color. Promotion
//! to VERIFIED is mechanical and auditable: the accept must be
//! BRACKETED by a constant-free bound built from fs-verify's
//! equilibrated-flux enclosures — here the Cauchy–Schwarz product
//! `|J(u) − J(u_h)| = |a(e_u, e_z)| ≤ ‖e_u‖_E · ‖e_z‖_E`, both factors
//! certified. Without the colors this mixture would silently launder
//! estimates into certificates; with them, a query is discharged
//! cheaply (estimated) or rigorously (verified) and the report never
//! confuses the two.

use fs_evidence::Color;
use fs_verify::estimator::VerifierReport;
use fs_verify::fem1d::{MmsProblem, gauss5};
use fs_verify::interval::up;

/// A QoI query: what the caller actually asked.
#[derive(Debug, Clone, PartialEq)]
pub struct DwrQuery {
    /// The quantity of interest (provenance label).
    pub qoi: String,
    /// The tolerance the answer must meet.
    pub tolerance: f64,
}

/// A constant-free bracket for the QoI error.
#[derive(Debug, Clone, PartialEq)]
pub struct Bracket {
    /// The guaranteed upper bound on `|J(u) − J(u_h)|`.
    pub bound: f64,
    /// True only when the bound derives from certified enclosures
    /// (fail closed: an unbounded factor forfeits the guarantee).
    pub guaranteed: bool,
    /// Where the bound came from (audit trail).
    pub source: String,
}

impl Bracket {
    /// The Cauchy–Schwarz bracket from two equilibrated energy-norm
    /// enclosures: `|a(e_u, e_z)| ≤ ‖e_u‖_E · ‖e_z‖_E`, outward-rounded.
    #[must_use]
    pub fn cauchy_schwarz(primal: &VerifierReport, dual: &VerifierReport) -> Bracket {
        let guaranteed = !primal.bound.is_unbounded() && !dual.bound.is_unbounded();
        Bracket {
            bound: up(primal.bound.hi * dual.bound.hi),
            guaranteed,
            source: format!(
                "cauchy-schwarz(equilibrated primal {:.3e} x equilibrated dual {:.3e})",
                primal.bound.hi, dual.bound.hi
            ),
        }
    }
}

/// The colored accept outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptOutcome {
    /// Was the query discharged?
    pub accepted: bool,
    /// The color the answer carries.
    pub color: Color,
    /// True when the DWR estimate exceeded a guaranteed bracket — the
    /// estimator lied about its own accuracy (a falsifier-grade
    /// inconsistency, reported, never hidden).
    pub estimator_inconsistent: bool,
    /// The audit trail.
    pub audit: String,
}

/// The accept test. Color logic (mechanical, auditable):
/// - no acceptance path → rejected (estimated color on the estimate);
/// - DWR-only accept (`|η| ≤ tol`, no valid bracket) → ESTIMATED;
/// - a GUARANTEED bracket with `bound ≤ tol` → VERIFIED `[0, bound]`
///   (the bracket alone carries the certificate; a DWR estimate above
///   the bracket flags the estimator as inconsistent).
#[must_use]
pub fn accept(query: &DwrQuery, dwr_abs: f64, bracket: Option<&Bracket>) -> AcceptOutcome {
    let valid_bracket = bracket.filter(|b| b.guaranteed && b.bound <= query.tolerance);
    if let Some(b) = valid_bracket {
        let inconsistent = dwr_abs > b.bound;
        return AcceptOutcome {
            accepted: true,
            color: Color::Verified {
                lo: 0.0,
                hi: b.bound,
            },
            estimator_inconsistent: inconsistent,
            audit: format!(
                "verified via {} (tol {:.3e}); dwr estimate {:.3e}{}",
                b.source,
                query.tolerance,
                dwr_abs,
                if inconsistent {
                    " — INCONSISTENT with the bracket (estimator bug report due)"
                } else {
                    ""
                }
            ),
        };
    }
    if dwr_abs <= query.tolerance {
        return AcceptOutcome {
            accepted: true,
            color: Color::Estimated {
                estimator: format!("dwr({}) — constants not guaranteed", query.qoi),
                dispersion: dwr_abs,
            },
            estimator_inconsistent: false,
            audit: format!(
                "estimated-only accept: dwr {:.3e} <= tol {:.3e}, no guaranteed bracket",
                dwr_abs, query.tolerance
            ),
        };
    }
    AcceptOutcome {
        accepted: false,
        color: Color::Estimated {
            estimator: format!("dwr({})", query.qoi),
            dispersion: dwr_abs,
        },
        estimator_inconsistent: false,
        audit: format!(
            "rejected: dwr {:.3e} > tol {:.3e} and no bracket discharges it",
            dwr_abs, query.tolerance
        ),
    }
}

/// The 1-D reference DWR estimator for integral QoIs
/// `J(u) = ∫_{w_lo}^{w_hi} u dx` over an fs-verify problem: the dual
/// `−z″ = 1_{[w_lo, w_hi]}` solves by P1 FEM on the ONCE-REFINED mesh
/// (the enriched dual), and the estimate is the dual-weighted residual
/// `η = r(z_f − I_h z_f)` with per-COARSE-element indicators.
#[derive(Debug, Clone)]
pub struct DwrOutput {
    /// `J(u_h)`.
    pub j_primal: f64,
    /// The signed estimate `η ≈ J(u) − J(u_h)`.
    pub eta: f64,
    /// Per-coarse-element |indicator| (refinement guidance).
    pub indicators: Vec<f64>,
}

fn thomas_solve(sub: &[f64], diag: &[f64], sup: &[f64], rhs: &mut [f64]) {
    let n = rhs.len();
    let mut c = vec![0.0f64; n];
    let mut d = diag[0];
    c[0] = sup[0] / d;
    rhs[0] /= d;
    for i in 1..n {
        d = diag[i] - sub[i] * c[i - 1];
        if i < n - 1 {
            c[i] = sup[i] / d;
        }
        rhs[i] = (rhs[i] - sub[i] * rhs[i - 1]) / d;
    }
    for i in (0..n - 1).rev() {
        rhs[i] -= c[i] * rhs[i + 1];
    }
}

/// P1 FEM solve of `−z″ = w` (zero Dirichlet BC) on `mesh`, with
/// `w = 1` on `[w_lo, w_hi]` — deterministic Thomas solve.
fn dual_solve(mesh: &[f64], w_lo: f64, w_hi: f64) -> Vec<f64> {
    let n = mesh.len();
    let free = n - 2;
    let mut sub = vec![0.0f64; free];
    let mut diag = vec![0.0f64; free];
    let mut sup = vec![0.0f64; free];
    let mut rhs = vec![0.0f64; free];
    for e in 0..n - 1 {
        let h = mesh[e + 1] - mesh[e];
        let k = 1.0 / h;
        for (a, b, v) in [(e, e, k), (e + 1, e + 1, k), (e, e + 1, -k), (e + 1, e, -k)] {
            if a >= 1 && a <= free && b >= 1 && b <= free {
                let (i, j) = (a - 1, b - 1);
                if i == j {
                    diag[i] += v;
                } else if j == i + 1 {
                    sup[i] += v;
                } else {
                    sub[i] += v;
                }
            }
        }
        // Load: ∫ w φ_a over the element (Gauss).
        for (gx, gw) in gauss5(mesh[e], mesh[e + 1]) {
            let w = f64::from(u8::from(gx >= w_lo && gx <= w_hi));
            let xi = (gx - mesh[e]) / h;
            for (node, shape) in [(e, 1.0 - xi), (e + 1, xi)] {
                if node >= 1 && node <= free {
                    rhs[node - 1] += gw * w * shape;
                }
            }
        }
    }
    thomas_solve(&sub, &diag, &sup, &mut rhs);
    let mut z = vec![0.0f64; n];
    z[1..=free].copy_from_slice(&rhs);
    z
}

fn refine(mesh: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(mesh.len() * 2 - 1);
    for e in 0..mesh.len() - 1 {
        out.push(mesh[e]);
        out.push(f64::midpoint(mesh[e], mesh[e + 1]));
    }
    out.push(*mesh.last().expect("nonempty"));
    out
}

/// Run the 1-D goal-oriented estimate (see [`DwrOutput`]).
#[must_use]
pub fn dwr_integral_qoi(
    problem: &MmsProblem,
    candidate: &[f64],
    w_lo: f64,
    w_hi: f64,
) -> DwrOutput {
    let mesh = &problem.mesh;
    let f = problem.u.derive().derive().neg();
    // J(u_h): the P1 interpolant integrated over the window.
    let mut j_primal = 0.0f64;
    for e in 0..mesh.len() - 1 {
        let h = mesh[e + 1] - mesh[e];
        for (gx, gw) in gauss5(mesh[e], mesh[e + 1]) {
            if gx >= w_lo && gx <= w_hi {
                let xi = (gx - mesh[e]) / h;
                j_primal += gw * ((1.0 - xi) * candidate[e] + xi * candidate[e + 1]);
            }
        }
    }
    // Enriched dual on the refined mesh.
    let fine = refine(mesh);
    let z = dual_solve(&fine, w_lo, w_hi);
    // Coarse-node interpolant of z, subtracted (Galerkin orthogonality
    // makes the coarse part vanish; the fine remainder drives η).
    let mut eta = 0.0f64;
    let mut indicators = vec![0.0f64; mesh.len() - 1];
    for e in 0..mesh.len() - 1 {
        let (x0, x1) = (mesh[e], mesh[e + 1]);
        let slope = (candidate[e + 1] - candidate[e]) / (x1 - x0);
        let (z0, z1) = (z[2 * e], z[2 * e + 2]);
        let mut local = 0.0f64;
        // Two fine halves of the coarse element.
        for half in 0..2usize {
            let (fa, fb) = (fine[2 * e + half], fine[2 * e + half + 1]);
            let (za, zb) = (z[2 * e + half], z[2 * e + half + 1]);
            let zslope = (zb - za) / (fb - fa);
            // Coarse interpolant of z on this fine piece.
            let islope = (z1 - z0) / (x1 - x0);
            for (gx, gw) in gauss5(fa, fb) {
                let xi_f = (gx - fa) / (fb - fa);
                let zf = (1.0 - xi_f) * za + xi_f * zb;
                let zi = z0 + (gx - x0) * islope;
                // r(v) = ∫ f v − ∫ u_h′ v′ with v = z_f − I_h z_f.
                local += gw * (f.eval(gx) * (zf - zi) - slope * (zslope - islope));
            }
        }
        eta += local;
        indicators[e] = local.abs();
    }
    DwrOutput {
        j_primal,
        eta,
        indicators,
    }
}
