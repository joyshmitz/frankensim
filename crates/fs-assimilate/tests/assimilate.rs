//! Battery for data assimilation (addendum Proposal 11). Covers the Kalman
//! fusion (mean shift + variance reduction), model-data misfit reduction, the
//! point-sensor (registration-free) vs scan (registration-laden) noise, the
//! order-independence of the linear-Gaussian posterior, the validated +
//! anchored colored posterior, and error paths.

use fs_assimilate::{
    AssimError, Belief, Color, assimilate, assimilate_all, assimilate_colored, misfit,
    point_sensor, scan_observation,
};

#[test]
fn a_measurement_shifts_the_mean_and_shrinks_the_variance() {
    let prior = Belief::scalar(0.0, 10.0);
    let obs = point_sensor(0, 1, 5.0, 1.0, "gauge-A");
    let post = assimilate(&prior, &obs).unwrap();
    // K = 10/11 -> mean ~ 4.545.
    assert!((post.mean[0] - 5.0 * 10.0 / 11.0).abs() < 1e-9);
    // posterior variance is below BOTH the prior (10) and the measurement (1).
    assert!(post.variance(0) < 1.0 && post.variance(0) < 10.0);
}

#[test]
fn assimilation_reduces_the_model_data_misfit() {
    let prior = Belief::diagonal(vec![0.0, 0.0], &[10.0, 10.0]);
    let obs = vec![
        point_sensor(0, 2, 5.0, 1.0, "gauge-x"),
        point_sensor(1, 2, -3.0, 1.0, "gauge-y"),
    ];
    let before = misfit(&prior, &obs);
    let post = assimilate_all(&prior, &obs).unwrap();
    let after = misfit(&post, &obs);
    assert!(after < before, "misfit {after} !< {before}");
    assert!(before > 30.0); // 25 + 9
}

#[test]
fn a_scan_observation_carries_more_noise_than_a_point_sensor() {
    // point sensors have NO registration problem (the R8 fallback).
    let point = point_sensor(0, 2, 1.0, 0.04, "thermocouple");
    let scan = scan_observation(vec![1.0, 0.0], 1.0, 0.04, 0.25, "CT-scan");
    assert!((point.noise_var - 0.04).abs() < 1e-12);
    // the scan's noise adds the registration variance on top.
    assert!((scan.noise_var - (0.04 + 0.25)).abs() < 1e-12);
    assert!(scan.noise_var > point.noise_var);
}

#[test]
fn the_linear_gaussian_posterior_is_order_independent() {
    let prior = Belief::scalar(0.0, 10.0);
    let o1 = point_sensor(0, 1, 5.0, 1.0, "a");
    let o2 = point_sensor(0, 1, 3.0, 2.0, "b");
    let fwd = assimilate_all(&prior, &[o1.clone(), o2.clone()]).unwrap();
    let rev = assimilate_all(&prior, &[o2, o1]).unwrap();
    assert!((fwd.mean[0] - rev.mean[0]).abs() < 1e-9);
    assert!((fwd.variance(0) - rev.variance(0)).abs() < 1e-9);
    // sequential fusion tightens: posterior variance below the single-obs one.
    assert!(
        fwd.variance(0)
            < assimilate(&prior, &point_sensor(0, 1, 5.0, 1.0, "a"))
                .unwrap()
                .variance(0)
    );
}

#[test]
fn the_assimilated_posterior_is_validated_and_anchored() {
    let prior = Belief::scalar(20.0, 4.0);
    let obs = vec![
        point_sensor(0, 1, 22.0, 0.25, "thermocouple-7"),
        point_sensor(0, 1, 21.5, 0.25, "thermocouple-7"),
    ];
    let out = assimilate_colored(&prior, &obs, "Re", 1e5, 3e5).unwrap();
    assert!(out.misfit_after <= out.misfit_before);
    match &out.color {
        Color::Validated { dataset, .. } => assert_eq!(dataset, "thermocouple-7"),
        other => panic!("expected validated, got {other:?}"),
    }
}

#[test]
fn assimilation_rejects_bad_input() {
    let prior = Belief::scalar(0.0, 1.0);
    // operator length != state dim.
    let bad_dim = point_sensor(0, 2, 1.0, 1.0, "x"); // dim-2 operator vs dim-1 state
    assert!(matches!(
        assimilate(&prior, &bad_dim),
        Err(AssimError::DimMismatch {
            state: 1,
            operator: 2
        })
    ));
    // non-positive noise.
    let mut bad_noise = point_sensor(0, 1, 1.0, 1.0, "x");
    bad_noise.noise_var = 0.0;
    assert_eq!(
        assimilate(&prior, &bad_noise),
        Err(AssimError::NonPositiveNoise)
    );
}

#[test]
fn assimilation_is_deterministic() {
    let prior = Belief::diagonal(vec![0.0, 0.0], &[5.0, 5.0]);
    let obs = vec![
        point_sensor(0, 2, 2.0, 0.5, "a"),
        point_sensor(1, 2, 1.0, 0.5, "b"),
    ];
    assert_eq!(assimilate_all(&prior, &obs), assimilate_all(&prior, &obs));
}
