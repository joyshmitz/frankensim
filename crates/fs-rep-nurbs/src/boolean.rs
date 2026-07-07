//! THE BOOLEAN POSITION (honest, per plan §7.2): watertight trimmed-NURBS
//! Booleans are the graveyard of CAD kernels. Boolean operations on
//! B-reps therefore ROUTE THROUGH F-rep/SDF charts BY DEFAULT (convert,
//! CSG in implicit form, re-fit splines when a spline chart is needed).
//! Direct B-rep Booleans are a certificate-gated capability: without a
//! sheaf watertightness certificate they REFUSE with teaching
//! diagnostics — an attempt, never a promise.

/// The requested set operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// A ∪ B.
    Union,
    /// A ∩ B.
    Intersect,
    /// A \ B.
    Subtract,
}

/// The policy under which a Boolean is attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BooleanPolicy {
    /// DEFAULT: convert both operands to SDF charts, perform the exact
    /// implicit CSG there, and re-fit a spline chart afterward if one is
    /// required (the Rep Router owns the conversions).
    #[default]
    RouteThroughSdf,
    /// Attempt a direct B-rep Boolean, gated on a sheaf watertightness
    /// certificate for the result. Refuses without one.
    DirectCertificateGated,
}

/// A structured, teaching refusal (this crate's v0 NEVER performs a
/// direct B-rep Boolean — the route is the product).
#[derive(Debug, Clone, PartialEq)]
pub struct BooleanRefusal {
    /// Which op was requested.
    pub op: BooleanOp,
    /// Which policy was in force.
    pub policy: BooleanPolicy,
    /// What to do instead, concretely.
    pub route: String,
    /// Teaching diagnostics.
    pub diagnostics: Vec<String>,
}

/// Request a Boolean between two spline charts. v0 always returns the
/// structured refusal that names the supported route — the honest
/// position, machine-readable.
#[must_use]
pub fn boolean(op: BooleanOp, policy: BooleanPolicy) -> BooleanRefusal {
    match policy {
        BooleanPolicy::RouteThroughSdf => BooleanRefusal {
            op,
            policy,
            route: "convert-nurbs-sdf (wqd.11) -> implicit CSG on F-rep charts -> \
                    convert-sdf-nurbs re-fit (wqd.12) when a spline chart is required"
                .to_string(),
            diagnostics: vec![
                "trimmed-NURBS Booleans are not watertight in general; the implicit route \
                 is exact on the CSG and carries a measured Hausdorff bound on the re-fit"
                    .to_string(),
            ],
        },
        BooleanPolicy::DirectCertificateGated => BooleanRefusal {
            op,
            policy,
            route: "attach a sheaf watertightness certificate (wqd.13) for the projected \
                    result, or fall back to BooleanPolicy::RouteThroughSdf"
                .to_string(),
            diagnostics: vec![
                "no watertightness certificate is available for the requested result".to_string(),
                "direct B-rep Booleans are certificate-gated by design (plan section 7.2): \
                 an attempt, never a promise"
                    .to_string(),
            ],
        },
    }
}
