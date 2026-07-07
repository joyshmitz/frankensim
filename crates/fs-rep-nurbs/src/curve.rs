//! Rational B-spline curves over a generic scalar: homogeneous de Boor
//! evaluation, derivatives to arbitrary order (f64 path), EXACT Boehm
//! knot insertion, Bézier decomposition, and EXACT degree elevation via
//! per-segment Bézier elevation (the elevated curve carries a
//! full-multiplicity knot vector — valid, evaluation-identical; minimal
//! knot vectors are a documented follow-up).

use crate::NurbsError;
use crate::basis::{KnotVector, Scalar};

/// One span's Cartesian control box: (min, max, t0, t1).
pub type SpanBox<S, const DIM: usize> = ([S; DIM], [S; DIM], S, S);

/// A rational curve in `DIM` dimensions: homogeneous control points
/// `(w·x…, w)` over a clamped knot vector.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve<S: Scalar, const DIM: usize> {
    /// The knot vector.
    pub knots: KnotVector<S>,
    /// Homogeneous control points: `DIM` weighted coordinates + weight.
    pub cpw: Vec<[S; 4]>,
}

impl<S: Scalar, const DIM: usize> NurbsCurve<S, DIM> {
    /// Build from Cartesian control points + weights.
    ///
    /// # Errors
    /// [`NurbsError::Structure`] on count mismatch or non-positive
    /// weights.
    pub fn new(
        knots: KnotVector<S>,
        points: &[[S; DIM]],
        weights: &[S],
    ) -> Result<Self, NurbsError> {
        assert!(DIM <= 3, "curves live in up to 3 dimensions");
        if points.len() != knots.control_count() || weights.len() != points.len() {
            return Err(NurbsError::Structure {
                what: format!(
                    "knot vector wants {} control points, got {} points / {} weights",
                    knots.control_count(),
                    points.len(),
                    weights.len()
                ),
            });
        }
        if weights.iter().any(|&w| w <= S::zero()) {
            return Err(NurbsError::Structure {
                what: "weights must be positive".to_string(),
            });
        }
        let cpw = points
            .iter()
            .zip(weights)
            .map(|(p, &w)| {
                let mut h = [S::zero(); 4];
                for (slot, &c) in h.iter_mut().zip(p.iter()) {
                    *slot = c * w;
                }
                h[3] = w;
                h
            })
            .collect();
        Ok(NurbsCurve { knots, cpw })
    }

    /// Homogeneous evaluation (the shared exact/fast core).
    ///
    /// # Errors
    /// [`NurbsError::Domain`] outside the domain.
    pub fn eval_homogeneous(&self, t: S) -> Result<[S; 4], NurbsError> {
        let (span, basis) = self.knots.basis(t)?;
        let p = self.knots.degree;
        let mut acc = [S::zero(); 4];
        for (r, &b) in basis.iter().enumerate() {
            let cp = &self.cpw[span - p + r];
            for (a, &c) in acc.iter_mut().zip(cp.iter()) {
                *a = *a + b * c;
            }
        }
        Ok(acc)
    }

    /// Cartesian evaluation.
    ///
    /// # Errors
    /// [`NurbsError::Domain`] outside the domain.
    pub fn eval(&self, t: S) -> Result<[S; DIM], NurbsError> {
        let h = self.eval_homogeneous(t)?;
        let mut out = [S::zero(); DIM];
        for (o, &c) in out.iter_mut().zip(h.iter()) {
            *o = c / h[3];
        }
        Ok(out)
    }

    /// EXACT Boehm knot insertion at `t` (multiplicity one per call).
    ///
    /// # Errors
    /// [`NurbsError::Domain`] outside the OPEN domain interior.
    pub fn insert_knot(&self, t: S) -> Result<Self, NurbsError> {
        let (lo, hi) = self.knots.domain();
        if t <= lo || hi <= t {
            return Err(NurbsError::Domain {
                what: format!("insertion parameter {t:?} must be interior to {lo:?}..{hi:?}"),
            });
        }
        let p = self.knots.degree;
        let k = self.knots.span(t)?;
        let mut new_cpw = Vec::with_capacity(self.cpw.len() + 1);
        new_cpw.extend_from_slice(&self.cpw[..=k - p]);
        for i in (k - p + 1)..=k {
            let denom = self.knots.knots[i + p] - self.knots.knots[i];
            let alpha = (t - self.knots.knots[i]) / denom;
            let mut q = [S::zero(); 4];
            for ((slot, &a), &b) in q.iter_mut().zip(&self.cpw[i - 1]).zip(&self.cpw[i]) {
                *slot = (S::one() - alpha) * a + alpha * b;
            }
            new_cpw.push(q);
        }
        new_cpw.extend_from_slice(&self.cpw[k..]);
        let mut new_knots = self.knots.knots.clone();
        new_knots.insert(k + 1, t);
        Ok(NurbsCurve {
            knots: KnotVector::new(new_knots, p)?,
            cpw: new_cpw,
        })
    }

    /// EXACT knot removal (inverse of [`Self::insert_knot`]) — succeeds
    /// only when the curve is exactly representable without the knot
    /// (e.g. a knot that was previously inserted); the reconstruction's
    /// consistency equation is checked with SCALAR EQUALITY, so in `Rat`
    /// this is a proof, not a tolerance.
    ///
    /// # Errors
    /// [`NurbsError::Domain`] when `t` is not an interior knot;
    /// [`NurbsError::Structure`] when removal is not exact.
    pub fn remove_knot(&self, t: S) -> Result<Self, NurbsError> {
        let p = self.knots.degree;
        let (lo, hi) = self.knots.domain();
        if t <= lo || hi <= t || !self.knots.knots.contains(&t) {
            return Err(NurbsError::Domain {
                what: format!("{t:?} is not an interior knot"),
            });
        }
        // Index of the LAST occurrence of t.
        let r = self
            .knots
            .knots
            .iter()
            .rposition(|&u| u == t)
            .expect("contains checked");
        let mut new_knots = self.knots.knots.clone();
        new_knots.remove(r);
        // Insertion produced: Q_i = (1−α_i) P_{i−1} + α_i P_i over the
        // affected band, with α from the REMOVED knot vector. Reconstruct
        // P forward and backward; the meet must agree exactly.
        let k = r - 1; // span index of t in the removed vector
        let q = &self.cpw;
        let mut fwd: Vec<[S; 4]> = Vec::new(); // P_{k-p} .. computed forward
        let mut prev = q[k - p]; // P_{k-p} = Q_{k-p}
        fwd.push(prev);
        for i in (k - p + 1)..=k {
            let denom = new_knots[i + p] - new_knots[i];
            let alpha = (t - new_knots[i]) / denom;
            if alpha == S::zero() {
                return Err(NurbsError::Structure {
                    what: "degenerate removal alpha".to_string(),
                });
            }
            let mut pi = [S::zero(); 4];
            for ((slot, &qi), &pm) in pi.iter_mut().zip(&q[i]).zip(&prev) {
                *slot = (qi - (S::one() - alpha) * pm) / alpha;
            }
            fwd.push(pi);
            prev = pi;
        }
        // Consistency: the reconstructed P_k must equal Q_{k+1} (= P_k).
        if fwd.last() != Some(&q[k + 1]) {
            return Err(NurbsError::Structure {
                what: "knot is not exactly removable (curve genuinely uses it)".to_string(),
            });
        }
        let mut new_cpw: Vec<[S; 4]> = Vec::with_capacity(q.len() - 1);
        new_cpw.extend_from_slice(&q[..k - p]);
        new_cpw.extend_from_slice(&fwd[..fwd.len() - 1]);
        new_cpw.extend_from_slice(&q[k + 1..]);
        Ok(NurbsCurve {
            knots: KnotVector::new(new_knots, p)?,
            cpw: new_cpw,
        })
    }

    /// Decompose into Bézier segments by raising every interior knot to
    /// multiplicity `degree` (EXACT). Returns the refined curve.
    ///
    /// # Errors
    /// Propagates structural errors (none for valid inputs).
    pub fn to_bezier_form(&self) -> Result<Self, NurbsError> {
        let p = self.knots.degree;
        let mut cur = self.clone();
        loop {
            // Find an interior knot with multiplicity < p.
            let (lo, hi) = cur.knots.domain();
            let mut target = None;
            let mut i = 0;
            while i < cur.knots.knots.len() {
                let t = cur.knots.knots[i];
                if t > lo && t < hi {
                    let mult = cur.knots.knots.iter().filter(|&&u| u == t).count();
                    if mult < p {
                        target = Some(t);
                        break;
                    }
                }
                i += 1;
            }
            match target {
                Some(t) => cur = cur.insert_knot(t)?,
                None => return Ok(cur),
            }
        }
    }

    /// EXACT degree elevation by one: decompose to Bézier form, elevate
    /// each segment with the exact binomial rule, and reassemble with a
    /// full-multiplicity knot vector. Evaluation is IDENTICAL (the
    /// conformance suite proves it with rational equality).
    ///
    /// # Errors
    /// Propagates structural/domain errors.
    pub fn elevate_degree(&self) -> Result<Self, NurbsError> {
        let p = self.knots.degree;
        let bez = self.to_bezier_form()?;
        // Collect distinct knots in order.
        let mut breaks: Vec<S> = Vec::new();
        for &u in &bez.knots.knots {
            if breaks.last() != Some(&u) {
                breaks.push(u);
            }
        }
        // Elevate each Bézier segment: Q_0 = P_0; Q_{p+1} = P_p;
        // Q_i = (i/(p+1)) P_{i-1} + (1 - i/(p+1)) P_i.
        let seg_count = breaks.len() - 1;
        let mut new_cpw: Vec<[S; 4]> = Vec::new();
        for seg in 0..seg_count {
            let start = seg * p;
            let pts = &bez.cpw[start..=start + p];
            let mut q = Vec::with_capacity(p + 2);
            q.push(pts[0]);
            for i in 1..=p {
                let alpha = S::from_int(i64::try_from(i).expect("small degree"))
                    / S::from_int(i64::try_from(p + 1).expect("small degree"));
                let mut v = [S::zero(); 4];
                for ((slot, &a), &b) in v.iter_mut().zip(&pts[i - 1]).zip(&pts[i]) {
                    *slot = alpha * a + (S::one() - alpha) * b;
                }
                q.push(v);
            }
            q.push(pts[p]);
            if seg == 0 {
                new_cpw.extend_from_slice(&q);
            } else {
                // Segment start point coincides with previous end.
                new_cpw.extend_from_slice(&q[1..]);
            }
        }
        // Knot vector: each break with multiplicity p+1 interior, p+2 ends.
        let mut new_knots = Vec::new();
        for (bi, &b) in breaks.iter().enumerate() {
            let mult = if bi == 0 || bi == breaks.len() - 1 {
                p + 2
            } else {
                p + 1
            };
            for _ in 0..mult {
                new_knots.push(b);
            }
        }
        Ok(NurbsCurve {
            knots: KnotVector::new(new_knots, p + 1)?,
            cpw: new_cpw,
        })
    }

    /// Per-span control-point bounding boxes in Cartesian space (the
    /// convex-hull property: each span's curve lies inside its box).
    /// Requires Bézier form for the tight per-segment claim; on general
    /// knot vectors the box of the span's `p+1` control points still
    /// bounds that span.
    ///
    /// # Errors
    /// Propagates domain errors (none for valid curves).
    pub fn span_boxes(&self) -> Result<Vec<SpanBox<S, DIM>>, NurbsError> {
        let p = self.knots.degree;
        let mut out = Vec::new();
        for span in p..self.knots.control_count() {
            let (t0, t1) = (self.knots.knots[span], self.knots.knots[span + 1]);
            if t1 <= t0 {
                continue;
            }
            let mut min = [S::zero(); DIM];
            let mut max = [S::zero(); DIM];
            let mut first = true;
            for cp in &self.cpw[span - p..=span] {
                let w = cp[3];
                for d in 0..DIM {
                    let c = cp[d] / w;
                    if first {
                        min[d] = c;
                        max[d] = c;
                    } else {
                        if c < min[d] {
                            min[d] = c;
                        }
                        if max[d] < c {
                            max[d] = c;
                        }
                    }
                }
                first = false;
            }
            out.push((min, max, t0, t1));
        }
        Ok(out)
    }
}

impl<const DIM: usize> NurbsCurve<f64, DIM> {
    /// Derivatives up to `order` at `t` (rational quotient rule over the
    /// homogeneous derivative curves). Returns `[C(t), C'(t), …]`.
    ///
    /// # Errors
    /// [`NurbsError::Domain`] outside the domain.
    pub fn derivatives(&self, t: f64, order: usize) -> Result<Vec<[f64; DIM]>, NurbsError> {
        // Homogeneous derivative control nets by repeated differencing.
        let p = self.knots.degree;
        let order = order.min(p);
        let mut nets: Vec<(Vec<[f64; 4]>, Vec<f64>, usize)> = Vec::with_capacity(order + 1);
        nets.push((self.cpw.clone(), self.knots.knots.clone(), p));
        for k in 1..=order {
            let (prev, knots, deg) = &nets[k - 1];
            let mut next = Vec::with_capacity(prev.len() - 1);
            #[allow(clippy::cast_precision_loss)]
            let degf = *deg as f64;
            for i in 0..prev.len() - 1 {
                let denom = knots[i + deg + 1] - knots[i + 1];
                let mut d = [0.0f64; 4];
                if denom != 0.0 {
                    for (slot, (a, b)) in d.iter_mut().zip(prev[i + 1].iter().zip(&prev[i])) {
                        *slot = degf * (a - b) / denom;
                    }
                }
                next.push(d);
            }
            let new_knots = knots[1..knots.len() - 1].to_vec();
            nets.push((next, new_knots, deg - 1));
        }
        // Evaluate each homogeneous derivative, then the quotient rule:
        // C^(k) = (A^(k) − Σ_{i=1..k} C(k−i) · w^(i) · binom(k,i)) / w.
        let mut hom = Vec::with_capacity(order + 1);
        for (net, knots, deg) in &nets {
            match (*deg, KnotVector::new(knots.clone(), (*deg).max(1))) {
                (0, _) => {
                    // Degree-0 net: piecewise constant per reduced span.
                    // Constant per span of the reduced knot vector.
                    let mut idx = 0usize;
                    while idx + 1 < knots.len() - 1 && knots[idx + 1] <= t {
                        idx += 1;
                    }
                    hom.push(net[idx.min(net.len() - 1)]);
                }
                (_, Ok(kv)) => {
                    let (span, basis) = kv.basis(t)?;
                    let mut acc = [0.0f64; 4];
                    for (r, &b) in basis.iter().enumerate() {
                        let cp = &net[span - deg + r];
                        for (a, &c) in acc.iter_mut().zip(cp.iter()) {
                            *a += b * c;
                        }
                    }
                    hom.push(acc);
                }
                (_, Err(e)) => return Err(e),
            }
        }
        let binom = |n: usize, k: usize| -> f64 {
            let mut b = 1.0f64;
            for j in 0..k {
                #[allow(clippy::cast_precision_loss)]
                {
                    b = b * (n - j) as f64 / (j + 1) as f64;
                }
            }
            b
        };
        let w0 = hom[0][3];
        let mut out: Vec<[f64; DIM]> = Vec::with_capacity(order + 1);
        for k in 0..=order {
            let mut num = [0.0f64; DIM];
            for (slot, &a) in num.iter_mut().zip(hom[k].iter()) {
                *slot = a;
            }
            for i in 1..=k {
                let c = binom(k, i) * hom[i][3];
                for (slot, prev) in num.iter_mut().zip(out[k - i].iter()) {
                    *slot -= c * prev;
                }
            }
            out.push(num.map(|v| v / w0));
        }
        Ok(out)
    }
}
