//! Exact rational arithmetic over i128 — the scalar the EXACT spline
//! algebra runs in. Every operation is gcd-reduced and overflow-checked:
//! leaving the exactness domain is a structured event (checked panics
//! with a named message, bounded by construction in the conformance
//! fixtures), never silent wraparound.

use core::cmp::Ordering;

/// A reduced fraction (denominator > 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rat {
    /// Numerator.
    num: i128,
    /// Denominator (always positive).
    den: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

impl Rat {
    /// Construct and reduce.
    ///
    /// # Panics
    /// On a zero denominator (structural misuse, not data).
    #[must_use]
    pub fn new(num: i128, den: i128) -> Rat {
        assert!(den != 0, "Rat with zero denominator");
        let sign = if den < 0 { -1 } else { 1 };
        let g = gcd(num, den);
        Rat {
            num: sign * num / g,
            den: sign * den / g,
        }
    }

    /// From an integer.
    #[must_use]
    pub fn int(v: i64) -> Rat {
        Rat {
            num: i128::from(v),
            den: 1,
        }
    }

    /// Numerator (reduced).
    #[must_use]
    pub fn numerator(self) -> i128 {
        self.num
    }

    /// Denominator (reduced, positive).
    #[must_use]
    pub fn denominator(self) -> i128 {
        self.den
    }

    /// Nearest f64 (for reporting/plotting only — never for exactness).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    fn checked(num: Option<i128>, den: Option<i128>, op: &str) -> Rat {
        let (Some(n), Some(d)) = (num, den) else {
            panic!("Rat {op}: i128 overflow — exactness domain exceeded");
        };
        Rat::new(n, d)
    }
}

impl core::ops::Add for Rat {
    type Output = Rat;
    fn add(self, o: Rat) -> Rat {
        // a/b + c/d over lcm to keep magnitudes down.
        let g = gcd(self.den, o.den);
        let (db, dd) = (self.den / g, o.den / g);
        Rat::checked(
            self.num
                .checked_mul(dd)
                .and_then(|l| o.num.checked_mul(db).and_then(|r| l.checked_add(r))),
            self.den.checked_mul(dd),
            "add",
        )
    }
}

impl core::ops::Sub for Rat {
    type Output = Rat;
    fn sub(self, o: Rat) -> Rat {
        self + Rat::new(-o.num, o.den)
    }
}

impl core::ops::Mul for Rat {
    type Output = Rat;
    fn mul(self, o: Rat) -> Rat {
        // Cross-reduce before multiplying.
        let g1 = gcd(self.num, o.den);
        let g2 = gcd(o.num, self.den);
        Rat::checked(
            (self.num / g1).checked_mul(o.num / g2),
            (self.den / g2).checked_mul(o.den / g1),
            "mul",
        )
    }
}

impl core::ops::Div for Rat {
    type Output = Rat;
    fn div(self, o: Rat) -> Rat {
        assert!(o.num != 0, "Rat division by zero");
        self * Rat::new(o.den, o.num)
    }
}

impl core::ops::Neg for Rat {
    type Output = Rat;
    fn neg(self) -> Rat {
        Rat {
            num: -self.num,
            den: self.den,
        }
    }
}

impl PartialOrd for Rat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rat {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare a/b vs c/d via a*d vs c*b (denominators positive).
        let left = self.num.checked_mul(other.den);
        let right = other.num.checked_mul(self.den);
        if let (Some(l), Some(r)) = (left, right) {
            l.cmp(&r)
        } else {
            // Fall back to reduced-magnitude comparison via subtraction.
            let diff = *self - *other;
            diff.num.cmp(&0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_exact_and_reduced() {
        let a = Rat::new(1, 3);
        let b = Rat::new(1, 6);
        assert_eq!(a + b, Rat::new(1, 2));
        assert_eq!(a - b, Rat::new(1, 6));
        assert_eq!(a * b, Rat::new(1, 18));
        assert_eq!(a / b, Rat::int(2));
        assert_eq!(Rat::new(-2, -4), Rat::new(1, 2));
        assert_eq!(Rat::new(2, -4), Rat::new(-1, 2));
        assert!(Rat::new(1, 3) > Rat::new(1, 4));
    }
}
