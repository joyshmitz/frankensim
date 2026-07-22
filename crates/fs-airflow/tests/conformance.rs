//! G0/G3 conformance and evidence-boundary tests for the airflow rung.

use fs_airflow::{
    AirflowError, EnclosureNetwork, FanArrangement, FanBank, FanCurve, FanPoint, LeakageElement,
    LossElement, LossNetwork, LossResistance, SourceProvenance, ToleranceBasis,
    solve_operating_point,
};
use fs_evidence::NumericalKind;
use fs_qty::{Area, Density, DynViscosity, Length, Pressure, VolumetricFlowRate};

fn synthetic_source(id: &str) -> SourceProvenance {
    SourceProvenance::new(
        "Synthetic G0 fixture; not manufacturer performance data",
        id,
    )
}

fn fan_curve(stall_flow: f64) -> FanCurve {
    FanCurve::new(
        "synthetic-reference-fan",
        vec![
            FanPoint::new(VolumetricFlowRate::new(0.00), Pressure::new(160.0)),
            FanPoint::new(VolumetricFlowRate::new(0.04), Pressure::new(130.0)),
            FanPoint::new(VolumetricFlowRate::new(0.08), Pressure::new(70.0)),
            FanPoint::new(VolumetricFlowRate::new(0.12), Pressure::new(0.0)),
        ],
        synthetic_source("synthetic-fan-v1"),
        0.08,
        ToleranceBasis::EngineeringAllowance,
        VolumetricFlowRate::new(stall_flow),
        (0.7, 1.3),
    )
    .expect("valid synthetic fan")
}

fn loss(name: &str, resistance: f64, uncertainty: f64) -> LossElement {
    LossElement::new(
        name,
        LossResistance::new(resistance),
        uncertainty,
        synthetic_source(&format!("synthetic-loss-{name}")),
        ToleranceBasis::EngineeringAllowance,
    )
    .expect("valid synthetic loss")
}

fn network(leakage_resistance: f64) -> EnclosureNetwork {
    let intake = LossNetwork::parallel(vec![
        LossNetwork::Element(loss("left-vent", 45_000.0, 0.10)),
        LossNetwork::Element(loss("right-vent", 45_000.0, 0.10)),
    ])
    .expect("parallel vents");
    let primary = LossNetwork::series(vec![
        intake,
        LossNetwork::Element(loss("heatsink-channel", 30_000.0, 0.12)),
        LossNetwork::Element(loss("outlet", 12_000.0, 0.08)),
    ])
    .expect("series path");
    EnclosureNetwork::new(
        primary,
        LeakageElement::new(loss("case-leakage", leakage_resistance, 0.25)),
    )
}

#[test]
fn interpolation_and_monotone_admission_are_explicit() {
    let bank =
        FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, 1.0).expect("admissible bank");
    let pressure = bank
        .pressure_at(VolumetricFlowRate::new(0.06))
        .expect("inside curve")
        .value();
    assert!((pressure - 100.0).abs() < 1.0e-10, "{pressure}");

    let error = FanCurve::new(
        "bad",
        vec![
            FanPoint::new(VolumetricFlowRate::new(0.02), Pressure::new(80.0)),
            FanPoint::new(VolumetricFlowRate::new(0.01), Pressure::new(70.0)),
        ],
        synthetic_source("bad"),
        0.0,
        ToleranceBasis::Analytic,
        VolumetricFlowRate::new(0.01),
        (1.0, 1.0),
    )
    .expect_err("non-monotone data must refuse");
    assert!(matches!(error, AirflowError::NonMonotoneFlow { .. }));
}

#[test]
fn quadratic_series_and_parallel_composition_obey_g0_identities() {
    let a = LossNetwork::Element(loss("a", 100.0, 0.0));
    let b = LossNetwork::Element(loss("b", 100.0, 0.0));
    let series = LossNetwork::series(vec![a.clone(), b.clone()]).expect("series");
    let parallel = LossNetwork::parallel(vec![a, b]).expect("parallel");
    assert_eq!(series.equivalent_resistance().value(), 200.0);
    assert!((parallel.equivalent_resistance().value() - 25.0).abs() < 1.0e-12);
}

#[test]
fn identical_fans_obey_series_pressure_and_parallel_flow_laws() {
    let single = FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, 1.0).expect("single");
    let series = FanBank::new(fan_curve(0.01), 2, FanArrangement::Series, 1.0).expect("series");
    let parallel =
        FanBank::new(fan_curve(0.01), 2, FanArrangement::Parallel, 1.0).expect("parallel");
    let single_pressure = single
        .pressure_at(VolumetricFlowRate::new(0.06))
        .expect("single point")
        .value();
    let series_pressure = series
        .pressure_at(VolumetricFlowRate::new(0.06))
        .expect("series point")
        .value();
    let parallel_pressure = parallel
        .pressure_at(VolumetricFlowRate::new(0.12))
        .expect("parallel point")
        .value();
    assert!((series_pressure - 2.0 * single_pressure).abs() < 1.0e-9);
    assert!((parallel_pressure - single_pressure).abs() < 1.0e-9);
}

#[test]
fn operating_point_has_unique_sign_changing_nominal_bracket() {
    let fan = FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, 1.0).expect("bank");
    let system = network(180_000.0);
    let point = solve_operating_point(&fan, &system).expect("certified root");
    let bracket = point.nominal_root.flow;
    let resistance = system.equivalent_resistance().value();
    let residual = |q: f64| {
        fan.pressure_at(VolumetricFlowRate::new(q))
            .expect("bracket inside fan curve")
            .value()
            - resistance * q * q
    };
    assert!(residual(bracket.lo()) >= -1.0e-8);
    assert!(residual(bracket.hi()) <= 1.0e-8);
    assert_eq!(point.flow.numerical.kind, NumericalKind::Estimate);
    assert!(point.flow.numerical.lo <= point.flow.value.value());
    assert!(point.flow.value.value() <= point.flow.numerical.hi);
}

#[test]
fn declared_stall_region_refuses() {
    let fan = FanBank::new(fan_curve(0.07), 1, FanArrangement::Series, 1.0).expect("bank");
    let high_resistance_network = network(5.0e8);
    let error = solve_operating_point(&fan, &high_resistance_network)
        .expect_err("intersection below stall boundary must refuse");
    assert!(matches!(error, AirflowError::StallRegion { .. }), "{error}");
}

#[test]
fn three_speed_points_follow_affinity_and_solve_deterministically() {
    let system = network(180_000.0);
    let mut flows = Vec::new();
    for speed in [0.8, 1.0, 1.2] {
        let fan = FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, speed)
            .expect("speed inside declared range");
        flows.push(
            solve_operating_point(&fan, &system)
                .expect("operating point")
                .flow
                .value
                .value(),
        );
    }
    assert!(flows[0] < flows[1] && flows[1] < flows[2], "{flows:?}");
}

#[test]
fn leakage_sensitivity_and_branch_balance_are_visible() {
    let fan = FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, 1.0).expect("bank");
    let leaky = solve_operating_point(&fan, &network(80_000.0)).expect("leaky solve");
    let tight = solve_operating_point(&fan, &network(800_000.0)).expect("tight solve");
    assert!(leaky.leakage_fraction > tight.leakage_fraction);
    let branch_sum: f64 = tight
        .branches
        .iter()
        .filter(|branch| branch.path != "heatsink-channel" && branch.path != "outlet")
        .map(|branch| branch.flow.value.value())
        .sum();
    assert!((branch_sum - tight.flow.value.value()).abs() < 1.0e-10);
    assert!(tight.branches.iter().any(|branch| branch.leakage));
}

#[test]
fn branch_flow_hands_typed_velocity_and_reynolds_to_convection() {
    let fan = FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, 1.0).expect("bank");
    let point = solve_operating_point(&fan, &network(180_000.0)).expect("solve");
    let handoff = point
        .correlation_handoff(
            "heatsink-channel",
            Area::new(0.012),
            Density::new(1.18),
            DynViscosity::new(1.85e-5),
            Length::new(0.008),
            0.71,
        )
        .expect("typed handoff");
    let expected_re = 1.18 * handoff.velocity.value.value() * 0.008 / 1.85e-5;
    assert!((handoff.reynolds - expected_re).abs() < 1.0e-10);
    assert_eq!(handoff.velocity.numerical.kind, NumericalKind::Estimate);
    assert_eq!(handoff.velocity.model, handoff.branch_flow.model);
}

#[test]
fn operating_identity_binds_uncertainty_authority() {
    let fan = FanBank::new(fan_curve(0.01), 1, FanArrangement::Series, 1.0).expect("bank");
    let make_network = |uncertainty| {
        EnclosureNetwork::new(
            LossNetwork::Element(loss("primary", 55_000.0, uncertainty)),
            LeakageElement::new(loss("leakage", 180_000.0, 0.25)),
        )
    };
    let first = solve_operating_point(&fan, &make_network(0.05)).expect("first solve");
    let second = solve_operating_point(&fan, &make_network(0.20)).expect("second solve");

    assert_eq!(first.flow.value, second.flow.value);
    assert_ne!(first.flow.provenance, second.flow.provenance);
}
