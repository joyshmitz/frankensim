//! Radius-edge quality refinement (Ruppert/Shewchuk-style first cut):
//! insert circumcenters of the worst tets until every tet's
//! radius-edge ratio meets the bound, a Steiner budget is spent, or the
//! remaining offenders' circumcenters fall outside the hull (a bounded
//! DOMAIN's encroachment handling is the successor bead's constrained
//! recovery — skipping is the honest v1 policy, and it is COUNTED).
//! Deterministic: worst-first with a canonical tie-break, sequential
//! inserts through the same exact-predicate kernel.

use crate::delaunay::{GHOST, MeshError, Tetrahedralization};
use fs_exec::Cx;

/// Refinement policy.
#[derive(Debug, Clone, Copy)]
pub struct RefineOptions {
    /// Radius-edge target (2.0 is the classical safe bound).
    pub max_radius_edge: f64,
    /// Steiner-point budget.
    pub max_steiner: u32,
}

impl Default for RefineOptions {
    fn default() -> Self {
        RefineOptions {
            max_radius_edge: 2.0,
            max_steiner: 2000,
        }
    }
}

/// What refinement did (ledger evidence).
#[derive(Debug, Clone, Copy, Default)]
pub struct RefineStats {
    /// Circumcenters inserted.
    pub steiner_inserted: u32,
    /// Offenders skipped because the circumcenter left the hull.
    pub skipped_outside_hull: u32,
    /// Worst radius-edge ratio before.
    pub worst_before: f64,
    /// Worst radius-edge ratio after.
    pub worst_after: f64,
    /// Offenders left whose circumcenters escape the hull (boundary
    /// handling is the successor bead — these are COUNTED, not hidden).
    pub unrefinable_remaining: u32,
    /// Offenders left that were still refinable when budgets ran out.
    pub refinable_remaining: u32,
}

impl RefineStats {
    /// Canonical JSON object.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"steiner_inserted\":{},\"skipped_outside_hull\":{},\
             \"worst_before\":{:.4},\"worst_after\":{:.4},\
             \"unrefinable_remaining\":{},\"refinable_remaining\":{}}}",
            self.steiner_inserted,
            self.skipped_outside_hull,
            self.worst_before,
            self.worst_after,
            self.unrefinable_remaining,
            self.refinable_remaining
        )
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn det3(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Circumcenter of a positively oriented tet (f64 — the point is a
/// STEINER point, not a certificate; exactness lives in the predicates
/// that re-triangulate around it).
fn circumcenter(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Option<[f64; 3]> {
    let (u, v, w) = (sub(b, a), sub(c, a), sub(d, a));
    let m = [u, v, w];
    let rhs = [0.5 * dot(u, u), 0.5 * dot(v, v), 0.5 * dot(w, w)];
    let den = det3(m);
    if den.abs() < 1e-300 {
        return None;
    }
    let col = |k: usize| {
        let mut mm = m;
        for (row, r) in mm.iter_mut().zip(rhs) {
            row[k] = r;
        }
        det3(mm) / den
    };
    let x = [col(0), col(1), col(2)];
    Some([a[0] + x[0], a[1] + x[1], a[2] + x[2]])
}

/// Radius-edge ratio of a tet (`circumradius / shortest edge`).
fn radius_edge(pts: &[[f64; 3]; 4]) -> Option<f64> {
    let cc = circumcenter(pts[0], pts[1], pts[2], pts[3])?;
    let r = dot(sub(cc, pts[0]), sub(cc, pts[0])).sqrt();
    let mut shortest = f64::INFINITY;
    for i in 0..4 {
        for j in (i + 1)..4 {
            let e = sub(pts[i], pts[j]);
            shortest = shortest.min(dot(e, e).sqrt());
        }
    }
    (shortest > 0.0).then(|| r / shortest)
}

/// Refine in place until the radius-edge bound holds or budgets run out.
/// Steiner points append after [`Tetrahedralization::steiner_from`].
///
/// # Errors
/// [`MeshError::Cancelled`] between insertions.
pub fn refine(
    tetra: &mut Tetrahedralization,
    opts: RefineOptions,
    cx: &Cx<'_>,
) -> Result<RefineStats, MeshError> {
    let mut stats = RefineStats::default();
    let worst_ratio = |t: &Tetrahedralization| -> Vec<(f64, [u32; 4])> {
        let pts = &t.mesh.points;
        let mut offenders: Vec<(f64, [u32; 4])> = t
            .tets()
            .into_iter()
            .filter_map(|tet| {
                let q: [[f64; 3]; 4] = core::array::from_fn(|k| pts[tet[k] as usize]);
                radius_edge(&q).map(|r| (r, tet))
            })
            .filter(|&(r, _)| r > opts.max_radius_edge)
            .collect();
        // Worst first; canonical vertex tuple breaks ties (P2).
        offenders.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(b.1.cmp(&a.1)));
        offenders
    };
    let canon = |t: [u32; 4]| {
        let mut s = t;
        s.sort_unstable();
        s
    };
    let live_set = |t: &Tetrahedralization| -> std::collections::BTreeSet<[u32; 4]> {
        t.tets().into_iter().map(canon).collect()
    };
    let initial = worst_ratio(tetra);
    stats.worst_before = initial.first().map_or(0.0, |o| o.0);
    // A skipped tet's identity never recurs after it dies (points only
    // accumulate), so the skip set is PERMANENT — each unrefinable tet
    // is counted once.
    let mut skipped: std::collections::BTreeSet<[u32; 4]> = std::collections::BTreeSet::new();
    'rounds: while stats.steiner_inserted < opts.max_steiner {
        cx.checkpoint()?;
        let offenders = worst_ratio(tetra);
        let mut live = live_set(tetra);
        let mut progressed = false;
        for &(_, tet) in &offenders {
            if stats.steiner_inserted >= opts.max_steiner {
                break;
            }
            let key = canon(tet);
            if skipped.contains(&key) || !live.contains(&key) {
                continue; // unrefinable, or killed earlier this round
            }
            let pts = &tetra.mesh.points;
            let q: [[f64; 3]; 4] = core::array::from_fn(|k| pts[tet[k] as usize]);
            let insertable =
                circumcenter(q[0], q[1], q[2], q[3]).filter(|cc| cc.iter().all(|x| x.is_finite()));
            let Some(cc) = insertable else {
                skipped.insert(key);
                continue;
            };
            // Insertable only if the circumcenter lands strictly inside
            // the hull (the conflict seed is a real tet).
            let new_idx = tetra.mesh.points.len() as u32;
            let seed = tetra.mesh.locate(cc, new_idx);
            if tetra.mesh.tets[seed as usize][3] == GHOST {
                stats.skipped_outside_hull += 1;
                skipped.insert(key);
                continue;
            }
            tetra.mesh.points.push(cc);
            if tetra.mesh.insert(new_idx) {
                stats.steiner_inserted += 1;
                progressed = true;
                live = live_set(tetra);
            } else {
                skipped.insert(key);
            }
        }
        if !progressed {
            break 'rounds;
        }
    }
    let remaining = worst_ratio(tetra);
    stats.worst_after = remaining.first().map_or(0.0, |o| o.0);
    stats.unrefinable_remaining = remaining
        .iter()
        .filter(|(_, t)| skipped.contains(&canon(*t)))
        .count() as u32;
    stats.refinable_remaining = remaining.len() as u32 - stats.unrefinable_remaining;
    tetra.mesh.stats.tets_final = tetra.tets().len() as u64;
    Ok(stats)
}
