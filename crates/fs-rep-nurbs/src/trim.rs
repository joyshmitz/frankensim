//! Trimmed patches with CERTIFIED point classification. Trim loops are
//! held in EXACT RATIONAL form (2-D parameter-space NURBS over `Rat`) —
//! the dual representation the bead demands. Classification is proved,
//! not sampled: if the query point lies strictly outside every Bézier
//! span's control hull box, the curve and its control polygon are
//! homotopic in a region avoiding the point, so the EXACTLY-computed
//! control-polygon winding number IS the curve's winding number.
//! Ambiguous points (inside a hull box after bounded exact subdivision)
//! are honestly `Boundary`, never a guessed in/out.

use crate::NurbsError;
use crate::curve::NurbsCurve;
use crate::rat::Rat;

/// One closed trim loop: an exact rational curve in (u, v) parameter
/// space (closure is validated).
#[derive(Debug, Clone, PartialEq)]
pub struct TrimLoop {
    /// The exact 2-D curve.
    pub curve: NurbsCurve<Rat, 2>,
}

impl TrimLoop {
    /// Validate closure and construct.
    ///
    /// # Errors
    /// [`NurbsError::Structure`] when the loop is not closed (exact
    /// endpoint equality — this is the rational representation).
    pub fn new(curve: NurbsCurve<Rat, 2>) -> Result<Self, NurbsError> {
        let (lo, hi) = curve.knots.domain();
        let start = curve.eval(lo)?;
        let end = curve.eval(hi)?;
        if start != end {
            return Err(NurbsError::Structure {
                what: "trim loop must close exactly (rational endpoint equality)".to_string(),
            });
        }
        Ok(TrimLoop { curve })
    }

    /// The same loop with reversed orientation (holes are wound opposite
    /// to outers under the nonzero rule): control points reversed, knot
    /// vector mirrored about the domain.
    #[must_use]
    pub fn reversed_for_hole(&self) -> TrimLoop {
        let (lo, hi) = self.curve.knots.domain();
        let mut knots: Vec<Rat> = self
            .curve
            .knots
            .knots
            .iter()
            .rev()
            .map(|&u| lo + hi - u)
            .collect();
        // Mirroring preserves ordering because the source was sorted.
        knots.sort();
        let cpw: Vec<[Rat; 4]> = self.curve.cpw.iter().rev().copied().collect();
        TrimLoop {
            curve: NurbsCurve {
                knots: crate::basis::KnotVector {
                    knots,
                    degree: self.curve.knots.degree,
                },
                cpw,
            },
        }
    }
}

/// A certified classification verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// Certified inside the trimmed region (nonzero total winding).
    Inside,
    /// Certified outside.
    Outside,
    /// Within the certification band of some trim curve — no in/out
    /// claim is made (the honest verdict on tangent/sliver cases).
    Boundary,
}

/// A trimmed patch: parameter-space loops over any surface. (The surface
/// itself is not needed for classification, which happens in parameter
/// space; carrying it is the B-rep bookkeeping.)
#[derive(Debug, Clone, PartialEq)]
pub struct TrimmedPatch {
    /// Outer boundary + hole loops (orientation encodes solidity via the
    /// nonzero-winding rule: outer CCW, holes CW).
    pub loops: Vec<TrimLoop>,
    /// Exact-subdivision depth before declaring `Boundary`.
    pub max_subdivision: u32,
}

impl TrimmedPatch {
    /// Construct with the default certification depth.
    #[must_use]
    pub fn new(loops: Vec<TrimLoop>) -> Self {
        TrimmedPatch {
            loops,
            max_subdivision: 12,
        }
    }

    /// Certified classification of a parameter-space point.
    ///
    /// # Errors
    /// Propagates structural errors from exact subdivision.
    pub fn classify(&self, q: [Rat; 2]) -> Result<Classification, NurbsError> {
        self.classify_box(q, q)
    }

    /// Certified classification of every point in a closed parameter-space
    /// box. A verdict is returned only after every trim-curve Bézier hull is
    /// separated from the entire box, which proves that winding is constant
    /// throughout the connected box. Otherwise bounded subdivision returns
    /// [`Classification::Boundary`] rather than guessing from its corners or
    /// centre.
    ///
    /// # Errors
    /// Returns [`NurbsError::Domain`] for an inverted box and propagates
    /// structural errors from exact subdivision.
    pub fn classify_box(
        &self,
        min: [Rat; 2],
        max: [Rat; 2],
    ) -> Result<Classification, NurbsError> {
        if min[0] > max[0] || min[1] > max[1] {
            return Err(NurbsError::Domain {
                what: "trim classification box must be componentwise ordered".to_string(),
            });
        }
        let two = Rat::int(2);
        let witness = [(min[0] + max[0]) / two, (min[1] + max[1]) / two];
        let mut winding = 0i64;
        for l in &self.loops {
            match loop_winding_box(&l.curve, min, max, witness, self.max_subdivision)? {
                Some(w) => winding += w,
                None => return Ok(Classification::Boundary),
            }
        }
        Ok(if winding != 0 {
            Classification::Inside
        } else {
            Classification::Outside
        })
    }
}

/// Certified winding number of one closed rational curve about `q`, or
/// `None` when `q` cannot be separated from the curve within the
/// subdivision budget.
fn loop_winding_box(
    curve: &NurbsCurve<Rat, 2>,
    query_min: [Rat; 2],
    query_max: [Rat; 2],
    witness: [Rat; 2],
    max_depth: u32,
) -> Result<Option<i64>, NurbsError> {
    // Work in Bézier form so each span's control hull tightly bounds it.
    let mut work = curve.to_bezier_form()?;
    let mut depth = 0u32;
    loop {
        let boxes = work.span_boxes()?;
        let offending: Vec<(Rat, Rat)> = boxes
            .iter()
            .filter(|(min, max, _, _)| {
                max[0] >= query_min[0]
                    && min[0] <= query_max[0]
                    && max[1] >= query_min[1]
                    && min[1] <= query_max[1]
            })
            .map(|&(_, _, t0, t1)| (t0, t1))
            .collect();
        if offending.is_empty() {
            // Separated from the whole connected query box: winding is
            // constant throughout it, so one exact witness is sufficient.
            return Ok(Some(polygon_winding(
                &control_polygon(&work),
                witness,
            )));
        }
        if depth >= max_depth {
            return Ok(None);
        }
        for (t0, t1) in offending {
            let mid = (t0 + t1) / Rat::int(2);
            // Exact midpoint insertion splits the offending span.
            if let Ok(next) = work.insert_knot(mid) {
                work = next;
            }
        }
        work = work.to_bezier_form()?;
        depth += 1;
    }
}

/// The Cartesian control polygon (weights divided out — the hull
/// property holds for rational Bézier segments in Cartesian space).
fn control_polygon(curve: &NurbsCurve<Rat, 2>) -> Vec<[Rat; 2]> {
    curve
        .cpw
        .iter()
        .map(|cp| [cp[0] / cp[3], cp[1] / cp[3]])
        .collect()
}

/// EXACT winding number of a closed polygon about `q` (crossing rule
/// with exact rational orientation tests — no epsilons anywhere).
fn polygon_winding(poly: &[[Rat; 2]], q: [Rat; 2]) -> i64 {
    let mut winding = 0i64;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        // Upward crossing: a.y <= q.y < b.y and q strictly left of ab.
        let orient = (b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= q[1] && q[1] < b[1] && orient > Rat::int(0) {
            winding += 1;
        } else if b[1] <= q[1] && q[1] < a[1] && orient < Rat::int(0) {
            winding -= 1;
        }
    }
    winding
}
