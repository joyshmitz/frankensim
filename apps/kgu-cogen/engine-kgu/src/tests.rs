//! Native tests of the plant. Each scenario checks at least one quantity
//! that was not an input (Gauntlet G0 for the component laws, G2-style
//! scenario checks for the assembled plant).

use fs_ports::component::conformance::{Case, check_component};

use crate::cards::{
    CombustionLimitsCard, ControllerSetpoints, DEFAULT_CONDITION_FACTOR, DumpCoolerCard,
    ExhaustExchangerCard, ExhaustGasCard, FuelGasCard, GasValveCard, GeneratorCard, HeatSplitCard,
    IndicatedWorkCard, NetworkExchangerCard, PassportCard, RATED_OMEGA_RAD_S, StarterCard,
    SynchronisationWindowCard,
};
use crate::heat::{
    DumpCoolerComponent, DumpCoolerState, ExhaustExchangerState, ExhaustHeatExchangerComponent,
    NetworkExchangerState, NetworkHeatExchangerComponent,
};
use crate::components::{
    GasControlValveComponent, GasEngineComponent, GasEngineState, GasValveState, GeneratorState,
    StarterMotorComponent, StarterState, SynchronousGeneratorComponent,
};
use crate::controller::{
    ControllerState, GensetControllerComponent, OperatorSignals, Sequence, alarm, refusal,
};
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
    for (state, omega, command, excitation, hz) in [
        (&open, RATED_OMEGA_RAD_S, 0.0, 1.0, 50.0),
        (&open, 0.0, 0.0, 1.0, 50.0),
        (&loaded, RATED_OMEGA_RAD_S, 1.0, 1.0, 50.0),
        (&loaded, RATED_OMEGA_RAD_S * 1.01, 1.0, 1.2, 50.0),
        (&loaded, RATED_OMEGA_RAD_S, 1.0, 0.8, 49.8),
        (&slipping, RATED_OMEGA_RAD_S * 1.05, 1.0, 1.0, 50.0),
    ] {
        let inputs = [-omega, 400.0, 303.15, command, excitation, hz];
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

#[test]
fn controller_conforms_in_every_step_of_its_sequence() {
    let controller = GensetControllerComponent::new(
        ControllerSetpoints::DEFAULT,
        SynchronisationWindowCard::ASSUMED,
        2,
        GeneratorCard::ASSUMED.excitation_range_pu,
        1_060.0,
    );
    let ready = ControllerState::ready();
    let operator = OperatorSignals {
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
    ] {
        let firing = matches!(
            sequence,
            Sequence::RunUp
                | Sequence::WarmUp
                | Sequence::Synchronising
                | Sequence::OnLoad
                | Sequence::Unloading
                | Sequence::CoolDown
        );
        let state = ControllerState {
            sequence,
            ignition: firing,
            gas_open: firing,
            speed_reference_rad_s: if firing { 150.0 } else { 0.0 },
            ..ready
        };
        for (omega, start) in [(0.0, true), (RATED_OMEGA_RAD_S, false), (190.0, false)] {
            let mut inputs = [0.0; 21];
            inputs[..6].copy_from_slice(&[omega, 0.05, 0.0, 0.0, 0.0, 360.0]);
            OperatorSignals { start, ..operator }.write(&mut inputs[6..]);
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

    let mut sim = on_load(30.0, 300.0);
    sim.set_grid_voltage_pu(0.62);
    advance(&mut sim, 0.10);
    assert_eq!(sim.snapshot()[snap::BREAKER_CLOSED], 1.0, "inside the 0.15 s delay");
    advance(&mut sim, 0.10);
    let s = sim.snapshot();
    assert_eq!(s[snap::STATE_CODE], 5.0);
    assert_eq!(s[snap::BREAKER_CLOSED], 0.0);
    assert_eq!(s[snap::ALARM_BITS] as u32 & alarm::GRID_UNDERVOLTAGE, alarm::GRID_UNDERVOLTAGE);
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
    };
    for (share, trips) in [(1.14, false), (1.15, true)] {
        let mut inputs = [0.0; 21];
        inputs[..6].copy_from_slice(&[share * RATED_OMEGA_RAD_S, 0.0, 0.0, 0.0, 0.0, 360.0]);
        operator.write(&mut inputs[6..]);
        let mut next = running;
        let mut outputs = [0.0; 5];
        with_clock_free_cx(|cx| {
            use fs_ports::component::Component;
            controller.propose(&running, 0.01, &inputs, &mut next, &mut outputs, cx)
        })
        .expect("controller step");
        assert_eq!(next.sequence == Sequence::Tripped, trips, "at {share} of rated speed");
        if trips {
            assert_eq!(next.alarms, alarm::OVERSPEED);
            assert_eq!(outputs, [0.0; 5], "everything off");
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
