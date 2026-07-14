//! Measured closest-point bracket estimates: BEST-FIRST branch-and-bound over
//! rational Bézier segments (in exact arithmetic the convex-hull property
//! supplies lower bounds; positive weights survive de Casteljau splitting).
//! The current Cartesian hull is evaluated in ordinary f64. Splits are LOCAL —
//! one segment per iteration — so heap selection costs O(log S) per split,
//! and the split junction point is a free upper-bound sample. Boxes are
//! heuristically expanded by one ULP against f64 rounding. Dense-oracle
//! conformance is useful evidence, but ordinary Cartesian division, distance
//! arithmetic, and evaluation are not outward-rounded; `[lower, upper]` is not
//! a rigorous enclosure until the interval/Taylor upgrade lands.

use crate::NurbsError;
use crate::curve::NurbsCurve;
use crate::surface::NurbsSurface;
use fs_math::{det, next_down, next_up};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Defensive ceiling for the legacy allocation-bearing subdivision path.
/// Caller-owned work/memory budgets and cancellation belong to the successor
/// certifying API; this cap only prevents an unbounded `u32` request here.
pub(crate) const CLOSEST_MAX_SPLITS: u32 = 1_048_576;

/// Deterministic minimum-priority entry. `BinaryHeap` is a max-heap, so the
/// comparisons are reversed: lower bound first, then lower logical ID. IDs
/// are unique among resident entries and are reused only when the same popped
/// unsplittable leaf is reinserted.
struct MinEntry<T> {
    key: f64,
    logical_id: u64,
    value: T,
}

impl<T> PartialEq for MinEntry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key.to_bits() == other.key.to_bits() && self.logical_id == other.logical_id
    }
}

impl<T> Eq for MinEntry<T> {}

impl<T> PartialOrd for MinEntry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for MinEntry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .key
            .total_cmp(&self.key)
            .then_with(|| other.logical_id.cmp(&self.logical_id))
    }
}

/// A measured distance-bracket estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceBracketEstimate {
    /// Convex-hull lower estimate with heuristic f64 inflation.
    pub lower: f64,
    /// Achieved evaluated-point distance estimate.
    pub upper: f64,
    /// Parameter of the best found point (curve: t; surface: (u, v)).
    pub param: [f64; 2],
    /// Branch-and-bound splits spent.
    pub iterations: u32,
}

pub(crate) fn norm3(value: [f64; 3]) -> f64 {
    let scale = value
        .iter()
        .fold(0.0f64, |largest, component| largest.max(component.abs()));
    if scale == 0.0 {
        return 0.0;
    }
    if !scale.is_finite() {
        return f64::INFINITY;
    }
    let normalized_square_sum: f64 = value
        .iter()
        .map(|component| (component / scale).powi(2))
        .sum();
    scale * det::sqrt(normalized_square_sum)
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm3([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

fn cartesian(h: &[f64; 4]) -> [f64; 3] {
    [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
}

fn validate_closest_request(q: [f64; 3], tol: f64, max_splits: u32) -> Result<(), NurbsError> {
    if q.iter().any(|coordinate| !coordinate.is_finite()) {
        return Err(NurbsError::Domain {
            what: "closest-point query coordinates must be finite".to_string(),
        });
    }
    if !tol.is_finite() || tol < 0.0 {
        return Err(NurbsError::Domain {
            what: "closest-point tolerance must be finite and non-negative".to_string(),
        });
    }
    if max_splits > CLOSEST_MAX_SPLITS {
        return Err(NurbsError::Domain {
            what: format!(
                "closest-point split request {max_splits} exceeds defensive ceiling {CLOSEST_MAX_SPLITS}"
            ),
        });
    }
    Ok(())
}

fn hull_lower_bound<'a>(q: [f64; 3], cps: impl Iterator<Item = &'a [f64; 4]>) -> f64 {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for h in cps {
        let c = cartesian(h);
        for k in 0..3 {
            min[k] = min[k].min(c[k]);
            max[k] = max[k].max(c[k]);
        }
    }
    let mut gaps = [0.0; 3];
    for k in 0..3 {
        // One-ULP expansion removes the old absolute-coordinate epsilon whose
        // width grew catastrophically under translation. This remains measured
        // rather than certified because upstream homogeneous arithmetic is not
        // interval-tracked.
        min[k] = next_down(min[k]);
        max[k] = next_up(max[k]);
        gaps[k] = if q[k] < min[k] {
            min[k] - q[k]
        } else if q[k] > max[k] {
            q[k] - max[k]
        } else {
            0.0
        };
    }
    norm3(gaps)
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
}

/// Measured closest-point bracket estimate on a curve (best-first B&B +
/// Newton polish).
///
/// # Errors
/// Propagates evaluation/domain errors.
pub fn closest_point_curve(
    curve: &NurbsCurve<f64, 3>,
    q: [f64; 3],
    tol: f64,
    max_splits: u32,
) -> Result<DistanceBracketEstimate, NurbsError> {
    validate_closest_request(q, tol, max_splits)?;
    let bez = curve.to_bezier_form()?;
    let p = bez.knots.degree;
    let mut queue: BinaryHeap<MinEntry<Seg>> = BinaryHeap::new();
    let mut next_logical_id = 0u64;
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
        let lb = hull_lower_bound(q, cpw.iter());
        queue.push(MinEntry {
            key: lb,
            logical_id: next_logical_id,
            value: Seg { cpw, t0, t1 },
        });
        next_logical_id += 1;
    }
    if queue.is_empty() || !upper.is_finite() || queue.iter().any(|entry| !entry.key.is_finite())
    {
        return Err(NurbsError::Domain {
            what: "closest-curve initial distance bounds are not finite".to_string(),
        });
    }
    let mut iterations = 0u32;
    while iterations < max_splits {
        let Some(entry) = queue.peek() else {
            break;
        };
        if upper - entry.key <= tol {
            break; // bracket closed
        }
        let Some(entry) = queue.pop() else {
            break;
        };
        let seg = entry.value;
        let tm = f64::midpoint(seg.t0, seg.t1);
        if tm == seg.t0 || tm == seg.t1 {
            // De Casteljau would still create a geometric half-split even
            // though the recorded parameter interval cannot shrink. Retain
            // the leaf so its lower estimate remains part of the bracket.
            queue.push(MinEntry {
                key: entry.key,
                logical_id: entry.logical_id,
                value: seg,
            });
            break;
        }
        let (l, r) = split_bezier(&seg.cpw);
        // The split junction is C(mid): a free upper-bound sample.
        let d = dist3(cartesian(&l[l.len() - 1]), q);
        if d < upper {
            upper = d;
            best_t = tm;
        }
        for (cpw, t0, t1) in [(l, seg.t0, tm), (r, tm, seg.t1)] {
            let lb = hull_lower_bound(q, cpw.iter());
            if !lb.is_finite() {
                return Err(NurbsError::Domain {
                    what: "closest-curve child distance bound is not finite".to_string(),
                });
            }
            if lb < upper {
                queue.push(MinEntry {
                    key: lb,
                    logical_id: next_logical_id,
                    value: Seg { cpw, t0, t1 },
                });
                next_logical_id += 1;
            }
        }
        iterations += 1;
    }
    let lower = queue.peek().map_or(upper, |entry| entry.key);
    // Newton polish on g(t) = (C − q)·C' sharpens the upper bound.
    let (dlo, dhi) = curve.knots.domain();
    let mut t = best_t;
    for _ in 0..12 {
        let ders = curve.derivatives(t, 2)?;
        if ders.len() < 2 {
            break;
        }
        let second = ders.get(2).copied().unwrap_or([0.0; 3]);
        let diff = [ders[0][0] - q[0], ders[0][1] - q[1], ders[0][2] - q[2]];
        let g: f64 = (0..3).map(|k| diff[k] * ders[1][k]).sum();
        let gp: f64 = (0..3)
            .map(|k| ders[1][k] * ders[1][k] + diff[k] * second[k])
            .sum();
        if !g.is_finite() || !gp.is_finite() || gp.abs() < 1e-300 {
            break;
        }
        let next = (t - g / gp).clamp(dlo, dhi);
        if !next.is_finite() || next == t {
            break;
        }
        t = next;
    }
    let polished = dist3(curve.eval(t)?, q);
    let (upper, best_t) = if polished < upper {
        (polished, t)
    } else {
        (upper, best_t)
    };
    Ok(DistanceBracketEstimate {
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
    depth_u: u32,
    depth_v: u32,
}

fn patch_lb(q: [f64; 3], net: &[Vec<[f64; 4]>]) -> f64 {
    hull_lower_bound(q, net.iter().flatten())
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
    queue: &mut BinaryHeap<MinEntry<Patch>>,
    next_logical_id: &mut u64,
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
            queue.push(MinEntry {
                key: lb,
                logical_id: *next_logical_id,
                value: Patch {
                    cpw: net,
                    u0,
                    u1,
                    v0,
                    v1,
                    depth_u: 0,
                    depth_v: 0,
                },
            });
            *next_logical_id += 1;
        }
    }
}

/// Measured closest point on a surface (best-first B&B over Bézier
/// patches with de Casteljau splits that balance normalized subdivision depth).
///
/// # Errors
/// Propagates evaluation/domain errors.
pub fn closest_point_surface(
    surface: &NurbsSurface<f64>,
    q: [f64; 3],
    tol: f64,
    max_splits: u32,
) -> Result<DistanceBracketEstimate, NurbsError> {
    validate_closest_request(q, tol, max_splits)?;
    let work = to_bezier_surface(surface)?;
    let mut queue: BinaryHeap<MinEntry<Patch>> = BinaryHeap::new();
    let mut next_logical_id = 0u64;
    let mut upper = f64::INFINITY;
    let mut best = [work.knots_u.domain().0, work.knots_v.domain().0];
    seed_patches(
        &work,
        q,
        &mut queue,
        &mut next_logical_id,
        &mut upper,
        &mut best,
    );
    if queue.is_empty() || !upper.is_finite() || queue.iter().any(|entry| !entry.key.is_finite()) {
        return Err(NurbsError::Domain {
            what: "closest-surface initial distance bounds are not finite".to_string(),
        });
    }
    let mut iterations = 0u32;
    while iterations < max_splits {
        let Some(entry) = queue.peek() else {
            break;
        };
        if upper - entry.key <= tol {
            break;
        }
        let Some(entry) = queue.pop() else {
            break;
        };
        let patch = entry.value;
        let midpoint_u = f64::midpoint(patch.u0, patch.u1);
        let midpoint_v = f64::midpoint(patch.v0, patch.v1);
        let can_split_u = midpoint_u != patch.u0 && midpoint_u != patch.u1;
        let can_split_v = midpoint_v != patch.v0 && midpoint_v != patch.v1;
        let preferred_u = patch.depth_u <= patch.depth_v;
        let split_u = match (can_split_u, can_split_v) {
            (true, true) => preferred_u,
            (true, false) => true,
            (false, true) => false,
            (false, false) => {
                queue.push(MinEntry {
                    key: entry.key,
                    logical_id: entry.logical_id,
                    value: patch,
                });
                break;
            }
        };
        let midpoint = if split_u { midpoint_u } else { midpoint_v };
        if !midpoint.is_finite() {
            queue.push(MinEntry {
                key: entry.key,
                logical_id: entry.logical_id,
                value: patch,
            });
            break;
        }
        let (l, r) = if split_u {
            split_patch_u(&patch.cpw)
        } else {
            split_patch_v(&patch.cpw)
        };
        let halves = if split_u {
            [
                (l, patch.u0, midpoint, patch.v0, patch.v1),
                (r, midpoint, patch.u1, patch.v0, patch.v1),
            ]
        } else {
            [
                (l, patch.u0, patch.u1, patch.v0, midpoint),
                (r, patch.u0, patch.u1, midpoint, patch.v1),
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
            if !lb.is_finite() {
                return Err(NurbsError::Domain {
                    what: "closest-surface child distance bound is not finite".to_string(),
                });
            }
            if lb < upper {
                queue.push(MinEntry {
                    key: lb,
                    logical_id: next_logical_id,
                    value: Patch {
                        cpw: net,
                        u0,
                        u1,
                        v0,
                        v1,
                        depth_u: patch.depth_u + u32::from(split_u),
                        depth_v: patch.depth_v + u32::from(!split_u),
                    },
                });
                next_logical_id += 1;
            }
        }
        iterations += 1;
    }
    let lower = queue.peek().map_or(upper, |entry| entry.key);
    // Sample the current best-lower-estimate patch center for a final
    // evaluated-point improvement. The former midpoint(best,best) expression
    // merely re-evaluated the already retained corner and never sampled a
    // patch center; it also failed to update `param` when the sample improved.
    if let Some(entry) = queue.peek() {
        let patch = &entry.value;
        let candidate = [
            f64::midpoint(patch.u0, patch.u1),
            f64::midpoint(patch.v0, patch.v1),
        ];
        let point = surface.eval(candidate[0], candidate[1])?;
        let distance = dist3(point, q);
        if distance < upper {
            upper = distance;
            best = candidate;
        }
    }
    Ok(DistanceBracketEstimate {
        lower: lower.min(upper),
        upper,
        param: best,
        iterations,
    })
}

#[cfg(test)]
mod tests {
    use super::{BinaryHeap, MinEntry};

    #[test]
    fn min_heap_order_is_key_then_logical_identity() {
        let mut heap = BinaryHeap::new();
        for (key, logical_id, value) in [
            (2.0, 8, 'd'),
            (1.0, 9, 'c'),
            (1.0, 3, 'a'),
            (1.0, 7, 'b'),
        ] {
            heap.push(MinEntry {
                key,
                logical_id,
                value,
            });
        }
        let popped: Vec<_> = core::iter::from_fn(|| heap.pop())
            .map(|entry| (entry.key, entry.logical_id, entry.value))
            .collect();
        assert_eq!(
            popped,
            vec![(1.0, 3, 'a'), (1.0, 7, 'b'), (1.0, 9, 'c'), (2.0, 8, 'd')]
        );
    }
}
