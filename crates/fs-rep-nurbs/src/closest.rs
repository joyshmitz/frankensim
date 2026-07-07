//! Certified closest-point queries: BEST-FIRST branch-and-bound over
//! rational Bézier segments (convex-hull property gives rigorous lower
//! bounds; positive weights survive de Casteljau splitting, so every
//! sub-segment's Cartesian control hull bounds it). Splits are LOCAL —
//! one segment per iteration — so work is linear in iterations, and the
//! split junction point is a free upper-bound sample. Boxes are
//! epsilon-inflated to absorb f64 rounding; the bracket `[lower, upper]`
//! is verified against dense-sampling oracles in conformance.

use crate::NurbsError;
use crate::curve::NurbsCurve;
use crate::surface::NurbsSurface;
use fs_math::det;

/// Rounding inflation applied to hull boxes (relative to box scale).
const HULL_EPS: f64 = 1e-9;

/// A certified distance bracket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CertifiedDistance {
    /// Rigorous lower bound.
    pub lower: f64,
    /// Achieved upper bound (distance to a found point).
    pub upper: f64,
    /// Parameter of the best found point (curve: t; surface: (u, v)).
    pub param: [f64; 2],
    /// Branch-and-bound splits spent.
    pub iterations: u32,
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    det::sqrt((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2))
}

fn cartesian(h: &[f64; 4]) -> [f64; 3] {
    [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
}

fn hull_lower_bound(q: [f64; 3], cps: &[[f64; 4]]) -> f64 {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for h in cps {
        let c = cartesian(h);
        for k in 0..3 {
            min[k] = min[k].min(c[k]);
            max[k] = max[k].max(c[k]);
        }
    }
    let mut d2 = 0.0f64;
    let mut scale = 1.0f64;
    for k in 0..3 {
        scale = scale.max(max[k].abs()).max(min[k].abs()).max(q[k].abs());
        let gap = if q[k] < min[k] {
            min[k] - q[k]
        } else if q[k] > max[k] {
            q[k] - max[k]
        } else {
            0.0
        };
        d2 += gap * gap;
    }
    (det::sqrt(d2) - HULL_EPS * scale).max(0.0)
}

/// De Casteljau split of a homogeneous Bézier control net at 1/2.
fn split_bezier(cps: &[[f64; 4]]) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
    let n = cps.len();
    let mut tri = cps.to_vec();
    let mut left = Vec::with_capacity(n);
    let mut right = vec![[0.0f64; 4]; n];
    left.push(tri[0]);
    right[n - 1] = tri[n - 1];
    for level in 1..n {
        for i in 0..n - level {
            let (head, tail) = tri.split_at_mut(i + 1);
            for (x, &y) in head[i].iter_mut().zip(&tail[0]) {
                *x = f64::midpoint(*x, y);
            }
        }
        left.push(tri[0]);
        right[n - 1 - level] = tri[n - 1 - level];
    }
    (left, right)
}

struct Seg {
    cpw: Vec<[f64; 4]>,
    t0: f64,
    t1: f64,
    lb: f64,
}

/// Certified closest point on a curve (best-first B&B + Newton polish).
///
/// # Errors
/// Propagates evaluation/domain errors.
pub fn closest_point_curve(
    curve: &NurbsCurve<f64, 3>,
    q: [f64; 3],
    tol: f64,
    max_splits: u32,
) -> Result<CertifiedDistance, NurbsError> {
    let bez = curve.to_bezier_form()?;
    let p = bez.knots.degree;
    let mut queue: Vec<Seg> = Vec::new();
    let mut upper = f64::INFINITY;
    let mut best_t = bez.knots.domain().0;
    for span in p..bez.knots.control_count() {
        let (t0, t1) = (bez.knots.knots[span], bez.knots.knots[span + 1]);
        if t1 <= t0 {
            continue;
        }
        let cpw = bez.cpw[span - p..=span].to_vec();
        for (h, tt) in [(&cpw[0], t0), (&cpw[p], t1)] {
            let d = dist3(cartesian(h), q);
            if d < upper {
                upper = d;
                best_t = tt;
            }
        }
        let lb = hull_lower_bound(q, &cpw);
        queue.push(Seg { cpw, t0, t1, lb });
    }
    let mut iterations = 0u32;
    while iterations < max_splits {
        // Best-first: pop the segment with the smallest lower bound.
        let Some(best_idx) = queue
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.lb.total_cmp(&b.1.lb))
            .map(|(i, _)| i)
        else {
            break;
        };
        if upper - queue[best_idx].lb <= tol {
            break; // bracket closed
        }
        let seg = queue.swap_remove(best_idx);
        let (l, r) = split_bezier(&seg.cpw);
        let tm = f64::midpoint(seg.t0, seg.t1);
        // The split junction is C(mid): a free upper-bound sample.
        let d = dist3(cartesian(&l[l.len() - 1]), q);
        if d < upper {
            upper = d;
            best_t = tm;
        }
        for (cpw, t0, t1) in [(l, seg.t0, tm), (r, tm, seg.t1)] {
            let lb = hull_lower_bound(q, &cpw);
            if lb < upper {
                queue.push(Seg { cpw, t0, t1, lb });
            }
        }
        iterations += 1;
    }
    let lower = queue.iter().map(|s| s.lb).fold(upper, f64::min);
    // Newton polish on g(t) = (C − q)·C' sharpens the upper bound.
    let (dlo, dhi) = curve.knots.domain();
    let mut t = best_t;
    for _ in 0..12 {
        let ders = curve.derivatives(t, 2)?;
        let diff = [ders[0][0] - q[0], ders[0][1] - q[1], ders[0][2] - q[2]];
        let g: f64 = (0..3).map(|k| diff[k] * ders[1][k]).sum();
        let gp: f64 = (0..3)
            .map(|k| ders[1][k] * ders[1][k] + diff[k] * ders[2][k])
            .sum();
        if gp.abs() < 1e-300 {
            break;
        }
        t = (t - g / gp).clamp(dlo, dhi);
    }
    let polished = dist3(curve.eval(t)?, q);
    let (upper, best_t) = if polished < upper {
        (polished, t)
    } else {
        (upper, best_t)
    };
    Ok(CertifiedDistance {
        lower: lower.min(upper),
        upper,
        param: [best_t, 0.0],
        iterations,
    })
}

/// A homogeneous Bézier control net (rows × cols).
type Net = Vec<Vec<[f64; 4]>>;

struct Patch {
    cpw: Net, // (pu+1) rows × (pv+1) cols
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    lb: f64,
}

fn patch_lb(q: [f64; 3], net: &[Vec<[f64; 4]>]) -> f64 {
    let flat: Vec<[f64; 4]> = net.iter().flatten().copied().collect();
    hull_lower_bound(q, &flat)
}

fn split_patch_u(net: &[Vec<[f64; 4]>]) -> (Net, Net) {
    // Split every v-column along u (rows are u direction).
    let rows = net.len();
    let cols = net[0].len();
    let mut left = vec![vec![[0.0f64; 4]; cols]; rows];
    let mut right = vec![vec![[0.0f64; 4]; cols]; rows];
    for j in 0..cols {
        let col: Vec<[f64; 4]> = (0..rows).map(|i| net[i][j]).collect();
        let (l, r) = split_bezier(&col);
        for i in 0..rows {
            left[i][j] = l[i];
            right[i][j] = r[i];
        }
    }
    (left, right)
}

fn split_patch_v(net: &[Vec<[f64; 4]>]) -> (Net, Net) {
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for row in net {
        let (l, r) = split_bezier(row);
        left.push(l);
        right.push(r);
    }
    (left, right)
}

/// Decompose a surface to Bézier patches via repeated knot insertion.
fn to_bezier_surface(surface: &NurbsSurface<f64>) -> Result<NurbsSurface<f64>, NurbsError> {
    let mut work = surface.clone();
    loop {
        let mut inserted = false;
        for dir_u in [true, false] {
            let (kv, p) = if dir_u {
                (&work.knots_u, work.knots_u.degree)
            } else {
                (&work.knots_v, work.knots_v.degree)
            };
            let (lo, hi) = kv.domain();
            let mut target = None;
            for &t in &kv.knots {
                if t > lo
                    && t < hi
                    && kv
                        .knots
                        .iter()
                        .filter(|&&u| u.to_bits() == t.to_bits())
                        .count()
                        < p
                {
                    target = Some(t);
                    break;
                }
            }
            if let Some(t) = target {
                work = if dir_u {
                    work.insert_knot_u(t)?
                } else {
                    work.insert_knot_v(t)?
                };
                inserted = true;
            }
        }
        if !inserted {
            return Ok(work);
        }
    }
}

/// Seed the patch queue from a Bézier-form surface.
fn seed_patches(
    work: &NurbsSurface<f64>,
    q: [f64; 3],
    queue: &mut Vec<Patch>,
    upper: &mut f64,
    best: &mut [f64; 2],
) {
    let (pu, pv) = (work.knots_u.degree, work.knots_v.degree);
    for su in pu..work.knots_u.control_count() {
        let (u0, u1) = (work.knots_u.knots[su], work.knots_u.knots[su + 1]);
        if u1 <= u0 {
            continue;
        }
        for sv in pv..work.knots_v.control_count() {
            let (v0, v1) = (work.knots_v.knots[sv], work.knots_v.knots[sv + 1]);
            if v1 <= v0 {
                continue;
            }
            let net: Net = work.cpw[su - pu..=su]
                .iter()
                .map(|row| row[sv - pv..=sv].to_vec())
                .collect();
            let d = dist3(cartesian(&net[0][0]), q);
            if d < *upper {
                *upper = d;
                *best = [u0, v0];
            }
            let lb = patch_lb(q, &net);
            queue.push(Patch {
                cpw: net,
                u0,
                u1,
                v0,
                v1,
                lb,
            });
        }
    }
}

/// Certified closest point on a surface (best-first B&B over Bézier
/// patches with de Casteljau splits along the longer parametric side).
///
/// # Errors
/// Propagates evaluation/domain errors.
pub fn closest_point_surface(
    surface: &NurbsSurface<f64>,
    q: [f64; 3],
    tol: f64,
    max_splits: u32,
) -> Result<CertifiedDistance, NurbsError> {
    let work = to_bezier_surface(surface)?;
    let mut queue: Vec<Patch> = Vec::new();
    let mut upper = f64::INFINITY;
    let mut best = [work.knots_u.domain().0, work.knots_v.domain().0];
    seed_patches(&work, q, &mut queue, &mut upper, &mut best);
    let mut iterations = 0u32;
    while iterations < max_splits {
        let Some(best_idx) = queue
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.lb.total_cmp(&b.1.lb))
            .map(|(i, _)| i)
        else {
            break;
        };
        if upper - queue[best_idx].lb <= tol {
            break;
        }
        let patch = queue.swap_remove(best_idx);
        let split_u = patch.u1 - patch.u0 >= patch.v1 - patch.v0;
        let (l, r) = if split_u {
            split_patch_u(&patch.cpw)
        } else {
            split_patch_v(&patch.cpw)
        };
        let halves = if split_u {
            let um = f64::midpoint(patch.u0, patch.u1);
            [
                (l, patch.u0, um, patch.v0, patch.v1),
                (r, um, patch.u1, patch.v0, patch.v1),
            ]
        } else {
            let vm = f64::midpoint(patch.v0, patch.v1);
            [
                (l, patch.u0, patch.u1, patch.v0, vm),
                (r, patch.u0, patch.u1, vm, patch.v1),
            ]
        };
        for (net, u0, u1, v0, v1) in halves {
            // Corner sample improves the upper bound cheaply.
            let d = dist3(cartesian(&net[0][0]), q);
            if d < upper {
                upper = d;
                best = [u0, v0];
            }
            let lb = patch_lb(q, &net);
            if lb < upper {
                queue.push(Patch {
                    cpw: net,
                    u0,
                    u1,
                    v0,
                    v1,
                    lb,
                });
            }
        }
        iterations += 1;
    }
    let lower = queue.iter().map(|p| p.lb).fold(upper, f64::min);
    // Sample the best patch center for a final upper-bound improvement.
    let center = surface.eval(
        f64::midpoint(best[0], best[0])
            .clamp(surface.knots_u.domain().0, surface.knots_u.domain().1),
        f64::midpoint(best[1], best[1])
            .clamp(surface.knots_v.domain().0, surface.knots_v.domain().1),
    )?;
    let d = dist3(center, q);
    let upper = upper.min(d);
    Ok(CertifiedDistance {
        lower: lower.min(upper),
        upper,
        param: best,
        iterations,
    })
}
