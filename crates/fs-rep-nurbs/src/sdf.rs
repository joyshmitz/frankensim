//! CONVERTER NURBS → SDF (plan §7.3 edge 3, bead wqd.11; [F] — behind
//! the `nurbs-sdf` feature until its Gauntlet tier is green): measured distance
//! estimates to trimmed NURBS shells. Bézier control hulls drive a useful
//! branch-and-bound bracket and damped Gauss–Newton improves an evaluated-point
//! estimate, but Cartesian division, hull inflation, surface evaluation, and
//! distance arithmetic are ordinary f64 operations rather than outward-rounded
//! enclosures. The chart therefore emits `Estimate`, no Lipschitz authority,
//! and an explicitly estimated name. Trim classification can further widen the
//! estimate. A successor interval/Taylor path owns certified distance and sign.

use crate::NurbsError;
use crate::closest::{CLOSEST_MAX_SPLITS, closest_point_surface, norm3};
use crate::rat::Rat;
use crate::surface::NurbsSurface;
use crate::trim::{Classification, TrimmedPatch};
use fs_evidence::NumericalCertificate;
use fs_exec::Cx;
use fs_geom::{Aabb, BettiBounds, Chart, ChartSample, Differentiability, Point3, Vec3};
use fs_math::{next_down, next_up};

/// Sign policy for the generated field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Surface normals (du × dv) point OUTWARD (B-rep topology says so):
    /// the lower-authority distance estimate uses an orientation-based sign.
    Outward,
    /// No orientation claim: the field is UNSIGNED (all non-negative)
    /// and the chart name says so.
    Unknown,
}

/// One measured distance-bracket query answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdfQuery {
    /// Convex-hull lower estimate with heuristic f64 inflation.
    pub lower: f64,
    /// Evaluated-point distance estimate.
    pub upper: f64,
    /// The best parameter found (u, v).
    pub param: [f64; 2],
    /// Which shell surface owned the minimum.
    pub surface: usize,
    /// The closest point fell outside the kept trim region (or in the
    /// boundary band): `upper` is infinite because no kept-surface witness was
    /// established. `param` and `surface` remain diagnostic only.
    pub trim_downgrade: bool,
    /// Branch-and-bound splits spent (throughput evidence).
    pub splits: u32,
}

/// A NURBS shell presented as a measured distance-field approximation.
#[derive(Debug)]
pub struct ShellSdf {
    surfaces: Vec<NurbsSurface<f64>>,
    trims: Vec<Option<TrimmedPatch>>,
    orientation: Orientation,
}

/// Gauss–Newton polish iterations (evaluated-distance improvement only).
const POLISH_STEPS: usize = 8;

/// Trim-classification grid. The entire closed cell containing the floating
/// parameter is classified; no point sample stands in for the original value.
const TRIM_SCALE: i128 = 1 << 20;
const TRIM_SCALE_F64: f64 = 1_048_576.0;
const MAX_EXACT_GRID_INDEX: f64 = 9_007_199_254_740_991.0;

/// Defensive sample ceiling for one legacy tile allocation.
const SDF_TILE_MAX_SAMPLES: usize = 16_777_216;

/// Defensive worst-case split ceiling for one legacy tile request.
const SDF_TILE_MAX_WORST_CASE_SPLITS: u128 = 1_073_741_824;

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

    /// One-ULP-outward control-net support, padded outward by one further ULP.
    /// For the exact-real interpretation of the stored f64 homogeneous
    /// controls, correctly rounded division plus this expansion contains each
    /// Cartesian control point and hence the rational surface.
    ///
    /// # Panics
    /// If `pad` is non-finite or negative. This legacy infallible API treats
    /// support padding as a caller configuration precondition.
    #[must_use]
    pub fn control_aabb(&self, pad: f64) -> Aabb {
        assert!(
            pad.is_finite() && pad >= 0.0,
            "NURBS SDF support padding must be finite and non-negative"
        );
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for s in &self.surfaces {
            for row in &s.cpw {
                for h in row {
                    let c = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
                    for k in 0..3 {
                        min[k] = min[k].min(next_down(c[k]));
                        max[k] = max[k].max(next_up(c[k]));
                    }
                }
            }
        }
        Aabb::new(
            Point3::new(
                next_down(min[0] - pad),
                next_down(min[1] - pad),
                next_down(min[2] - pad),
            ),
            Point3::new(
                next_up(max[0] + pad),
                next_up(max[1] + pad),
                next_up(max[2] + pad),
            ),
        )
    }

    /// Measured unsigned-distance bracket per surface, Gauss–Newton polish,
    /// then trim classification of the winning parameter.
    ///
    /// # Errors
    /// Propagates surface-evaluation structural errors.
    pub fn distance(&self, q: [f64; 3], tol: f64, max_splits: u32) -> Result<SdfQuery, NurbsError> {
        let surface_count = u64::try_from(self.surfaces.len()).map_err(|_| NurbsError::Domain {
            what: "NURBS shell surface count cannot be represented as u64".to_string(),
        })?;
        let requested_splits = u64::from(max_splits)
            .checked_mul(surface_count)
            .ok_or_else(|| NurbsError::Domain {
                what: "NURBS shell split accounting overflows u64".to_string(),
            })?;
        if requested_splits > u64::from(u32::MAX) {
            return Err(NurbsError::Domain {
                what: "NURBS shell worst-case split accounting exceeds u32".to_string(),
            });
        }
        let mut best: Option<SdfQuery> = None;
        let mut global_lower = f64::INFINITY;
        let mut total_splits = 0u32;
        for (idx, s) in self.surfaces.iter().enumerate() {
            let cd = closest_point_surface(s, q, tol, max_splits)?;
            global_lower = global_lower.min(cd.lower);
            total_splits = total_splits.checked_add(cd.iterations).ok_or_else(|| {
                NurbsError::Domain {
                    what: "NURBS shell split accounting exceeds u32".to_string(),
                }
            })?;
            let (upper, param) = polish_upper(s, q, cd.param, cd.upper);
            let trim_downgrade = if let Some(trim) = &self.trims[idx] {
                match (
                    trim_parameter_cell(param[0]),
                    trim_parameter_cell(param[1]),
                ) {
                    (Some((umin, umax)), Some((vmin, vmax))) => {
                        trim.classify_box([umin, vmin], [umax, vmax])?
                            != Classification::Inside
                    }
                    // A parameter outside the checked rationalization domain
                    // cannot acquire trim authority from a saturating cast.
                    _ => true,
                }
            } else {
                false
            };
            let cand = SdfQuery {
                lower: cd.lower.min(upper),
                upper,
                param,
                surface: idx,
                trim_downgrade,
                splits: 0,
            };
            best = Some(match best {
                None => cand,
                // A point on a kept surface is a usable upper witness and
                // always outranks a numerically closer point that was trimmed
                // away. Within the same trim class, retain the smaller
                // evaluated distance deterministically.
                Some(b) if b.trim_downgrade && !cand.trim_downgrade => cand,
                Some(b) if !b.trim_downgrade && cand.trim_downgrade => b,
                Some(b) if cand.upper < b.upper => cand,
                Some(b) => b,
            });
        }
        let mut out = best.expect("non-empty shell");
        out.lower = global_lower.min(out.upper);
        out.splits = total_splits;
        if out.trim_downgrade {
            out.upper = f64::INFINITY;
        }
        Ok(out)
    }
}

/// Exact rational endpoints of the 2^-20 cell containing a finite f64.
/// Multiplication by this power of two is exact while representable. Values
/// outside the exactly integral f64 grid-index range fail closed so no
/// saturating float-to-integer cast can manufacture trim authority.
fn trim_parameter_cell(value: f64) -> Option<(Rat, Rat)> {
    let scaled = value * TRIM_SCALE_F64;
    if !scaled.is_finite() || scaled.abs() > MAX_EXACT_GRID_INDEX {
        return None;
    }
    let lo_f = scaled.floor();
    let hi_f = scaled.ceil();
    if lo_f.abs() > MAX_EXACT_GRID_INDEX || hi_f.abs() > MAX_EXACT_GRID_INDEX {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let lo = i128::from(lo_f as i64);
    #[allow(clippy::cast_possible_truncation)]
    let hi = i128::from(hi_f as i64);
    Some((Rat::new(lo, TRIM_SCALE), Rat::new(hi, TRIM_SCALE)))
}

/// Damped Gauss–Newton on `min ‖S(u,v) − q‖²`: only an ACCEPTED
/// improvement (a strictly smaller evaluated distance inside the domain)
/// updates the answer, so the returned upper estimate comes from a genuinely
/// evaluated surface point (still ordinary, non-directed f64 arithmetic).
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
                let d = norm3([p[0] - q[0], p[1] - q[1], p[2] - q[2]]);
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
    ///
    /// # Panics
    /// If `tol` or `support_pad` is non-finite or negative, or if
    /// `max_splits` exceeds the defensive legacy-path ceiling.
    #[must_use]
    pub fn new(shell: ShellSdf, tol: f64, max_splits: u32, support_pad: f64) -> ShellSdfChart {
        assert!(
            tol.is_finite() && tol >= 0.0,
            "NURBS SDF tolerance must be finite and non-negative"
        );
        assert!(
            max_splits <= CLOSEST_MAX_SPLITS,
            "NURBS SDF split request exceeds the defensive legacy-path ceiling"
        );
        assert!(
            support_pad.is_finite() && support_pad >= 0.0,
            "NURBS SDF support padding must be finite and non-negative"
        );
        ShellSdfChart {
            shell,
            tol,
            max_splits,
            support_pad,
        }
    }

    /// Signed-or-unsigned distance estimate for one point.
    fn sample(&self, x: Point3) -> Result<ChartSample, NurbsError> {
        let q = [x.x, x.y, x.z];
        let query = self.shell.distance(q, self.tol, self.max_splits)?;
        if query.trim_downgrade {
            // The found point is trimmed away, so it is not a witness on the
            // kept surface. Do not retain its finite value as the chart's
            // nominal distance: consumers that forget to inspect `error` must
            // fail closed too.
            return Ok(ChartSample {
                signed_distance: f64::INFINITY,
                gradient: None,
                lipschitz: None,
                error: NumericalCertificate::no_claim(),
            });
        }
        let (mut lo, mut hi) = (query.lower, query.upper);
        let (sign, gradient) = self.sign_and_gradient(q, &query);
        let signed = sign * query.upper;
        if sign < 0.0 {
            (lo, hi) = (-hi, -lo);
        }
        Ok(ChartSample {
            signed_distance: signed,
            gradient,
            lipschitz: None,
            error: NumericalCertificate::estimate(lo, hi),
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
        let off_norm = norm3(off);
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
            Orientation::Outward => "nurbs-sdf/estimated-signed",
            Orientation::Unknown => "nurbs-sdf/estimated-unsigned",
        }
    }

    fn differentiability(&self) -> Differentiability {
        Differentiability::Unknown
    }
}

/// One generated tile of measured samples on a regular grid.
#[derive(Debug, Clone)]
pub struct SdfTile {
    /// Grid resolution per axis.
    pub n: usize,
    /// Field values (x-fastest), signed per the shell orientation; a
    /// trim-downgraded cell stores positive infinity as an unusable sentinel.
    pub values: Vec<f32>,
    /// Worst measured bracket width among near-surface cells.
    pub worst_near_width: f64,
    /// Worst width among far cells (cheap bounds are allowed there).
    pub worst_far_width: f64,
    /// Total branch-and-bound splits (the throughput ledger line).
    pub total_splits: u64,
    /// Cells downgraded by trim classification.
    pub downgraded: usize,
}

/// Tiled generation with adaptive effort under defensive static ceilings:
/// a tighter requested tolerance inside the near band (|d| ≤ 2 cell
/// diagonals), and cheaper measured bounds elsewhere.
///
/// # Errors
/// Returns a structured domain/structure error for malformed tile settings,
/// overflow, requests above the defensive static ceilings, allocation refusal,
/// or a structural surface error.
pub fn generate_tile(
    chart: &ShellSdfChart,
    aabb: &Aabb,
    n: usize,
    tol_near: f64,
    max_splits: u32,
) -> Result<SdfTile, NurbsError> {
    if n < 2 {
        return Err(NurbsError::Domain {
            what: "an SDF tile needs at least 2 samples per axis".to_string(),
        });
    }
    if !aabb.is_finite() {
        return Err(NurbsError::Domain {
            what: "an SDF tile requires a finite well-formed AABB".to_string(),
        });
    }
    if !tol_near.is_finite() || tol_near < 0.0 {
        return Err(NurbsError::Domain {
            what: "SDF tile tolerance must be finite and non-negative".to_string(),
        });
    }
    if max_splits > CLOSEST_MAX_SPLITS {
        return Err(NurbsError::Domain {
            what: "SDF tile split request exceeds the defensive legacy-path ceiling".to_string(),
        });
    }
    let sample_count = n
        .checked_mul(n)
        .and_then(|square| square.checked_mul(n))
        .ok_or_else(|| NurbsError::Domain {
            what: "SDF tile sample count overflows usize".to_string(),
        })?;
    if sample_count > SDF_TILE_MAX_SAMPLES {
        return Err(NurbsError::Domain {
            what: format!(
                "SDF tile sample count {sample_count} exceeds defensive ceiling {SDF_TILE_MAX_SAMPLES}"
            ),
        });
    }
    let sample_count_u128 = u128::try_from(sample_count).map_err(|_| NurbsError::Domain {
        what: "SDF tile sample count cannot be represented as u128".to_string(),
    })?;
    let surface_count = u128::try_from(chart.shell.surfaces.len()).map_err(|_| {
        NurbsError::Domain {
            what: "SDF shell surface count cannot be represented as u128".to_string(),
        }
    })?;
    let splits_per_query = u128::from(max_splits)
        .checked_mul(surface_count)
        .ok_or_else(|| NurbsError::Domain {
            what: "SDF shell per-query split count overflows u128".to_string(),
        })?;
    if splits_per_query > u128::from(u32::MAX) {
        return Err(NurbsError::Domain {
            what: "SDF shell per-query split accounting exceeds u32".to_string(),
        });
    }
    let worst_case_splits = sample_count_u128
        .checked_mul(u128::from(max_splits) + u128::from(max_splits / 4))
        .and_then(|one_surface| one_surface.checked_mul(surface_count))
        .ok_or_else(|| NurbsError::Domain {
            what: "SDF tile worst-case split count overflows u128".to_string(),
        })?;
    if worst_case_splits > SDF_TILE_MAX_WORST_CASE_SPLITS {
        return Err(NurbsError::Domain {
            what: format!(
                "SDF tile worst-case split request {worst_case_splits} exceeds defensive ceiling {SDF_TILE_MAX_WORST_CASE_SPLITS}"
            ),
        });
    }
    #[allow(clippy::cast_precision_loss)]
    let step = [
        (aabb.max.x - aabb.min.x) / (n - 1) as f64,
        (aabb.max.y - aabb.min.y) / (n - 1) as f64,
        (aabb.max.z - aabb.min.z) / (n - 1) as f64,
    ];
    let diag = norm3(step);
    if step.iter().any(|value| !value.is_finite()) || !diag.is_finite() {
        return Err(NurbsError::Domain {
            what: "SDF tile step or diagonal is not representable as finite f64".to_string(),
        });
    }
    // Refinement fires within two diagonals of the surface; the tighter
    // measured-effort lane is used only for cells adjacent by this heuristic
    // (medial-axis
    // cells are equidistant from many patches — hull pruning stalls
    // there, and the estimate honestly stays wider).
    let refine_band = 2.0 * diag;
    let near_band = 0.75 * diag;
    let tol_far = (refine_band * 0.5).max(tol_near);
    if !refine_band.is_finite() || !near_band.is_finite() || !tol_far.is_finite() {
        return Err(NurbsError::Domain {
            what: "SDF tile refinement scale is not representable as finite f64".to_string(),
        });
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(sample_count)
        .map_err(|_| NurbsError::Structure {
            what: format!("SDF tile allocation refused for {sample_count} samples"),
        })?;
    let mut tile = SdfTile {
        n,
        values,
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
                    tile.worst_near_width = f64::INFINITY;
                    tile.worst_far_width = f64::INFINITY;
                    tile.values.push(f32::INFINITY);
                    continue;
                }
                let (sign, _) = chart.sign_and_gradient(q, &query);
                #[allow(clippy::cast_possible_truncation)]
                tile.values.push((sign * query.upper) as f32);
            }
        }
    }
    Ok(tile)
}
