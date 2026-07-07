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
}

impl RefineStats {
    /// Canonical JSON object.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"steiner_inserted\":{},\"skipped_outside_hull\":{},\
             \"worst_before\":{:.4},\"worst_after\":{:.4}}}",
            self.steiner_inserted,
            self.skipped_outside_hull,
            self.worst_before,
            self.worst_after
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
    let initial = worst_ratio(tetra);
    stats.worst_before = initial.first().map_or(0.0, |o| o.0);
    let mut skipped: std::collections::BTreeSet<[u32; 4]> = std::collections::BTreeSet::new();
    while stats.steiner_inserted < opts.max_steiner {
        cx.checkpoint()?;
        let offenders = worst_ratio(tetra);
        let Some(&(_, tet)) = offenders.iter().find(|(_, t)| !skipped.contains(t)) else {
            break;
        };
        let pts = &tetra.mesh.points;
        let q: [[f64; 3]; 4] = core::array::from_fn(|k| pts[tet[k] as usize]);
        let Some(cc) = circumcenter(q[0], q[1], q[2], q[3]) else {
            skipped.insert(tet);
            continue;
        };
        if !cc.iter().all(|x| x.is_finite()) {
            skipped.insert(tet);
            continue;
        }
        // Insertable only if the circumcenter lands strictly inside the
        // hull (the conflict seed is a real tet).
        let new_idx = tetra.mesh.points.len() as u32;
        let seed = tetra.mesh.locate(cc, new_idx);
        if tetra.mesh.tets[seed as usize][3] == GHOST {
            stats.skipped_outside_hull += 1;
            skipped.insert(tet);
            continue;
        }
        tetra.mesh.points.push(cc);
        if tetra.mesh.insert(new_idx) {
            stats.steiner_inserted += 1;
            // Geometry changed: previously skipped tets may be gone.
            skipped.clear();
        } else {
            skipped.insert(tet);
        }
    }
    stats.worst_after = worst_ratio(tetra).first().map_or(0.0, |o| o.0);
    tetra.mesh.stats.tets_final = tetra.tets().len() as u64;
    Ok(stats)
}
