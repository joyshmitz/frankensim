//! fs-rep-nurbs (plan §7.2): rational B-spline charts — EXACT spline
//! algebra (knot insertion and degree elevation in i128 rational
//! arithmetic, so "identical before/after refinement" is provable
//! equality, not tolerance), trimmed patches with CERTIFIED point
//! classification (convex-hull winding with exact subdivision), certified
//! closest-point brackets (branch-and-bound over exact hulls), and the
//! HONEST Boolean position: route through SDF by default, refuse direct
//! B-rep Booleans without a certificate.
//!
//! Layer: L2 (MORPH). Runtime deps: `std`, fs-ivl, fs-math.

pub mod basis;
pub mod boolean;
pub mod closest;
pub mod curve;
pub mod rat;
pub mod surface;
pub mod trim;

pub use basis::{KnotVector, Scalar};
pub use boolean::{BooleanOp, BooleanPolicy, BooleanRefusal, boolean};
pub use closest::{CertifiedDistance, closest_point_curve, closest_point_surface};
pub use curve::NurbsCurve;
pub use rat::Rat;
pub use surface::NurbsSurface;
pub use trim::{Classification, TrimLoop, TrimmedPatch};

use core::fmt;

/// Crate version (compile-time stamp).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Structured spline failures (Decalogue P10).
#[derive(Debug, Clone, PartialEq)]
pub enum NurbsError {
    /// A structurally invalid spline (knot ordering, count mismatches,
    /// non-positive weights).
    Structure {
        /// Diagnosis.
        what: String,
    },
    /// A parameter outside the knot domain.
    Domain {
        /// Diagnosis.
        what: String,
    },
    /// The exact-arithmetic domain was exceeded (i128 overflow).
    Exactness {
        /// Diagnosis.
        what: String,
    },
}

impl fmt::Display for NurbsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NurbsError::Structure { what } => write!(f, "invalid spline: {what}"),
            NurbsError::Domain { what } => write!(f, "parameter out of domain: {what}"),
            NurbsError::Exactness { what } => write!(f, "exact-arithmetic overflow: {what}"),
        }
    }
}

impl std::error::Error for NurbsError {}
