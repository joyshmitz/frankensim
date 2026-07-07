//! The auto-derived INTERVAL evaluator: `Frep::interval(box)` returns a
//! range guaranteed to contain `f(p)` for every `p` in the box (the G0
//! containment law, frep-001). Per-node inclusion rules — exact for
//! spheres/half-spaces, conservative interval arithmetic elsewhere;
//! transforms map the box (a rotated box is covered by its corner AABB).
//! Booleans use monotonicity: `min`/`smin` are nondecreasing in both
//! arguments, so endpoint evaluation is an inclusion. A minimal local
//! interval kit is used on purpose; unification with fs-ivl's types (and
//! its tighter forms) is a contract no-claim.

use crate::{BoolStyle, Frep, Node, NodeId, bool_signs, corners, rotate_vec, smin};
use fs_geom::{Aabb, Point3, Vec3};

/// Closed interval `[lo, hi]`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Iv {
    pub lo: f64,
    pub hi: f64,
}

impl Iv {
    fn new(lo: f64, hi: f64) -> Iv {
        Iv { lo, hi }
    }

    fn add_c(self, c: f64) -> Iv {
        Iv::new(self.lo + c, self.hi + c)
    }

    fn neg(self) -> Iv {
        Iv::new(-self.hi, -self.lo)
    }

    fn scale_pos(self, s: f64) -> Iv {
        Iv::new(self.lo * s, self.hi * s)
    }

    fn abs(self) -> Iv {
        if self.lo >= 0.0 {
            self
        } else if self.hi <= 0.0 {
            self.neg()
        } else {
            Iv::new(0.0, self.hi.max(-self.lo))
        }
    }

    fn sq(self) -> Iv {
        let a = self.abs();
        Iv::new(a.lo * a.lo, a.hi * a.hi)
    }

    fn sqrt(self) -> Iv {
        Iv::new(self.lo.max(0.0).sqrt(), self.hi.max(0.0).sqrt())
    }

    fn max_c(self, c: f64) -> Iv {
        Iv::new(self.lo.max(c), self.hi.max(c))
    }

    fn min_c(self, c: f64) -> Iv {
        Iv::new(self.lo.min(c), self.hi.min(c))
    }

    fn max_iv(self, o: Iv) -> Iv {
        Iv::new(self.lo.max(o.lo), self.hi.max(o.hi))
    }

    fn min_iv(self, o: Iv) -> Iv {
        Iv::new(self.lo.min(o.lo), self.hi.min(o.hi))
    }

    /// `smin` is nondecreasing in both arguments (its partials are the
    /// convex weights), so endpoint evaluation is an inclusion.
    fn smin_iv(self, o: Iv, r: f64) -> Iv {
        Iv::new(smin(self.lo, o.lo, r), smin(self.hi, o.hi, r))
    }

    /// `hypot`-style √(a² + b²) inclusion.
    fn hypot_iv(self, o: Iv) -> Iv {
        let s = Iv::new(self.sq().lo + o.sq().lo, self.sq().hi + o.sq().hi);
        s.sqrt()
    }
}

/// Component intervals of `p − c` for `p` in the box.
fn delta_iv(b: &Aabb, c: Point3) -> [Iv; 3] {
    [
        Iv::new(b.min.x - c.x, b.max.x - c.x),
        Iv::new(b.min.y - c.y, b.max.y - c.y),
        Iv::new(b.min.z - c.z, b.max.z - c.z),
    ]
}

/// Exact `|p − c|` range over a box (nearest/farthest point distances).
fn dist_iv(b: &Aabb, c: Point3) -> Iv {
    let mut near2 = 0.0;
    let mut far2 = 0.0;
    for (lo, hi, cc) in [
        (b.min.x, b.max.x, c.x),
        (b.min.y, b.max.y, c.y),
        (b.min.z, b.max.z, c.z),
    ] {
        let below = (lo - cc).max(0.0);
        let above = (cc - hi).max(0.0);
        let near = below.max(above);
        near2 += near * near;
        let far = (cc - lo).abs().max((hi - cc).abs());
        far2 += far * far;
    }
    Iv::new(near2.sqrt(), far2.sqrt())
}

impl Frep {
    /// Range guaranteed to contain `f(p)` for all `p ∈ region`.
    #[must_use]
    pub fn interval(&self, region: &Aabb) -> (f64, f64) {
        let iv = self.iv_at(self.root(), region);
        (iv.lo, iv.hi)
    }

    fn iv_at(&self, id: NodeId, b: &Aabb) -> Iv {
        match self.nodes()[id.0 as usize] {
            Node::Sphere { center, radius } => dist_iv(b, center).add_c(-radius),
            Node::HalfSpace { normal, offset } => {
                let mut lo = -offset;
                let mut hi = -offset;
                for (n, bmin, bmax) in [
                    (normal.x, b.min.x, b.max.x),
                    (normal.y, b.min.y, b.max.y),
                    (normal.z, b.min.z, b.max.z),
                ] {
                    lo += (n * bmin).min(n * bmax);
                    hi += (n * bmin).max(n * bmax);
                }
                Iv::new(lo, hi)
            }
            Node::BoxPrim { center, half } => {
                let d = delta_iv(b, center);
                let q = [
                    d[0].abs().add_c(-half.x),
                    d[1].abs().add_c(-half.y),
                    d[2].abs().add_c(-half.z),
                ];
                let out = [q[0].max_c(0.0), q[1].max_c(0.0), q[2].max_c(0.0)];
                let norm = Iv::new(
                    out[0].sq().lo + out[1].sq().lo + out[2].sq().lo,
                    out[0].sq().hi + out[1].sq().hi + out[2].sq().hi,
                )
                .sqrt();
                let inner = q[0].max_iv(q[1]).max_iv(q[2]).min_c(0.0);
                Iv::new(norm.lo + inner.lo, norm.hi + inner.hi)
            }
            Node::Torus {
                center,
                major,
                minor,
            } => {
                let d = delta_iv(b, center);
                let ring = d[0].hypot_iv(d[1]).add_c(-major);
                ring.hypot_iv(d[2]).add_c(-minor)
            }
            Node::Cylinder { center, radius } => {
                let d = delta_iv(b, center);
                d[0].hypot_iv(d[1]).add_c(-radius)
            }
            Node::Translate { child, offset } => {
                let shifted = Aabb::new(
                    b.min.offset(Vec3::new(-offset.x, -offset.y, -offset.z)),
                    b.max.offset(Vec3::new(-offset.x, -offset.y, -offset.z)),
                );
                self.iv_at(child, &shifted)
            }
            Node::Rotate { child, axis, angle } => {
                // Query points map through R⁻¹; cover the rotated box by
                // its corner AABB (a superset — containment is preserved).
                let mut out: Option<Aabb> = None;
                for corner in corners(b) {
                    let v = rotate_vec(corner.delta_from(Point3::new(0.0, 0.0, 0.0)), axis, -angle);
                    let p = Point3::new(v.x, v.y, v.z);
                    let cell = Aabb::new(p, p);
                    out = Some(match out {
                        Some(acc) => acc.union(&cell),
                        None => cell,
                    });
                }
                self.iv_at(child, &out.expect("a box has corners"))
            }
            Node::Scale { child, factor } => {
                let shrunk = Aabb::new(
                    Point3::new(b.min.x / factor, b.min.y / factor, b.min.z / factor),
                    Point3::new(b.max.x / factor, b.max.y / factor, b.max.z / factor),
                );
                self.iv_at(child, &shrunk).scale_pos(factor)
            }
            Node::Offset { child, distance } => self.iv_at(child, b).add_c(-distance),
            Node::Bool {
                op,
                style,
                a,
                b: rhs,
            } => {
                let (sa, sb, sr) = bool_signs(op);
                let ia = if sa < 0.0 {
                    self.iv_at(a, b).neg()
                } else {
                    self.iv_at(a, b)
                };
                let ib = if sb < 0.0 {
                    self.iv_at(rhs, b).neg()
                } else {
                    self.iv_at(rhs, b)
                };
                let m = match style {
                    BoolStyle::Hard => ia.min_iv(ib),
                    BoolStyle::Blend { radius } => ia.smin_iv(ib, radius),
                };
                if sr < 0.0 { m.neg() } else { m }
            }
        }
    }
}
