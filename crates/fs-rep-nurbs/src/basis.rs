//! The scalar abstraction and B-spline basis machinery, written ONCE and
//! instantiated at both `f64` (fast path) and [`crate::Rat`] (the exact
//! path the refinement-exactness claims are proved in).

use crate::NurbsError;
use crate::rat::Rat;

/// The field the spline algebra runs over.
pub trait Scalar:
    Copy
    + PartialEq
    + PartialOrd
    + core::fmt::Debug
    + core::ops::Add<Output = Self>
    + core::ops::Sub<Output = Self>
    + core::ops::Mul<Output = Self>
    + core::ops::Div<Output = Self>
    + core::ops::Neg<Output = Self>
{
    /// Additive identity.
    fn zero() -> Self;
    /// Multiplicative identity.
    fn one() -> Self;
    /// Lift a small integer.
    fn from_int(v: i64) -> Self;
}

impl Scalar for f64 {
    fn zero() -> Self {
        0.0
    }
    fn one() -> Self {
        1.0
    }
    fn from_int(v: i64) -> Self {
        #[allow(clippy::cast_precision_loss)]
        {
            v as f64
        }
    }
}

impl Scalar for Rat {
    fn zero() -> Self {
        Rat::int(0)
    }
    fn one() -> Self {
        Rat::int(1)
    }
    fn from_int(v: i64) -> Self {
        Rat::int(v)
    }
}

/// A clamped knot vector for degree-p splines.
#[derive(Debug, Clone, PartialEq)]
pub struct KnotVector<S: Scalar> {
    /// Non-decreasing knots (first/last with multiplicity p+1).
    pub knots: Vec<S>,
    /// Polynomial degree.
    pub degree: usize,
}

impl<S: Scalar> KnotVector<S> {
    /// Validate and construct.
    ///
    /// # Errors
    /// [`NurbsError::Structure`] on ordering/clamping defects.
    pub fn new(knots: Vec<S>, degree: usize) -> Result<Self, NurbsError> {
        if degree == 0 || knots.len() < 2 * (degree + 1) {
            return Err(NurbsError::Structure {
                what: format!(
                    "degree {degree} needs at least {} knots, got {}",
                    2 * (degree + 1),
                    knots.len()
                ),
            });
        }
        if knots.windows(2).any(|w| w[1] < w[0]) {
            return Err(NurbsError::Structure {
                what: "knots must be non-decreasing".to_string(),
            });
        }
        for k in 0..degree {
            if knots[k + 1] != knots[0] || knots[knots.len() - 2 - k] != knots[knots.len() - 1] {
                return Err(NurbsError::Structure {
                    what: "knot vector must be clamped (end multiplicity degree+1)".to_string(),
                });
            }
        }
        Ok(KnotVector { knots, degree })
    }

    /// Number of basis functions / control points.
    #[must_use]
    pub fn control_count(&self) -> usize {
        self.knots.len() - self.degree - 1
    }

    /// The parametric domain `[u_min, u_max]`.
    #[must_use]
    pub fn domain(&self) -> (S, S) {
        (
            self.knots[self.degree],
            self.knots[self.knots.len() - 1 - self.degree],
        )
    }

    /// The knot span index containing `t` (Piegl–Tiller A2.1 semantics;
    /// the end parameter maps into the last non-empty span).
    ///
    /// # Errors
    /// [`NurbsError::Domain`] outside the domain.
    pub fn span(&self, t: S) -> Result<usize, NurbsError> {
        let (lo, hi) = self.domain();
        if t < lo || t > hi {
            return Err(NurbsError::Domain {
                what: format!("parameter {t:?} outside {lo:?}..{hi:?}"),
            });
        }
        let n = self.control_count() - 1;
        if t == hi {
            // Walk back to the last span with positive width.
            let mut s = n;
            while self.knots[s] == self.knots[s + 1] {
                s -= 1;
            }
            return Ok(s);
        }
        let mut span = self.degree;
        while span < n && self.knots[span + 1] <= t {
            span += 1;
        }
        Ok(span)
    }

    /// All nonzero basis-function values at `t` (Cox–de Boor triangle,
    /// Piegl–Tiller A2.2): `N_{span-p..=span, p}(t)`.
    ///
    /// # Errors
    /// [`NurbsError::Domain`] outside the domain.
    pub fn basis(&self, t: S) -> Result<(usize, Vec<S>), NurbsError> {
        let p = self.degree;
        let span = self.span(t)?;
        let mut n = vec![S::zero(); p + 1];
        let mut left = vec![S::zero(); p + 1];
        let mut right = vec![S::zero(); p + 1];
        n[0] = S::one();
        for j in 1..=p {
            left[j] = t - self.knots[span + 1 - j];
            right[j] = self.knots[span + j] - t;
            let mut saved = S::zero();
            for r in 0..j {
                let denom = right[r + 1] + left[j - r];
                let temp = n[r] / denom;
                n[r] = saved + right[r + 1] * temp;
                saved = left[j - r] * temp;
            }
            n[j] = saved;
        }
        Ok((span, n))
    }
}
