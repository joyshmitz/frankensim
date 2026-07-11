//! fs-asbuilt — reality is just another chart (plan addendum, Proposal 11).
//! Layer: L2.
//!
//! A CT scan (or laser point cloud) of the manufactured part is one more
//! REPRESENTATION with its own restriction maps, so "as-built vs as-designed"
//! becomes a δ between two sections — computed by the same watertightness
//! machinery, closing the loop through the physical world. The "validated"
//! color stops being a static stamp and becomes a living, regime-tagged belief.
//!
//! Two facts make this honest:
//! - REGISTRATION (aligning scan to design) is an OPTIMIZATION WITH ERROR, so
//!   its error is carried forward, not discarded. [`register`] solves the rigid
//!   2-D fit in closed form (no SVD) and is made WELL-POSED by fiducials/datums
//!   specified at design time (≥ 3 non-collinear points) — the
//!   design-for-verification requirement pushed upstream.
//! - The R8 kill criterion is explicit: if registration uncertainty exceeds the
//!   geometric deviation being certified, the signal is below the noise floor
//!   ([`well_posed`]).
//!
//! The as-built δ ([`as_built_diff`]) is measurement-noise-aware and emits
//! an **estimated candidate** with a proposed regime. A caller-supplied
//! calibration identity is provenance, not authority: this crate exposes no
//! validated-promotion API until an authenticated verifier and retained
//! calibration artifact are available. Deterministic; pure Rust.

use fs_evidence::color_leaf_identity_reason;
pub use fs_evidence::{Color, ValidityDomain};

const AS_BUILT_ESTIMATOR_DOMAIN: &str = "org.frankensim.fs-asbuilt.diff-estimator.v2";
const AS_BUILT_ESTIMATOR_SCHEMA: &[u8] = b"fs-asbuilt-diff-estimator-v2";
/// Maximum points accepted by registration or one as-built comparison.
pub const MAX_AS_BUILT_POINTS: usize = 1_000_000;

/// A 2-D point (design or measured coordinate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2 {
    /// x coordinate.
    x: f64,
    /// y coordinate.
    y: f64,
}

impl Point2 {
    /// Construct a finite point.
    ///
    /// # Errors
    /// Refuses NaN or infinite coordinates.
    pub fn new(x: f64, y: f64) -> Result<Point2, RegError> {
        require_finite("point.x", x)?;
        require_finite("point.y", y)?;
        Ok(Point2 {
            x: canonical_zero(x),
            y: canonical_zero(y),
        })
    }

    /// x coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// y coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    fn dist(self, other: Point2) -> Result<f64, RegError> {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let distance = dx.hypot(dy);
        require_finite("point distance", distance)?;
        Ok(distance)
    }
}

/// A fiducial/datum correspondence: a design reference point and where the scan
/// measured it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fiducial {
    /// The design-time reference location.
    design: Point2,
    /// The location the scan measured for it.
    measured: Point2,
}

impl Fiducial {
    /// A fiducial correspondence.
    #[must_use]
    pub fn new(design: Point2, measured: Point2) -> Fiducial {
        Fiducial { design, measured }
    }

    /// Design-time reference location.
    #[must_use]
    pub const fn design(self) -> Point2 {
        self.design
    }

    /// Measured location.
    #[must_use]
    pub const fn measured(self) -> Point2 {
        self.measured
    }
}

/// A structured registration/ingestion failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegError {
    /// Fewer fiducials than needed for a well-posed fit.
    TooFewFiducials {
        /// Supplied.
        have: usize,
        /// Required.
        need: usize,
    },
    /// The fiducials are (near-)collinear — the fit is ill-posed.
    CollinearFiducials,
    /// Two point sets have mismatched lengths.
    LengthMismatch {
        /// Expected.
        expected: usize,
        /// Found.
        found: usize,
    },
    /// An empty point set.
    Empty,
    /// A resource-driving point set exceeds the public bound.
    TooManyPoints {
        /// Supplied point count.
        have: usize,
        /// Maximum accepted point count.
        max: usize,
    },
    /// A public numeric input is NaN or infinite.
    NonFinite {
        /// Stable field or computation name.
        field: &'static str,
    },
    /// A quantity that must be non-negative was negative.
    Negative {
        /// Stable field name.
        field: &'static str,
    },
    /// A calibration candidate identity is not an admissible provenance leaf.
    InvalidCalibrationIdentity {
        /// Stable structural reason from the shared evidence grammar.
        reason: &'static str,
        /// Input byte length, retained without cloning hostile input.
        bytes: usize,
    },
    /// The bounded deviations vector could not reserve memory.
    AllocationFailed,
    /// A canonical identity field length could not be represented as `u64`.
    IdentityEncodingOverflow,
}

impl core::fmt::Display for RegError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooFewFiducials { have, need } => {
                write!(formatter, "need at least {need} fiducials, got {have}")
            }
            Self::CollinearFiducials => formatter.write_str("fiducials are collinear"),
            Self::LengthMismatch { expected, found } => {
                write!(formatter, "expected {expected} scanned points, got {found}")
            }
            Self::Empty => formatter.write_str("point set is empty"),
            Self::TooManyPoints { have, max } => {
                write!(formatter, "point count {have} exceeds bound {max}")
            }
            Self::NonFinite { field } => write!(formatter, "{field} must be finite"),
            Self::Negative { field } => write!(formatter, "{field} must be non-negative"),
            Self::InvalidCalibrationIdentity { reason, bytes } => write!(
                formatter,
                "calibration candidate identity is invalid ({reason}, {bytes} bytes)"
            ),
            Self::AllocationFailed => {
                formatter.write_str("could not reserve the bounded deviations vector")
            }
            Self::IdentityEncodingOverflow => {
                formatter.write_str("canonical identity field length exceeds u64")
            }
        }
    }
}

impl std::error::Error for RegError {}

fn require_finite(field: &'static str, value: f64) -> Result<(), RegError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(RegError::NonFinite { field })
    }
}

fn require_non_negative(field: &'static str, value: f64) -> Result<(), RegError> {
    require_finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(RegError::Negative { field })
    }
}

const fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

/// A rigid registration (rotation + translation) mapping design → measured,
/// with the residual it carries forward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Registration {
    /// Rotation angle (radians).
    rotation_rad: f64,
    /// Translation x.
    tx: f64,
    /// Translation y.
    ty: f64,
    /// Root-mean-square residual of the fit (the registration uncertainty).
    residual_rms: f64,
}

impl Registration {
    /// Construct a finite rigid registration with a non-negative residual.
    ///
    /// # Errors
    /// Refuses non-finite transform components or a negative residual.
    pub fn new(rotation_rad: f64, tx: f64, ty: f64, residual_rms: f64) -> Result<Self, RegError> {
        require_finite("registration.rotation_rad", rotation_rad)?;
        require_finite("registration.tx", tx)?;
        require_finite("registration.ty", ty)?;
        require_non_negative("registration.residual_rms", residual_rms)?;
        Ok(Self {
            rotation_rad: canonical_zero(rotation_rad),
            tx: canonical_zero(tx),
            ty: canonical_zero(ty),
            residual_rms: canonical_zero(residual_rms),
        })
    }

    /// Rotation angle in radians.
    #[must_use]
    pub const fn rotation_rad(&self) -> f64 {
        self.rotation_rad
    }

    /// x translation.
    #[must_use]
    pub const fn tx(&self) -> f64 {
        self.tx
    }

    /// y translation.
    #[must_use]
    pub const fn ty(&self) -> f64 {
        self.ty
    }

    /// Registration residual RMS.
    #[must_use]
    pub const fn residual_rms(&self) -> f64 {
        self.residual_rms
    }

    /// Map a design point into measured coordinates.
    ///
    /// # Errors
    /// Refuses arithmetic overflow to a non-finite mapped point.
    pub fn apply(&self, point: Point2) -> Result<Point2, RegError> {
        let (s, c) = self.rotation_rad.sin_cos();
        Point2::new(
            c * point.x - s * point.y + self.tx,
            s * point.x + c * point.y + self.ty,
        )
    }
}

/// The minimum fiducials for a well-posed 2-D rigid fit.
pub const MIN_FIDUCIALS: usize = 3;

/// Solve the rigid 2-D registration that best maps `fiducials`' design points
/// onto their measured points (closed-form least squares — the 2-D Umeyama /
/// Procrustes rotation). Requires ≥ 3 non-collinear fiducials.
///
/// # Errors
/// [`RegError::TooFewFiducials`] or [`RegError::CollinearFiducials`].
pub fn register(fiducials: &[Fiducial]) -> Result<Registration, RegError> {
    let n = fiducials.len();
    if n < MIN_FIDUCIALS {
        return Err(RegError::TooFewFiducials {
            have: n,
            need: MIN_FIDUCIALS,
        });
    }
    if n > MAX_AS_BUILT_POINTS {
        return Err(RegError::TooManyPoints {
            have: n,
            max: MAX_AS_BUILT_POINTS,
        });
    }
    let nf = f64::from(u32::try_from(n).map_err(|_| RegError::TooManyPoints {
        have: n,
        max: MAX_AS_BUILT_POINTS,
    })?);
    let cp = centroid(fiducials.iter().map(|f| f.design))?;
    let cq = centroid(fiducials.iter().map(|f| f.measured))?;

    // scatter of the centered DESIGN points — collinear iff it is rank-deficient.
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    // cross-covariance terms for the optimal rotation.
    let (mut s_dot, mut s_cross) = (0.0, 0.0);
    for f in fiducials {
        let (dpx, dpy) = (f.design.x - cp.x, f.design.y - cp.y);
        let (dqx, dqy) = (f.measured.x - cq.x, f.measured.y - cq.y);
        sxx += dpx * dpx;
        syy += dpy * dpy;
        sxy += dpx * dpy;
        s_dot += dpx * dqx + dpy * dqy;
        s_cross += dpx * dqy - dpy * dqx;
    }
    for (field, value) in [
        ("registration design scatter xx", sxx),
        ("registration design scatter yy", syy),
        ("registration design scatter xy", sxy),
        ("registration cross-covariance dot", s_dot),
        ("registration cross-covariance cross", s_cross),
    ] {
        require_finite(field, value)?;
    }
    let det = sxx * syy - sxy * sxy;
    let trace = sxx + syy;
    require_finite("registration scatter determinant", det)?;
    require_finite("registration scatter trace", trace)?;
    let trace_squared = trace * trace;
    require_finite("registration squared scatter trace", trace_squared)?;
    if trace <= 0.0 || det <= 1e-12 * trace_squared {
        return Err(RegError::CollinearFiducials);
    }

    let rotation_rad = s_cross.atan2(s_dot);
    let (s, c) = rotation_rad.sin_cos();
    let tx = cq.x - (c * cp.x - s * cp.y);
    let ty = cq.y - (s * cp.x + c * cp.y);
    let reg = Registration::new(rotation_rad, tx, ty, 0.0)?;
    // residual RMS = the carried-forward registration uncertainty.
    let mut ss = 0.0;
    for fiducial in fiducials {
        let distance = reg.apply(fiducial.design)?.dist(fiducial.measured)?;
        ss += distance * distance;
        require_finite("registration residual sum of squares", ss)?;
    }
    Registration::new(rotation_rad, tx, ty, (ss / nf).sqrt())
}

fn centroid(points: impl Iterator<Item = Point2>) -> Result<Point2, RegError> {
    let mut n = 0.0;
    let (mut sx, mut sy) = (0.0, 0.0);
    for point in points {
        sx += point.x;
        sy += point.y;
        n += 1.0;
        require_finite("point centroid x sum", sx)?;
        require_finite("point centroid y sum", sy)?;
    }
    Point2::new(sx / n, sy / n)
}

/// The R8 well-posedness gate: registration is trustworthy only when its
/// uncertainty (`residual_rms`) is BELOW the geometric deviation being
/// certified. If the residual meets or exceeds the signal, the as-built loop is
/// premature for that part class (defer to point-sensor assimilation).
#[must_use]
pub fn well_posed(reg: &Registration, certified_deviation: f64) -> bool {
    certified_deviation.is_finite()
        && certified_deviation > 0.0
        && reg.residual_rms < certified_deviation
}

/// The as-built δ between design and scanned sections.
#[derive(Debug, Clone, PartialEq)]
pub struct AsBuiltDiff {
    /// Per-point deviation `||registered(design) − scanned||`.
    deviations: Vec<f64>,
    /// The largest deviation.
    max_deviation: f64,
    /// Is the whole part within the design tolerance?
    within_tolerance: bool,
    /// Is the max deviation ABOVE the measurement noise floor (distinguishable
    /// from noise)?
    above_noise_floor: bool,
    /// Proposed regime for later calibration-authority review.
    proposed_regime: ValidityDomain,
    /// The δ's honest candidate color. This API never emits `Validated`.
    color: Color,
}

impl AsBuiltDiff {
    /// Per-point deviations in the input order.
    #[must_use]
    pub fn deviations(&self) -> &[f64] {
        &self.deviations
    }

    /// Largest point deviation.
    #[must_use]
    pub const fn max_deviation(&self) -> f64 {
        self.max_deviation
    }

    /// Whether every deviation fits the supplied design tolerance.
    #[must_use]
    pub const fn within_tolerance(&self) -> bool {
        self.within_tolerance
    }

    /// Whether the largest deviation exceeds the supplied noise floor.
    #[must_use]
    pub const fn above_noise_floor(&self) -> bool {
        self.above_noise_floor
    }

    /// Proposed, unauthenticated validity regime for later review.
    #[must_use]
    pub const fn proposed_regime(&self) -> &ValidityDomain {
        &self.proposed_regime
    }

    /// Honest candidate color produced by [`as_built_diff`].
    #[must_use]
    pub const fn color(&self) -> &Color {
        &self.color
    }
}

/// Compute the as-built δ after registration: apply the registration to each
/// design point and measure its deviation from the corresponding scanned point.
/// The δ is colored ESTIMATED. Its bounded, domain-separated identity binds
/// every point, registration component, tolerance, noise value, and the
/// structurally valid calibration candidate identity. The proposed regime is
/// carried separately for later authenticated calibration review.
///
/// # Errors
/// Refuses empty/mismatched/oversized point sets, malformed calibration
/// identities, negative or non-finite tolerances/noise, and non-finite
/// arithmetic results.
pub fn as_built_diff(
    reg: &Registration,
    design: &[Point2],
    scanned: &[Point2],
    design_tolerance: f64,
    measurement_noise: f64,
    calibration_candidate: &str,
) -> Result<AsBuiltDiff, RegError> {
    if design.is_empty() {
        return Err(RegError::Empty);
    }
    if design.len() != scanned.len() {
        return Err(RegError::LengthMismatch {
            expected: design.len(),
            found: scanned.len(),
        });
    }
    if design.len() > MAX_AS_BUILT_POINTS {
        return Err(RegError::TooManyPoints {
            have: design.len(),
            max: MAX_AS_BUILT_POINTS,
        });
    }
    require_non_negative("design_tolerance", design_tolerance)?;
    require_non_negative("measurement_noise", measurement_noise)?;
    if let Some(reason) = color_leaf_identity_reason(calibration_candidate) {
        return Err(RegError::InvalidCalibrationIdentity {
            reason,
            bytes: calibration_candidate.len(),
        });
    }

    let mut deviations = Vec::new();
    deviations
        .try_reserve_exact(design.len())
        .map_err(|_| RegError::AllocationFailed)?;
    for (design_point, scanned_point) in design.iter().zip(scanned) {
        deviations.push(reg.apply(*design_point)?.dist(*scanned_point)?);
    }
    let max_deviation = deviations.iter().copied().fold(0.0_f64, f64::max);
    require_finite("maximum as-built deviation", max_deviation)?;
    let proposed_regime = ValidityDomain::unconstrained()
        .with(
            "registration_residual",
            0.0,
            reg.residual_rms.max(f64::MIN_POSITIVE),
        )
        .with(
            "measurement_noise",
            0.0,
            measurement_noise.max(f64::MIN_POSITIVE),
        )
        .with(
            "design_tolerance",
            0.0,
            design_tolerance.max(f64::MIN_POSITIVE),
        );
    let dispersion = reg.residual_rms.hypot(measurement_noise);
    require_finite("combined as-built dispersion", dispersion)?;
    let estimator = estimator_identity(
        reg,
        design,
        scanned,
        design_tolerance,
        measurement_noise,
        calibration_candidate,
    )?;
    Ok(AsBuiltDiff {
        deviations,
        max_deviation,
        within_tolerance: max_deviation <= design_tolerance,
        above_noise_floor: max_deviation > measurement_noise,
        proposed_regime,
        color: Color::Estimated {
            estimator,
            dispersion,
        },
    })
}

fn estimator_identity(
    registration: &Registration,
    design: &[Point2],
    scanned: &[Point2],
    design_tolerance: f64,
    measurement_noise: f64,
    calibration_candidate: &str,
) -> Result<String, RegError> {
    fn field(hasher: &mut fs_blake3::Blake3, bytes: &[u8]) -> Result<(), RegError> {
        let length = u64::try_from(bytes.len()).map_err(|_| RegError::IdentityEncodingOverflow)?;
        hasher.update(&length.to_le_bytes());
        hasher.update(bytes);
        Ok(())
    }

    fn number(hasher: &mut fs_blake3::Blake3, label: &[u8], value: f64) -> Result<(), RegError> {
        field(hasher, label)?;
        field(hasher, &canonical_zero(value).to_bits().to_le_bytes())
    }

    let mut hasher = fs_blake3::Blake3::new();
    field(&mut hasher, AS_BUILT_ESTIMATOR_SCHEMA)?;
    field(&mut hasher, b"calibration-candidate")?;
    field(&mut hasher, calibration_candidate.as_bytes())?;
    number(
        &mut hasher,
        b"registration.rotation_rad",
        registration.rotation_rad,
    )?;
    number(&mut hasher, b"registration.tx", registration.tx)?;
    number(&mut hasher, b"registration.ty", registration.ty)?;
    number(
        &mut hasher,
        b"registration.residual_rms",
        registration.residual_rms,
    )?;
    number(&mut hasher, b"design_tolerance", design_tolerance)?;
    number(&mut hasher, b"measurement_noise", measurement_noise)?;
    field(&mut hasher, b"point-count")?;
    let point_count =
        u64::try_from(design.len()).map_err(|_| RegError::IdentityEncodingOverflow)?;
    field(&mut hasher, &point_count.to_le_bytes())?;
    for (ordinal, (design_point, scanned_point)) in design.iter().zip(scanned).enumerate() {
        field(&mut hasher, b"point-pair")?;
        let ordinal = u64::try_from(ordinal).map_err(|_| RegError::IdentityEncodingOverflow)?;
        field(&mut hasher, &ordinal.to_le_bytes())?;
        number(&mut hasher, b"design.x", design_point.x)?;
        number(&mut hasher, b"design.y", design_point.y)?;
        number(&mut hasher, b"scanned.x", scanned_point.x)?;
        number(&mut hasher, b"scanned.y", scanned_point.y)?;
    }
    let preimage_hash = hasher.finalize();
    Ok(format!(
        "asbuilt-diff-v2:{}",
        fs_blake3::hash_domain(AS_BUILT_ESTIMATOR_DOMAIN, preimage_hash.as_bytes())
    ))
}
