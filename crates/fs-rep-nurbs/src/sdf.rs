//! CONVERTER NURBS → SDF (plan §7.3 edge 3, bead wqd.11; [F] — behind
//! the `nurbs-sdf` feature until its Gauntlet tier is green): CERTIFIED
//! distance to trimmed NURBS shells. The bracket comes from the
//! convex-hull property (Bézier-clipping branch-and-bound: the patch
//! lies inside its control hull, so hull distance is a rigorous lower
//! bound; any evaluated point is a rigorous upper bound), POLISHED by
//! damped Gauss–Newton on the projection equations (which can only
//! tighten the upper bound — certification never depends on Newton
//! converging). Trim classification decides which surface regions count;
//! a closest point landing outside the kept region (or in the boundary
//! band) DOWNGRADES the certificate rather than lying. Sign comes from
//! declared B-rep orientation; without one, the field is unsigned and
//! the chart says so — the router edge label (certified vs
//! measured-only) is decided PER INPUT, never assumed.

use crate::NurbsError;
use crate::closest::closest_point_surface;
use crate::rat::Rat;
use crate::surface::NurbsSurface;
use crate::trim::{Classification, TrimmedPatch};
use fs_evidence::NumericalCertificate;
use fs_exec::Cx;
use fs_geom::{Aabb, BettiBounds, Chart, ChartSample, Differentiability, Point3, Vec3};
use fs_math::det;

/// Sign policy for the generated field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Surface normals (du × dv) point OUTWARD (B-rep topology says so):
    /// the field is signed.
    Outward,
    /// No orientation claim: the field is UNSIGNED (all non-negative)
    /// and the chart name says so.
    Unknown,
}

/// One certified distance query answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdfQuery {
    /// Rigorous lower bound of the (unsigned) distance.
    pub lower: f64,
    /// Rigorous upper bound (distance to an evaluated surface point).
    pub upper: f64,
    /// The best parameter found (u, v).
    pub param: [f64; 2],
    /// Which shell surface owned the minimum.
    pub surface: usize,
    /// The closest point fell outside the kept trim region (or in the
    /// boundary band): the certificate below is WIDENED and the label
    /// degrades to measured-only.
    pub trim_downgrade: bool,
    /// Branch-and-bound splits spent (throughput evidence).
    pub splits: u32,
}

/// A NURBS shell presented as a certified distance field.
#[derive(Debug)]
pub struct ShellSdf {
    surfaces: Vec<NurbsSurface<f64>>,
    trims: Vec<Option<TrimmedPatch>>,
    orientation: Orientation,
}

/// Gauss–Newton polish iterations (upper-bound tightening only).
const POLISH_STEPS: usize = 8;

/// Trim classification scale: params are rationalized at 2^20
/// (sub-ppm parameter resolution — the boundary band absorbs the rest).
const TRIM_SCALE: i128 = 1 << 20;

impl ShellSdf {
    /// A shell from surfaces + optional trims (parallel arrays).
    ///
    /// # Errors
    /// [`NurbsError::Structure`] on length mismatch or an empty shell.
    pub fn new(
        surfaces: Vec<NurbsSurface<f64>>,
        trims: Vec<Option<TrimmedPatch>>,
        orientation: Orientation,
    ) -> Result<ShellSdf, NurbsError> {
        if surfaces.is_empty() {
            return Err(NurbsError::Structure {
                what: "a shell needs at least one surface".to_string(),
            });
        }
        if surfaces.len() != trims.len() {
            return Err(NurbsError::Structure {
                what: format!(
                    "{} surfaces but {} trim slots (parallel arrays)",
                    surfaces.len(),
                    trims.len()
                ),
            });
        }
        Ok(ShellSdf {
            surfaces,
            trims,
            orientation,
        })
    }

    /// The control-net bounding box (contains the shell by the convex
    /// hull property), padded.
    #[must_use]
    pub fn control_aabb(&self, pad: f64) -> Aabb {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for s in &self.surfaces {
            for row in &s.cpw {
                for h in row {
                    let c = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
                    for k in 0..3 {
                        min[k] = min[k].min(c[k]);
                        max[k] = max[k].max(c[k]);
                    }
                }
            }
        }
        Aabb::new(
            Point3::new(min[0] - pad, min[1] - pad, min[2] - pad),
            Point3::new(max[0] + pad, max[1] + pad, max[2] + pad),
        )
    }

    /// Certified unsigned distance: branch-and-bound bracket per surface
    /// (hull lower bounds are rigorous; evaluated points are rigorous
    /// upper bounds), Gauss–Newton polish, then trim classification of
    /// the winning parameter.
    ///
    /// # Errors
    /// Propagates surface-evaluation structural errors.
    pub fn distance(&self, q: [f64; 3], tol: f64, max_splits: u32) -> Result<SdfQuery, NurbsError> {
        let mut best: Option<SdfQuery> = None;
        for (idx, s) in self.surfaces.iter().enumerate() {
            let cd = closest_point_surface(s, q, tol, max_splits)?;
            let (upper, param) = polish_upper(s, q, cd.param, cd.upper);
            let cand = SdfQuery {
                lower: cd.lower.min(upper),
                upper,
                param,
                surface: idx,
                trim_downgrade: false,
                splits: cd.iterations,
            };
            best = Some(match best {
                None => cand,
                Some(b) if cand.upper < b.upper => SdfQuery {
                    // The shell minimum: the lower bound must cover ALL
                    // surfaces (min of lowers), the upper the best found.
                    lower: cand.lower.min(b.lower),
                    splits: b.splits + cand.splits,
                    ..cand
                },
                Some(b) => SdfQuery {
                    lower: b.lower.min(cand.lower),
                    splits: b.splits + cand.splits,
                    ..b
                },
            });
        }
        let mut out = best.expect("non-empty shell");
        if let Some(trim) = &self.trims[out.surface] {
            let ru = Rat::new(
                (out.param[0] * TRIM_SCALE as f64).round() as i128,
                TRIM_SCALE,
            );
            let rv = Rat::new(
                (out.param[1] * TRIM_SCALE as f64).round() as i128,
                TRIM_SCALE,
            );
            if trim.classify([ru, rv])? != Classification::Inside {
                out.trim_downgrade = true;
            }
        }
        Ok(out)
    }
}

/// Damped Gauss–Newton on `min ‖S(u,v) − q‖²`: only an ACCEPTED
/// improvement (a strictly smaller evaluated distance inside the domain)
/// updates the answer, so the returned upper bound is always the
/// distance to a genuinely evaluated surface point.
fn polish_upper(
    s: &NurbsSurface<f64>,
    q: [f64; 3],
    start: [f64; 2],
    upper0: f64,
) -> (f64, [f64; 2]) {
    let (ulo, uhi) = s.knots_u.domain();
    let (vlo, vhi) = s.knots_v.domain();
    let mut best = (upper0, start);
    let mut uv = start;
    for _ in 0..POLISH_STEPS {
        let Ok((pos, du, dv)) = s.partials(uv[0], uv[1]) else {
            break;
        };
        let r = [pos[0] - q[0], pos[1] - q[1], pos[2] - q[2]];
        // Normal equations of the 2x3 Jacobian.
        let (a, b, c) = (dot3(du, du), dot3(du, dv), dot3(dv, dv));
        let (g0, g1) = (dot3(du, r), dot3(dv, r));
        let detm = a * c - b * b;
        if detm.abs() < 1e-300 {
            break;
        }
        let step = [-(c * g0 - b * g1) / detm, -(a * g1 - b * g0) / detm];
        let mut damp = 1.0f64;
        let mut improved = false;
        for _ in 0..4 {
            let cand = [
                (uv[0] + damp * step[0]).clamp(ulo, uhi),
                (uv[1] + damp * step[1]).clamp(vlo, vhi),
            ];
            if let Ok(p) = s.eval(cand[0], cand[1]) {
                let d = det::sqrt(
                    (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2),
                );
                if d < best.0 {
                    best = (d, cand);
                    uv = cand;
                    improved = true;
                    break;
                }
            }
            damp *= 0.25;
        }
        if !improved {
            break;
        }
    }
    best
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The Chart presentation of a [`ShellSdf`] (the router-visible form).
#[derive(Debug)]
pub struct ShellSdfChart {
    shell: ShellSdf,
    tol: f64,
    max_splits: u32,
    support_pad: f64,
}

impl ShellSdfChart {
    /// Wrap a shell with query effort settings.
    #[must_use]
    pub fn new(shell: ShellSdf, tol: f64, max_splits: u32, support_pad: f64) -> ShellSdfChart {
        ShellSdfChart {
            shell,
            tol,
            max_splits,
            support_pad,
        }
    }

    /// Signed (or unsigned) distance + certificate for one point.
    fn sample(&self, x: Point3) -> Result<ChartSample, NurbsError> {
        let q = [x.x, x.y, x.z];
        let query = self.shell.distance(q, self.tol, self.max_splits)?;
        let (mut lo, mut hi) = (query.lower, query.upper);
        if query.trim_downgrade {
            // The closest kept-region point is at least this far, but the
            // upper bound no longer certifies (the found point is
            // trimmed away): widen honestly.
            hi = f64::INFINITY;
        }
        let (sign, gradient) = self.sign_and_gradient(q, &query);
        let signed = sign * query.upper;
        if sign < 0.0 {
            (lo, hi) = (-hi, -lo);
        }
        Ok(ChartSample {
            signed_distance: signed,
            gradient,
            lipschitz: Some(1.0),
            error: NumericalCertificate::enclosure(lo, hi),
        })
    }

    /// Sign from declared orientation (normal · offset); gradient from
    /// the offset direction when it is well-defined.
    fn sign_and_gradient(&self, q: [f64; 3], query: &SdfQuery) -> (f64, Option<Vec3>) {
        let s = &self.shell.surfaces[query.surface];
        let Ok((pos, du, dv)) = s.partials(query.param[0], query.param[1]) else {
            return (1.0, None);
        };
        let off = [q[0] - pos[0], q[1] - pos[1], q[2] - pos[2]];
        let off_norm = det::sqrt(dot3(off, off));
        let gradient = if off_norm > 1e-12 {
            Some(Vec3::new(
                off[0] / off_norm,
                off[1] / off_norm,
                off[2] / off_norm,
            ))
        } else {
            None // on the surface: the medial-axis caveat
        };
        match self.shell.orientation {
            Orientation::Unknown => (1.0, gradient),
            Orientation::Outward => {
                let n = [
                    du[1] * dv[2] - du[2] * dv[1],
                    du[2] * dv[0] - du[0] * dv[2],
                    du[0] * dv[1] - du[1] * dv[0],
                ];
                let sign = if dot3(n, off) < 0.0 { -1.0 } else { 1.0 };
                let g = gradient.map(|g| if sign < 0.0 { g.scale(-1.0) } else { g });
                (sign, g)
            }
        }
    }
}

impl Chart for ShellSdfChart {
    fn eval(&self, x: Point3, _cx: &Cx<'_>) -> ChartSample {
        self.sample(x).unwrap_or(ChartSample {
            signed_distance: f64::INFINITY,
            gradient: None,
            lipschitz: None,
            error: NumericalCertificate::no_claim(),
        })
    }

    fn support(&self) -> Aabb {
        self.shell.control_aabb(self.support_pad)
    }

    fn topology_hint(&self) -> BettiBounds {
        BettiBounds::unknown()
    }

    fn name(&self) -> &'static str {
        match self.shell.orientation {
            Orientation::Outward => "nurbs-sdf/signed",
            Orientation::Unknown => "nurbs-sdf/unsigned",
        }
    }

    fn differentiability(&self) -> Differentiability {
        Differentiability::C1
    }
}

/// One generated tile: certified samples on a regular grid.
#[derive(Debug, Clone)]
pub struct SdfTile {
    /// Grid resolution per axis.
    pub n: usize,
    /// Field values (x-fastest), signed per the shell orientation.
    pub values: Vec<f32>,
    /// Worst certified bound width among NEAR-surface cells.
    pub worst_near_width: f64,
    /// Worst width among far cells (cheap bounds are allowed there).
    pub worst_far_width: f64,
    /// Total branch-and-bound splits (the throughput ledger line).
    pub total_splits: u64,
    /// Cells downgraded by trim classification.
    pub downgraded: usize,
}

/// Tiled generation with ADAPTIVE effort (budget-aware per P4): tight
/// tolerance inside the near band (|d| ≤ 2 cell diagonals), cheap
/// bounds elsewhere — far-field accuracy is not paid for.
///
/// # Errors
/// Propagates structural surface errors.
pub fn generate_tile(
    chart: &ShellSdfChart,
    aabb: &Aabb,
    n: usize,
    tol_near: f64,
    max_splits: u32,
) -> Result<SdfTile, NurbsError> {
    assert!(n >= 2, "a tile needs at least 2 samples per axis");
    #[allow(clippy::cast_precision_loss)]
    let step = [
        (aabb.max.x - aabb.min.x) / (n - 1) as f64,
        (aabb.max.y - aabb.min.y) / (n - 1) as f64,
        (aabb.max.z - aabb.min.z) / (n - 1) as f64,
    ];
    let diag = det::sqrt(step.iter().map(|s| s * s).sum());
    // Refinement fires within two diagonals of the surface; the TIGHT
    // claim is made only for cells genuinely adjacent to it (medial-axis
    // cells are equidistant from many patches — hull pruning stalls
    // there, and the certificate honestly stays wider).
    let refine_band = 2.0 * diag;
    let near_band = 0.75 * diag;
    let tol_far = (refine_band * 0.5).max(tol_near);
    let mut tile = SdfTile {
        n,
        values: Vec::with_capacity(n * n * n),
        worst_near_width: 0.0,
        worst_far_width: 0.0,
        total_splits: 0,
        downgraded: 0,
    };
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                #[allow(clippy::cast_precision_loss)]
                let q = [
                    aabb.min.x + i as f64 * step[0],
                    aabb.min.y + j as f64 * step[1],
                    aabb.min.z + k as f64 * step[2],
                ];
                // Cheap pass first; refine only inside the near band.
                let coarse = chart.shell.distance(q, tol_far, max_splits / 4)?;
                let query = if coarse.lower <= refine_band {
                    let fine = chart.shell.distance(q, tol_near, max_splits)?;
                    tile.total_splits += u64::from(coarse.splits);
                    fine
                } else {
                    coarse
                };
                tile.total_splits += u64::from(query.splits);
                let width = query.upper - query.lower;
                if query.upper <= near_band {
                    tile.worst_near_width = tile.worst_near_width.max(width);
                } else {
                    tile.worst_far_width = tile.worst_far_width.max(width);
                }
                if query.trim_downgrade {
                    tile.downgraded += 1;
                }
                let (sign, _) = chart.sign_and_gradient(q, &query);
                #[allow(clippy::cast_possible_truncation)]
                tile.values.push((sign * query.upper) as f32);
            }
        }
    }
    Ok(tile)
}
