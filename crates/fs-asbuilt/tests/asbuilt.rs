//! Battery for as-built ingestion (addendum Proposal 11). Covers rigid
//! registration recovery (exact + noisy), fiducial well-posedness (too-few,
//! collinear), the R8 signal-vs-noise gate, and the as-built δ retained as an
//! estimated candidate until calibration authority exists.

use fs_asbuilt::{
    Color, Fiducial, Point2, RegError, Registration, as_built_diff, register, well_posed,
};

/// Apply a ground-truth rigid transform to a design point (for building scans).
fn xform(p: Point2, theta: f64, tx: f64, ty: f64) -> Point2 {
    let (s, c) = theta.sin_cos();
    point(c * p.x() - s * p.y() + tx, s * p.x() + c * p.y() + ty)
}

fn point(x: f64, y: f64) -> Point2 {
    Point2::new(x, y).expect("finite test point")
}

fn registration(rotation: f64, tx: f64, ty: f64, residual: f64) -> Registration {
    Registration::new(rotation, tx, ty, residual).expect("valid test registration")
}

fn triangle() -> [Point2; 3] {
    [point(0.0, 0.0), point(2.0, 0.0), point(0.0, 2.0)]
}

#[test]
fn registration_recovers_a_known_rigid_transform() {
    let (theta, tx, ty) = (std::f64::consts::FRAC_PI_6, 5.0, 2.0); // 30 degrees
    let fids: Vec<Fiducial> = triangle()
        .iter()
        .map(|&d| Fiducial::new(d, xform(d, theta, tx, ty)))
        .collect();
    let reg = register(&fids).unwrap();
    assert!(
        (reg.rotation_rad() - theta).abs() < 1e-9,
        "theta {}",
        reg.rotation_rad()
    );
    assert!((reg.tx() - tx).abs() < 1e-9 && (reg.ty() - ty).abs() < 1e-9);
    assert!(reg.residual_rms() < 1e-9, "residual {}", reg.residual_rms());
    // and it maps a design point onto its scanned location.
    let p = point(1.0, 1.0);
    let mapped = reg.apply(p).expect("finite mapped point");
    let truth = xform(p, theta, tx, ty);
    assert!((mapped.x() - truth.x()).abs() < 1e-9 && (mapped.y() - truth.y()).abs() < 1e-9);
}

#[test]
fn noisy_measurements_carry_a_positive_residual() {
    let (theta, tx, ty) = (0.2, 1.0, -1.0);
    let noise = [(0.01, -0.02), (-0.015, 0.01), (0.02, 0.005)];
    let fids: Vec<Fiducial> = triangle()
        .iter()
        .zip(noise)
        .map(|(&d, (nx, ny))| {
            let m = xform(d, theta, tx, ty);
            Fiducial::new(d, point(m.x() + nx, m.y() + ny))
        })
        .collect();
    let reg = register(&fids).unwrap();
    // the registration error is carried forward, not discarded.
    assert!(reg.residual_rms() > 0.0 && reg.residual_rms() < 0.1);
}

#[test]
fn registration_is_ill_posed_without_enough_non_collinear_fiducials() {
    // too few.
    let two = [
        Fiducial::new(point(0.0, 0.0), point(1.0, 1.0)),
        Fiducial::new(point(1.0, 0.0), point(2.0, 1.0)),
    ];
    assert!(matches!(
        register(&two),
        Err(RegError::TooFewFiducials { have: 2, need: 3 })
    ));
    // collinear design points (all on the x-axis) are rank-deficient.
    let collinear: Vec<Fiducial> = [0.0, 1.0, 2.0]
        .iter()
        .map(|&x| Fiducial::new(point(x, 0.0), point(x + 0.3, 5.0)))
        .collect();
    assert_eq!(register(&collinear), Err(RegError::CollinearFiducials));
}

#[test]
fn registration_rank_gate_is_scale_invariant_for_small_geometry() {
    let scale = 1.0e-9;
    let design = [point(0.0, 0.0), point(scale, 0.0), point(0.0, scale)];
    let fiducials: Vec<_> = design
        .iter()
        .copied()
        .map(|datum| Fiducial::new(datum, datum))
        .collect();

    let registration = register(&fiducials).expect("small non-collinear triangle is rank two");
    assert_eq!(registration.rotation_rad().to_bits(), 0.0f64.to_bits());
    assert_eq!(registration.tx().to_bits(), 0.0f64.to_bits());
    assert_eq!(registration.ty().to_bits(), 0.0f64.to_bits());
    assert!(registration.residual_rms() <= f64::MIN_POSITIVE);
}

#[test]
fn the_r8_gate_rejects_registration_below_the_noise_floor() {
    // signal (certified deviation) 0.5 above the registration residual 0.01 -> ok.
    let sharp = registration(0.0, 0.0, 0.0, 0.01);
    assert!(well_posed(&sharp, 0.5));
    // registration residual 0.6 exceeds the 0.5 deviation being certified -> R8 kill.
    let blurry = registration(0.0, 0.0, 0.0, 0.6);
    assert!(!well_posed(&blurry, 0.5));
    // a non-positive certified deviation is never well-posed.
    assert!(!well_posed(&sharp, 0.0));
}

#[test]
fn the_as_built_diff_is_an_estimated_candidate_with_a_proposed_regime() {
    let reg = registration(0.0, 0.0, 0.0, 0.0);
    let design = vec![point(0.0, 0.0), point(1.0, 1.0)];
    let scanned = vec![point(0.0, 0.1), point(1.0, 1.0)];
    let diff = as_built_diff(&reg, &design, &scanned, 0.2, 0.05, "metrology-cal-2026").unwrap();
    assert!((diff.max_deviation() - 0.1).abs() < 1e-12);
    assert!(diff.within_tolerance()); // 0.1 <= 0.2
    assert!(diff.above_noise_floor()); // 0.1 > 0.05 (distinguishable from noise)
    assert_eq!(
        diff.proposed_regime().bound("measurement_noise"),
        Some((0.0, 0.05))
    );
    assert_eq!(
        diff.proposed_regime().bound("design_tolerance"),
        Some((0.0, 0.2))
    );
    match diff.color() {
        Color::Estimated {
            estimator,
            dispersion,
        } => {
            assert!(estimator.starts_with("asbuilt-diff-v2:"));
            assert_eq!(dispersion.to_bits(), 0.05f64.to_bits());
        }
        other => panic!("expected estimated candidate, got {other:?}"),
    }
}

#[test]
fn a_deviation_below_the_noise_floor_is_flagged() {
    let reg = registration(0.0, 0.0, 0.0, 0.0);
    let design = vec![point(0.0, 0.0), point(1.0, 1.0)];
    let scanned = vec![point(0.0, 0.01), point(1.0, 1.0)];
    // deviation 0.01 is below the 0.05 measurement noise floor.
    let diff = as_built_diff(&reg, &design, &scanned, 0.2, 0.05, "cal").unwrap();
    assert!(!diff.above_noise_floor());
}

#[test]
fn as_built_diff_rejects_malformed_input() {
    let reg = registration(0.0, 0.0, 0.0, 0.0);
    assert_eq!(
        as_built_diff(&reg, &[], &[], 0.1, 0.01, "c"),
        Err(RegError::Empty)
    );
    assert!(matches!(
        as_built_diff(&reg, &[point(0.0, 0.0)], &[], 0.1, 0.01, "c"),
        Err(RegError::LengthMismatch { .. })
    ));
}

#[test]
fn registration_is_deterministic() {
    let fids: Vec<Fiducial> = triangle()
        .iter()
        .map(|&d| Fiducial::new(d, xform(d, 0.3, 1.0, 2.0)))
        .collect();
    assert_eq!(register(&fids), register(&fids));
}

fn estimator_identity(diff: &fs_asbuilt::AsBuiltDiff) -> &str {
    match diff.color() {
        Color::Estimated { estimator, .. } => estimator,
        other => panic!("default diff must remain estimated, got {other:?}"),
    }
}

#[test]
fn public_geometry_and_registration_construction_refuse_non_finite_values() {
    for (x, y, field) in [
        (f64::NAN, 0.0, "point.x"),
        (f64::INFINITY, 0.0, "point.x"),
        (0.0, f64::NEG_INFINITY, "point.y"),
    ] {
        assert_eq!(Point2::new(x, y), Err(RegError::NonFinite { field }));
    }

    for (rotation, tx, ty, residual, field) in [
        (f64::NAN, 0.0, 0.0, 0.0, "registration.rotation_rad"),
        (0.0, f64::INFINITY, 0.0, 0.0, "registration.tx"),
        (0.0, 0.0, f64::NEG_INFINITY, 0.0, "registration.ty"),
        (0.0, 0.0, 0.0, f64::NAN, "registration.residual_rms"),
    ] {
        assert_eq!(
            Registration::new(rotation, tx, ty, residual),
            Err(RegError::NonFinite { field })
        );
    }
    assert_eq!(
        Registration::new(0.0, 0.0, 0.0, -0.01),
        Err(RegError::Negative {
            field: "registration.residual_rms"
        })
    );

    let overflowing = Registration::new(0.0, f64::MAX, 0.0, 0.0).unwrap();
    assert!(matches!(
        overflowing.apply(point(f64::MAX, 0.0)),
        Err(RegError::NonFinite { field: "point.x" })
    ));
}

#[test]
fn public_numeric_values_canonicalize_signed_zero() {
    let point = point(-0.0, -0.0);
    assert_eq!(point.x().to_bits(), 0.0f64.to_bits());
    assert_eq!(point.y().to_bits(), 0.0f64.to_bits());

    let registration = registration(-0.0, -0.0, -0.0, -0.0);
    assert_eq!(registration.rotation_rad().to_bits(), 0.0f64.to_bits());
    assert_eq!(registration.tx().to_bits(), 0.0f64.to_bits());
    assert_eq!(registration.ty().to_bits(), 0.0f64.to_bits());
    assert_eq!(registration.residual_rms().to_bits(), 0.0f64.to_bits());
}

#[test]
fn registration_rejects_finite_inputs_whose_intermediates_overflow() {
    let extreme = [
        Fiducial::new(point(f64::MAX, 0.0), point(0.0, 0.0)),
        Fiducial::new(point(f64::MAX, 1.0), point(1.0, 0.0)),
        Fiducial::new(point(-f64::MAX, 0.0), point(0.0, 1.0)),
    ];
    assert!(matches!(
        register(&extreme),
        Err(RegError::NonFinite { .. })
    ));
}

#[test]
fn diff_rejects_negative_or_non_finite_tolerance_and_noise() {
    let reg = registration(0.0, 0.0, 0.0, 0.0);
    let design = [point(0.0, 0.0)];
    let scanned = [point(0.0, 0.0)];
    for (tolerance, noise, expected) in [
        (
            f64::NAN,
            0.0,
            RegError::NonFinite {
                field: "design_tolerance",
            },
        ),
        (
            f64::INFINITY,
            0.0,
            RegError::NonFinite {
                field: "design_tolerance",
            },
        ),
        (
            -0.1,
            0.0,
            RegError::Negative {
                field: "design_tolerance",
            },
        ),
        (
            0.1,
            f64::NAN,
            RegError::NonFinite {
                field: "measurement_noise",
            },
        ),
        (
            0.1,
            f64::INFINITY,
            RegError::NonFinite {
                field: "measurement_noise",
            },
        ),
        (
            -0.0,
            -0.1,
            RegError::Negative {
                field: "measurement_noise",
            },
        ),
    ] {
        assert_eq!(
            as_built_diff(&reg, &design, &scanned, tolerance, noise, "cal-2026"),
            Err(expected)
        );
    }
}

#[test]
fn malformed_calibration_identities_are_refused_without_cloning_them() {
    let reg = registration(0.0, 0.0, 0.0, 0.0);
    let design = [point(0.0, 0.0)];
    let scanned = design;
    let too_long = "x".repeat(fs_evidence::MAX_COLOR_IDENTITY_BYTES + 1);
    for identity in [
        "",
        " leading",
        "trailing ",
        "unknown",
        "derived:v2:forged",
        "calibration|shell",
        too_long.as_str(),
    ] {
        assert!(matches!(
            as_built_diff(&reg, &design, &scanned, 0.1, 0.01, identity),
            Err(RegError::InvalidCalibrationIdentity { .. })
        ));
    }
}

#[test]
fn forged_calibration_text_cannot_promote_the_default_diff() {
    let reg = registration(0.0, 0.0, 0.0, 0.0);
    let design = [point(0.0, 0.0)];
    let diff = as_built_diff(
        &reg,
        &design,
        &design,
        0.1,
        0.01,
        "forged-calibration-claim",
    )
    .expect("well-formed text remains usable only as candidate provenance");
    assert!(matches!(diff.color(), Color::Estimated { .. }));
}

#[test]
fn estimator_identity_is_deterministic_bounded_and_delimiter_safe() {
    let reg = registration(0.1, 2.0, -3.0, 0.01);
    let design = [point(0.0, 0.0), point(1.0, 1.0)];
    let scanned = [reg.apply(design[0]).unwrap(), reg.apply(design[1]).unwrap()];
    let first = as_built_diff(&reg, &design, &scanned, 0.2, 0.05, "cal+a").unwrap();
    let replay = as_built_diff(&reg, &design, &scanned, 0.2, 0.05, "cal+a").unwrap();
    let delimiter_neighbor = as_built_diff(&reg, &design, &scanned, 0.2, 0.05, "cal").unwrap();
    assert_eq!(estimator_identity(&first), estimator_identity(&replay));
    assert_ne!(
        estimator_identity(&first),
        estimator_identity(&delimiter_neighbor)
    );
    assert!(estimator_identity(&first).len() <= fs_evidence::MAX_COLOR_IDENTITY_BYTES);
}

#[test]
fn estimator_identity_canonicalizes_signed_zero() {
    let positive_registration = registration(0.0, 0.0, 0.0, 0.0);
    let negative_registration = registration(-0.0, -0.0, -0.0, -0.0);
    let positive_points = [point(0.0, 0.0)];
    let negative_points = [point(-0.0, -0.0)];

    let positive = as_built_diff(
        &positive_registration,
        &positive_points,
        &positive_points,
        0.0,
        0.0,
        "cal-zero",
    )
    .unwrap();
    let negative = as_built_diff(
        &negative_registration,
        &negative_points,
        &negative_points,
        -0.0,
        -0.0,
        "cal-zero",
    )
    .unwrap();

    assert_eq!(estimator_identity(&positive), estimator_identity(&negative));
}
