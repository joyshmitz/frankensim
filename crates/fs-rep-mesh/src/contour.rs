//! Certified converter SDF → mesh via DUAL CONTOURING with QEF vertex
//! placement (plan §7.3 edge 2): sharp-feature capable, with THE
//! certificate — a Lipschitz-interval verification that the extracted
//! surface BRACKETS the zero set within tolerance, everywhere, not just
//! at samples.
//!
//! The certificate's honesty: for a chart with a CERTIFIED Lipschitz
//! bound L, `|φ(x)| ≤ |φ(c)| + L·r` for every x within distance r of c.
//! Subdividing each output triangle until that bound closes below the
//! tolerance PROVES the bracket over the whole surface; triangles that
//! cannot close are reported with their margins (the localized failure
//! diagnostics the acceptance criteria demand). A chart that certifies no
//! Lipschitz bound cannot get a bracket certificate — the refusal says so.

use crate::winding::Soup;
use fs_exec::Cx;
use fs_geom::{Chart, Point3, Vec3};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Vertex-placement strategy: QEF preserves sharp features; `MassPoint`
/// is the marching-cubes-class baseline the acceptance criteria compare
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Quadratic-error-function minimization (sharp features).
    Qef,
    /// Edge-crossing centroid (feature-blurring baseline).
    MassPoint,
}

/// Dual-contouring options.
#[derive(Debug, Clone, Copy)]
pub struct DcOptions {
    /// Cell edge length.
    pub h: f64,
    /// Vertex placement strategy.
    pub placement: Placement,
    /// QEF regularization toward the mass point (Schaefer-style; keeps
    /// near-planar systems well-posed without SVD).
    pub regularization: f64,
}

impl DcOptions {
    /// Sharp-feature defaults at cell size `h`.
    #[must_use]
    pub fn sharp(h: f64) -> Self {
        DcOptions {
            h,
            placement: Placement::Qef,
            regularization: 0.05,
        }
    }
}

/// Structured contouring failure.
#[derive(Debug, Clone, PartialEq)]
pub enum ContourError {
    /// The grid would exceed the per-axis cell cap.
    ResolutionTooFine {
        /// Cells/axis needed.
        need: u64,
        /// The cap.
        cap: u64,
    },
    /// The zero set never crossed the sampled grid.
    EmptySurface,
}

impl core::fmt::Display for ContourError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ContourError::ResolutionTooFine { need, cap } => write!(
                f,
                "dual contouring refused: {need} cells/axis exceed the {cap} cap; coarsen h \
                 or shrink the region"
            ),
            ContourError::EmptySurface => write!(
                f,
                "dual contouring found no zero crossings: the chart's zero set does not \
                 intersect the sampled support (empty or out-of-band geometry)"
            ),
        }
    }
}

impl core::error::Error for ContourError {}

/// Per-axis cell cap.
pub const DC_MAX_CELLS_PER_AXIS: u64 = 256;

/// Contouring statistics (ledgered evidence).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DcStats {
    /// Cells carrying a vertex.
    pub active_cells: u64,
    /// Output triangles.
    pub triangles: u64,
    /// Hermite edge crossings found.
    pub crossings: u64,
}

impl DcStats {
    /// Canonical JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut s = String::with_capacity(72);
        let _ = write!(
            s,
            "{{\"active_cells\":{},\"triangles\":{},\"crossings\":{}}}",
            self.active_cells, self.triangles, self.crossings
        );
        s
    }
}

/// Dual-contour any signed-distance chart on a uniform grid.
///
/// # Errors
/// [`ContourError`] refusals; cancellation surfaces through the chart's
/// own eval polls on the shared `Cx`.
// One coherent pass (sample -> hermite -> place -> stitch); splitting
// would scatter the winding/orientation invariants rmesh-008 audits.
#[allow(clippy::too_many_lines)]
pub fn dual_contour(
    chart: &dyn Chart,
    opts: DcOptions,
    cx: &Cx<'_>,
) -> Result<(Soup, DcStats), ContourError> {
    let support = chart.support().inflate(2.0 * opts.h);
    let n = [
        ((support.max.x - support.min.x) / opts.h).ceil() as u64 + 1,
        ((support.max.y - support.min.y) / opts.h).ceil() as u64 + 1,
        ((support.max.z - support.min.z) / opts.h).ceil() as u64 + 1,
    ];
    for &axis_n in &n {
        if axis_n > DC_MAX_CELLS_PER_AXIS {
            return Err(ContourError::ResolutionTooFine {
                need: axis_n,
                cap: DC_MAX_CELLS_PER_AXIS,
            });
        }
    }
    let n = [n[0] as u32, n[1] as u32, n[2] as u32];
    let pos = |i: u32, j: u32, k: u32| {
        Point3::new(
            support.min.x + f64::from(i) * opts.h,
            support.min.y + f64::from(j) * opts.h,
            support.min.z + f64::from(k) * opts.h,
        )
    };
    // Sample the corner lattice once.
    let idx = |i: u32, j: u32, k: u32| ((k * n[1] + j) * n[0] + i) as usize;
    let mut phi = vec![0.0f64; (n[0] * n[1] * n[2]) as usize];
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                phi[idx(i, j, k)] = chart.eval(pos(i, j, k), cx).signed_distance;
            }
        }
    }
    // Hermite data per sign-changing lattice edge; cell -> crossings map.
    let mut cell_hermite: BTreeMap<[u32; 3], Vec<(Point3, Vec3)>> = BTreeMap::new();
    let mut crossings = 0u64;
    let axes: [[u32; 3]; 3] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                for d in axes {
                    let (i2, j2, k2) = (i + d[0], j + d[1], k + d[2]);
                    if i2 >= n[0] || j2 >= n[1] || k2 >= n[2] {
                        continue;
                    }
                    let (fa, fb) = (phi[idx(i, j, k)], phi[idx(i2, j2, k2)]);
                    if (fa < 0.0) == (fb < 0.0) {
                        continue;
                    }
                    crossings += 1;
                    let (pa, pb) = (pos(i, j, k), pos(i2, j2, k2));
                    let crossing = secant_crossing(chart, pa, fa, pb, fb, cx);
                    let normal = gradient_at(chart, crossing, opts.h, cx);
                    // Every VALID adjacent cell gets the Hermite pair
                    // (boundary edges feed fewer cells; their cells still
                    // place vertices even though no quad is emitted).
                    for (du, dv) in [(0u32, 0u32), (1, 0), (1, 1), (0, 1)] {
                        let (u, v) = match d {
                            [1, 0, 0] => ([0u32, 1, 0], [0u32, 0, 1]),
                            [0, 1, 0] => ([0u32, 0, 1], [1u32, 0, 0]),
                            _ => ([1u32, 0, 0], [0u32, 1, 0]),
                        };
                        let cell = [
                            i.wrapping_sub(du * u[0]).wrapping_sub(dv * v[0]),
                            j.wrapping_sub(du * u[1]).wrapping_sub(dv * v[1]),
                            k.wrapping_sub(du * u[2]).wrapping_sub(dv * v[2]),
                        ];
                        if cell[0] < n[0] - 1 && cell[1] < n[1] - 1 && cell[2] < n[2] - 1 {
                            cell_hermite
                                .entry(cell)
                                .or_default()
                                .push((crossing, normal));
                        }
                    }
                }
            }
        }
    }
    if cell_hermite.is_empty() {
        return Err(ContourError::EmptySurface);
    }
    // One vertex per active cell.
    let mut vertex_of: BTreeMap<[u32; 3], u32> = BTreeMap::new();
    let mut positions: Vec<Point3> = Vec::with_capacity(cell_hermite.len());
    for (&cell, hermite) in &cell_hermite {
        let cell_min = pos(cell[0], cell[1], cell[2]);
        let cell_max = pos(cell[0] + 1, cell[1] + 1, cell[2] + 1);
        let v = match opts.placement {
            Placement::MassPoint => mass_point(hermite),
            Placement::Qef => solve_qef(hermite, opts.regularization, cell_min, cell_max),
        };
        vertex_of.insert(cell, positions.len() as u32);
        positions.push(v);
    }
    // Quads per interior sign-changing edge, oriented negative -> positive.
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                for d in axes {
                    let (i2, j2, k2) = (i + d[0], j + d[1], k + d[2]);
                    if i2 >= n[0] || j2 >= n[1] || k2 >= n[2] {
                        continue;
                    }
                    let (fa, fb) = (phi[idx(i, j, k)], phi[idx(i2, j2, k2)]);
                    if (fa < 0.0) == (fb < 0.0) {
                        continue;
                    }
                    let Some(ring) = edge_cells([i, j, k], d, n) else {
                        continue; // boundary edge: no full quad (open rim)
                    };
                    let q: Vec<u32> = ring.iter().map(|c| vertex_of[c]).collect();
                    // The ring circulates with normal +d; outward normals
                    // point from negative to positive phi, so fa < 0
                    // (inside at base) keeps the ring, else reverse.
                    if fa < 0.0 {
                        triangles.push([q[0], q[1], q[2]]);
                        triangles.push([q[0], q[2], q[3]]);
                    } else {
                        triangles.push([q[0], q[2], q[1]]);
                        triangles.push([q[0], q[3], q[2]]);
                    }
                }
            }
        }
    }
    let stats = DcStats {
        active_cells: positions.len() as u64,
        triangles: triangles.len() as u64,
        crossings,
    };
    Ok((
        Soup {
            positions,
            triangles,
        },
        stats,
    ))
}

/// The four cells sharing lattice edge `(base, base+d)` in RING order
/// whose circulation normal points along +d (cyclic (u, v) axes make the
/// orientation axis-uniform); `None` when the edge sits on the lattice
/// boundary (no full quad — the open rim).
fn edge_cells(base: [u32; 3], d: [u32; 3], n: [u32; 3]) -> Option<[[u32; 3]; 4]> {
    // Cyclic successors keep u x v = +d for every axis.
    let (u, v) = match d {
        [1, 0, 0] => ([0u32, 1, 0], [0u32, 0, 1]), // (y, z)
        [0, 1, 0] => ([0u32, 0, 1], [1u32, 0, 0]), // (z, x)
        _ => ([1u32, 0, 0], [0u32, 1, 0]),         // (x, y)
    };
    // CCW ring viewed from +d: offsets (0,0), (1,0), (1,1), (0,1).
    let mut ring = [[0u32; 3]; 4];
    for (slot, (du, dv)) in [(0u32, 0u32), (1, 0), (1, 1), (0, 1)]
        .into_iter()
        .enumerate()
    {
        let c = [
            base[0].wrapping_sub(du * u[0]).wrapping_sub(dv * v[0]),
            base[1].wrapping_sub(du * u[1]).wrapping_sub(dv * v[1]),
            base[2].wrapping_sub(du * u[2]).wrapping_sub(dv * v[2]),
        ];
        // Cells are corner-indexed: valid when strictly inside the lattice.
        if !(c[0] < n[0] - 1 && c[1] < n[1] - 1 && c[2] < n[2] - 1) {
            return None;
        }
        ring[slot] = c;
    }
    Some(ring)
}

fn secant_crossing(
    chart: &dyn Chart,
    mut a: Point3,
    mut fa: f64,
    mut b: Point3,
    mut fb: f64,
    cx: &Cx<'_>,
) -> Point3 {
    for _ in 0..8 {
        let t = if (fb - fa).abs() < 1e-300 {
            0.5
        } else {
            (-fa / (fb - fa)).clamp(0.0, 1.0)
        };
        let m = Point3::new(
            a.x + (b.x - a.x) * t,
            a.y + (b.y - a.y) * t,
            a.z + (b.z - a.z) * t,
        );
        let fm = chart.eval(m, cx).signed_distance;
        if fm.abs() < 1e-12 {
            return m;
        }
        if (fm < 0.0) == (fa < 0.0) {
            a = m;
            fa = fm;
        } else {
            b = m;
            fb = fm;
        }
    }
    Point3::new(
        f64::midpoint(a.x, b.x),
        f64::midpoint(a.y, b.y),
        f64::midpoint(a.z, b.z),
    )
}

fn gradient_at(chart: &dyn Chart, p: Point3, h: f64, cx: &Cx<'_>) -> Vec3 {
    if let Some(g) = chart.eval(p, cx).gradient {
        return g;
    }
    let e = 0.1 * h;
    let f = |q: Point3| chart.eval(q, cx).signed_distance;
    let g = Vec3::new(
        (f(p.offset(Vec3::new(e, 0.0, 0.0))) - f(p.offset(Vec3::new(-e, 0.0, 0.0)))) / (2.0 * e),
        (f(p.offset(Vec3::new(0.0, e, 0.0))) - f(p.offset(Vec3::new(0.0, -e, 0.0)))) / (2.0 * e),
        (f(p.offset(Vec3::new(0.0, 0.0, e))) - f(p.offset(Vec3::new(0.0, 0.0, -e)))) / (2.0 * e),
    );
    let n = g.norm().max(1e-12);
    g.scale(1.0 / n)
}

fn mass_point(hermite: &[(Point3, Vec3)]) -> Point3 {
    let n = hermite.len().max(1) as f64;
    let mut c = Point3::new(0.0, 0.0, 0.0);
    for &(p, _) in hermite {
        c = Point3::new(c.x + p.x, c.y + p.y, c.z + p.z);
    }
    Point3::new(c.x / n, c.y / n, c.z / n)
}

/// Regularized 3×3 QEF solve: minimize Σ(nᵢ·(x−pᵢ))² + λ|x−m|² where m is
/// the mass point (Schaefer-style regularization keeps near-planar
/// systems well-posed); the result clamps into the cell.
fn solve_qef(
    hermite: &[(Point3, Vec3)],
    lambda: f64,
    cell_min: Point3,
    cell_max: Point3,
) -> Point3 {
    let m = mass_point(hermite);
    // Normal equations A x = b with A = Σ nnᵀ + λI, b = Σ n(n·p) + λm.
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for &(p, nrm) in hermite {
        let nv = [nrm.x, nrm.y, nrm.z];
        let nd = nrm.dot(p.delta_from(Point3::new(0.0, 0.0, 0.0)));
        for (r, (row, rhs)) in a.iter_mut().zip(&mut b).enumerate() {
            for (entry, nc) in row.iter_mut().zip(nv) {
                *entry += nv[r] * nc;
            }
            *rhs += nv[r] * nd;
        }
    }
    for (r, row) in a.iter_mut().enumerate() {
        row[r] += lambda;
    }
    let mv = [m.x, m.y, m.z];
    for (rhs, mr) in b.iter_mut().zip(mv) {
        *rhs += lambda * mr;
    }
    // Cramer's rule (3×3; λ > 0 keeps the determinant away from zero).
    let det = det3(&a);
    if det.abs() < 1e-30 {
        return m;
    }
    let solve_col = |col: usize| {
        let mut ac = a;
        for r in 0..3 {
            ac[r][col] = b[r];
        }
        det3(&ac) / det
    };
    let x = Point3::new(solve_col(0), solve_col(1), solve_col(2));
    Point3::new(
        x.x.clamp(cell_min.x, cell_max.x),
        x.y.clamp(cell_min.y, cell_max.y),
        x.z.clamp(cell_min.z, cell_max.z),
    )
}

fn det3(a: &[[f64; 3]; 3]) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// A triangle that failed the bracket certificate, with its margin.
#[derive(Debug, Clone, PartialEq)]
pub struct BracketFailure {
    /// Output triangle index.
    pub triangle: usize,
    /// The best (smallest) proven upper bound on |φ| over the triangle.
    pub proven_bound: f64,
    /// The tolerance it had to close under.
    pub tolerance: f64,
}

/// The bracket certificate report.
#[derive(Debug, Clone, PartialEq)]
pub struct BracketReport {
    /// Triangles proven within tolerance.
    pub proven: u64,
    /// Worst proven bound across all passing triangles.
    pub worst_margin: f64,
    /// Evaluations spent.
    pub evals: u64,
}

/// Why a certificate could not be attempted at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoLipschitz;

impl core::fmt::Display for NoLipschitz {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "bracket certificate refused: the chart certifies no Lipschitz bound, so \
             |phi(centroid)| + L*r proves nothing; use a certified chart"
        )
    }
}

impl core::error::Error for NoLipschitz {}

/// Verify the bracket certificate: every output triangle's surface lies
/// within `tol` of the zero set, PROVEN via `|φ(c)| + L·r ≤ tol` with
/// recursive 4-way subdivision (depth-capped). Failures localize.
///
/// # Errors
/// `Ok(Err(failures))` is expressed as the `Result` in the return: outer
/// [`NoLipschitz`] when the chart certifies no bound; inner
/// `Result<BracketReport, Vec<BracketFailure>>` is the verdict.
#[allow(clippy::type_complexity)] // verdict-of-verdicts is the honest shape
pub fn bracket_certificate(
    chart: &dyn Chart,
    soup: &Soup,
    tol: f64,
    cx: &Cx<'_>,
) -> Result<Result<BracketReport, Vec<BracketFailure>>, NoLipschitz> {
    let probe = chart.eval(
        soup.positions
            .first()
            .copied()
            .unwrap_or(Point3::new(0.0, 0.0, 0.0)),
        cx,
    );
    let Some(lipschitz) = probe.lipschitz.filter(|l| l.is_finite()) else {
        return Err(NoLipschitz);
    };
    let mut failures = Vec::new();
    let mut worst = 0.0f64;
    let mut evals = 0u64;
    for (ti, _) in soup.triangles.iter().enumerate() {
        let [a, b, c] = soup.tri(ti);
        let mut vctx = VerifyCtx {
            chart,
            lipschitz,
            tol,
            evals: &mut evals,
            cx,
        };
        let bound = verify_triangle(&mut vctx, a, b, c, 5);
        if bound <= tol {
            worst = worst.max(bound);
        } else {
            failures.push(BracketFailure {
                triangle: ti,
                proven_bound: bound,
                tolerance: tol,
            });
        }
    }
    if failures.is_empty() {
        Ok(Ok(BracketReport {
            proven: soup.triangles.len() as u64,
            worst_margin: worst,
            evals,
        }))
    } else {
        Ok(Err(failures))
    }
}

/// Best proven upper bound on |φ| over triangle (a, b, c): Lipschitz cone
/// at the centroid, refined by subdivision while the bound fails.
struct VerifyCtx<'a, 'c> {
    chart: &'a dyn Chart,
    lipschitz: f64,
    tol: f64,
    evals: &'a mut u64,
    cx: &'a Cx<'c>,
}

fn verify_triangle(v: &mut VerifyCtx<'_, '_>, a: Point3, b: Point3, c: Point3, depth: u32) -> f64 {
    let centroid = Point3::new(
        (a.x + b.x + c.x) / 3.0,
        (a.y + b.y + c.y) / 3.0,
        (a.z + b.z + c.z) / 3.0,
    );
    let r = a
        .delta_from(centroid)
        .norm()
        .max(b.delta_from(centroid).norm())
        .max(c.delta_from(centroid).norm());
    *v.evals += 1;
    let phi_c = v.chart.eval(centroid, v.cx).signed_distance.abs();
    let bound = phi_c + v.lipschitz * r;
    if bound <= v.tol || depth == 0 {
        return bound;
    }
    // 4-way midpoint subdivision.
    let mab = mid(a, b);
    let mbc = mid(b, c);
    let mca = mid(c, a);
    let sub = [(a, mab, mca), (mab, b, mbc), (mca, mbc, c), (mab, mbc, mca)];
    sub.iter()
        .map(|&(x, y, z)| verify_triangle(v, x, y, z, depth - 1))
        .fold(0.0f64, f64::max)
}

fn mid(a: Point3, b: Point3) -> Point3 {
    Point3::new(
        f64::midpoint(a.x, b.x),
        f64::midpoint(a.y, b.y),
        f64::midpoint(a.z, b.z),
    )
}
