//! Self-intersection freedom as a PROOF: sweep-and-prune broad phase,
//! EXACT narrow phase — plane-separation early exits, then exact
//! edge-vs-triangle tests (four `orient3d` signs each; complete for
//! non-coplanar pairs because every intersection-segment endpoint lies
//! on some edge), with exact 2D `orient2d` handling for the coplanar
//! case. Exactness makes a false PASS impossible; configurations in
//! exact contact (shared plane touching, coincident patches) are
//! reported CONSERVATIVELY as intersections of kind `Touching` — the
//! bounded, listed false-FAIL class the acceptance contract allows.

use fs_geom::Point3;
use fs_ivl::{Sign, orient2d, orient3d};
use fs_rep_mesh::Soup;

/// How a flagged pair intersects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntersectKind {
    /// Interiors cross (strict intersection, exact).
    Crossing,
    /// Exact contact / coplanar overlap — conservative flag.
    Touching,
}

/// The certificate.
#[derive(Debug, Clone)]
pub struct SelfIntersectReport {
    /// Empty ⟺ PROVEN free of self-intersections among non-adjacent
    /// face pairs (exact arithmetic).
    pub intersections: Vec<(usize, usize, IntersectKind)>,
    /// Candidate pairs the narrow phase examined.
    pub pairs_tested: u64,
}

impl SelfIntersectReport {
    /// True when non-intersection is PROVEN.
    #[must_use]
    pub fn proven_free(&self) -> bool {
        self.intersections.is_empty()
    }
}

fn p3(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// Exact sign classification of `t2`'s plane against `t1`'s vertices.
fn plane_signs(t_plane: &[[f64; 3]; 3], pts: &[[f64; 3]; 3]) -> [Sign; 3] {
    core::array::from_fn(|i| orient3d(t_plane[0], t_plane[1], t_plane[2], pts[i]))
}

fn all(signs: [Sign; 3], s: Sign) -> bool {
    signs.iter().all(|&x| x == s)
}

/// Exact triangle-triangle intersection (closed triangles). `None`
/// means PROVEN disjoint; `Some(kind)` localizes the contact class.
#[must_use]
pub fn tri_tri_intersect(t1: [Point3; 3], t2: [Point3; 3]) -> Option<IntersectKind> {
    let a = [p3(t1[0]), p3(t1[1]), p3(t1[2])];
    let b = [p3(t2[0]), p3(t2[1]), p3(t2[2])];
    let sa = plane_signs(&b, &a); // T1's vertices vs plane(T2)
    if all(sa, Sign::Positive) || all(sa, Sign::Negative) {
        return None; // strictly separated by plane(T2): PROVEN
    }
    let sb = plane_signs(&a, &b);
    if all(sb, Sign::Positive) || all(sb, Sign::Negative) {
        return None;
    }
    if sa == [Sign::Zero; 3] {
        // Coplanar: exact 2D overlap test.
        return coplanar_overlap(&a, &b);
    }
    // General case: for non-coplanar triangles, any intersection
    // segment ends on an edge of one of them — so T1 ∩ T2 ≠ ∅ iff
    // some edge of T1 meets T2 or some edge of T2 meets T1. Each
    // edge-triangle test is four exact orient3d signs.
    let mut touching = false;
    for i in 0..3 {
        match segment_triangle(a[i], a[(i + 1) % 3], &b) {
            Some(IntersectKind::Crossing) => return Some(IntersectKind::Crossing),
            Some(IntersectKind::Touching) => touching = true,
            None => {}
        }
        match segment_triangle(b[i], b[(i + 1) % 3], &a) {
            Some(IntersectKind::Crossing) => return Some(IntersectKind::Crossing),
            Some(IntersectKind::Touching) => touching = true,
            None => {}
        }
    }
    touching.then_some(IntersectKind::Touching)
}

/// Exact segment-vs-triangle: `(p, q)` against `(a, b, c)`.
/// Strict crossing needs the endpoints strictly on opposite sides of
/// the plane AND the segment's line passing strictly inside the
/// triangle; any on-boundary sign yields the conservative `Touching`.
fn segment_triangle(p: [f64; 3], q: [f64; 3], t: &[[f64; 3]; 3]) -> Option<IntersectKind> {
    let s1 = orient3d(t[0], t[1], t[2], p);
    let s2 = orient3d(t[0], t[1], t[2], q);
    if s1 == s2 && s1 != Sign::Zero {
        return None; // both endpoints strictly on one side
    }
    if s1 == Sign::Zero && s2 == Sign::Zero {
        return None; // collinear-with-plane handled by the coplanar path
    }
    // Side volumes: the segment's line passes through the triangle iff
    // the three tetrahedra (p,q,edge) agree in orientation.
    let v1 = orient3d(p, q, t[0], t[1]);
    let v2 = orient3d(p, q, t[1], t[2]);
    let v3 = orient3d(p, q, t[2], t[0]);
    let signs = [v1, v2, v3];
    let pos = signs.iter().filter(|&&s| s == Sign::Positive).count();
    let neg = signs.iter().filter(|&&s| s == Sign::Negative).count();
    if pos > 0 && neg > 0 {
        return None; // the line misses the triangle: PROVEN
    }
    let boundary = signs.contains(&Sign::Zero) || s1 == Sign::Zero || s2 == Sign::Zero;
    if boundary {
        Some(IntersectKind::Touching)
    } else {
        Some(IntersectKind::Crossing)
    }
}

/// Exact coplanar overlap: any 2D edge pair crosses, or one triangle's
/// vertex lies inside the other (all via exact `orient2d`).
fn coplanar_overlap(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> Option<IntersectKind> {
    // Projection axis: drop the dominant normal component (choice only
    // affects degeneracy; correctness comes from exact 2D predicates).
    let e1 = [a[1][0] - a[0][0], a[1][1] - a[0][1], a[1][2] - a[0][2]];
    let e2 = [a[2][0] - a[0][0], a[2][1] - a[0][1], a[2][2] - a[0][2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let drop = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() {
        0
    } else if n[1].abs() >= n[2].abs() {
        1
    } else {
        2
    };
    let q = |p: [f64; 3]| -> [f64; 2] {
        match drop {
            0 => [p[1], p[2]],
            1 => [p[2], p[0]],
            _ => [p[0], p[1]],
        }
    };
    let qa = [q(a[0]), q(a[1]), q(a[2])];
    let qb = [q(b[0]), q(b[1]), q(b[2])];
    // Segment-pair crossings.
    for i in 0..3 {
        for j in 0..3 {
            let (p1, p2) = (qa[i], qa[(i + 1) % 3]);
            let (p3v, p4) = (qb[j], qb[(j + 1) % 3]);
            let d1 = orient2d(p3v, p4, p1);
            let d2 = orient2d(p3v, p4, p2);
            let d3 = orient2d(p1, p2, p3v);
            let d4 = orient2d(p1, p2, p4);
            let opposite = |x: Sign, y: Sign| {
                matches!(
                    (x, y),
                    (Sign::Positive, Sign::Negative) | (Sign::Negative, Sign::Positive)
                )
            };
            if opposite(d1, d2) && opposite(d3, d4) {
                return Some(IntersectKind::Touching); // coplanar contact class
            }
        }
    }
    // Containment either way (strict interior via consistent signs).
    let inside = |p: [f64; 2], t: &[[f64; 2]; 3]| -> bool {
        let s0 = orient2d(t[0], t[1], p);
        let s1 = orient2d(t[1], t[2], p);
        let s2 = orient2d(t[2], t[0], p);
        (s0 == s1 && s1 == s2 && s0 != Sign::Zero)
            || (s0 != Sign::Zero || s1 != Sign::Zero || s2 != Sign::Zero)
                && [s0, s1, s2]
                    .iter()
                    .filter(|&&s| s != Sign::Zero)
                    .collect::<Vec<_>>()
                    .windows(2)
                    .all(|w| w[0] == w[1])
    };
    if qa.iter().any(|&p| inside(p, &qb)) || qb.iter().any(|&p| inside(p, &qa)) {
        return Some(IntersectKind::Touching);
    }
    None
}

/// Prove a soup free of self-intersections among NON-ADJACENT face
/// pairs (faces sharing a vertex legitimately touch). Sweep-and-prune
/// on x, then exact narrow phase.
#[must_use]
pub fn self_intersection_certificate(soup: &Soup) -> SelfIntersectReport {
    let nf = soup.triangles.len();
    // AABBs + sweep order.
    let mut boxes = Vec::with_capacity(nf);
    for t in &soup.triangles {
        let ps = t.map(|v| soup.positions[v as usize]);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in ps {
            for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                lo[k] = lo[k].min(c);
                hi[k] = hi[k].max(c);
            }
        }
        boxes.push((lo, hi));
    }
    let mut order: Vec<usize> = (0..nf).collect();
    order.sort_by(|&i, &j| {
        boxes[i].0[0]
            .partial_cmp(&boxes[j].0[0])
            .expect("finite coords")
            .then(i.cmp(&j))
    });
    let mut intersections = Vec::new();
    let mut pairs_tested = 0u64;
    for (oi, &i) in order.iter().enumerate() {
        for &j in order.iter().skip(oi + 1) {
            if boxes[j].0[0] > boxes[i].1[0] {
                break; // sweep axis separation: no further overlaps
            }
            // Remaining axes.
            if boxes[j].0[1] > boxes[i].1[1]
                || boxes[i].0[1] > boxes[j].1[1]
                || boxes[j].0[2] > boxes[i].1[2]
                || boxes[i].0[2] > boxes[j].1[2]
            {
                continue;
            }
            // Adjacency exclusion: shared vertices touch legitimately.
            let ti = soup.triangles[i];
            let tj = soup.triangles[j];
            if ti.iter().any(|v| tj.contains(v)) {
                continue;
            }
            pairs_tested += 1;
            let pi = ti.map(|v| soup.positions[v as usize]);
            let pj = tj.map(|v| soup.positions[v as usize]);
            if let Some(kind) = tri_tri_intersect(pi, pj) {
                let (lo, hi) = (i.min(j), i.max(j));
                intersections.push((lo, hi, kind));
            }
        }
    }
    intersections.sort_unstable_by_key(|&(a, b, _)| (a, b));
    SelfIntersectReport {
        intersections,
        pairs_tested,
    }
}
