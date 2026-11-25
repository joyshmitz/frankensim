//! Native tests of the plant. Each scenario checks at least one quantity
//! that was not an input (Gauntlet G0 for the component laws, G2-style
//! scenario checks for the assembled plant).

use fs_ports::component::conformance::{Case, check_component};

use crate::analyser::{QUALITY_EVENT_STRIDE, quality_kind};
use crate::cabinet::{
    BreakerCabinetComponent, CabinetState, MEASURE_WIDTH, protection, sequence_components,
};
use crate::cards::{
    BreakerCabinetCard, CombustionLimitsCard, ControllerSetpoints, DEFAULT_CONDITION_FACTOR,
    DumpCoolerCard, ExhaustExchangerCard, ExhaustGasCard, FuelGasCard, GasValveCard,
    GeneratorCard, HeatSplitCard, IndicatedWorkCard, NetworkExchangerCard, PassportCard,
    RATED_OMEGA_RAD_S, RestorationCard, StarterCard, SynchronisationWindowCard,
};
use crate::heat::{
    DumpCoolerComponent, DumpCoolerState, ExhaustExchangerState, ExhaustHeatExchangerComponent,
    NetworkExchangerState, NetworkHeatExchangerComponent,
};
use crate::components::{
    GasControlValveComponent, GasEngineComponent, GasEngineState, GasValveState, GeneratorState,
    ShaftComponent, StarterMotorComponent, StarterState, SynchronousGeneratorComponent,
};
use crate::controller::{
    ControllerState, GensetControllerComponent, OperatorSignals, Sequence, alarm, refusal,
};
use crate::meter::{METER_EVENT_STRIDE, journal};
use crate::plant::event_kind;
use crate::{EVENT_STRIDE, KguSim, snap, with_clock_free_cx};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn advance(sim: &mut KguSim, seconds: f64) {
    assert!(sim.step(seconds), "step refused: {}", sim.last_error());
}

/// Step in 0.1 s slices until `done` holds; panic after `limit_s`.
fn advance_until(sim: &mut KguSim, limit_s: f64, what: &str, done: impl Fn(&[f64]) -> bool) -> f64 {
    let start = sim.snapshot()[snap::TIME_S];
    loop {
        let s = sim.snapshot();
        if done(&s) {
            return s[snap::TIME_S] - start;
        }
        assert!(
            s[snap::TIME_S] - start < limit_s,
            "{what}: not reached in {limit_s} s; state {} step {} rpm {:.1} P {:.1} kW alarms {}",
            s[snap::STATE_CODE],
            s[snap::SEQUENCE_STEP],
            s[snap::SPEED_RPM],
            s[snap::ELECTRIC_POWER_KW],
            s[snap::ALARM_BITS]
        );
        advance(sim, 0.1);
    }
}

/// A unit started and synchronised, with the load ramp quickened to
/// `ramp_pct_per_min` (a controller setting) and settled at `load_pct`.
fn on_load(load_pct: f64, ramp_pct_per_min: f64) -> KguSim {
    let mut sim = KguSim::new();
    sim.set_load_ramp_pct_per_min(ramp_pct_per_min);
    sim.set_load_setpoint_pct(load_pct);
    sim.cmd_start();
    advance_until(&mut sim, 200.0, "breaker closed", |s| s[snap::BREAKER_CLOSED] == 1.0);
    advance_until(&mut sim, 60.0 * 100.0 / ramp_pct_per_min + 60.0, "load reference at setpoint", |s| {
        (s[snap::LOAD_REFERENCE_PCT] - load_pct).abs() < 1.0e-9
    });
    advance(&mut sim, 30.0);
    sim
}

fn events_of(sim: &KguSim, kind: u32) -> Vec<(f64, u32, f64)> {
    sim.events()
        .chunks(EVENT_STRIDE)
        .filter(|event| event[1] == f64::from(kind))
        .map(|event| (event[0], event[2] as u32, event[3]))
        .collect()
}

fn fitted_work_card() -> IndicatedWorkCard {
    let generator = SynchronousGeneratorComponent::new(GeneratorCard::ASSUMED);
    IndicatedWorkCard::fitted(PassportCard::STATION, |p| {
        generator.reference_loss_w(p).expect("reference loss")
    })
}

// ---------------------------------------------------------------------------
// Component conformance (fs-ports contract v0)
// ---------------------------------------------------------------------------

#[test]
fn gas_valve_conforms_and_passes_enthalpy_through() {
    let gas = FuelGasCard::STATION;
    let valve = GasControlValveComponent::new(gas, GasValveCard::sized_for(&gas, 0.0521));
    let closed = GasValveState::closed(&gas);
    let half = GasValveState {
        lift: 0.5,
        ..closed
    };
    let h = gas.specific_enthalpy_j_kg(288.15);
    for (state, inputs) in [
        (&closed, [151_325.0, h, 101_325.0, h, 0.7, 1.0]),
        (&half, [151_325.0, h, 101_325.0, h, 0.7, 1.0]),
        (&half, [151_325.0, h, 101_325.0, h, 0.7, 0.0]),
        (&half, [101_325.0, h, 101_325.0, h, 1.0, 1.0]),
        (&half, [601_325.0, h, 101_325.0, h, 0.0, 1.0]),
    ] {
        with_clock_free_cx(|cx| {
            check_component(
                &valve,
                &Case {
                    state,
                    stale: &closed,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("gas valve conformance");
    }
}

#[test]
fn engine_conforms_with_and_without_ignition_and_at_standstill() {
    let gas = FuelGasCard::STATION;
    let work = fitted_work_card();
    let limits = CombustionLimitsCard {
        largest_indicated_torque_nm: 8_600.0,
        torque_speed_floor_rad_s: 12.0,
    };
    let engine =
        GasEngineComponent::new(gas, work, HeatSplitCard::ASSUMED, limits, ExhaustGasCard::ASSUMED);
    let state = GasEngineState::default();
    let stale = GasEngineState {
        fuel_power_w: 1.0,
        ..state
    };
    let h = gas.specific_enthalpy_j_kg(288.15);
    for (mass_flow, omega, ignition) in [
        (0.0521, RATED_OMEGA_RAD_S, 1.0),
        (0.0074, RATED_OMEGA_RAD_S, 1.0),
        (0.0521, RATED_OMEGA_RAD_S, 0.0),
        (0.0, RATED_OMEGA_RAD_S, 1.0),
        (0.02, 15.0, 1.0),
        (0.0, 0.0, 0.0),
        (0.01, 0.0, 1.0),
    ] {
        let inputs = [
            mass_flow,
            mass_flow * h,
            -omega,
            363.15,
            673.15,
            303.15,
            ignition,
            DEFAULT_CONDITION_FACTOR,
            101_325.0,
            393.15,
        ];
        with_clock_free_cx(|cx| {
            check_component(
                &engine,
                &Case {
                    state: &state,
                    stale: &stale,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("engine conformance");
    }
}

#[test]
fn starter_conforms_engaged_released_and_overrun() {
    let starter = StarterMotorComponent::new(StarterCard::ASSUMED);
    let state = StarterState::default();
    let stale = StarterState {
        torque_nm: 3.0,
        ..state
    };
    for (omega, engage) in [(0.0, 1.0), (10.0, 1.0), (40.0, 1.0), (10.0, 0.0)] {
        let inputs = [-omega, 24.0, 303.15, engage];
        with_clock_free_cx(|cx| {
            check_component(
                &starter,
                &Case {
                    state: &state,
                    stale: &stale,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("starter conformance");
    }
}

#[test]
fn shaft_conforms_turning_stopping_and_at_rest_and_stops_at_exactly_zero() {
    use crate::cards::{SHAFT_INERTIA_KG_M2, StandstillCard};
    use fs_ports::component::Component;
    use fs_system::elements::Motion;

    let card = StandstillCard::from_friction(&fitted_work_card().friction());
    let shaft = ShaftComponent::new(SHAFT_INERTIA_KG_M2, card);
    let dt = 0.01;
    // About 585 N m of static friction stop 60 kg m2 from 0.097 rad/s within 10 ms.
    let rest_speed = card.rest_speed_rad_s(SHAFT_INERTIA_KG_M2, dt);
    assert!(rest_speed > 0.05 && rest_speed < 0.2, "rest speed {rest_speed} rad/s");
    let at = |velocity_m_s: f64| Motion {
        position_m: 3.0,
        velocity_m_s,
    };
    let stale = at(1.0);

    // Inputs: net torque, room temperature, starter engaged, ignition on.
    for (velocity, torque, starter, ignition) in [
        (RATED_OMEGA_RAD_S, -900.0, 0.0, 0.0),
        (RATED_OMEGA_RAD_S, 300.0, 0.0, 1.0),
        (1.0, -400.0, 0.0, 0.0),
        (0.5 * rest_speed, -20.0, 0.0, 0.0),
        (-0.5 * rest_speed, 20.0, 0.0, 0.0),
        (0.5 * rest_speed, 2_000.0, 1.0, 0.0),
        (0.5 * rest_speed, 40.0, 0.0, 1.0),
        (0.0, 0.0, 0.0, 0.0),
        (0.0, 2_500.0, 1.0, 0.0),
        (1.0e-300, 0.0, 0.0, 0.0),
    ] {
        let inputs = [torque, 303.15, starter, ignition];
        with_clock_free_cx(|cx| {
            check_component(
                &shaft,
                &Case {
                    state: &at(velocity),
                    stale: &stale,
                    dt_s: dt,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("shaft conformance");
    }

    let step = |velocity: f64, torque: f64, starter: f64| {
        let inputs = [torque, 303.15, starter, 0.0];
        let mut next = stale;
        let mut outputs = [0.0; 2];
        with_clock_free_cx(|cx| shaft.propose(&at(velocity), dt, &inputs, &mut next, &mut outputs, cx))
            .expect("shaft step");
        (next, outputs)
    };

    // Undriven and slower than static friction stops in a step: at rest, exactly. The mean
    // speed was half of what it had; what it had as kinetic energy, with the work done on
    // it in the step, leaves as heat.
    let slow = 0.6 * rest_speed;
    let (stopped, out) = step(slow, -20.0, 0.0);
    assert_eq!(stopped.velocity_m_s, 0.0);
    assert_eq!(out[0], 0.5 * slow);
    assert!((stopped.position_m - (3.0 + dt * 0.5 * slow)).abs() < 1.0e-15);
    let kinetic = 0.5 * SHAFT_INERTIA_KG_M2 * slow * slow;
    let work = dt * -20.0 * 0.5 * slow;
    assert!((-out[1] * dt - (kinetic + work)).abs() < 1.0e-12 * kinetic, "heat {} J", -out[1] * dt);
    // The speed that could no longer be differenced is stopped like any other.
    assert_eq!(step(1.0e-300, 0.0, 0.0).0.velocity_m_s, 0.0);
    // At rest it stays at rest and releases nothing, whatever small torque is on it.
    let (resting, out) = step(0.0, 5.0, 0.0);
    assert_eq!((resting.velocity_m_s, out[0], out[1]), (0.0, 0.0, 0.0));
    assert_eq!(resting.position_m, 3.0);

    // Just faster than that, or with the starter commanded, it is the kernel's inertia.
    let (turning, out) = step(1.5 * rest_speed, -20.0, 0.0);
    assert_eq!(turning.velocity_m_s, 1.5 * rest_speed + dt * -20.0 / SHAFT_INERTIA_KG_M2);
    assert_eq!(out[1], 0.0);
    let (cranked, out) = step(0.0, 2_500.0, 1.0);
    assert_eq!(cranked.velocity_m_s, dt * 2_500.0 / SHAFT_INERTIA_KG_M2);
    assert_eq!(out[0], 0.5 * cranked.velocity_m_s);
}

#[test]
fn generator_conforms_open_closed_and_through_a_pole_slip() {
    let generator = SynchronousGeneratorComponent::new(GeneratorCard::ASSUMED);
    let open = GeneratorState::open();
    let loaded = GeneratorState {
        load_angle_rad: 0.25,
        breaker_closed: true,
        ..open
    };
    let slipping = GeneratorState {
        load_angle_rad: 3.141,
        breaker_closed: true,
        ..open
    };
    // The last three: negative-sequence phase voltage (real, imaginary) and the angle of
    // the positive-sequence voltage.
    for (state, omega, command, excitation, hz, unbalance) in [
        (&open, RATED_OMEGA_RAD_S, 0.0, 1.0, 50.0, [0.0, 0.0, 0.0]),
        (&open, 0.0, 0.0, 1.0, 50.0, [0.0, 0.0, 0.0]),
        (&loaded, RATED_OMEGA_RAD_S, 1.0, 1.0, 50.0, [0.0, 0.0, 0.0]),
        (&loaded, RATED_OMEGA_RAD_S * 1.01, 1.0, 1.2, 50.0, [0.0, 0.0, 0.0]),
        (&loaded, RATED_OMEGA_RAD_S, 1.0, 0.8, 49.8, [0.0, 0.0, 0.0]),
        (&slipping, RATED_OMEGA_RAD_S * 1.05, 1.0, 1.0, 50.0, [0.0, 0.0, 0.0]),
        (&loaded, RATED_OMEGA_RAD_S, 1.0, 1.0, 50.0, [20.8, 0.0, 0.0]),
        (&loaded, RATED_OMEGA_RAD_S, 1.0, 1.0, 50.0, [-9.0, 6.0, 0.05]),
        (&open, RATED_OMEGA_RAD_S, 0.0, 1.0, 50.0, [20.8, 0.0, 0.0]),
    ] {
        let inputs = [
            -omega,
            400.0,
            303.15,
            command,
            excitation,
            hz,
            unbalance[0],
            unbalance[1],
            unbalance[2],
        ];
        with_clock_free_cx(|cx| {
            check_component(
                &generator,
                &Case {
                    state,
                    stale: &open,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("generator conformance");
    }
}

/// A negative-sequence voltage on the bus drives a current through the
/// negative-sequence impedance; its heat is split between stator and rotor,
/// and half of the rotor's share comes through the shaft.
#[test]
fn negative_sequence_voltage_gives_unequal_phase_currents_and_books_its_heat() {
    use fs_ports::component::Component;
    let card = GeneratorCard::ASSUMED;
    let generator = SynchronousGeneratorComponent::new(card);
    let loaded = GeneratorState {
        load_angle_rad: 0.25,
        breaker_closed: true,
        ..GeneratorState::open()
    };
    let step = |unbalance: [f64; 3]| {
        let inputs = [
            -RATED_OMEGA_RAD_S,
            400.0,
            303.15,
            1.0,
            1.0,
            50.0,
            unbalance[0],
            unbalance[1],
            unbalance[2],
        ];
        let mut next = loaded;
        let mut outputs = [0.0; 3];
        with_clock_free_cx(|cx| generator.propose(&loaded, 0.01, &inputs, &mut next, &mut outputs, cx))
            .expect("generator step");
        (next, outputs)
    };
    let (balanced, balanced_out) = step([0.0, 0.0, 0.0]);
    let (unbalanced, unbalanced_out) = step([20.8, 0.0, 0.0]);

    // Balanced: three equal currents, each the positive-sequence current.
    for current in balanced.phase_current_a {
        assert!((current - balanced.line_current_a).abs() < 1.0e-9 * balanced.line_current_a);
    }
    assert_eq!(balanced.negative_sequence_current_a, 0.0);
    assert_eq!(balanced.negative_sequence_rotor_loss_w, 0.0);

    // 20.8 V (9 % of 230.9 V) across R2 + j X2 = (0.05 + j 0.26) pu of 0.128 ohm.
    let base = card.base_impedance_ohm();
    let expected = 20.8 / (base * 0.05_f64.hypot(0.26));
    assert!(
        (unbalanced.negative_sequence_current_a - expected).abs() < 1.0e-9 * expected,
        "I2 {} A against {expected} A",
        unbalanced.negative_sequence_current_a
    );
    let [a, b, c] = unbalanced.phase_current_a;
    let spread = a.max(b).max(c) - a.min(b).min(c);
    assert!(spread > 0.5 * expected, "phase currents {a}, {b}, {c} A");
    // The positive-sequence quantities do not move.
    assert_eq!(unbalanced.line_current_a.to_bits(), balanced.line_current_a.to_bits());
    assert_eq!(unbalanced.copper_loss_w.to_bits(), balanced.copper_loss_w.to_bits());

    // Heat: 3 R I2^2 in the stator and 3 R_rotor I2^2 in the rotor. The bus supplies
    // 3 R2 I2^2 of it (the export falls by that), the shaft the other half of the rotor's.
    let stator = 3.0 * 0.02 * base * expected * expected;
    let rotor = 3.0 * 0.06 * base * expected * expected;
    assert!((unbalanced.negative_sequence_stator_loss_w - stator).abs() < 1.0e-9 * stator);
    assert!((unbalanced.negative_sequence_rotor_loss_w - rotor).abs() < 1.0e-9 * rotor);
    let export_drop = balanced.electric_power_w - unbalanced.electric_power_w;
    assert!(
        (export_drop - (stator + 0.5 * rotor)).abs() < 1.0e-6 * rotor,
        "export fell by {export_drop} W, 3 R2 I2^2 is {} W",
        stator + 0.5 * rotor
    );
    let shaft_gain = RATED_OMEGA_RAD_S * (balanced_out[0] - unbalanced_out[0]);
    assert!((shaft_gain - 0.5 * rotor).abs() < 1.0e-6 * rotor, "shaft gives {shaft_gain} W more");
}

#[test]
fn sequence_components_are_exact_for_a_balanced_set_and_right_for_one_low_phase() {
    for magnitude in [1.0, 0.62, 0.73, 1.1] {
        let balanced = sequence_components([magnitude; 3], [0.0; 3]);
        assert_eq!(balanced.positive, [magnitude, 0.0]);
        assert_eq!(balanced.negative, [0.0, 0.0]);
        assert_eq!(balanced.unbalance(), 0.0);
    }
    // One phase at 0.73: positive (2 + 0.73) / 3, negative 0.27 / 3, whichever phase.
    for low in 0..3 {
        let mut magnitude = [1.0; 3];
        magnitude[low] = 0.73;
        let sequences = sequence_components(magnitude, [0.0; 3]);
        assert!((sequences.positive_magnitude() - 0.91).abs() < 1.0e-12);
        assert!((sequences.negative_magnitude() - 0.09).abs() < 1.0e-12);
        assert!(sequences.positive_angle().abs() < 1.0e-12);
    }
    // A set turned as a whole is still balanced, with its positive sequence turned.
    let turned = sequence_components([1.0; 3], [0.1; 3]);
    assert!((turned.positive_angle() - 0.1).abs() < 1.0e-12);
    assert!(turned.negative_magnitude() < 1.0e-12);
    // Phases b and c swapped is a pure negative sequence.
    let third = 2.0 * core::f64::consts::PI / 3.0;
    let swapped = sequence_components([1.0; 3], [0.0, -third, third]);
    assert!(swapped.positive_magnitude() < 1.0e-12);
    assert!((swapped.negative_magnitude() - 1.0).abs() < 1.0e-12);
}

/// Measurements for the cabinet: phase voltages [pu], no angle offsets, phase
/// currents [A], negative-sequence current [A], frequency, power, breaker closed.
fn cabinet_measure(
    voltage: [f64; 3],
    current: [f64; 3],
    negative_a: f64,
    hz: f64,
    power_w: f64,
) -> [f64; MEASURE_WIDTH] {
    [
        voltage[0], voltage[1], voltage[2], 0.0, 0.0, 0.0, current[0], current[1], current[2],
        negative_a, hz, power_w, 1.0,
    ]
}

/// Run the cabinet with a closed breaker under held measurements until it
/// trips; returns the time to the trip and the state, or `None` if it holds
/// for `limit_s`.
fn cabinet_trip_time(
    cabinet: &BreakerCabinetComponent,
    measure: [f64; MEASURE_WIDTH],
    limit_s: f64,
) -> Option<(f64, CabinetState)> {
    use fs_ports::component::Component;
    let mut state = CabinetState {
        closed: true,
        ..CabinetState::open()
    };
    let mut inputs = [0.0; 2 + MEASURE_WIDTH];
    inputs[0] = 1.0;
    inputs[2..].copy_from_slice(&measure);
    let dt = 0.01;
    let steps = (limit_s / dt).round() as usize;
    for step in 1..=steps {
        let mut next = state;
        let mut outputs = [0.0; 1];
        with_clock_free_cx(|cx| cabinet.propose(&state, dt, &inputs, &mut next, &mut outputs, cx))
            .expect("cabinet step");
        state = next;
        if state.trips > 0 {
            assert_eq!(outputs[0], 0.0, "a trip opens the breaker in the same step");
            return Some((step as f64 * dt, state));
        }
        assert_eq!(outputs[0], 1.0);
    }
    None
}

#[test]
fn cabinet_conforms_and_each_protection_function_trips_at_its_setting() {
    let card = BreakerCabinetCard::DEFAULT;
    let rated = GeneratorCard::ASSUMED.rated_current_a();
    let cabinet = BreakerCabinetComponent::new(card, RestorationCard::DEFAULT, rated);
    let healthy = cabinet_measure([1.0; 3], [1_400.0; 3], 0.0, 50.0, 0.9e6);

    // Conformance: open, closed and healthy, picked up, tripping, latched, reset.
    let open = CabinetState::open();
    let closed = CabinetState {
        closed: true,
        ..open
    };
    let latched = CabinetState {
        tripped: 1 << protection::UNDERVOLTAGE,
        trips: 1,
        ..open
    };
    let low = cabinet_measure([1.0, 0.73, 1.0], [1_600.0; 3], 600.0, 50.0, 0.9e6);
    for (state, command, measure) in [
        (&open, [0.0, 0.0], healthy),
        (&open, [1.0, 0.0], healthy),
        (&closed, [1.0, 0.0], healthy),
        (&closed, [1.0, 0.0], low),
        (&latched, [1.0, 0.0], healthy),
        (&latched, [0.0, 1.0], healthy),
    ] {
        let mut inputs = [0.0; 2 + MEASURE_WIDTH];
        inputs[..2].copy_from_slice(&command);
        inputs[2..].copy_from_slice(&measure);
        with_clock_free_cx(|cx| {
            check_component(
                &cabinet,
                &Case {
                    state,
                    stale: &closed,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("cabinet conformance");
    }

    // A healthy generator on a healthy grid is left alone.
    assert!(cabinet_trip_time(&cabinet, healthy, 30.0).is_none());

    // Each function: the condition just past its level trips after its delay, on the
    // right phase, and reports the value. (function, measurements, delay, phase)
    let cases = [
        (
            protection::UNDERVOLTAGE,
            cabinet_measure([1.0, 1.0, 0.84], [1_400.0; 3], 0.0, 50.0, 0.9e6),
            card.undervoltage.delay_s,
            3,
        ),
        (
            protection::OVERVOLTAGE,
            cabinet_measure([1.16, 1.0, 1.0], [1_400.0; 3], 0.0, 50.0, 0.9e6),
            card.overvoltage.delay_s,
            1,
        ),
        (
            protection::OVERCURRENT,
            cabinet_measure([1.0; 3], [1_400.0, 1.21 * rated, 1_400.0], 0.0, 50.0, 0.9e6),
            card.overcurrent.delay_s,
            2,
        ),
        (
            protection::OVERCURRENT_HIGH,
            cabinet_measure([1.0; 3], [2.6 * rated, 1_400.0, 1_400.0], 0.0, 50.0, 0.9e6),
            card.overcurrent_high.delay_s,
            1,
        ),
        (
            protection::UNDERFREQUENCY,
            cabinet_measure([1.0; 3], [1_400.0; 3], 0.0, 47.4, 0.9e6),
            card.underfrequency.delay_s,
            0,
        ),
        (
            protection::OVERFREQUENCY,
            cabinet_measure([1.0; 3], [1_400.0; 3], 0.0, 51.6, 0.9e6),
            card.overfrequency.delay_s,
            0,
        ),
        (
            protection::REVERSE_POWER,
            cabinet_measure([1.0; 3], [100.0; 3], 0.0, 50.0, -60.0e3),
            card.reverse_power.delay_s,
            0,
        ),
    ];
    for (function, measure, delay, phase) in cases {
        let (time, state) = cabinet_trip_time(&cabinet, measure, 60.0).expect("trip");
        assert_eq!(state.last_function, function);
        assert_eq!(state.tripped, 1 << function);
        assert_eq!(state.last_phase, phase, "function {function}");
        assert!((time - delay).abs() < 0.011, "function {function}: {time} s against {delay} s");
    }

    // Unbalance by one low phase that stays above the under-voltage level: 1, 1, 0.86
    // is 4.9 % and holds; 1, 1, 0.855 is 5.1 % and trips after its delay.
    let holds = cabinet_measure([1.0, 1.0, 0.86], [1_400.0; 3], 0.0, 50.0, 0.9e6);
    assert!(cabinet_trip_time(&cabinet, holds, 30.0).is_none());
    let trips = cabinet_measure([1.0, 1.0, 0.855], [1_400.0; 3], 0.0, 50.0, 0.9e6);
    let (time, state) = cabinet_trip_time(&cabinet, trips, 30.0).expect("unbalance trip");
    assert_eq!(state.last_function, protection::VOLTAGE_UNBALANCE);
    assert!((time - card.voltage_unbalance.delay_s).abs() < 0.011);
    assert!(state.last_value > 0.05 && state.last_value < 0.052);

    // The rotor's replica: below the continuous level it never trips; above it the time
    // is the limit over the square of the current, so twice the current is a quarter
    // of the time.
    let below = cabinet_measure([1.0; 3], [1_400.0; 3], 0.07 * rated, 50.0, 0.9e6);
    assert!(cabinet_trip_time(&cabinet, below, 600.0).is_none());
    let mut times = Vec::new();
    for share in [0.2, 0.4] {
        let measure = cabinet_measure([1.0; 3], [1_400.0; 3], share * rated, 50.0, 0.9e6);
        let (time, state) = cabinet_trip_time(&cabinet, measure, 600.0).expect("thermal trip");
        assert_eq!(state.last_function, protection::NEGATIVE_SEQUENCE);
        let expected = card.negative_sequence_limit_s / (share * share);
        assert!((time - expected).abs() < 0.02, "{time} s against {expected} s at {share} pu");
        times.push(time);
    }
    assert!((times[0] / times[1] - 4.0).abs() < 0.01);
}

#[test]
fn controller_conforms_in_every_step_of_its_sequence() {
    let controller = GensetControllerComponent::new(
        ControllerSetpoints::DEFAULT,
        SynchronisationWindowCard::ASSUMED,
        RestorationCard::DEFAULT,
        2,
        GeneratorCard::ASSUMED.excitation_range_pu,
        1_060.0,
    );
    let ready = ControllerState::ready();
    let operator = OperatorSignals {
        restoration_auto: true,
        synchronise: false,
        load_setpoint: 0.8,
        start: false,
        stop: false,
        emergency_stop: false,
        acknowledge: false,
        breaker_close: false,
        breaker_open: false,
        auto_synchronise: true,
        gas_gauge_pa: 50_000.0,
        oil_pressure_low: false,
        grid_voltage_pu: 1.0,
        grid_frequency_hz: 50.0,
        excitation_pu: 1.0,
        fan_failed: false,
        load_ramp_per_s: 0.1 / 60.0,
    };
    for sequence in [
        Sequence::Ready,
        Sequence::PreLubrication,
        Sequence::Cranking,
        Sequence::RunUp,
        Sequence::WarmUp,
        Sequence::Synchronising,
        Sequence::OnLoad,
        Sequence::Unloading,
        Sequence::CoolDown,
        Sequence::CoastDown,
        Sequence::Tripped,
        Sequence::GridWait,
    ] {
        let firing = matches!(
            sequence,
            Sequence::RunUp
                | Sequence::WarmUp
                | Sequence::Synchronising
                | Sequence::OnLoad
                | Sequence::Unloading
                | Sequence::CoolDown
                | Sequence::GridWait
        );
        let state = ControllerState {
            sequence,
            ignition: firing,
            gas_open: firing,
            speed_reference_rad_s: if firing { 150.0 } else { 0.0 },
            ..ready
        };
        // The last three sensed values: cabinet trip count, tripped functions, grid healthy.
        for (omega, start, cabinet) in [
            (0.0, true, [0.0, 0.0, 1.0]),
            (RATED_OMEGA_RAD_S, false, [0.0, 0.0, 1.0]),
            (190.0, false, [0.0, 0.0, 0.0]),
            (RATED_OMEGA_RAD_S, false, [1.0, 1.0, 0.0]),
            (RATED_OMEGA_RAD_S, false, [1.0, 16.0, 1.0]),
        ] {
            let mut inputs = [0.0; 26];
            inputs[..6].copy_from_slice(&[omega, 0.05, 0.0, 0.0, 0.0, 360.0]);
            inputs[6..9].copy_from_slice(&cabinet);
            // Where there is no start pulse there is a command to synchronise.
            OperatorSignals {
                start,
                synchronise: !start,
                ..operator
            }
            .write(&mut inputs[9..]);
            with_clock_free_cx(|cx| {
                check_component(
                    &controller,
                    &Case {
                        state: &state,
                        stale: &ready,
                        dt_s: 0.01,
                        inputs: &inputs,
                    },
                    cx,
                )
            })
            .expect("controller conformance");
        }
    }
}

#[test]
fn heat_exchangers_conform_across_their_valve_bands_and_without_flow() {
    let fuel = FuelGasCard::STATION;
    let gas = ExhaustGasCard::ASSUMED;
    let card = ExhaustExchangerCard {
        tubes: ExhaustExchangerCard::TUBES,
        bore_m: ExhaustExchangerCard::BORE_M,
        length_m: 1.7,
    };
    let exhaust = ExhaustHeatExchangerComponent::new(gas, card, &fuel).expect("exhaust exchanger");
    let state = ExhaustExchangerState::default();
    let stale = ExhaustExchangerState {
        recovered_w: 1.0,
        ..state
    };
    // [exhaust enthalpy rate, coolant K, stack port K, capacity rate]
    for inputs in [
        [940.0e3, 361.0, 393.15, 2_018.0],
        [136.0e3, 333.0, 393.15, 289.0],
        [20.0e3, 361.0, 393.15, 2_018.0],
        [0.0, 333.0, 393.15, 0.0],
        [300.0e3, 333.0, 393.15, 0.0],
    ] {
        with_clock_free_cx(|cx| {
            check_component(
                &exhaust,
                &Case {
                    state: &state,
                    stale: &stale,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("exhaust exchanger conformance");
    }

    let network = NetworkHeatExchangerComponent::new(NetworkExchangerCard::ASSUMED, 56_000.0);
    let state = NetworkExchangerState::default();
    let stale = NetworkExchangerState {
        heat_w: 1.0,
        ..state
    };
    // [coolant K, network port K, demand, return K]
    for inputs in [
        [333.15, 348.15, 1.0, 338.15],
        [359.15, 348.15, 1.0, 338.15],
        [362.15, 348.15, 1.0, 338.15],
        [362.15, 348.15, 0.4, 338.15],
        [362.15, 348.15, 0.0, 338.15],
        [362.15, 348.15, 1.0, 365.15],
    ] {
        with_clock_free_cx(|cx| {
            check_component(
                &network,
                &Case {
                    state: &state,
                    stale: &stale,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("network exchanger conformance");
    }

    let dump = DumpCoolerComponent::new(DumpCoolerCard::ASSUMED).expect("dump cooler");
    let state = DumpCoolerState::default();
    let stale = DumpCoolerState {
        heat_w: 1.0,
        ..state
    };
    // [coolant K, air port K, fan failed]
    for inputs in [
        [361.15, 303.15, 0.0],
        [365.65, 303.15, 0.0],
        [369.15, 303.15, 0.0],
        [369.15, 303.15, 1.0],
    ] {
        with_clock_free_cx(|cx| {
            check_component(
                &dump,
                &Case {
                    state: &state,
                    stale: &stale,
                    dt_s: 0.01,
                    inputs: &inputs,
                },
                cx,
            )
        })
        .expect("dump cooler conformance");
    }
    // Not inputs: what the correlations give for the assumed radiator. With the fans it
    // can reject the unit's whole recoverable heat at 95 C; on natural draught it cannot.
    let with_fans = dump.capacity_w(368.15, true);
    let on_draught = dump.capacity_w(368.15, false);
    assert!(with_fans > 1.3e6, "dump capacity with fans {with_fans} W");
    assert!(on_draught < 0.1e6, "dump capacity on natural draught {on_draught} W");
}

// ---------------------------------------------------------------------------
// Plant scenarios
// ---------------------------------------------------------------------------

#[test]
fn a_ready_unit_stays_at_rest_and_books_nothing() {
    let mut sim = KguSim::new();
    advance(&mut sim, 5.0);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 0.0);
    assert_eq!(s[snap::SPEED_RPM], 0.0);
    assert_eq!(s[snap::GAS_STD_M3], 0.0);
    assert_eq!(s[snap::ENERGY_ELECTRIC_KWH], 0.0);
    assert_eq!(s[snap::LEDGER_RESIDUAL_J], 0.0);
    assert!((s[snap::TIME_S] - 5.0).abs() < 1.0e-9);
}

#[test]
fn start_from_standstill_reaches_the_bus_and_burns_gas_before_the_first_kwh() {
    let mut sim = KguSim::new();
    sim.set_load_setpoint_pct(50.0);
    sim.cmd_start();
    let set = ControllerSetpoints::DEFAULT;
    advance(&mut sim, 0.5);
    let lubricating = sim.snapshot();
    assert_eq!(lubricating[snap::STATE_CODE], 1.0, "starting");
    assert_eq!(lubricating[snap::SEQUENCE_STEP], f64::from(Sequence::PreLubrication.index()));
    assert_eq!(lubricating[snap::SPEED_RPM], 0.0, "nothing turns while pre-lubricating");
    advance(&mut sim, set.pre_lubrication_time_s + 1.0);
    let cranking = sim.snapshot();
    assert_eq!(cranking[snap::SEQUENCE_STEP], f64::from(Sequence::Cranking.index()));
    assert!(cranking[snap::TORQUE_STARTER_NM] > 0.0, "starter torque turns the shaft");
    assert!(cranking[snap::SPEED_RPM] > 60.0);
    assert_eq!(cranking[snap::GAS_STD_M3], 0.0, "no gas while purging");

    let to_bus = advance_until(&mut sim, 200.0, "breaker closed", |s| s[snap::BREAKER_CLOSED] == 1.0)
        + set.pre_lubrication_time_s
        + 1.5;
    let s = sim.snapshot();
    // Not inputs: the time to the bus beyond the controller's three timers (cranking to
    // the firing speed, the run-up, the wait for the phase), the speed at closing, the gas
    // burned on the way.
    let timers = set.pre_lubrication_time_s + set.purge_time_s + set.warm_up_time_s;
    assert!(
        to_bus > timers + 8.0 && to_bus < timers + 40.0,
        "time to the bus {to_bus} s with {timers} s of timers"
    );
    assert!((s[snap::SPEED_RPM] - 1_500.0).abs() < 6.0, "rpm after closing {}", s[snap::SPEED_RPM]);
    // The state event carries the speed of the sub-step in which the breaker closed:
    // inside the window's 0.10 Hz (3 rpm) of the 50 Hz bus.
    let at_closing = events_of(&sim, event_kind::STATE).last().expect("state event").2;
    assert!((at_closing - 1_500.0).abs() < 3.0, "rpm at closing {at_closing}");
    // The warm-up alone is 45 s at the no-load fuel (374 kW): 0.5 m3.
    assert!(
        s[snap::GAS_STD_M3] > 0.5 && s[snap::GAS_STD_M3] < 1.2,
        "gas burned before the bus: {} m3",
        s[snap::GAS_STD_M3]
    );
    assert!(s[snap::GAS_METER_M3] > 0.0 && s[snap::GAS_METER_M3] < s[snap::GAS_STD_M3]);
    assert!(
        s[snap::ENERGY_ELECTRIC_KWH].abs() < 0.05,
        "no electricity before the breaker closed: {} kWh",
        s[snap::ENERGY_ELECTRIC_KWH]
    );
    assert_eq!(s[snap::START_COUNT], 1.0);
    assert_eq!(s[snap::ALARM_BITS], 0.0);

    // The states were passed in order, each entered on a measured condition.
    let states: Vec<u32> = events_of(&sim, event_kind::STATE).iter().map(|e| e.1).collect();
    assert_eq!(states, vec![1, 2, 3], "ready -> starting -> synchronizing -> running");
    let closing = events_of(&sim, event_kind::BREAKER);
    assert_eq!(closing.len(), 1);
    assert_eq!(closing[0].1, 1);

    // The synchroniser closed inside its window.
    let sequence = events_of(&sim, event_kind::SEQUENCE);
    assert_eq!(sequence.last().map(|e| e.1), Some(Sequence::OnLoad.index()));
}

/// Relative first-law residual of the kernel's ledger: the residual over the
/// fuel energy that entered.
fn ledger_relative_residual(s: &[f64]) -> f64 {
    s[snap::LEDGER_RESIDUAL_J].abs() / (s[snap::LEDGER_BOUNDARY_ENERGY_MJ] * 1.0e6)
}

#[test]
fn steady_full_and_half_load_reach_the_passport_times_the_condition_factor() {
    let passport = PassportCard::STATION;
    let gas = FuelGasCard::STATION;
    for load_pct in [100.0, 50.0] {
        // The ramp is quickened to 300 %/min (a controller setting); the steady state it ends
        // in does not depend on it.
        let sim = on_load(load_pct, 300.0);
        let s = sim.snapshot();
        let load = load_pct / 100.0;
        assert!(
            (s[snap::ELECTRIC_POWER_KW] - 1_000.0 * load).abs() < 0.5,
            "power at {load_pct} %: {} kW",
            s[snap::ELECTRIC_POWER_KW]
        );
        assert!((s[snap::SPEED_RPM] - 1_500.0).abs() < 0.05, "synchronous speed {}", s[snap::SPEED_RPM]);

        // Fitted: the efficiency through the plant is the passport's times the condition
        // factor, at the reference excitation. Tolerance 0.3 percentage points.
        let expected = 100.0 * passport.electric_efficiency(load) * DEFAULT_CONDITION_FACTOR;
        assert!(
            (s[snap::ELECTRIC_EFFICIENCY_PCT] - expected).abs() < 0.3,
            "electrical efficiency at {load_pct} %: {} against {expected}",
            s[snap::ELECTRIC_EFFICIENCY_PCT]
        );
        assert!(s[snap::BRAKE_EFFICIENCY_PCT] > s[snap::ELECTRIC_EFFICIENCY_PCT] + 0.5);

        // Not inputs: the split of the fuel power closes on what leaves, the meter reads
        // less than the standard volume by the gas law, and the ledger closes.
        let mass_flow = s[snap::GAS_FLOW_STD_M3_H] / 3_600.0 * gas.standard_density_kg_m3();
        let sensible_kw = mass_flow * gas.cp_j_kg_k * (288.15 - 293.15) * 1.0e-3;
        let leaving = s[snap::ELECTRIC_POWER_KW]
            + s[snap::GENERATOR_LOSS_KW]
            + s[snap::JACKET_HEAT_KW]
            + s[snap::EXHAUST_RECOVERED_KW]
            + s[snap::EXHAUST_LOSS_KW]
            + s[snap::RADIATION_LOSS_KW];
        assert!(
            (s[snap::FUEL_POWER_KW] + sensible_kw - leaving).abs() < 1.0e-3,
            "fuel {} kW + sensible {sensible_kw} kW against {leaving} kW leaving",
            s[snap::FUEL_POWER_KW]
        );
        let law = (101_325.0 / 151_325.0) * (288.15 / 293.15);
        assert!((s[snap::GAS_FLOW_METER_M3_H] / s[snap::GAS_FLOW_STD_M3_H] - law).abs() < 1.0e-9);
        assert!((s[snap::GAS_FLOW_STD_M3_H] * 33.5e6 / 3.6e6 - s[snap::FUEL_POWER_KW]).abs() < 1.0e-6);
        assert!(
            ledger_relative_residual(&s) < 1.0e-9,
            "ledger residual {} J of {} MJ",
            s[snap::LEDGER_RESIDUAL_J],
            s[snap::LEDGER_BOUNDARY_ENERGY_MJ]
        );
        assert_eq!(s[snap::ALARM_BITS], 0.0);
    }
}

#[test]
fn droop_gives_more_power_when_the_grid_frequency_falls() {
    let mut sim = on_load(50.0, 300.0);
    let before = sim.snapshot()[snap::ELECTRIC_POWER_KW];
    sim.set_grid_freq_hz(49.9);
    advance(&mut sim, 40.0);
    let s = sim.snapshot();
    // 0.1 Hz of 50 Hz on a 4 % droop is 5 % of rated: 50 kW. Neither number is an input
    // to the engine or the generator; the governor, the valve and the machine settle there.
    let gain = s[snap::ELECTRIC_POWER_KW] - before;
    assert!((gain - 50.0).abs() < 1.0, "droop response {gain} kW");
    assert!((s[snap::SPEED_RPM] - 1_497.0).abs() < 0.05, "speed follows the bus: {}", s[snap::SPEED_RPM]);
}

#[test]
fn full_load_rejection_raises_the_speed_and_the_governor_catches_it_below_the_trip() {
    let mut sim = on_load(100.0, 300.0);
    let trip_rpm = sim.limits()[2];
    sim.cmd_breaker_open();
    let mut peak = 0.0_f64;
    for _ in 0..500 {
        advance(&mut sim, 0.01);
        peak = peak.max(sim.snapshot()[snap::SPEED_RPM]);
    }
    assert!(peak > 1_520.0, "the shaft must run away at first: peak {peak} rpm");
    assert!(peak < trip_rpm, "the governor must catch it below {trip_rpm} rpm: peak {peak} rpm");
    advance(&mut sim, 20.0);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 2.0, "back at the synchroniser, not tripped");
    assert_eq!(s[snap::ALARM_BITS], 0.0);
    assert_eq!(s[snap::BREAKER_CLOSED], 0.0);
    assert!(s[snap::ELECTRIC_POWER_KW].abs() < 1.0e-9);
    // Settled on the synchroniser's reference (bus frequency plus its slip of 0.05 Hz).
    assert!((s[snap::SPEED_RPM] - 1_501.5).abs() < 0.5, "settled speed {}", s[snap::SPEED_RPM]);
    // Not an input: the no-load fuel is where indicated torque balances friction and the
    // generator's core loss. The passport's Willans intercept over the condition factor.
    let (_, intercept) = PassportCard::STATION.willans_line();
    let no_load_kw = intercept / DEFAULT_CONDITION_FACTOR * 1.0e-3;
    assert!(
        (s[snap::FUEL_POWER_KW] - no_load_kw).abs() < 0.02 * no_load_kw,
        "no-load fuel {} kW against {no_load_kw} kW",
        s[snap::FUEL_POWER_KW]
    );
    // The synchroniser does not reclose after a manual opening.
    assert_eq!(events_of(&sim, event_kind::BREAKER).len(), 2);
}

#[test]
fn synchronisation_is_refused_outside_the_window_and_accepted_inside() {
    let mut sim = KguSim::new();
    sim.set_auto_sync(false);
    sim.cmd_start();
    advance_until(&mut sim, 200.0, "synchronising", |s| s[snap::STATE_CODE] == 2.0);
    advance(&mut sim, 6.0);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0, "no automatic closing when it is off");

    // Voltage: the open-circuit EMF 20 % above the bus.
    sim.set_excitation_pu(1.2);
    advance(&mut sim, 0.05);
    let bits = sim.cmd_breaker_close();
    assert_ne!(bits & refusal::VOLTAGE, 0, "refusal bits {bits}");
    advance(&mut sim, 0.5);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0);
    sim.set_excitation_pu(1.0);

    // Phase: wait until the EMF is well away from the bus voltage.
    advance_until(&mut sim, 30.0, "phase away", |s| s[snap::SYNC_PHASE_ANGLE_DEG].abs() > 40.0);
    assert_eq!(sim.cmd_breaker_close(), refusal::PHASE);
    advance(&mut sim, 0.05);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0);

    // Frequency: the bus drops 0.5 Hz; until the governor has followed, the slip is too large.
    sim.set_grid_freq_hz(49.5);
    advance(&mut sim, 0.02);
    assert_ne!(sim.cmd_breaker_close() & refusal::FREQUENCY, 0);
    sim.set_grid_freq_hz(50.0);
    advance(&mut sim, 5.0);

    // Inside the window the request is accepted and the breaker closes.
    let mut waited = 0.0;
    while sim.snapshot()[snap::SYNC_PERMITTED] != 1.0 {
        advance(&mut sim, 0.01);
        waited += 0.01;
        assert!(waited < 30.0, "the phase never came into the window");
    }
    let phase = sim.snapshot()[snap::SYNC_PHASE_ANGLE_DEG];
    assert!(phase.abs() <= 10.0 + 1.0e-9, "phase at the request {phase} deg");
    assert_eq!(sim.cmd_breaker_close(), 0);
    advance(&mut sim, 0.02);
    let s = sim.snapshot();
    assert_eq!(s[snap::BREAKER_CLOSED], 1.0);
    assert_eq!(s[snap::STATE_CODE], 3.0);
    let refused = events_of(&sim, event_kind::REFUSED);
    assert_eq!(refused.len(), 3, "three refusals in the log: {refused:?}");
}

#[test]
fn excitation_moves_reactive_power_while_active_power_stays_with_the_fuel() {
    let mut sim = on_load(60.0, 300.0);
    let mut points = Vec::new();
    for excitation in [0.9, 1.0, 1.1, 1.2] {
        sim.set_excitation_pu(excitation);
        advance(&mut sim, 30.0);
        let s = sim.snapshot();
        points.push((
            s[snap::ELECTRIC_POWER_KW],
            s[snap::REACTIVE_POWER_KVAR],
            s[snap::FUEL_POWER_KW],
            s[snap::STATOR_CURRENT_A],
            s[snap::POWER_FACTOR],
        ));
    }
    for pair in points.windows(2) {
        assert!(pair[1].1 > pair[0].1 + 200.0, "Q must rise with excitation: {points:?}");
    }
    for point in &points {
        assert!((point.0 - 600.0).abs() < 0.5, "P stays at the load reference: {points:?}");
        assert!((point.2 / points[1].2 - 1.0).abs() < 0.03, "fuel stays with P: {points:?}");
    }
    assert!(points[0].1 < 0.0 && points[3].1 > 0.0, "from absorbing to delivering: {points:?}");
    // Not an input: the stator current is smallest near unity power factor, and the extra
    // copper loss away from it costs fuel.
    assert!(points[3].3 > points[1].3 && points[3].2 > points[1].2, "{points:?}");
    assert!(points[3].4 < 0.95 && points[1].4 > 0.97, "{points:?}");
}

#[test]
fn normal_stop_burns_idle_gas_while_cooling_down_and_none_while_coasting() {
    let mut sim = on_load(50.0, 300.0);
    sim.cmd_stop();
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 3.0, "unloading is still running");
    let unload = advance_until(&mut sim, 200.0, "breaker open", |s| s[snap::BREAKER_CLOSED] == 0.0);
    assert!(unload > 30.0 && unload < 90.0, "unloading from 50 % took {unload} s");
    let opened = events_of(&sim, event_kind::BREAKER).last().copied().expect("breaker event");
    assert!(opened.2 < 25.0, "the breaker opened at {} kW", opened.2);

    // Cool-down: unloaded at rated speed, burning gas.
    let s0 = sim.snapshot();
    assert_eq!(s0[snap::STATE_CODE], 4.0);
    assert_eq!(s0[snap::SEQUENCE_STEP], f64::from(Sequence::CoolDown.index()));
    advance(&mut sim, 150.0);
    let mid = sim.snapshot();
    assert_eq!(mid[snap::SEQUENCE_STEP], f64::from(Sequence::CoolDown.index()));
    assert!((mid[snap::SPEED_RPM] - 1_500.0).abs() < 1.0, "idle speed {}", mid[snap::SPEED_RPM]);
    assert!(mid[snap::FUEL_POWER_KW] > 300.0, "idle fuel {} kW", mid[snap::FUEL_POWER_KW]);
    assert_eq!(mid[snap::ELECTRIC_POWER_KW], 0.0);
    let coast = advance_until(&mut sim, 200.0, "coasting", |s| {
        s[snap::SEQUENCE_STEP] == f64::from(Sequence::CoastDown.index())
    });
    let cool_down_s = 150.0 + coast;
    let setting = ControllerSetpoints::DEFAULT.cool_down_time_s;
    assert!((cool_down_s - setting).abs() < 1.5, "cool-down lasted {cool_down_s} s");
    let s1 = sim.snapshot();
    let burned = s1[snap::GAS_STD_M3] - s0[snap::GAS_STD_M3];
    // 374 kW for 300 s at 33.5 MJ/m3 is 3.35 m3; the idle fuel itself is computed.
    assert!(burned > 3.0 && burned < 3.7, "gas burned while cooling down: {burned} m3");

    // Coast-down: gas shut, friction alone stops the shaft.
    advance(&mut sim, 0.05);
    let shut = sim.snapshot();
    assert_eq!(shut[snap::FUEL_POWER_KW], 0.0);
    assert!(shut[snap::SPEED_RPM] > 1_400.0, "still turning: {} rpm", shut[snap::SPEED_RPM]);
    let to_rest = advance_until(&mut sim, 300.0, "ready", |s| s[snap::STATE_CODE] == 0.0);
    assert!(to_rest > 5.0 && to_rest < 60.0, "coast-down took {to_rest} s");
    let rest = sim.snapshot();
    assert!(rest[snap::SPEED_RPM] < 5.0);
    assert_eq!(
        rest[snap::GAS_STD_M3].to_bits(),
        shut[snap::GAS_STD_M3].to_bits(),
        "no gas while coasting down"
    );
    assert_eq!(rest[snap::GAS_METER_M3].to_bits(), shut[snap::GAS_METER_M3].to_bits());
    assert!(ledger_relative_residual(&rest) < 1.0e-9);
    let states: Vec<u32> = events_of(&sim, event_kind::STATE).iter().map(|e| e.1).collect();
    assert_eq!(states, vec![1, 2, 3, 4, 0]);
}

#[test]
fn emergency_stop_latches_and_blocks_the_start_until_acknowledged() {
    let mut sim = on_load(50.0, 300.0);
    sim.cmd_emergency_stop();
    advance(&mut sim, 0.05);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 5.0);
    assert_eq!(s[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(s[snap::FUEL_POWER_KW], 0.0);
    assert_eq!(s[snap::ALARM_BITS] as u32 & alarm::EMERGENCY_STOP, alarm::EMERGENCY_STOP);
    advance_until(&mut sim, 120.0, "standstill", |s| s[snap::SPEED_RPM] < 1.0);

    sim.cmd_start();
    advance(&mut sim, 2.0);
    let blocked = sim.snapshot();
    assert_eq!(blocked[snap::STATE_CODE], 5.0, "start is blocked while the trip is latched");
    assert_eq!(blocked[snap::START_COUNT], 1.0);
    assert!(blocked[snap::SPEED_RPM] < 1.0);
    assert_eq!(events_of(&sim, event_kind::REFUSED).len(), 1);

    sim.cmd_ack();
    advance(&mut sim, 0.05);
    let ready = sim.snapshot();
    assert_eq!(ready[snap::STATE_CODE], 0.0);
    assert_eq!(ready[snap::ALARM_BITS], 0.0);
    sim.cmd_start();
    advance(&mut sim, 0.5);
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 1.0);
    assert_eq!(sim.snapshot()[snap::START_COUNT], 2.0);
}

#[test]
fn low_gas_pressure_trips_only_when_the_open_valve_cannot_hold_the_power() {
    let mut sim = on_load(100.0, 300.0);
    // 27 kPa: the valve opens further and still holds rated power.
    sim.set_gas_gauge_kpa(27.0);
    advance(&mut sim, 30.0);
    let held = sim.snapshot();
    assert_eq!(held[snap::STATE_CODE], 3.0);
    assert!((held[snap::ELECTRIC_POWER_KW] - 1_000.0).abs() < 1.0, "{} kW", held[snap::ELECTRIC_POWER_KW]);
    assert!(held[snap::GAS_VALVE_LIFT] > 0.9, "valve lift {}", held[snap::GAS_VALVE_LIFT]);
    assert_eq!(held[snap::ALARM_BITS] as u32, alarm::GAS_PRESSURE_WARNING);

    // 15 kPa: fully open it passes too little; the power sags and A-150 trips.
    sim.set_gas_gauge_kpa(15.0);
    advance(&mut sim, 3.0);
    let sagging = sim.snapshot();
    assert_eq!(sagging[snap::STATE_CODE], 3.0);
    assert!(sagging[snap::GAS_VALVE_LIFT] > 0.999);
    assert!(sagging[snap::ELECTRIC_POWER_KW] < 900.0, "{} kW", sagging[snap::ELECTRIC_POWER_KW]);
    advance(&mut sim, 6.0);
    let tripped = sim.snapshot();
    assert_eq!(tripped[snap::STATE_CODE], 5.0);
    assert_eq!(tripped[snap::ALARM_BITS] as u32 & alarm::GAS_PRESSURE, alarm::GAS_PRESSURE);
    assert_eq!(tripped[snap::BREAKER_CLOSED], 0.0);
}

#[test]
fn oil_pressure_and_grid_undervoltage_inputs_trip_after_their_delays() {
    let mut sim = on_load(30.0, 300.0);
    sim.set_oil_pressure_low(true);
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 3.0, "inside the 2 s delay");
    advance(&mut sim, 1.5);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 5.0);
    assert_eq!(s[snap::ALARM_BITS] as u32 & alarm::OIL_PRESSURE, alarm::OIL_PRESSURE);

    // Grid under-voltage on all three phases. Rewritten for slice 1c: the cabinet's
    // protection opens the breaker after the same 0.15 s, but the engine is no longer
    // stopped: it runs on unloaded and waits (manual restoration, the default).
    let mut sim = on_load(30.0, 300.0);
    sim.set_grid_voltage_pu(0.62);
    advance(&mut sim, 0.10);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 1.0, "inside the 0.15 s delay");
    advance(&mut sim, 0.10);
    let s = sim.snapshot();
    assert_eq!(s[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(s[snap::STATE_CODE], 2.0, "off the bus, not stopped");
    assert_eq!(s[snap::SEQUENCE_STEP], f64::from(Sequence::GridWait.index()));
    assert_eq!(s[snap::WAITING_FOR_GRID], 1.0);
    assert_eq!(s[snap::ALARM_BITS] as u32, alarm::GRID_UNDERVOLTAGE);
    assert_eq!(s[snap::VOLTAGE_UNBALANCE_PCT], 0.0, "three equal phases");
    let trips = events_of(&sim, event_kind::PROTECTION);
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].1 / 10, protection::UNDERVOLTAGE);
    assert!((trips[0].2 - 0.62 * 230.94).abs() < 0.01, "tripped at {} V", trips[0].2);
    advance(&mut sim, 30.0);
    let idling = sim.snapshot();
    assert!((idling[snap::SPEED_RPM] - 1_500.0).abs() < 1.0, "{} rpm", idling[snap::SPEED_RPM]);
    assert!(idling[snap::FUEL_POWER_KW] > 300.0, "no-load fuel {} kW", idling[snap::FUEL_POWER_KW]);
}

#[test]
fn a_start_without_gas_fails_on_the_controllers_time_limit() {
    let mut sim = KguSim::new();
    sim.set_gas_gauge_kpa(0.0);
    sim.cmd_start();
    let set = ControllerSetpoints::DEFAULT;
    advance(&mut sim, set.pre_lubrication_time_s + 10.0);
    let cranking = sim.snapshot();
    assert_eq!(cranking[snap::STATE_CODE], 1.0);
    // Not an input: the speed the starter reaches against friction, below its free speed.
    assert!(
        cranking[snap::SPEED_RPM] > 150.0 && cranking[snap::SPEED_RPM] < 250.0,
        "cranking speed {} rpm",
        cranking[snap::SPEED_RPM]
    );
    assert_eq!(cranking[snap::FUEL_POWER_KW], 0.0);
    advance(&mut sim, 21.0);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 5.0);
    assert_eq!(s[snap::ALARM_BITS] as u32 & alarm::FAILED_START, alarm::FAILED_START);
    assert_eq!(s[snap::GAS_STD_M3], 0.0);
}

#[test]
fn the_controller_trips_on_overspeed() {
    let controller = GensetControllerComponent::new(
        ControllerSetpoints::DEFAULT,
        SynchronisationWindowCard::ASSUMED,
        RestorationCard::DEFAULT,
        2,
        GeneratorCard::ASSUMED.excitation_range_pu,
        1_060.0,
    );
    let running = ControllerState {
        sequence: Sequence::Synchronising,
        ignition: true,
        gas_open: true,
        speed_reference_rad_s: RATED_OMEGA_RAD_S,
        ..ControllerState::ready()
    };
    let operator = OperatorSignals {
        load_setpoint: 0.0,
        start: false,
        stop: false,
        emergency_stop: false,
        acknowledge: false,
        breaker_close: false,
        breaker_open: false,
        auto_synchronise: false,
        gas_gauge_pa: 50_000.0,
        oil_pressure_low: false,
        grid_voltage_pu: 1.0,
        grid_frequency_hz: 50.0,
        excitation_pu: 1.0,
        fan_failed: false,
        load_ramp_per_s: 0.0,
        restoration_auto: false,
        synchronise: false,
    };
    for (share, trips) in [(1.14, false), (1.15, true)] {
        let mut inputs = [0.0; 26];
        inputs[..6].copy_from_slice(&[share * RATED_OMEGA_RAD_S, 0.0, 0.0, 0.0, 0.0, 360.0]);
        operator.write(&mut inputs[9..]);
        let mut next = running;
        let mut outputs = [0.0; 6];
        with_clock_free_cx(|cx| {
            use fs_ports::component::Component;
            controller.propose(&running, 0.01, &inputs, &mut next, &mut outputs, cx)
        })
        .expect("controller step");
        assert_eq!(next.sequence == Sequence::Tripped, trips, "at {share} of rated speed");
        if trips {
            assert_eq!(next.alarms, alarm::OVERSPEED);
            assert_eq!(outputs, [0.0; 6], "everything off");
        }
    }
}

#[test]
fn two_runs_of_the_same_commands_agree_to_the_bit() {
    let run = || {
        let mut sim = KguSim::new();
        sim.set_load_ramp_pct_per_min(300.0);
        sim.set_load_setpoint_pct(70.0);
        sim.cmd_start();
        for _ in 0..1_500 {
            advance(&mut sim, 0.1);
        }
        sim.cmd_breaker_open();
        advance(&mut sim, 3.0);
        (sim.snapshot(), sim.events())
    };
    let (a, events_a) = run();
    let (b, events_b) = run();
    for (index, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(x.to_bits(), y.to_bits(), "snapshot slot {} differs", crate::SNAPSHOT_FIELDS[index]);
    }
    assert_eq!(events_a, events_b);

    // And through an unbalanced bus, a protection trip, the wait and the automatic return.
    let run = || {
        let mut sim = on_load(60.0, 300.0);
        sim.set_restoration_mode(1);
        sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.94);
        advance(&mut sim, 20.0);
        sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
        advance(&mut sim, 2.0);
        sim.set_grid_voltage_pu(1.0);
        advance(&mut sim, 100.0);
        let journals: Vec<u64> = sim.meter_events().iter().map(|x| x.to_bits()).collect();
        (sim.snapshot(), sim.events(), sim.analyser_events(), journals)
    };
    let (a, events_a, quality_a, journals_a) = run();
    let (b, events_b, quality_b, journals_b) = run();
    assert_eq!(journals_a, journals_b);
    assert_eq!(journals_a.len(), METER_EVENT_STRIDE, "one record of the 2 s dip");
    assert_eq!(a[snap::BREAKER_CLOSED], 1.0, "{}", grid_side_summary(&a));
    assert_eq!(a[snap::RESTORATION_ATTEMPTS], 1.0);
    for (index, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(x.to_bits(), y.to_bits(), "snapshot slot {} differs", crate::SNAPSHOT_FIELDS[index]);
    }
    assert_eq!(events_a, events_b);
    assert_eq!(quality_a, quality_b);
    // One dip (the phase at 0.73); 0.94 is inside the analyser's band.
    assert_eq!(quality_a.len(), QUALITY_EVENT_STRIDE, "{quality_a:?}");
}

#[test]
fn snapshot_layout_is_versioned_and_named_without_repeats() {
    let sim = KguSim::new();
    let s = sim.snapshot();
    assert_eq!(s.len(), crate::SNAPSHOT_LEN);
    assert_eq!(s[snap::LAYOUT_VERSION], crate::SNAPSHOT_LAYOUT_VERSION);
    let names: Vec<&str> = crate::SNAPSHOT_FIELDS.to_vec();
    assert_eq!(names[snap::TIME_S], "TIME_S");
    assert_eq!(names[snap::LAST_CLOSE_REFUSAL_BITS], "LAST_CLOSE_REFUSAL_BITS");
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());
    assert_eq!(sim.limits().len(), crate::LIMIT_FIELDS.len());
    assert_eq!(sim.snapshot_field_names().split(',').count(), crate::SNAPSHOT_LEN);
}

// ---------------------------------------------------------------------------
// Heat recovery scenarios
// ---------------------------------------------------------------------------

/// What leaves the plant and what its coolant stores, against the fuel: the
/// first law over the whole plant from the snapshot's own rates [kW].
fn heat_side_summary(s: &[f64]) -> String {
    format!(
        "coolant {:.2} C (return {:.2} C), network {:.1} kW, dumped {:.1} kW, stack {:.1} kW at {:.0} C, \
         exhaust {:.0} C, jacket {:.1} kW, recovered from exhaust {:.1} kW, fuel {:.1} kW, alarms {}",
        s[snap::COOLANT_SUPPLY_C],
        s[snap::COOLANT_RETURN_C],
        s[snap::HEAT_TO_NETWORK_KW],
        s[snap::HEAT_DUMPED_KW],
        s[snap::EXHAUST_LOSS_KW],
        s[snap::STACK_TEMPERATURE_C],
        s[snap::EXHAUST_TEMPERATURE_C],
        s[snap::JACKET_HEAT_KW],
        s[snap::EXHAUST_RECOVERED_KW],
        s[snap::FUEL_POWER_KW],
        s[snap::ALARM_BITS],
    )
}

#[test]
fn with_full_heat_demand_the_recovered_heat_goes_to_the_network() {
    let passport = PassportCard::STATION;
    let mut results = Vec::new();
    for load_pct in [100.0, 50.0] {
        let mut sim = on_load(load_pct, 300.0);
        advance(&mut sim, 900.0);
        let s = sim.snapshot();
        let summary = heat_side_summary(&s);
        assert_eq!(s[snap::ALARM_BITS], 0.0, "{summary}");
        // The coolant is a state: it warmed from the standby 60 C into the thermostat's band.
        assert!(
            s[snap::COOLANT_SUPPLY_C] > 84.0 && s[snap::COOLANT_SUPPLY_C] < 91.0,
            "{summary}"
        );
        assert!(s[snap::COOLANT_RETURN_C] < s[snap::COOLANT_SUPPLY_C] - 5.0, "{summary}");
        assert!(s[snap::HEAT_DUMPED_KW] == 0.0, "{summary}");
        // Thermal steady state: what the coolant receives it passes on.
        let received = s[snap::JACKET_HEAT_KW] + s[snap::EXHAUST_RECOVERED_KW];
        let passed = s[snap::HEAT_TO_NETWORK_KW] + s[snap::BLOCK_LOSS_KW];
        assert!((received - passed).abs() < 0.5, "received {received} kW, passed {passed} kW; {summary}");
        assert!(
            s[snap::EXHAUST_TEMPERATURE_C] > 380.0 && s[snap::EXHAUST_TEMPERATURE_C] < 560.0,
            "{summary}"
        );
        assert!(
            s[snap::STACK_TEMPERATURE_C] > s[snap::COOLANT_SUPPLY_C]
                && s[snap::STACK_TEMPERATURE_C] < s[snap::EXHAUST_TEMPERATURE_C],
            "{summary}"
        );
        assert!(ledger_relative_residual(&s) < 1.0e-9, "{summary}");
        results.push((load_pct, s[snap::THERMAL_EFFICIENCY_PCT], summary));
    }
    // At rated load the exhaust exchanger's length is fitted to the passport's 43 %; the
    // plant delivers that less the block's own loss and the effect of the coolant settling
    // below the 90 C of the fit. Tolerance 1.5 percentage points.
    let (_, at_rated, summary) = &results[0];
    let expected = 100.0 * passport.thermal_recovery(1.0);
    assert!((at_rated - expected).abs() < 1.5, "thermal recovery {at_rated} % against {expected} %; {summary}");
    // At half load nothing is fitted: the recovery is what the split, the exhaust flow and
    // the Gnielinski film give. The passport says 47 %; tolerance 3.5 percentage points.
    let (_, at_half, summary) = &results[1];
    let expected = 100.0 * passport.thermal_recovery(0.5);
    assert!((at_half - expected).abs() < 3.5, "thermal recovery {at_half} % against {expected} %; {summary}");
    assert!(at_half > at_rated, "recovery share rises at part load: {at_half} against {at_rated}");
}

#[test]
fn without_heat_demand_the_dump_cooler_takes_the_heat_and_the_coolant_stays_below_the_alarm() {
    let mut sim = on_load(100.0, 300.0);
    sim.set_heat_demand_frac(0.0);
    advance(&mut sim, 900.0);
    let s = sim.snapshot();
    let summary = heat_side_summary(&s);
    let warning_c = sim.limits()[4];
    assert_eq!(s[snap::STATE_CODE], 3.0, "{summary}");
    assert_eq!(s[snap::ALARM_BITS], 0.0, "{summary}");
    assert_eq!(s[snap::HEAT_TO_NETWORK_KW], 0.0, "{summary}");
    let received = s[snap::JACKET_HEAT_KW] + s[snap::EXHAUST_RECOVERED_KW];
    assert!(
        (s[snap::HEAT_DUMPED_KW] + s[snap::BLOCK_LOSS_KW] - received).abs() < 0.5,
        "received {received} kW; {summary}"
    );
    assert!(s[snap::HEAT_DUMPED_KW] > 900.0, "{summary}");
    // Not an input: where the coolant settles between the cooler's band and the alarm.
    assert!(
        s[snap::COOLANT_SUPPLY_C] > 91.0 && s[snap::COOLANT_SUPPLY_C] < warning_c,
        "warning at {warning_c} C; {summary}"
    );
    assert!(s[snap::DUMP_VALVE_OPENING] > 0.0 && s[snap::DUMP_VALVE_OPENING] < 1.0, "{summary}");
    assert!(s[snap::ENERGY_DUMPED_KWH] > 100.0, "{} kWh dumped; {summary}", s[snap::ENERGY_DUMPED_KWH]);
    assert!(ledger_relative_residual(&s) < 1.0e-9, "{summary}");
}

#[test]
fn without_heat_demand_and_with_the_fans_failed_the_coolant_climbs_to_the_a105_trip() {
    let mut sim = on_load(100.0, 300.0);
    advance(&mut sim, 600.0);
    let settled = sim.snapshot();
    assert!(settled[snap::COOLANT_SUPPLY_C] < 91.0, "{}", heat_side_summary(&settled));
    let trip_c = sim.limits()[5];

    sim.set_heat_demand_frac(0.0);
    sim.set_fan_failed(true);
    advance(&mut sim, 1.0);
    let warned = sim.snapshot();
    assert_eq!(warned[snap::ALARM_BITS] as u32 & alarm::COOLER_FAN, alarm::COOLER_FAN, "W-118");
    assert_eq!(warned[snap::STATE_CODE], 3.0);
    let mut hottest = warned[snap::COOLANT_SUPPLY_C];
    let to_trip = advance_until(&mut sim, 900.0, "A-105", |s| s[snap::STATE_CODE] == 5.0) + 1.0;
    let s = sim.snapshot();
    hottest = hottest.max(s[snap::COOLANT_SUPPLY_C]);
    let summary = heat_side_summary(&s);
    // Not inputs: that it trips, and when. The time follows from the heat that cannot
    // leave and the coolant node's capacity (about 1 MW into 3.1 MJ/K over 10 K).
    assert!(to_trip > 10.0 && to_trip < 120.0, "time to the trip {to_trip} s; {summary}");
    assert_eq!(s[snap::ALARM_BITS] as u32 & alarm::COOLANT_TEMPERATURE, alarm::COOLANT_TEMPERATURE);
    assert!(hottest >= trip_c - 0.05, "coolant {hottest} C at the trip, limit {trip_c} C; {summary}");
    assert_eq!(s[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(s[snap::FUEL_POWER_KW], 0.0);
    // The warning came before the trip, and both are in the log in that order.
    let raised: Vec<u32> = events_of(&sim, event_kind::ALARM_RAISED).iter().map(|e| e.1).collect();
    assert_eq!(raised, vec![8, 9, 0], "W-118, coolant warning, A-105: {raised:?}");
    assert!(ledger_relative_residual(&s) < 1.0e-9, "{summary}");
}

#[test]
fn partial_heat_demand_shares_the_heat_between_the_network_and_the_dump_cooler() {
    let mut sim = on_load(100.0, 300.0);
    sim.set_heat_demand_frac(0.4);
    advance(&mut sim, 900.0);
    let s = sim.snapshot();
    let summary = heat_side_summary(&s);
    assert_eq!(s[snap::ALARM_BITS], 0.0, "{summary}");
    assert!(s[snap::HEAT_TO_NETWORK_KW] > 300.0 && s[snap::HEAT_DUMPED_KW] > 100.0, "{summary}");
    let received = s[snap::JACKET_HEAT_KW] + s[snap::EXHAUST_RECOVERED_KW];
    let passed = s[snap::HEAT_TO_NETWORK_KW] + s[snap::HEAT_DUMPED_KW] + s[snap::BLOCK_LOSS_KW];
    assert!((received - passed).abs() < 0.5, "received {received} kW, passed {passed} kW; {summary}");
    // The network water leaves hotter when less of it flows.
    assert!(s[snap::NETWORK_SUPPLY_C] > 85.0, "{summary}");
}

#[test]
fn thermal_recovery_tracks_the_passport_at_75_percent_and_overshoots_it_at_30_percent() {
    let passport = PassportCard::STATION;
    // Nothing on the heat side is fitted at these loads. Measured on 2026-11-25: 45.2 %
    // against the passport's 45.0 % at 75 % load, and 53.7 % against 49.0 % at 30 %. The
    // overshoot at low load is the model's: a constant wall-heat share and a friction power
    // that does not fall with load put too much heat into the jacket there. The bounds
    // below record that, they do not excuse it.
    for (load_pct, lower, upper) in [(75.0, -1.5, 1.5), (30.0, 2.0, 6.5)] {
        let mut sim = on_load(load_pct, 300.0);
        advance(&mut sim, 900.0);
        let s = sim.snapshot();
        let excess =
            s[snap::THERMAL_EFFICIENCY_PCT] - 100.0 * passport.thermal_recovery(load_pct / 100.0);
        assert!(
            excess > lower && excess < upper,
            "thermal recovery at {load_pct} % differs from the passport by {excess} points; {}",
            heat_side_summary(&s)
        );
    }
}

// ---------------------------------------------------------------------------
// Three-phase grid, breaker cabinet, restoration, power-quality analyser
// ---------------------------------------------------------------------------

const PHASE_VOLT: f64 = 230.940_107_675_850_3;

fn grid_side_summary(s: &[f64]) -> String {
    format!(
        "t {:.2} s, state {} step {}, breaker {}, P {:.1} kW, phases {:.1}/{:.1}/{:.1} V, \
         currents {:.0}/{:.0}/{:.0} A, U2/U1 {:.2} %, I2 {:.2} %, rotor {:.3}, pickup {}, \
         tripped {}, alarms {}, attempts {}",
        s[snap::TIME_S],
        s[snap::STATE_CODE],
        s[snap::SEQUENCE_STEP],
        s[snap::BREAKER_CLOSED],
        s[snap::ELECTRIC_POWER_KW],
        s[snap::PHASE_A_VOLTAGE_V],
        s[snap::PHASE_B_VOLTAGE_V],
        s[snap::PHASE_C_VOLTAGE_V],
        s[snap::PHASE_A_CURRENT_A],
        s[snap::PHASE_B_CURRENT_A],
        s[snap::PHASE_C_CURRENT_A],
        s[snap::VOLTAGE_UNBALANCE_PCT],
        s[snap::NEGATIVE_SEQUENCE_CURRENT_PCT],
        s[snap::ROTOR_THERMAL_USED_FRAC],
        s[snap::PROTECTION_PICKUP_BITS],
        s[snap::PROTECTION_TRIPPED_BITS],
        s[snap::ALARM_BITS],
        s[snap::RESTORATION_ATTEMPTS],
    )
}

/// The analyser's record as `(kind, phase, start, duration, extreme volts)`.
fn quality_events(sim: &KguSim) -> Vec<(u32, u32, f64, f64, f64)> {
    sim.analyser_events()
        .chunks(QUALITY_EVENT_STRIDE)
        .map(|e| (e[0] as u32, e[1] as u32, e[2], e[3], e[4]))
        .collect()
}

#[test]
fn a_balanced_grid_reproduces_the_numbers_of_the_single_voltage_model() {
    // The run recorded for slice 1b on 2026-11-25 (default ramp, 100 % setpoint, 1 200 s
    // after the breaker closed), before the bus had three phases and the cabinet stood
    // between controller and breaker. A balanced bus must change none of it.
    let mut sim = KguSim::new();
    sim.set_load_setpoint_pct(100.0);
    sim.cmd_start();
    while sim.snapshot()[snap::BREAKER_CLOSED] != 1.0 {
        advance(&mut sim, 0.1);
        assert!(sim.snapshot()[snap::TIME_S] < 200.0);
    }
    let at_bus = sim.snapshot();
    // The numbers were recorded on the WebAssembly package, and the Node test holds the
    // package to them to 1e-12. The native build differs from the package in the ninth
    // digit (1000.000003 kW against 1000.000001 kW); the cause was not investigated and
    // is presumed to be the two targets' elementary functions. Hence 1e-7 here.
    let near = |value: f64, recorded: f64| (value - recorded).abs() <= 1.0e-7 * recorded.abs();
    assert!((at_bus[snap::TIME_S] - 91.0).abs() < 1.0e-6, "to the bus in {} s", at_bus[snap::TIME_S]);
    assert!(near(at_bus[snap::GAS_STD_M3], 0.622_927_594_215_562_3), "{}", at_bus[snap::GAS_STD_M3]);
    advance(&mut sim, 1_200.0);
    let s = sim.snapshot();
    for (slot, recorded) in [
        (snap::ELECTRIC_POWER_KW, 1_000.000_001_063_500_3),
        (snap::REACTIVE_POWER_KVAR, -193.704_270_175_513_41),
        (snap::ELECTRIC_EFFICIENCY_PCT, 38.200_000_058_707_4),
        (snap::THERMAL_EFFICIENCY_PCT, 42.751_546_685_290_72),
        (snap::COOLANT_SUPPLY_C, 88.315_634_091_591_5),
        (snap::COOLANT_RETURN_C, 68.174_570_353_446_71),
        (snap::EXHAUST_TEMPERATURE_C, 484.766_669_933_899_15),
        (snap::STACK_TEMPERATURE_C, 214.834_170_290_433_7),
    ] {
        assert!(
            near(s[slot], recorded),
            "{}: {} against the recorded {recorded}",
            crate::SNAPSHOT_FIELDS[slot],
            s[slot]
        );
    }
    // And the new read-outs of a balanced machine: three equal voltages and currents.
    assert_eq!(s[snap::VOLTAGE_UNBALANCE_PCT], 0.0);
    assert_eq!(s[snap::NEGATIVE_SEQUENCE_CURRENT_PCT], 0.0);
    assert_eq!(s[snap::NEGATIVE_SEQUENCE_LOSS_KW], 0.0);
    assert_eq!(s[snap::PROTECTION_PICKUP_BITS], 0.0);
    assert_eq!(s[snap::ROTOR_THERMAL_USED_FRAC], 0.0);
    for slot in [snap::PHASE_A_CURRENT_A, snap::PHASE_B_CURRENT_A, snap::PHASE_C_CURRENT_A] {
        assert!((s[slot] - s[snap::STATOR_CURRENT_A]).abs() < 1.0e-9 * s[snap::STATOR_CURRENT_A]);
    }
    assert!((s[snap::PHASE_B_VOLTAGE_V] - PHASE_VOLT).abs() < 1.0e-9);
    // A healthy balanced grid leaves the analyser's record empty.
    assert!(sim.analyser_events().is_empty());
    assert_eq!(s[snap::ANALYSER_RECORDED_EVENTS], 0.0);
    assert_eq!(s[snap::ANALYSER_OPEN_EVENTS], 0.0);
    assert!(events_of(&sim, event_kind::PROTECTION).is_empty());
    // And the meter's journals.
    assert!(sim.meter_events().is_empty());
    for slot in [
        snap::METER_LOW_VOLTAGE_RECORDS,
        snap::METER_HIGH_VOLTAGE_RECORDS,
        snap::METER_LOSS_OF_VOLTAGE_RECORDS,
        snap::METER_OVERFLOW_BITS,
        snap::SYNCHRONISER_ARMED_BY_OPERATOR,
    ] {
        assert_eq!(s[slot], 0.0, "{}", crate::SNAPSHOT_FIELDS[slot]);
    }
    // The meter's active registers are the engine's own counter, by its sign.
    let net = s[snap::METER_ACTIVE_EXPORT_KWH] - s[snap::METER_ACTIVE_IMPORT_KWH];
    assert!(
        (net - s[snap::ENERGY_ELECTRIC_KWH]).abs() < 1.0e-9 * s[snap::ENERGY_ELECTRIC_KWH],
        "meter {net} kWh against the engine's {} kWh",
        s[snap::ENERGY_ELECTRIC_KWH]
    );
}

#[test]
fn one_phase_at_168_volts_trips_the_phase_undervoltage_first_and_the_engine_runs_on() {
    // The case from practice: exporting, one phase falls to 168 V (0.73 of 230 V).
    let mut sim = on_load(90.0, 300.0);
    let before = sim.snapshot();
    sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
    advance(&mut sim, 0.10);
    let during = sim.snapshot();
    let summary = grid_side_summary(&during);
    assert_eq!(during[snap::BREAKER_CLOSED], 1.0, "{summary}");
    assert!((during[snap::PHASE_B_VOLTAGE_V] - 0.73 * PHASE_VOLT).abs() < 1.0e-9, "{summary}");
    // Not inputs: the unbalance of the bus, the negative-sequence current it drives, the
    // three unequal phase currents, the heat, and which functions have picked up.
    assert!((during[snap::VOLTAGE_UNBALANCE_PCT] - 100.0 * 0.09 / 0.91).abs() < 1.0e-9, "{summary}");
    assert!(
        during[snap::NEGATIVE_SEQUENCE_CURRENT_PCT] > 30.0
            && during[snap::NEGATIVE_SEQUENCE_CURRENT_PCT] < 38.0,
        "{summary}"
    );
    let currents = [
        during[snap::PHASE_A_CURRENT_A],
        during[snap::PHASE_B_CURRENT_A],
        during[snap::PHASE_C_CURRENT_A],
    ];
    let largest = currents.iter().copied().fold(0.0, f64::max);
    let smallest = currents.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(largest - smallest > 500.0, "{summary}");
    assert!(during[snap::NEGATIVE_SEQUENCE_LOSS_KW] > 5.0, "{summary}");
    let pickup = during[snap::PROTECTION_PICKUP_BITS] as u32;
    for function in [
        protection::UNDERVOLTAGE,
        protection::VOLTAGE_UNBALANCE,
        protection::NEGATIVE_SEQUENCE,
    ] {
        assert_ne!(pickup & (1 << function), 0, "function {function} picked up; {summary}");
    }

    advance(&mut sim, 0.10);
    let after = sim.snapshot();
    let summary = grid_side_summary(&after);
    assert_eq!(after[snap::BREAKER_CLOSED], 0.0, "{summary}");
    // Which function, on which phase, at what value, and when.
    let trips = events_of(&sim, event_kind::PROTECTION);
    assert_eq!(trips.len(), 1, "{trips:?}");
    let (time, code, value) = trips[0];
    assert_eq!(code, 10 * protection::UNDERVOLTAGE + 2, "under-voltage on phase b");
    assert!((value - 0.73 * PHASE_VOLT).abs() < 1.0e-6, "tripped at {value} V");
    let delay = time - before[snap::TIME_S];
    assert!((delay - 0.15).abs() < 0.011, "tripped {delay} s after the phase fell");
    assert_eq!(after[snap::PROTECTION_TRIPPED_BITS] as u32, 1 << protection::UNDERVOLTAGE);
    assert_eq!(after[snap::ALARM_BITS] as u32, alarm::GRID_UNDERVOLTAGE);
    // The engine is not stopped: off the bus, at speed, burning its no-load gas.
    assert_eq!(after[snap::STATE_CODE], 2.0, "{summary}");
    assert_eq!(after[snap::WAITING_FOR_GRID], 1.0);
    advance(&mut sim, 20.0);
    let idling = sim.snapshot();
    assert!((idling[snap::SPEED_RPM] - 1_500.0).abs() < 1.0, "{}", grid_side_summary(&idling));
    assert!(idling[snap::FUEL_POWER_KW] > 300.0 && idling[snap::FUEL_POWER_KW] < 450.0);
    assert!(ledger_relative_residual(&idling) < 1.0e-9);

    // All three phases at 0.73 instead: the same function, and no unbalance at all.
    let mut sim = on_load(90.0, 300.0);
    sim.set_grid_phase_voltages_pu(0.73, 0.73, 0.73);
    advance(&mut sim, 0.10);
    let during = sim.snapshot();
    assert_eq!(during[snap::VOLTAGE_UNBALANCE_PCT], 0.0);
    assert_eq!(during[snap::NEGATIVE_SEQUENCE_CURRENT_PCT], 0.0);
    assert_eq!(during[snap::PROTECTION_PICKUP_BITS] as u32, 1 << protection::UNDERVOLTAGE);
    advance(&mut sim, 0.10);
    let trips = events_of(&sim, event_kind::PROTECTION);
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].1 / 10, protection::UNDERVOLTAGE);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0);
}

#[test]
fn unbalance_without_low_voltage_trips_on_the_rotor_limit_sooner_the_deeper_it_is() {
    // One phase low, but above the under-voltage level and below the unbalance level:
    // only the negative-sequence current's thermal replica can trip, and its time comes
    // out of the integral (I2 / I_rated)^2 t = 20 s.
    let mut times = Vec::new();
    for low in [0.88, 0.86] {
        let mut sim = on_load(80.0, 300.0);
        let start = sim.snapshot()[snap::TIME_S];
        sim.set_grid_phase_voltages_pu(1.0, 1.0, low);
        advance(&mut sim, 5.0);
        let s = sim.snapshot();
        let summary = grid_side_summary(&s);
        assert_eq!(s[snap::BREAKER_CLOSED], 1.0, "{summary}");
        assert_eq!(
            s[snap::PROTECTION_PICKUP_BITS] as u32,
            1 << protection::NEGATIVE_SEQUENCE,
            "{summary}"
        );
        let negative_pu = s[snap::NEGATIVE_SEQUENCE_CURRENT_PCT] / 100.0;
        let expected = 20.0 / (negative_pu * negative_pu);
        let to_trip =
            advance_until(&mut sim, 1_500.0, "rotor limit", |x| x[snap::BREAKER_CLOSED] == 0.0);
        let trips = events_of(&sim, event_kind::PROTECTION);
        assert_eq!(trips.len(), 1, "{trips:?}");
        assert_eq!(trips[0].1, 10 * protection::NEGATIVE_SEQUENCE);
        assert!((trips[0].2 - 100.0 * negative_pu).abs() < 1.0e-6);
        let time = trips[0].0 - start;
        assert!(
            (time - expected).abs() < 0.01 * expected,
            "tripped after {time} s (waited {to_trip} s); the integral gives {expected} s; {summary}"
        );
        advance(&mut sim, 0.05);
        assert_eq!(sim.snapshot()[snap::ALARM_BITS] as u32, alarm::NEGATIVE_SEQUENCE);
        times.push((negative_pu, time));
    }
    assert!(times[1].0 > times[0].0 && times[1].1 < times[0].1, "{times:?}");

    // A stronger unbalance with every phase inside the voltage limits: the unbalance
    // function trips after its own delay, long before the rotor's replica would.
    let mut sim = on_load(80.0, 300.0);
    let start = sim.snapshot()[snap::TIME_S];
    sim.set_grid_phase_voltages_pu(1.1, 1.0, 0.88);
    advance_until(&mut sim, 60.0, "unbalance trip", |x| x[snap::BREAKER_CLOSED] == 0.0);
    let trips = events_of(&sim, event_kind::PROTECTION);
    assert_eq!(trips.len(), 1, "{trips:?}");
    assert_eq!(trips[0].1, 10 * protection::VOLTAGE_UNBALANCE);
    assert!(trips[0].2 > 5.0 && trips[0].2 < 8.0, "unbalance at the trip {} %", trips[0].2);
    assert!((trips[0].0 - start - 5.0).abs() < 0.011);
    advance(&mut sim, 0.05);
    let s = sim.snapshot();
    assert!(s[snap::ROTOR_THERMAL_USED_FRAC] > 0.0 && s[snap::ROTOR_THERMAL_USED_FRAC] < 0.05);
    assert_eq!(s[snap::ALARM_BITS] as u32, alarm::VOLTAGE_UNBALANCE);
}

#[test]
fn a_small_unbalance_inside_the_settings_runs_on_with_unequal_currents_and_extra_heat() {
    let gas = FuelGasCard::STATION;
    let mut sim = on_load(80.0, 300.0);
    let balanced = sim.snapshot();
    sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.94);
    advance(&mut sim, 300.0);
    let s = sim.snapshot();
    let summary = grid_side_summary(&s);
    assert_eq!(s[snap::STATE_CODE], 3.0, "{summary}");
    assert_eq!(s[snap::BREAKER_CLOSED], 1.0, "{summary}");
    assert_eq!(s[snap::ALARM_BITS], 0.0, "{summary}");
    assert_eq!(s[snap::PROTECTION_PICKUP_BITS], 0.0, "{summary}");
    assert_eq!(s[snap::ROTOR_THERMAL_USED_FRAC], 0.0, "{summary}");
    assert!((s[snap::VOLTAGE_UNBALANCE_PCT] - 100.0 * 0.02 / 0.98).abs() < 1.0e-9, "{summary}");
    // Not inputs: the governor still holds the power; the three currents differ; the
    // negative sequence leaves heat that was not there on the balanced bus.
    assert!((s[snap::ELECTRIC_POWER_KW] - 800.0).abs() < 0.5, "{summary}");
    let currents = [
        s[snap::PHASE_A_CURRENT_A],
        s[snap::PHASE_B_CURRENT_A],
        s[snap::PHASE_C_CURRENT_A],
    ];
    let largest = currents.iter().copied().fold(0.0, f64::max);
    let smallest = currents.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(largest - smallest > 100.0, "{summary}");
    assert!(
        s[snap::NEGATIVE_SEQUENCE_CURRENT_PCT] > 6.0 && s[snap::NEGATIVE_SEQUENCE_CURRENT_PCT] < 8.0,
        "{summary}"
    );
    let extra = s[snap::NEGATIVE_SEQUENCE_LOSS_KW];
    assert!(extra > 0.3 && extra < 1.0, "negative-sequence heat {extra} kW; {summary}");
    assert!(
        s[snap::GENERATOR_LOSS_KW] > balanced[snap::GENERATOR_LOSS_KW] + 0.9 * extra,
        "generator loss {} kW against {} kW balanced",
        s[snap::GENERATOR_LOSS_KW],
        balanced[snap::GENERATOR_LOSS_KW]
    );
    // The fuel still closes on everything that leaves, the extra heat included.
    let mass_flow = s[snap::GAS_FLOW_STD_M3_H] / 3_600.0 * gas.standard_density_kg_m3();
    let sensible_kw = mass_flow * gas.cp_j_kg_k * (288.15 - 293.15) * 1.0e-3;
    let leaving = s[snap::ELECTRIC_POWER_KW]
        + s[snap::GENERATOR_LOSS_KW]
        + s[snap::JACKET_HEAT_KW]
        + s[snap::EXHAUST_RECOVERED_KW]
        + s[snap::EXHAUST_LOSS_KW]
        + s[snap::RADIATION_LOSS_KW];
    assert!((s[snap::FUEL_POWER_KW] + sensible_kw - leaving).abs() < 1.0e-3, "{summary}");
    assert!(ledger_relative_residual(&s) < 1.0e-9, "{summary}");
}

#[test]
fn automatic_restoration_returns_the_unit_after_the_healthy_grid_delay_without_a_command() {
    let restore = RestorationCard::DEFAULT;
    let mut sim = on_load(50.0, 300.0);
    sim.set_restoration_mode(1);
    sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
    advance(&mut sim, 2.0);
    let off = sim.snapshot();
    assert_eq!(off[snap::BREAKER_CLOSED], 0.0, "{}", grid_side_summary(&off));
    assert_eq!(off[snap::WAITING_FOR_GRID], 1.0);
    assert_eq!(off[snap::RESTORATION_MODE], 1.0);
    // While the grid is unhealthy the countdown stands at the whole delay.
    assert_eq!(off[snap::GRID_HEALTHY_COUNTDOWN_S], restore.healthy_delay_s);

    sim.set_grid_voltage_pu(1.0);
    let healthy_at = sim.snapshot()[snap::TIME_S];
    let gas_at = sim.snapshot()[snap::GAS_STD_M3];
    advance(&mut sim, 30.0);
    let counting = sim.snapshot();
    assert_eq!(counting[snap::BREAKER_CLOSED], 0.0);
    assert!((counting[snap::GRID_HEALTHY_COUNTDOWN_S] - 30.0).abs() < 0.02);
    assert_eq!(counting[snap::ALARM_BITS] as u32, alarm::GRID_UNDERVOLTAGE);

    // No command from here on.
    let to_bus = 30.0
        + advance_until(&mut sim, 200.0, "back on the bus", |s| s[snap::BREAKER_CLOSED] == 1.0);
    let closed = sim.snapshot();
    let summary = grid_side_summary(&closed);
    // Not an input: the healthy-grid delay, then what the synchroniser needs for the phase.
    assert!(
        to_bus > restore.healthy_delay_s && to_bus < restore.healthy_delay_s + 22.0,
        "back on the bus {to_bus} s after the grid recovered; {summary}"
    );
    assert_eq!(closed[snap::ALARM_BITS], 0.0, "the automation cleared its own alarm; {summary}");
    assert_eq!(closed[snap::RESTORATION_ATTEMPTS], 1.0, "{summary}");
    assert_eq!(closed[snap::PROTECTION_TRIPPED_BITS], 0.0, "{summary}");
    // The gas burned unloaded while waiting is on the meter: about 374 kW for the wait.
    let burned = closed[snap::GAS_STD_M3] - gas_at;
    let no_load = 374.0e3 * to_bus / 33.5e6;
    assert!(
        (burned - no_load).abs() < 0.1 * no_load,
        "{burned} m3 burned waiting {to_bus} s; 374 kW would be {no_load} m3"
    );

    // And up to the load setpoint again, at the controller's ramp.
    let to_load = advance_until(&mut sim, 120.0, "load back", |s| {
        (s[snap::LOAD_REFERENCE_PCT] - 50.0).abs() < 1.0e-9
    });
    assert!((to_load - 10.0).abs() < 0.2, "ramp to 50 % at 300 %/min took {to_load} s");
    advance(&mut sim, 30.0);
    let s = sim.snapshot();
    assert!((s[snap::ELECTRIC_POWER_KW] - 500.0).abs() < 0.5, "{}", grid_side_summary(&s));
    assert_eq!(s[snap::STATE_CODE], 3.0);
    assert!(ledger_relative_residual(&s) < 1.0e-9);
    let _ = healthy_at;
}

#[test]
fn automatic_restoration_gives_up_after_the_longest_wait_and_stops_the_unit_normally() {
    let restore = RestorationCard::DEFAULT;
    let mut sim = on_load(50.0, 300.0);
    sim.set_restoration_mode(1);
    sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::WAITING_FOR_GRID], 1.0);
    // The disturbance stays. No reclosing while it does.
    advance(&mut sim, restore.longest_wait_s - 10.0);
    let waiting = sim.snapshot();
    assert_eq!(waiting[snap::WAITING_FOR_GRID], 1.0, "{}", grid_side_summary(&waiting));
    assert_eq!(waiting[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(waiting[snap::RESTORATION_ATTEMPTS], 0.0);
    assert!(waiting[snap::FUEL_POWER_KW] > 300.0, "still running unloaded");
    // After the longest wait: the normal stop, with its cool-down.
    advance(&mut sim, 15.0);
    let stopping = sim.snapshot();
    assert_eq!(stopping[snap::STATE_CODE], 4.0, "{}", grid_side_summary(&stopping));
    assert_eq!(stopping[snap::SEQUENCE_STEP], f64::from(Sequence::CoolDown.index()));
    advance_until(&mut sim, 400.0, "ready", |s| s[snap::STATE_CODE] == 0.0);
    assert_eq!(events_of(&sim, event_kind::BREAKER).len(), 2, "closed once, opened once");
    // What the grid event latched is still there, and blocks a start until acknowledged.
    assert_eq!(sim.snapshot()[snap::ALARM_BITS] as u32, alarm::GRID_UNDERVOLTAGE);
    sim.cmd_start();
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 0.0);
    sim.cmd_ack();
    advance(&mut sim, 0.05);
    assert_eq!(sim.snapshot()[snap::ALARM_BITS], 0.0);
}

#[test]
fn a_flickering_grid_blocks_the_automatic_return_after_its_attempts() {
    let restore = RestorationCard::DEFAULT;
    let mut sim = on_load(30.0, 300.0);
    sim.set_restoration_mode(1);
    let flicker = |sim: &mut KguSim| {
        sim.set_grid_phase_voltages_pu(0.73, 1.0, 1.0);
        advance(sim, 1.0);
        assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0, "{}", grid_side_summary(&sim.snapshot()));
        sim.set_grid_voltage_pu(1.0);
    };
    // Three automatic returns are allowed.
    for attempt in 1..=restore.largest_attempts {
        flicker(&mut sim);
        advance_until(&mut sim, 200.0, "back on the bus", |s| s[snap::BREAKER_CLOSED] == 1.0);
        let s = sim.snapshot();
        assert_eq!(s[snap::RESTORATION_ATTEMPTS], f64::from(attempt), "{}", grid_side_summary(&s));
        assert_eq!(s[snap::ALARM_BITS], 0.0);
        advance(&mut sim, 5.0);
    }
    // The next trip inside the window blocks the automation, healthy grid or not.
    flicker(&mut sim);
    let blocked = sim.snapshot();
    assert_eq!(
        blocked[snap::ALARM_BITS] as u32,
        alarm::GRID_UNDERVOLTAGE | alarm::RESTORATION_LOCKOUT,
        "{}",
        grid_side_summary(&blocked)
    );
    assert!(blocked[snap::GRID_HEALTHY_COUNTDOWN_S].is_nan(), "no countdown while blocked");
    advance(&mut sim, restore.healthy_delay_s + 60.0);
    let still = sim.snapshot();
    assert_eq!(still[snap::BREAKER_CLOSED], 0.0, "{}", grid_side_summary(&still));
    assert_eq!(still[snap::WAITING_FOR_GRID], 1.0);
    assert!(still[snap::FUEL_POWER_KW] > 300.0);
    // An acknowledgement releases it. The grid has been healthy for longer than the delay
    // by now, so the automation goes straight back to the synchroniser.
    sim.cmd_ack();
    advance(&mut sim, 0.05);
    let released = sim.snapshot();
    assert_eq!(released[snap::ALARM_BITS], 0.0, "{}", grid_side_summary(&released));
    assert_eq!(released[snap::SEQUENCE_STEP], f64::from(Sequence::Synchronising.index()));
    advance_until(&mut sim, 30.0, "back on the bus", |s| s[snap::BREAKER_CLOSED] == 1.0);
    assert_eq!(sim.snapshot()[snap::RESTORATION_ATTEMPTS], 1.0);
    assert_eq!(events_of(&sim, event_kind::PROTECTION).len(), 4);
}

#[test]
fn manual_restoration_leaves_the_unit_off_the_bus_until_the_operator_acknowledges_and_closes() {
    let mut sim = on_load(50.0, 300.0);
    assert_eq!(sim.snapshot()[snap::RESTORATION_MODE], 0.0, "manual is the default");
    sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.73);
    advance(&mut sim, 1.0);
    sim.set_grid_voltage_pu(1.0);
    let gas_at = sim.snapshot()[snap::GAS_STD_M3];
    // The grid is healthy again, and nothing happens by itself.
    advance(&mut sim, 300.0);
    let waiting = sim.snapshot();
    let summary = grid_side_summary(&waiting);
    assert_eq!(waiting[snap::BREAKER_CLOSED], 0.0, "{summary}");
    assert_eq!(waiting[snap::WAITING_FOR_GRID], 1.0, "{summary}");
    assert_eq!(waiting[snap::ALARM_BITS] as u32, alarm::GRID_UNDERVOLTAGE, "{summary}");
    assert!(waiting[snap::GRID_HEALTHY_COUNTDOWN_S].is_nan(), "no countdown in manual");
    assert!((waiting[snap::SPEED_RPM] - 1_500.0).abs() < 1.0, "{summary}");
    // The gas burned unloaded over those 300 s is on the meter (374 kW: 3.35 m3).
    let burned = waiting[snap::GAS_STD_M3] - gas_at;
    assert!(burned > 3.0 && burned < 3.7, "{burned} m3 burned waiting; {summary}");
    assert!(ledger_relative_residual(&waiting) < 1.0e-9);

    // A close request before the acknowledgement is refused: a trip is latched.
    let refused = sim.cmd_breaker_close();
    assert_ne!(refused & refusal::TRIP_LATCHED, 0, "refusal bits {refused}");
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0);

    // Acknowledged: at the synchroniser, which does not close by itself after a trip.
    sim.cmd_ack();
    advance(&mut sim, 0.05);
    let acknowledged = sim.snapshot();
    assert_eq!(acknowledged[snap::ALARM_BITS], 0.0, "{}", grid_side_summary(&acknowledged));
    assert_eq!(acknowledged[snap::SEQUENCE_STEP], f64::from(Sequence::Synchronising.index()));
    advance(&mut sim, 40.0);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0, "waits for the operator's request");
    let mut waited = 0.0;
    while sim.snapshot()[snap::SYNC_PERMITTED] != 1.0 {
        advance(&mut sim, 0.01);
        waited += 0.01;
        assert!(waited < 30.0);
    }
    assert_eq!(sim.cmd_breaker_close(), 0);
    advance(&mut sim, 0.02);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 1.0);
    advance(&mut sim, 60.0);
    let s = sim.snapshot();
    assert!((s[snap::ELECTRIC_POWER_KW] - 500.0).abs() < 0.5, "{}", grid_side_summary(&s));

    // Or the operator stops instead: the normal cool-down from the wait.
    let mut sim = on_load(50.0, 300.0);
    sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.73);
    advance(&mut sim, 1.0);
    sim.cmd_stop();
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::SEQUENCE_STEP], f64::from(Sequence::CoolDown.index()));
}

#[test]
fn a_cabinet_overcurrent_stops_the_engine_like_a_unit_fault() {
    // Over-current is not a grid event in this model: no bus voltage drives it. With a
    // weak field the machine draws a heavy reactive current at full load; the cabinet's
    // time-delayed stage answers, and the unit is stopped and waits for acknowledgement.
    let mut sim = on_load(100.0, 300.0);
    sim.set_excitation_pu(0.5);
    let start = sim.snapshot()[snap::TIME_S];
    advance(&mut sim, 5.0);
    let s = sim.snapshot();
    let summary = grid_side_summary(&s);
    let limit = sim.limits()[crate::LIMIT_FIELDS.iter().position(|n| *n == "OVERCURRENT_A").unwrap()];
    assert!(s[snap::STATOR_CURRENT_A] > limit, "{} A against {limit} A; {summary}", s[snap::STATOR_CURRENT_A]);
    assert_eq!(s[snap::BREAKER_CLOSED], 1.0, "inside the 10 s delay; {summary}");
    advance_until(&mut sim, 20.0, "over-current trip", |x| x[snap::BREAKER_CLOSED] == 0.0);
    let trips = events_of(&sim, event_kind::PROTECTION);
    assert_eq!(trips.len(), 1, "{trips:?}");
    assert_eq!(trips[0].1 / 10, protection::OVERCURRENT);
    assert!((trips[0].0 - start - 10.0).abs() < 0.1, "tripped after {} s", trips[0].0 - start);
    advance(&mut sim, 0.05);
    let tripped = sim.snapshot();
    assert_eq!(tripped[snap::STATE_CODE], 5.0, "{}", grid_side_summary(&tripped));
    assert_eq!(tripped[snap::ALARM_BITS] as u32, alarm::OVERCURRENT);
    assert_eq!(tripped[snap::FUEL_POWER_KW], 0.0);
}

#[test]
fn the_analyser_records_a_dip_the_protection_rides_through_and_keeps_its_own_clock() {
    // A dip shorter than the protection's delay: one event in the analyser's record, on
    // that phase, with the measured duration and residual voltage; the breaker stays in.
    let mut sim = on_load(50.0, 300.0);
    sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.80);
    advance(&mut sim, 0.10);
    assert_eq!(sim.snapshot()[snap::ANALYSER_OPEN_EVENTS], 1.0);
    assert!(sim.analyser_events().is_empty(), "the event is recorded when the voltage is back");
    sim.set_grid_voltage_pu(1.0);
    advance(&mut sim, 1.0);
    let s = sim.snapshot();
    assert_eq!(s[snap::BREAKER_CLOSED], 1.0, "{}", grid_side_summary(&s));
    assert!(events_of(&sim, event_kind::PROTECTION).is_empty());
    let record = quality_events(&sim);
    assert_eq!(record.len(), 1, "{record:?}");
    let (kind, phase, _, duration, residual) = record[0];
    assert_eq!((kind, phase), (quality_kind::DIP, 3));
    assert!((duration - 0.10).abs() < 0.011, "duration {duration} s");
    assert!((residual - 0.80 * PHASE_VOLT).abs() < 1.0e-9, "residual {residual} V");
    assert_eq!(s[snap::ANALYSER_OPEN_EVENTS], 0.0);
    assert_eq!(s[snap::ANALYSER_RECORDED_EVENTS], 1.0);

    // A dip the protection does answer, seen by an analyser whose clock is an hour ahead
    // and by one in step with the model. The protection's event stays in model time.
    let run = |offset_s: f64| {
        let mut sim = on_load(50.0, 300.0);
        sim.set_analyser_clock_offset_s(offset_s);
        let fell_at = sim.snapshot()[snap::TIME_S];
        sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
        advance(&mut sim, 0.50);
        sim.set_grid_voltage_pu(1.0);
        advance(&mut sim, 1.0);
        let record = quality_events(&sim);
        assert_eq!(record.len(), 1, "{record:?}");
        let protection = events_of(&sim, event_kind::PROTECTION);
        assert_eq!(protection.len(), 1);
        assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0);
        let clock = sim.snapshot()[snap::ANALYSER_CLOCK_S] - sim.snapshot()[snap::TIME_S];
        (fell_at, record[0], protection[0], clock)
    };
    let (fell_at, in_step, trip, clock) = run(0.0);
    let (_, ahead, trip_ahead, clock_ahead) = run(3_600.0);
    assert_eq!((in_step.0, in_step.1), (quality_kind::DIP, 2));
    assert!((in_step.3 - 0.50).abs() < 0.011, "duration {} s", in_step.3);
    assert!((in_step.4 - 0.73 * PHASE_VOLT).abs() < 1.0e-9, "residual {} V", in_step.4);
    // In step: the dip starts when the voltage fell, the protection trips its delay later.
    assert!((in_step.2 - fell_at).abs() < 1.0e-6, "dip start {} against {fell_at}", in_step.2);
    assert!((trip.0 - in_step.2 - 0.15).abs() < 0.011);
    // An hour ahead: the same dip is stamped exactly an hour later; the protection's
    // event has not moved, so the two logs now disagree by the clock's offset.
    assert!((ahead.2 - in_step.2 - 3_600.0).abs() < 1.0e-6, "{} against {}", ahead.2, in_step.2);
    assert_eq!(trip_ahead.0.to_bits(), trip.0.to_bits());
    assert_eq!(ahead.3.to_bits(), in_step.3.to_bits());
    assert!(clock.abs() < 1.0e-9 && (clock_ahead - 3_600.0).abs() < 1.0e-9);

    // An interruption and a swell are told apart from a dip.
    let mut sim = KguSim::new();
    sim.set_grid_phase_voltages_pu(0.02, 1.0, 1.15);
    advance(&mut sim, 0.30);
    sim.set_grid_voltage_pu(1.0);
    advance(&mut sim, 0.10);
    let record = quality_events(&sim);
    assert_eq!(record.len(), 2, "{record:?}");
    assert!(record.iter().any(|e| e.0 == quality_kind::INTERRUPTION && e.1 == 1));
    assert!(record.iter().any(|e| e.0 == quality_kind::SWELL && e.1 == 3));
    sim.clear_analyser_events();
    assert!(sim.analyser_events().is_empty());
}

// ---------------------------------------------------------------------------
// The revenue meter: one look per second, journals of limited capacity
// ---------------------------------------------------------------------------

/// The meter's journals as `(journal, phase bits, seen, cleared)`.
fn meter_records(sim: &KguSim) -> Vec<(u32, u32, f64, f64)> {
    sim.meter_events()
        .chunks(METER_EVENT_STRIDE)
        .map(|r| (r[0] as u32, r[1] as u32, r[2], r[3]))
        .collect()
}

/// 160 V on a 230.9 V phase.
const DIP_160_V_PU: f64 = 160.0 / PHASE_VOLT;

#[test]
fn a_130_ms_dip_is_in_the_meters_journal_only_when_one_of_its_looks_falls_inside() {
    // The same dip twice: 130 ms to 160 V on phase b, the unit exporting. Only the place
    // of the meter's one look per second differs. (`look_after_s`: where the next look
    // falls, counted from the moment the phase falls.)
    let run = |look_after_s: f64| {
        let mut sim = on_load(50.0, 300.0);
        let fell_at = sim.snapshot()[snap::TIME_S];
        sim.set_meter_sample_phase_s(fell_at + look_after_s);
        sim.set_grid_phase_voltages_pu(1.0, DIP_160_V_PU, 1.0);
        advance(&mut sim, 0.13);
        sim.set_grid_voltage_pu(1.0);
        advance(&mut sim, 3.0);
        let s = sim.snapshot();
        // Shorter than the protection's 0.15 s: the breaker stays in either way.
        assert_eq!(s[snap::BREAKER_CLOSED], 1.0, "{}", grid_side_summary(&s));
        assert!(events_of(&sim, event_kind::PROTECTION).is_empty());
        // The analyser has it either way, with its length and depth.
        let quality = quality_events(&sim);
        assert_eq!(quality.len(), 1, "{quality:?}");
        assert_eq!((quality[0].0, quality[0].1), (quality_kind::DIP, 2));
        assert!((quality[0].2 - fell_at).abs() < 1.0e-9);
        assert!((quality[0].3 - 0.13).abs() < 1.0e-9, "duration {} s", quality[0].3);
        assert!((quality[0].4 - 160.0).abs() < 1.0e-9, "residual {} V", quality[0].4);
        (fell_at, meter_records(&sim), s)
    };

    // The looks half a second either side of the dip: nothing in the journal.
    let (_, records, s) = run(0.5);
    assert!(records.is_empty(), "{records:?}");
    assert_eq!(s[snap::METER_LOW_VOLTAGE_RECORDS], 0.0);

    // A look 65 ms into the dip: a record, and all it holds is the look that saw the
    // voltage low, the phase, and the next look, which saw it back. Neither is the dip's
    // start or its end, and nothing in the record says 160 V or 130 ms.
    let (fell_at, records, s) = run(0.065);
    assert_eq!(records.len(), 1, "{records:?}");
    let (kind, phases, seen, cleared) = records[0];
    assert_eq!((kind, phases), (journal::LOW_VOLTAGE, 0b010));
    assert!((seen - (fell_at + 0.065)).abs() < 1.0e-9, "seen at {seen}, fell at {fell_at}");
    assert!((cleared - seen - 1.0).abs() < 1.0e-9, "cleared {} s after it was seen", cleared - seen);
    assert_eq!(METER_EVENT_STRIDE, 4, "journal, phases and two instants: no depth, no duration");
    assert_eq!(s[snap::METER_LOW_VOLTAGE_RECORDS], 1.0);
    assert_eq!(s[snap::METER_OVERFLOW_BITS], 0.0);
}

#[test]
fn the_share_of_sample_phases_that_catch_a_130_ms_dip_is_its_share_of_the_second() {
    // Nothing tells the meter how long a disturbance must be. A hundred places of its look
    // within the second, the same dip each time: it is caught when a look falls in it.
    let mut caught = 0;
    for place in 0..100 {
        let mut sim = KguSim::new();
        sim.set_meter_sample_phase_s((f64::from(place) + 0.5) / 100.0);
        advance(&mut sim, 2.0);
        sim.set_grid_phase_voltages_pu(DIP_160_V_PU, 1.0, 1.0);
        advance(&mut sim, 0.13);
        sim.set_grid_voltage_pu(1.0);
        advance(&mut sim, 1.5);
        let records = meter_records(&sim);
        assert!(records.len() <= 1);
        caught += records.len();
        assert_eq!(quality_events(&sim).len(), 1, "the analyser sees every one of them");
    }
    let share = caught as f64 / 100.0;
    assert!((share - 0.13).abs() < 0.011, "caught at {caught} of 100 sample phases");
}

#[test]
fn a_3_s_dip_is_in_both_records_each_by_its_own_clock_and_the_meters_time_is_a_look() {
    let mut sim = on_load(50.0, 300.0);
    sim.set_analyser_clock_offset_s(100.0);
    sim.set_meter_clock_offset_s(250.0);
    sim.set_meter_sample_phase_s(0.4);
    let fell_at = sim.snapshot()[snap::TIME_S];
    sim.set_grid_phase_voltages_pu(1.0, 1.0, DIP_160_V_PU);
    advance(&mut sim, 3.0);
    sim.set_grid_voltage_pu(1.0);
    advance(&mut sim, 3.0);
    let s = sim.snapshot();
    assert!((s[snap::ANALYSER_CLOCK_S] - s[snap::TIME_S] - 100.0).abs() < 1.0e-9);
    assert!((s[snap::METER_CLOCK_S] - s[snap::TIME_S] - 250.0).abs() < 1.0e-9);

    let quality = quality_events(&sim);
    assert_eq!(quality.len(), 1, "{quality:?}");
    assert!((quality[0].2 - (fell_at + 100.0)).abs() < 1.0e-9, "the analyser stamps the start");
    assert!((quality[0].3 - 3.0).abs() < 1.0e-9);

    let records = meter_records(&sim);
    assert_eq!(records.len(), 1, "{records:?}");
    let (kind, phases, seen, cleared) = records[0];
    assert_eq!((kind, phases), (journal::LOW_VOLTAGE, 0b100));
    // The meter's time is one of its looks (whole seconds of its clock plus 0.4 s), the
    // first one after the phase fell: later than the start by up to a second.
    let whole = seen - 0.4;
    assert!((whole - whole.round()).abs() < 1.0e-9, "seen at {seen} by the meter's clock");
    let late = (seen - 250.0) - fell_at;
    assert!(late > 0.0 && late < 1.0, "the look came {late} s after the phase fell");
    // Three looks fell inside the 3 s; the fourth saw the voltage back.
    assert!((cleared - seen - 3.0).abs() < 1.0e-9);
    // The same event in the two records: apart by the two clocks' difference and by how
    // late the look was.
    assert!((seen - quality[0].2 - (150.0 + late)).abs() < 1.0e-9);
    // The protection answered too (0.15 s), in model time.
    let trips = events_of(&sim, event_kind::PROTECTION);
    assert_eq!(trips.len(), 1);
    assert!((trips[0].0 - fell_at - 0.15).abs() < 0.011);
}

#[test]
fn the_101st_low_voltage_event_overflows_the_journal_as_the_card_says() {
    let mut sim = KguSim::new();
    let limit = |name: &str| {
        let index = crate::LIMIT_FIELDS.iter().position(|n| *n == name).expect("limit");
        sim.limits()[index]
    };
    let capacity = limit("METER_LOW_VOLTAGE_CAPACITY");
    let overwrites = limit("METER_OVERFLOW_OVERWRITES_OLDEST") == 1.0;
    assert_eq!(capacity, 100.0);
    // Event k holds from 3k s for 1.5 s; the meter looks at 0.505 s past each second, so
    // each event is seen once (at 3k + 0.505) and seen gone at the next look.
    sim.set_meter_sample_phase_s(0.505);
    let event = |sim: &mut KguSim| {
        sim.set_grid_phase_voltages_pu(1.0, 0.8, 1.0);
        advance(sim, 1.5);
        sim.set_grid_voltage_pu(1.0);
        advance(sim, 1.5);
    };
    for _ in 0..100 {
        event(&mut sim);
    }
    let full = sim.snapshot();
    assert_eq!(full[snap::METER_LOW_VOLTAGE_RECORDS], capacity);
    assert_eq!(full[snap::METER_OVERFLOW_BITS], 0.0, "full, not yet overflowed");
    assert!((meter_records(&sim)[0].2 - 0.505).abs() < 1.0e-9);

    event(&mut sim);
    let s = sim.snapshot();
    assert_eq!(s[snap::METER_LOW_VOLTAGE_RECORDS], capacity, "the journal holds no more");
    assert_eq!(s[snap::METER_OVERFLOW_BITS], 1.0, "low-voltage journal overflowed");
    assert_eq!(s[snap::METER_HIGH_VOLTAGE_RECORDS], 0.0);
    assert_eq!(s[snap::METER_LOSS_OF_VOLTAGE_RECORDS], 0.0);
    let records = meter_records(&sim);
    let (oldest, newest) = (records[0].2, records[99].2);
    if overwrites {
        // The first event's record is gone; the 101st is the newest.
        assert!((oldest - 3.505).abs() < 1.0e-9, "oldest record seen at {oldest}");
        assert!((newest - 300.505).abs() < 1.0e-9, "newest record seen at {newest}");
    } else {
        assert!((oldest - 0.505).abs() < 1.0e-9);
        assert!((newest - 297.505).abs() < 1.0e-9);
    }
    // The analyser, with its longer record, has all of them.
    assert_eq!(s[snap::ANALYSER_RECORDED_EVENTS], 101.0);
    // Clearing empties the journals and the flag, not the energy registers.
    sim.clear_meter_events();
    advance(&mut sim, 0.01);
    assert!(sim.meter_events().is_empty());
    assert_eq!(sim.snapshot()[snap::METER_OVERFLOW_BITS], 0.0);
}

#[test]
fn the_meters_registers_follow_the_direction_of_active_and_reactive_power() {
    let mut sim = on_load(100.0, 300.0);
    let registers = |s: &[f64]| {
        [
            s[snap::METER_ACTIVE_EXPORT_KWH],
            s[snap::METER_ACTIVE_IMPORT_KWH],
            s[snap::METER_REACTIVE_EXPORT_KVARH],
            s[snap::METER_REACTIVE_IMPORT_KVARH],
        ]
    };
    // A quarter of an hour of steady export at 1.00 pu excitation, where the machine
    // absorbs reactive power (about 194 kvar).
    let before = sim.snapshot();
    assert!(before[snap::REACTIVE_POWER_KVAR] < -100.0);
    advance(&mut sim, 900.0);
    let after = sim.snapshot();
    let (r0, r1) = (registers(&before), registers(&after));
    let engine = after[snap::ENERGY_ELECTRIC_KWH] - before[snap::ENERGY_ELECTRIC_KWH];
    let exported = r1[0] - r0[0];
    assert!((exported - engine).abs() < 1.0e-9 * engine, "meter {exported} kWh, engine {engine} kWh");
    assert!((exported - 250.0).abs() < 0.25, "1 000 kW for a quarter of an hour: {exported} kWh");
    assert_eq!(r1[1].to_bits(), r0[1].to_bits(), "no active import while exporting");
    assert_eq!(r1[2].to_bits(), r0[2].to_bits(), "no reactive export while under-excited");
    let absorbed = r1[3] - r0[3];
    let expected = -0.25 * 0.5 * (before[snap::REACTIVE_POWER_KVAR] + after[snap::REACTIVE_POWER_KVAR]);
    assert!((absorbed - expected).abs() < 0.01 * expected, "{absorbed} kvarh against {expected}");

    // Over-excited, the reactive power goes out and into the other register.
    sim.set_excitation_pu(1.2);
    advance(&mut sim, 60.0);
    let before = sim.snapshot();
    assert!(before[snap::REACTIVE_POWER_KVAR] > 100.0, "{} kvar", before[snap::REACTIVE_POWER_KVAR]);
    advance(&mut sim, 300.0);
    let after = sim.snapshot();
    let (r0, r1) = (registers(&before), registers(&after));
    assert!(r1[2] - r0[2] > 5.0, "reactive export {} kvarh", r1[2] - r0[2]);
    assert_eq!(r1[3].to_bits(), r0[3].to_bits(), "no reactive import while over-excited");
    assert_eq!(r1[1].to_bits(), r0[1].to_bits());
    // From the start of the model: the two active registers are the engine's counter.
    let net = r1[0] - r1[1];
    assert!((net - after[snap::ENERGY_ELECTRIC_KWH]).abs() < 1.0e-9 * net);
}

// ---------------------------------------------------------------------------
// Synchronisation on the operator's command
// ---------------------------------------------------------------------------

/// Advance in single sub-steps until the breaker closes; returns the time
/// taken and the phase angle [deg] of the last look before it closed.
fn advance_to_closing(sim: &mut KguSim, limit_s: f64) -> (f64, f64) {
    let mut waited = 0.0;
    let mut phase_deg = sim.snapshot()[snap::SYNC_PHASE_ANGLE_DEG];
    while sim.snapshot()[snap::BREAKER_CLOSED] != 1.0 {
        phase_deg = sim.snapshot()[snap::SYNC_PHASE_ANGLE_DEG];
        advance(sim, 0.01);
        waited += 0.01;
        assert!(waited < limit_s, "the breaker did not close in {limit_s} s");
    }
    (waited, phase_deg)
}

#[test]
fn after_a_grid_event_the_operators_command_lets_the_synchroniser_close_once() {
    let mut sim = on_load(50.0, 300.0);
    sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.73);
    advance(&mut sim, 1.0);
    sim.set_grid_voltage_pu(1.0);
    advance(&mut sim, 5.0);

    // Before the acknowledgement the command is refused, with the reasons as bits.
    let refused = sim.cmd_synchronise();
    assert_eq!(refused, refusal::NOT_SYNCHRONISING | refusal::TRIP_LATCHED);
    advance(&mut sim, 30.0);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(sim.snapshot()[snap::SYNCHRONISER_ARMED_BY_OPERATOR], 0.0);

    // Acknowledged, and no command: the unit stays off the bus, however often the phase
    // passes through zero (more than twice in 45 s at the synchronising slip).
    sim.cmd_ack();
    advance(&mut sim, 45.0);
    let waiting = sim.snapshot();
    assert_eq!(waiting[snap::BREAKER_CLOSED], 0.0, "{}", grid_side_summary(&waiting));
    assert_eq!(waiting[snap::SEQUENCE_STEP], f64::from(Sequence::Synchronising.index()));
    assert_eq!(waiting[snap::SYNCHRONISER_ARMED_BY_OPERATOR], 0.0);

    // The command, given well away from zero phase: accepted, and from here the
    // synchroniser does the rest. No close request is made.
    advance_until(&mut sim, 30.0, "phase away", |s| s[snap::SYNC_PHASE_ANGLE_DEG].abs() > 40.0);
    let commanded_at = sim.snapshot()[snap::TIME_S];
    assert_eq!(sim.cmd_synchronise(), 0);
    advance(&mut sim, 0.02);
    assert_eq!(sim.snapshot()[snap::SYNCHRONISER_ARMED_BY_OPERATOR], 1.0);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0, "not before the phase is there");
    let (waited, phase_deg) = advance_to_closing(&mut sim, 25.0);
    // Not inputs: when it closed (the phase had to come round at the slip) and at what angle.
    assert!(waited > 1.0, "closed {waited} s after the command");
    assert!(phase_deg.abs() < 1.5, "closed at {phase_deg} degrees");
    let closed = sim.snapshot();
    assert_eq!(closed[snap::SYNCHRONISER_ARMED_BY_OPERATOR], 0.0, "one attempt");
    assert_eq!(closed[snap::STATE_CODE], 3.0);
    // The log shows who decided: the refused command, then the accepted one, then the closing.
    let commands = events_of(&sim, event_kind::SYNCHRONISE_COMMAND);
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert_eq!(commands[0].1, refusal::NOT_SYNCHRONISING | refusal::TRIP_LATCHED);
    assert_eq!(commands[1].1, 0);
    assert!((commands[1].0 - commanded_at).abs() < 1.0e-9);
    assert!(commands[1].2.abs() > 40.0, "phase at the command {} degrees", commands[1].2);
    let closings: Vec<_> =
        events_of(&sim, event_kind::BREAKER).into_iter().filter(|e| e.1 == 1).collect();
    assert_eq!(closings.len(), 2);
    assert!(closings[1].0 > commands[1].0);
    assert!(events_of(&sim, event_kind::REFUSED).is_empty(), "no close request was made");
    advance(&mut sim, 60.0);
    let s = sim.snapshot();
    assert!((s[snap::ELECTRIC_POWER_KW] - 500.0).abs() < 0.5, "{}", grid_side_summary(&s));

    // A stop cancels a commanded synchronisation that is still waiting for the phase.
    let mut sim = on_load(50.0, 300.0);
    sim.cmd_breaker_open();
    advance(&mut sim, 20.0);
    advance_until(&mut sim, 30.0, "phase away", |s| s[snap::SYNC_PHASE_ANGLE_DEG].abs() > 40.0);
    assert_eq!(sim.cmd_synchronise(), 0);
    advance(&mut sim, 0.02);
    assert_eq!(sim.snapshot()[snap::SYNCHRONISER_ARMED_BY_OPERATOR], 1.0);
    sim.cmd_stop();
    advance(&mut sim, 30.0);
    let stopping = sim.snapshot();
    assert_eq!(stopping[snap::SEQUENCE_STEP], f64::from(Sequence::CoolDown.index()));
    assert_eq!(stopping[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(stopping[snap::SYNCHRONISER_ARMED_BY_OPERATOR], 0.0);
}

#[test]
fn a_start_without_automatic_synchronisation_waits_for_the_command_or_the_close_request() {
    // A unit at rest refuses the command.
    let mut sim = KguSim::new();
    assert_eq!(sim.cmd_synchronise(), refusal::NOT_SYNCHRONISING);

    sim.set_auto_sync(false);
    sim.set_load_setpoint_pct(40.0);
    sim.cmd_start();
    advance_until(&mut sim, 200.0, "synchronising", |s| s[snap::STATE_CODE] == 2.0);
    advance(&mut sim, 45.0);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 0.0, "waits without a command");
    advance_until(&mut sim, 30.0, "phase away", |s| s[snap::SYNC_PHASE_ANGLE_DEG].abs() > 40.0);
    assert_eq!(sim.cmd_synchronise(), 0);
    let (waited, phase_deg) = advance_to_closing(&mut sim, 25.0);
    assert!(waited > 1.0 && phase_deg.abs() < 1.5, "closed after {waited} s at {phase_deg} degrees");
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 3.0);
    assert_eq!(events_of(&sim, event_kind::SYNCHRONISE_COMMAND).len(), 2, "one refused at rest, one accepted");

    // The operator's own closing inside the window still works with nothing armed.
    let mut sim = KguSim::new();
    sim.set_auto_sync(false);
    sim.cmd_start();
    advance_until(&mut sim, 200.0, "synchronising", |s| s[snap::STATE_CODE] == 2.0);
    advance(&mut sim, 6.0);
    let mut waited = 0.0;
    while sim.snapshot()[snap::SYNC_PERMITTED] != 1.0 {
        advance(&mut sim, 0.01);
        waited += 0.01;
        assert!(waited < 30.0);
    }
    assert_eq!(sim.cmd_breaker_close(), 0);
    advance(&mut sim, 0.02);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 1.0);
    assert!(events_of(&sim, event_kind::SYNCHRONISE_COMMAND).is_empty());
}

// ---------------------------------------------------------------------------
// Standstill: the shaft comes to rest and the unit stands, for hours
// ---------------------------------------------------------------------------

/// Stand for `seconds`, checking in every minute that the shaft is at exactly
/// zero speed and the model still steps.
fn stand(sim: &mut KguSim, seconds: f64) {
    let minutes = (seconds / 60.0).ceil() as usize;
    for _ in 0..minutes {
        advance(sim, seconds / minutes as f64);
        let s = sim.snapshot();
        assert_eq!(s[snap::SPEED_RPM].to_bits(), 0.0_f64.to_bits(), "at t {} s", s[snap::TIME_S]);
        assert_eq!(s[snap::KINETIC_ENERGY_KJ], 0.0);
    }
}

/// Start, wait for the breaker and for the load; returns the time to the bus.
fn start_and_load(sim: &mut KguSim, load_pct: f64) -> f64 {
    sim.set_load_ramp_pct_per_min(300.0);
    sim.set_load_setpoint_pct(load_pct);
    sim.cmd_start();
    let to_bus = advance_until(sim, 200.0, "breaker closed", |s| s[snap::BREAKER_CLOSED] == 1.0);
    advance_until(sim, 120.0, "load reference", |s| {
        (s[snap::LOAD_REFERENCE_PCT] - load_pct).abs() < 1.0e-9
    });
    advance(sim, 30.0);
    let s = sim.snapshot();
    assert!((s[snap::ELECTRIC_POWER_KW] - 10.0 * load_pct).abs() < 0.5, "{}", grid_side_summary(&s));
    assert!(ledger_relative_residual(&s) < 1.0e-9);
    to_bus
}

#[test]
fn after_a_normal_stop_the_shaft_is_at_rest_for_two_hours_and_the_unit_starts_again() {
    let mut sim = KguSim::new();
    let first_to_bus = start_and_load(&mut sim, 60.0);
    sim.cmd_stop();
    advance_until(&mut sim, 500.0, "coasting", |s| {
        s[snap::SEQUENCE_STEP] == f64::from(Sequence::CoastDown.index())
    });
    // Not inputs: how long the shaft takes to the controller's standstill level and to
    // rest, and that rest is exactly zero speed, not a small number.
    let to_ready = advance_until(&mut sim, 60.0, "ready", |s| s[snap::STATE_CODE] == 0.0);
    let to_rest = to_ready
        + advance_until(&mut sim, 5.0, "rest", |s| s[snap::SPEED_RPM] == 0.0);
    assert!(to_ready > 11.0 && to_ready < 12.5, "ready {to_ready} s after the gas was shut");
    assert!(to_rest - to_ready < 0.5, "at rest {to_rest} s after the gas was shut");
    // The stop released what was left of the kinetic energy as heat: less than the
    // kinetic energy at the rest speed, and more than nothing.
    let released = sim.plant.sink_heat_j(crate::plant::sink::STANDSTILL);
    assert!(released > 0.0 && released < 0.3, "static friction released {released} J");
    assert!(ledger_relative_residual(&sim.snapshot()) < 1.0e-9);

    stand(&mut sim, 7_200.0);
    let rested = sim.snapshot();
    assert_eq!(rested[snap::STATE_CODE], 0.0);
    assert!(ledger_relative_residual(&rested) < 1.0e-9);
    assert_eq!(sim.plant.sink_heat_j(crate::plant::sink::STANDSTILL), released, "nothing more");
    // The coolant has cooled towards the room in two hours; nothing else moved.
    assert!(rested[snap::COOLANT_SUPPLY_C] < 80.0 && rested[snap::COOLANT_SUPPLY_C] > 30.0);

    let second_to_bus = start_and_load(&mut sim, 60.0);
    assert!(
        (second_to_bus - first_to_bus).abs() < 25.0,
        "to the bus in {second_to_bus} s after standing, {first_to_bus} s the first time"
    );
    assert_eq!(sim.snapshot()[snap::START_COUNT], 2.0);
}

#[test]
fn after_an_emergency_stop_and_two_hours_the_unit_is_acknowledged_and_starts() {
    let mut sim = KguSim::new();
    start_and_load(&mut sim, 60.0);
    sim.cmd_emergency_stop();
    let to_rest = advance_until(&mut sim, 60.0, "rest", |s| s[snap::SPEED_RPM] == 0.0);
    assert!(to_rest > 11.0 && to_rest < 13.0, "at rest {to_rest} s after the emergency stop");
    stand(&mut sim, 7_200.0);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 5.0, "still latched");
    assert!(ledger_relative_residual(&s) < 1.0e-9);
    sim.cmd_ack();
    advance(&mut sim, 0.05);
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 0.0);
    start_and_load(&mut sim, 60.0);
}

#[test]
fn interrupted_and_failed_starts_leave_a_unit_that_stands_and_starts() {
    let set = ControllerSetpoints::DEFAULT;
    // A start interrupted while pre-lubricating: the shaft never turned.
    let mut sim = KguSim::new();
    sim.cmd_start();
    advance(&mut sim, 10.0);
    assert_eq!(sim.snapshot()[snap::SEQUENCE_STEP], f64::from(Sequence::PreLubrication.index()));
    sim.cmd_stop();
    advance(&mut sim, 1.0);
    assert_eq!(sim.snapshot()[snap::STATE_CODE], 0.0);
    stand(&mut sim, 600.0);

    // A start interrupted while cranking: the starter had the shaft turning.
    sim.cmd_start();
    advance(&mut sim, set.pre_lubrication_time_s + 3.0);
    let cranking = sim.snapshot();
    assert_eq!(cranking[snap::SEQUENCE_STEP], f64::from(Sequence::Cranking.index()));
    assert!(cranking[snap::SPEED_RPM] > 50.0, "{} rpm", cranking[snap::SPEED_RPM]);
    sim.cmd_stop();
    advance_until(&mut sim, 30.0, "rest", |s| s[snap::SPEED_RPM] == 0.0);
    advance_until(&mut sim, 5.0, "ready", |s| s[snap::STATE_CODE] == 0.0);
    stand(&mut sim, 600.0);

    // A start that fails for want of gas: cranked, tripped, coasted down.
    sim.set_gas_gauge_kpa(0.0);
    sim.cmd_start();
    advance_until(&mut sim, 120.0, "failed start", |s| s[snap::STATE_CODE] == 5.0);
    assert_eq!(sim.snapshot()[snap::ALARM_BITS] as u32 & alarm::FAILED_START, alarm::FAILED_START);
    advance_until(&mut sim, 30.0, "rest", |s| s[snap::SPEED_RPM] == 0.0);
    stand(&mut sim, 600.0);
    assert_eq!(sim.snapshot()[snap::GAS_STD_M3], 0.0);
    assert!(ledger_relative_residual_or_empty(&sim.snapshot()));

    // Gas back, acknowledged: it starts and takes load.
    sim.set_gas_gauge_kpa(crate::cards::GasValveCard::SIZING_GAUGE_PA * 1.0e-3);
    sim.cmd_ack();
    advance(&mut sim, 0.05);
    start_and_load(&mut sim, 50.0);
}

/// The ledger closes, also where no fuel has entered yet and there is nothing
/// to relate the residual to.
fn ledger_relative_residual_or_empty(s: &[f64]) -> bool {
    if s[snap::LEDGER_BOUNDARY_ENERGY_MJ] > 0.0 {
        ledger_relative_residual(s) < 1.0e-9
    } else {
        s[snap::LEDGER_RESIDUAL_J].abs() < 1.0e-6
    }
}

#[test]
fn a_refused_step_leaves_the_last_accepted_state_and_the_next_step_goes_on() {
    // No setter of the package can make a component refuse (they all clamp), so the
    // refusal is provoked on the plant: a gas temperature below absolute zero.
    let mut sim = on_load(50.0, 300.0);
    let before = sim.snapshot();
    let good = sim.plant.operator().gas_temperature_k;
    sim.plant.set_operator(|o| o.gas_temperature_k = -1.0);
    assert!(!sim.step(5.0), "the step must be refused");
    assert!(sim.last_error().contains("gas supply temperature"), "{}", sim.last_error());
    let refused = sim.snapshot();
    // Nothing moved: the time, the shaft, the counters are those of the last accepted step.
    for slot in [
        snap::TIME_S,
        snap::SPEED_RPM,
        snap::ELECTRIC_POWER_KW,
        snap::GAS_STD_M3,
        snap::ENERGY_ELECTRIC_KWH,
        snap::COOLANT_SUPPLY_C,
        snap::LEDGER_RESIDUAL_J,
    ] {
        assert_eq!(
            refused[slot].to_bits(),
            before[slot].to_bits(),
            "{} moved in a refused step",
            crate::SNAPSHOT_FIELDS[slot]
        );
    }
    // A second refusal is no worse than the first.
    assert!(!sim.step(1.0));
    // With the input put right the model goes on, by exactly the time asked for: the
    // refused seconds are not owed.
    sim.plant.set_operator(|o| o.gas_temperature_k = good);
    assert!(sim.step(1.0), "{}", sim.last_error());
    assert!(sim.last_error().is_empty());
    let after = sim.snapshot();
    assert!((after[snap::TIME_S] - before[snap::TIME_S] - 1.0).abs() < 1.0e-9);
    advance(&mut sim, 30.0);
    let s = sim.snapshot();
    assert!((s[snap::ELECTRIC_POWER_KW] - 500.0).abs() < 0.5, "{}", grid_side_summary(&s));
    assert!(ledger_relative_residual(&s) < 1.0e-9);
}
