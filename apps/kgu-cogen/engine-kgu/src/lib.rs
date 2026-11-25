//! FrankenSim gas-piston cogeneration unit (1 000 kW CHP genset), stage 1.
//!
//! Native Rust and `wasm32-unknown-unknown`. The plant is a description on
//! the composition kernel (Component contract v0):
//!
//! - [`cards`]: every fitted, assumed or passport number, with its origin.
//! - [`components`]: gas control valve, engine (brake side), starter and
//!   synchronous generator as `fs_ports::component::Component`s.
//! - [`controller`]: the genset controller (governor on
//!   `fs_control::speed::SpeedController`, sequencer, synchroniser,
//!   protection) as a Component with signal ports.
//! - [`plant`]: the assembly on `fs_system::SystemBuilder`, stepped by
//!   `fs_system::System::step` and audited by `fs_system::System::ledger`.
//!
//! [`KguSim`] is the boundary the teaching console drives: setters, commands,
//! `step`, a flat versioned `snapshot`, `limits` and `events`.

pub mod cards;
pub mod components;
pub mod analyser;
pub mod cabinet;
pub mod controller;
pub mod heat;
pub mod plant;

#[cfg(test)]
mod tests;

use core::f64::consts::PI;
use std::sync::OnceLock;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::cards::{
    BUS_VOLTAGE_V, CYLINDERS, RATED_ELECTRIC_W, RATED_SPEED_RPM, SHAFT_INERTIA_KG_M2,
};
use crate::plant::{KguPlant, SUB_STEP_S};

/// Version of the snapshot layout. It changes when a slot changes meaning or
/// position; slots are only ever appended within one version.
pub const SNAPSHOT_LAYOUT_VERSION: f64 = 1.0;

macro_rules! snapshot_layout {
    ($($name:ident),* $(,)?) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms, dead_code)]
        #[repr(usize)]
        enum Slot { $($name),*, END }
        /// Index of every slot of the flat snapshot.
        pub mod snap {
            $(
                #[allow(missing_docs)]
                pub const $name: usize = super::Slot::$name as usize;
            )*
        }
        /// Number of slots of the flat snapshot.
        pub const SNAPSHOT_LEN: usize = Slot::END as usize;
        /// Names of the slots, in order.
        pub const SNAPSHOT_FIELDS: [&str; SNAPSHOT_LEN] = [$(stringify!($name)),*];
    };
}

// Units are in the names. A slot with nothing to show (an efficiency without fuel, a gas
// temperature without gas) holds NaN.
snapshot_layout!(
    LAYOUT_VERSION,
    TIME_S,
    // 0 ready, 1 starting, 2 synchronizing, 3 running, 4 stopping, 5 trip
    STATE_CODE,
    // controller::Sequence::index()
    SEQUENCE_STEP,
    BREAKER_CLOSED,
    // controller::alarm bits: latched trips and live warnings
    ALARM_BITS,
    SPEED_RPM,
    TORQUE_INDICATED_NM,
    TORQUE_FRICTION_NM,
    TORQUE_STARTER_NM,
    TORQUE_GENERATOR_NM,
    TORQUE_NET_NM,
    ELECTRIC_POWER_KW,
    REACTIVE_POWER_KVAR,
    STATOR_CURRENT_A,
    POWER_FACTOR,
    LOAD_ANGLE_DEG,
    GENERATOR_VOLTAGE_V,
    BUS_VOLTAGE_KV,
    BUS_FREQUENCY_HZ,
    GENERATOR_FREQUENCY_HZ,
    SYNC_VOLTAGE_DIFFERENCE_PU,
    SYNC_FREQUENCY_DIFFERENCE_HZ,
    SYNC_PHASE_ANGLE_DEG,
    SYNC_PERMITTED,
    FUEL_POWER_KW,
    GAS_FLOW_STD_M3_H,
    GAS_FLOW_METER_M3_H,
    GAS_VALVE_LIFT,
    VALVE_DEMAND,
    BRAKE_EFFICIENCY_PCT,
    ELECTRIC_EFFICIENCY_PCT,
    HEAT_TO_NETWORK_KW,
    HEAT_DUMPED_KW,
    EXHAUST_LOSS_KW,
    RADIATION_LOSS_KW,
    FRICTION_POWER_KW,
    GENERATOR_LOSS_KW,
    JACKET_HEAT_KW,
    COOLANT_SUPPLY_C,
    COOLANT_RETURN_C,
    EXHAUST_TEMPERATURE_C,
    LOAD_REFERENCE_PCT,
    SPEED_REFERENCE_RPM,
    PASSPORT_LOAD_PCT,
    LEDGER_RESIDUAL_J,
    LEDGER_BOUNDARY_ENERGY_MJ,
    ENERGY_ELECTRIC_KWH,
    ENERGY_HEAT_KWH,
    ENERGY_FUEL_KWH,
    GAS_STD_M3,
    GAS_METER_M3,
    ENGINE_HOURS,
    START_COUNT,
    KINETIC_ENERGY_KJ,
    NEWTON_ITERATIONS,
    LAST_CLOSE_REFUSAL_BITS,
    EXHAUST_RECOVERED_KW,
    STACK_TEMPERATURE_C,
    NETWORK_SUPPLY_C,
    THERMOSTAT_OPENING,
    DUMP_VALVE_OPENING,
    BLOCK_LOSS_KW,
    THERMAL_EFFICIENCY_PCT,
    ENERGY_DUMPED_KWH,
    // Bus phase voltages on the generator's side of the transformer, phase to neutral
    PHASE_A_VOLTAGE_V,
    PHASE_B_VOLTAGE_V,
    PHASE_C_VOLTAGE_V,
    PHASE_A_CURRENT_A,
    PHASE_B_CURRENT_A,
    PHASE_C_CURRENT_A,
    // negative over positive sequence voltage
    VOLTAGE_UNBALANCE_PCT,
    // of the generator's rated current
    NEGATIVE_SEQUENCE_CURRENT_PCT,
    // the protection's replica of the rotor's heating over its limit
    ROTOR_THERMAL_USED_FRAC,
    // cabinet::protection bits: functions whose condition holds / that have tripped
    PROTECTION_PICKUP_BITS,
    PROTECTION_TRIPPED_BITS,
    NEGATIVE_SEQUENCE_LOSS_KW,
    // 0 manual, 1 automatic
    RESTORATION_MODE,
    // seconds of healthy grid still to wait before the automatic return; NaN when not counting
    GRID_HEALTHY_COUNTDOWN_S,
    RESTORATION_ATTEMPTS,
    WAITING_FOR_GRID,
    ANALYSER_CLOCK_S,
    ANALYSER_OPEN_EVENTS,
    ANALYSER_RECORDED_EVENTS,
);

/// Names of the entries of [`KguSim::limits`], in order.
pub const LIMIT_FIELDS: [&str; 54] = [
    "RATED_ELECTRIC_KW",
    "RATED_SPEED_RPM",
    "OVERSPEED_TRIP_RPM",
    "FIRING_SPEED_RPM",
    "COOLANT_WARNING_C",
    "COOLANT_TRIP_C",
    "GAS_LOW_PRESSURE_KPA",
    "GRID_UNDERVOLTAGE_PU",
    "SYNC_VOLTAGE_WINDOW_PU",
    "SYNC_FREQUENCY_WINDOW_HZ",
    "SYNC_PHASE_WINDOW_DEG",
    "LOAD_RAMP_PCT_PER_MIN",
    "COOL_DOWN_TIME_S",
    "START_TIME_LIMIT_S",
    "EXCITATION_MIN_PU",
    "EXCITATION_MAX_PU",
    "EXCITATION_DEFAULT_PU",
    "PRE_LUBRICATION_TIME_S",
    "PURGE_TIME_S",
    "WARM_UP_TIME_S",
    // The breaker cabinet's protection settings.
    "RATED_CURRENT_A",
    "RATED_PHASE_VOLTAGE_V",
    "PHASE_UNDERVOLTAGE_PU",
    "PHASE_UNDERVOLTAGE_DELAY_S",
    "PHASE_OVERVOLTAGE_PU",
    "PHASE_OVERVOLTAGE_DELAY_S",
    "VOLTAGE_UNBALANCE_TRIP_PCT",
    "VOLTAGE_UNBALANCE_DELAY_S",
    "NEGATIVE_SEQUENCE_PICKUP_PCT",
    "NEGATIVE_SEQUENCE_LIMIT_S",
    "NEGATIVE_SEQUENCE_COOLING_S",
    "OVERCURRENT_A",
    "OVERCURRENT_DELAY_S",
    "OVERCURRENT_HIGH_A",
    "OVERCURRENT_HIGH_DELAY_S",
    "UNDERFREQUENCY_HZ",
    "UNDERFREQUENCY_DELAY_S",
    "OVERFREQUENCY_HZ",
    "OVERFREQUENCY_DELAY_S",
    "REVERSE_POWER_KW",
    "REVERSE_POWER_DELAY_S",
    // Restoration after a grid event.
    "HEALTHY_VOLTAGE_LOW_PU",
    "HEALTHY_VOLTAGE_HIGH_PU",
    "HEALTHY_UNBALANCE_PCT",
    "HEALTHY_FREQUENCY_LOW_HZ",
    "HEALTHY_FREQUENCY_HIGH_HZ",
    "HEALTHY_GRID_DELAY_S",
    "LONGEST_GRID_WAIT_S",
    "LARGEST_AUTO_ATTEMPTS",
    "ATTEMPT_WINDOW_S",
    // The power-quality analyser's thresholds.
    "ANALYSER_DIP_PU",
    "ANALYSER_SWELL_PU",
    "ANALYSER_INTERRUPTION_PU",
    "ANALYSER_HYSTERESIS_PU",
];

/// Number of values per entry of [`KguSim::events`].
pub const EVENT_STRIDE: usize = 4;

/// Run a closure with a deterministic, clock-free execution context that is
/// safe on native targets and on `wasm32-unknown-unknown`.
pub fn with_clock_free_cx<R>(f: impl FnOnce(&fs_exec::Cx<'_>) -> R) -> R {
    static GATE: OnceLock<fs_exec::CancelGate> = OnceLock::new();
    static POOL: OnceLock<fs_alloc::ArenaPool> = OnceLock::new();
    let gate = GATE.get_or_init(fs_exec::CancelGate::new_clock_free);
    let pool = POOL.get_or_init(|| fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default()));
    let arena = pool.arena();
    let cx = fs_exec::Cx::new(
        gate,
        &arena,
        fs_exec::StreamKey {
            seed: 0x4B47_5531_3030_3001,
            kernel_id: 0x4B47_552D_434F_4745,
            tile: 0,
            iteration: 0,
        },
        fs_exec::Budget::INFINITE,
        fs_exec::ExecMode::Deterministic,
    );
    f(&cx)
}

/// The cogeneration unit as the console drives it.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct KguSim {
    plant: KguPlant,
    snapshot: Vec<f64>,
    last_error: String,
}

impl Default for KguSim {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl KguSim {
    /// A unit at standstill, cold and ready to start, on a healthy bus.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    #[must_use]
    pub fn new() -> Self {
        let mut sim = Self {
            plant: KguPlant::build(),
            snapshot: vec![f64::NAN; SNAPSHOT_LEN],
            last_error: String::new(),
        };
        sim.pack_snapshot();
        sim
    }

    // --- Setters ----------------------------------------------------------------------

    /// Load setpoint [% of rated], 0 to 100.
    pub fn set_load_setpoint_pct(&mut self, pct: f64) {
        if pct.is_finite() {
            self.plant.set_operator(|o| o.load_setpoint = (pct / 100.0).clamp(0.0, 1.0));
        }
    }

    /// Load ramp of the controller [% of rated per minute]; default 10.
    pub fn set_load_ramp_pct_per_min(&mut self, pct_per_min: f64) {
        if pct_per_min.is_finite() {
            self.plant
                .set_operator(|o| o.load_ramp_per_s = pct_per_min.clamp(0.1, 600.0) / 6_000.0);
        }
    }

    /// Excitation [pu]; the open-circuit voltage before synchronising, the
    /// EMF behind the reactance after it.
    pub fn set_excitation_pu(&mut self, pu: f64) {
        if pu.is_finite() {
            let (low, high) = self.plant.generator_card.excitation_range_pu;
            self.plant.set_operator(|o| o.excitation_pu = pu.clamp(low, high));
        }
    }

    /// Whether the synchroniser closes the breaker by itself; default on.
    pub fn set_auto_sync(&mut self, on: bool) {
        self.plant.set_operator(|o| o.auto_synchronise = on);
    }

    /// Heat demand of the network, 0 to 1: the share of its rated water flow
    /// that passes the plate heat exchanger. What the network does not take
    /// the emergency cooler has to reject.
    pub fn set_heat_demand_frac(&mut self, frac: f64) {
        if frac.is_finite() {
            self.plant.set_operator(|o| o.heat_demand = frac.clamp(0.0, 1.0));
        }
    }

    /// Temperature of the water returning from the network [C].
    pub fn set_network_return_temp_c(&mut self, celsius: f64) {
        if celsius.is_finite() {
            self.plant
                .set_operator(|o| o.network_return_k = celsius.clamp(5.0, 95.0) + 273.15);
        }
    }

    /// Gas supply gauge pressure [kPa].
    pub fn set_gas_gauge_kpa(&mut self, kpa: f64) {
        if kpa.is_finite() {
            self.plant
                .set_operator(|o| o.gas_gauge_pa = kpa.clamp(0.0, 500.0) * 1.0e3);
        }
    }

    /// Gas supply temperature [C].
    pub fn set_gas_temp_c(&mut self, celsius: f64) {
        if celsius.is_finite() {
            self.plant
                .set_operator(|o| o.gas_temperature_k = celsius.clamp(-30.0, 60.0) + 273.15);
        }
    }

    /// Atmospheric pressure [kPa]; default 101.325.
    pub fn set_atm_pressure_kpa(&mut self, kpa: f64) {
        if kpa.is_finite() {
            self.plant
                .set_operator(|o| o.atmospheric_pa = kpa.clamp(80.0, 110.0) * 1.0e3);
        }
    }

    /// Bus voltage [pu]: all three phases at this magnitude.
    pub fn set_grid_voltage_pu(&mut self, pu: f64) {
        self.set_grid_phase_voltages_pu(pu, pu, pu);
    }

    /// Magnitudes of the three phase voltages of the bus [pu of the rated
    /// phase voltage], phases a, b, c. A grid disturbance is a circumstance
    /// of the scenario: any phase, any depth.
    pub fn set_grid_phase_voltages_pu(&mut self, a: f64, b: f64, c: f64) {
        if a.is_finite() && b.is_finite() && c.is_finite() {
            self.plant.set_operator(|o| {
                o.grid_phase_voltage_pu = [a.clamp(0.0, 1.3), b.clamp(0.0, 1.3), c.clamp(0.0, 1.3)];
            });
        }
    }

    /// How far each phase voltage is turned from its nominal position
    /// [degrees], phases a, b, c; zero for a grid with its phases a third of
    /// a turn apart.
    pub fn set_grid_phase_angles_deg(&mut self, a: f64, b: f64, c: f64) {
        if a.is_finite() && b.is_finite() && c.is_finite() {
            self.plant.set_operator(|o| {
                o.grid_phase_angle_rad = [
                    a.clamp(-60.0, 60.0).to_radians(),
                    b.clamp(-60.0, 60.0).to_radians(),
                    c.clamp(-60.0, 60.0).to_radians(),
                ];
            });
        }
    }

    /// How the unit comes back after the cabinet's protection has opened the
    /// breaker on a grid event: `0` manual (people acknowledge and close),
    /// `1` automatic (the automation waits for a healthy grid, synchronises
    /// and loads the unit by itself). Default manual.
    pub fn set_restoration_mode(&mut self, mode: u32) {
        self.plant.set_operator(|o| o.restoration_auto = mode == 1);
    }

    /// How far the power-quality analyser's clock is ahead of the model's
    /// time [s]. The controller's and the protection's events stay in model
    /// time.
    pub fn set_analyser_clock_offset_s(&mut self, offset_s: f64) {
        if offset_s.is_finite() {
            self.plant.analyser.set_clock_offset_s(offset_s);
        }
    }

    /// Bus frequency [Hz].
    pub fn set_grid_freq_hz(&mut self, hz: f64) {
        if hz.is_finite() {
            self.plant
                .set_operator(|o| o.grid_frequency_hz = hz.clamp(45.0, 55.0));
        }
    }

    /// Machine-condition factor [-]; default 0.955.
    pub fn set_condition_factor(&mut self, factor: f64) {
        if factor.is_finite() {
            self.plant
                .set_operator(|o| o.condition_factor = factor.clamp(0.5, 1.0));
        }
    }

    /// Fault input: emergency cooler fan failed.
    pub fn set_fan_failed(&mut self, failed: bool) {
        self.plant.set_operator(|o| o.fan_failed = failed);
    }

    /// Fault input: low oil pressure (input-driven; no oil circuit is modelled).
    pub fn set_oil_pressure_low(&mut self, low: bool) {
        self.plant.set_operator(|o| o.oil_pressure_low = low);
    }

    // --- Commands ---------------------------------------------------------------------

    /// Start command. Refused by the controller unless the unit is ready.
    pub fn cmd_start(&mut self) {
        self.plant.set_operator(|o| o.start = true);
    }

    /// Normal stop: unload, open the breaker, cool down, shut the gas.
    pub fn cmd_stop(&mut self) {
        self.plant.set_operator(|o| o.stop = true);
    }

    /// Emergency stop: gas shut and breaker open at once; latches.
    pub fn cmd_emergency_stop(&mut self) {
        self.plant.set_operator(|o| o.emergency_stop = true);
    }

    /// Acknowledge alarms; a latched trip whose cause is gone is cleared.
    pub fn cmd_ack(&mut self) {
        self.plant.set_operator(|o| o.acknowledge = true);
    }

    /// Request to close the generator breaker. Returns zero when the request
    /// is accepted (the breaker closes in the next sub-step), otherwise the
    /// reasons as bits: 1 voltage difference, 2 frequency difference, 4 phase
    /// angle, 8 dead bus, 16 the unit is not synchronising, 32 a trip is
    /// latched. A refused request does nothing.
    pub fn cmd_breaker_close(&mut self) -> u32 {
        let bits = self.plant.close_refusal();
        if bits == 0 {
            self.plant.set_operator(|o| o.breaker_close = true);
        } else {
            self.plant.log_refused_close(bits);
        }
        bits
    }

    /// Open the generator breaker by hand (a load rejection when on load).
    /// The synchroniser does not reclose by itself afterwards.
    pub fn cmd_breaker_open(&mut self) {
        self.plant.set_operator(|o| o.breaker_open = true);
    }

    // --- Time -------------------------------------------------------------------------

    /// Advance by `dt_s` in the plant's fixed 10 ms sub-steps. Returns false
    /// when the kernel refused a sub-step; see [`Self::last_error`].
    pub fn step(&mut self, dt_s: f64) -> bool {
        if !(dt_s.is_finite() && dt_s > 0.0) {
            return false;
        }
        let dt = dt_s.min(3_600.0);
        match with_clock_free_cx(|cx| self.plant.step(dt, cx)) {
            Ok(()) => {
                self.last_error.clear();
                self.pack_snapshot();
                true
            }
            Err(error) => {
                self.last_error = error;
                self.pack_snapshot();
                false
            }
        }
    }

    /// The plant's sub-step [s].
    #[must_use]
    pub fn sub_step_s(&self) -> f64 {
        SUB_STEP_S
    }

    /// The last solver refusal, empty when healthy.
    #[must_use]
    pub fn last_error(&self) -> String {
        self.last_error.clone()
    }

    // --- Read-out ---------------------------------------------------------------------

    /// The flat snapshot; slot indices are [`snap`], names
    /// [`Self::snapshot_field_names`].
    #[must_use]
    pub fn snapshot(&self) -> Vec<f64> {
        self.snapshot.clone()
    }

    /// Names of the snapshot slots in order, comma separated.
    #[must_use]
    pub fn snapshot_field_names(&self) -> String {
        SNAPSHOT_FIELDS.join(",")
    }

    /// Thresholds and ratings the page draws against; names are
    /// [`Self::limit_field_names`].
    #[must_use]
    pub fn limits(&self) -> Vec<f64> {
        let set = &self.plant.setpoints;
        let window = &self.plant.window;
        let cabinet = &self.plant.cabinet_card;
        let restore = &self.plant.restoration_card;
        let quality = self.plant.analyser.card();
        let rated_current = self.plant.generator_card.rated_current_a();
        let rpm = 30.0 / PI;
        vec![
            RATED_ELECTRIC_W * 1.0e-3,
            RATED_SPEED_RPM,
            set.overspeed_trip_rad_s * rpm,
            set.firing_speed_rad_s * rpm,
            set.coolant_warning_k - 273.15,
            set.coolant_trip_k - 273.15,
            set.gas_low_pressure_pa * 1.0e-3,
            cabinet.undervoltage.pickup,
            window.voltage_difference_pu,
            window.frequency_difference_hz,
            window.phase_angle_rad.to_degrees(),
            self.plant.operator().load_ramp_per_s * 6_000.0,
            set.cool_down_time_s,
            set.start_time_limit_s,
            self.plant.generator_card.excitation_range_pu.0,
            self.plant.generator_card.excitation_range_pu.1,
            crate::cards::GeneratorCard::REFERENCE_EXCITATION_PU,
            set.pre_lubrication_time_s,
            set.purge_time_s,
            set.warm_up_time_s,
            rated_current,
            self.plant.generator_card.rated_phase_voltage_v(),
            cabinet.undervoltage.pickup,
            cabinet.undervoltage.delay_s,
            cabinet.overvoltage.pickup,
            cabinet.overvoltage.delay_s,
            100.0 * cabinet.voltage_unbalance.pickup,
            cabinet.voltage_unbalance.delay_s,
            100.0 * cabinet.negative_sequence_pickup_pu,
            cabinet.negative_sequence_limit_s,
            cabinet.negative_sequence_cooling_s,
            cabinet.overcurrent.pickup * rated_current,
            cabinet.overcurrent.delay_s,
            cabinet.overcurrent_high.pickup * rated_current,
            cabinet.overcurrent_high.delay_s,
            cabinet.underfrequency.pickup,
            cabinet.underfrequency.delay_s,
            cabinet.overfrequency.pickup,
            cabinet.overfrequency.delay_s,
            cabinet.reverse_power.pickup * 1.0e-3,
            cabinet.reverse_power.delay_s,
            restore.healthy_voltage_low_pu,
            restore.healthy_voltage_high_pu,
            100.0 * restore.healthy_unbalance,
            restore.healthy_frequency_low_hz,
            restore.healthy_frequency_high_hz,
            restore.healthy_delay_s,
            restore.longest_wait_s,
            f64::from(restore.largest_attempts),
            restore.attempt_window_s,
            quality.dip_pu,
            quality.swell_pu,
            quality.interruption_pu,
            quality.hysteresis_pu,
        ]
    }

    /// Names of the entries of [`Self::limits`], comma separated.
    #[must_use]
    pub fn limit_field_names(&self) -> String {
        LIMIT_FIELDS.join(",")
    }

    /// The event log, oldest first, four values per event: time [s], kind
    /// (1 state, 2 breaker, 3 alarm raised, 4 alarm cleared, 5 command
    /// refused, 6 sequence step, 7 cabinet protection trip), code, value.
    /// For a protection trip the code is ten times the function's index plus
    /// the phase (1 to 3, or 0), and the value is what it tripped at.
    #[must_use]
    pub fn events(&self) -> Vec<f64> {
        let mut flat = Vec::with_capacity(self.plant.events().len() * EVENT_STRIDE);
        for event in self.plant.events() {
            flat.extend_from_slice(&[
                event.time_s,
                f64::from(event.kind),
                f64::from(event.code),
                event.value,
            ]);
        }
        flat
    }

    /// Empty the event log.
    pub fn clear_events(&mut self) {
        self.plant.clear_events();
    }

    /// The power-quality analyser's record of closed voltage events, oldest
    /// first, five values per event: kind (1 dip, 2 swell, 3 interruption),
    /// phase (1, 2, 3 for a, b, c), start by the analyser's own clock [s],
    /// duration [s], residual or highest voltage [V, phase to neutral].
    #[must_use]
    pub fn analyser_events(&self) -> Vec<f64> {
        let events = self.plant.analyser.events();
        let mut flat = Vec::with_capacity(events.len() * crate::analyser::QUALITY_EVENT_STRIDE);
        for event in events {
            flat.extend_from_slice(&[
                f64::from(event.kind),
                f64::from(event.phase),
                event.start_s,
                event.duration_s,
                event.extreme_v,
            ]);
        }
        flat
    }

    /// Empty the analyser's record. Events still open stay open.
    pub fn clear_analyser_events(&mut self) {
        self.plant.analyser.clear_events();
    }

    /// Fixed facts of the machine: `[cylinders, pole pairs, shaft inertia
    /// kg m2, generator kVA, generator V, bus kV, fitted friction power at
    /// rated speed kW, rated fuel flow kg/s]`.
    #[must_use]
    pub fn machine_facts(&self) -> Vec<f64> {
        let card = &self.plant.generator_card;
        vec![
            f64::from(CYLINDERS),
            f64::from(card.pole_pairs),
            SHAFT_INERTIA_KG_M2,
            card.rated_va * 1.0e-3,
            card.rated_voltage_v,
            BUS_VOLTAGE_V * 1.0e-3,
            self.plant.rated_friction_w * 1.0e-3,
            self.plant.rated_fuel_kg_s,
        ]
    }
}

impl KguSim {
    /// The plant, for native tests and tools.
    #[must_use]
    pub fn plant(&self) -> &KguPlant {
        &self.plant
    }

    fn pack_snapshot(&mut self) {
        let plant = &self.plant;
        let operator = plant.operator();
        let controller = plant.controller_state();
        let engine = plant.engine_state();
        let shaft = plant.shaft_state();
        let starter = plant.starter_state();
        let generator = plant.generator_state();
        let valve = plant.gas_valve_state();
        let ledger = plant.system.ledger();
        let check = plant.synchro_check();
        let coolant_k = plant.coolant_k();
        let exhaust = plant.exhaust_hx_state();
        let network = plant.network_hx_state();
        let dump = plant.dump_cooler_state();
        let omega = shaft.velocity_m_s;
        let pole_pairs = f64::from(plant.generator_card.pole_pairs);

        let net_torque =
            engine.indicated_torque_nm - engine.friction_torque_nm + starter.torque_nm + generator.torque_nm;
        // Efficiencies are this step's outputs over the fuel power; without fuel they are undefined.
        let fuel_flowing = engine.firing && engine.fuel_power_w > 1.0e3;
        let (brake_efficiency, electric_efficiency) = if fuel_flowing {
            let brake = (engine.indicated_torque_nm - engine.friction_torque_nm) * omega;
            (
                100.0 * brake / engine.fuel_power_w,
                100.0 * generator.electric_power_w / engine.fuel_power_w,
            )
        } else {
            (f64::NAN, f64::NAN)
        };
        let gas_mass_kg = plant.boundary_mass_in_kg("gas_valve.inlet");
        let gas_std_m3 = gas_mass_kg / plant.gas.standard_density_kg_m3();

        let s = &mut self.snapshot;
        s[snap::LAYOUT_VERSION] = SNAPSHOT_LAYOUT_VERSION;
        s[snap::TIME_S] = plant.system.time_s();
        s[snap::STATE_CODE] = f64::from(controller.sequence.state_code());
        s[snap::SEQUENCE_STEP] = f64::from(controller.sequence.index());
        s[snap::BREAKER_CLOSED] = if generator.breaker_closed { 1.0 } else { 0.0 };
        s[snap::ALARM_BITS] = f64::from(controller.alarms | controller.warnings);
        s[snap::SPEED_RPM] = omega * 30.0 / PI;
        s[snap::TORQUE_INDICATED_NM] = engine.indicated_torque_nm;
        s[snap::TORQUE_FRICTION_NM] = engine.friction_torque_nm;
        s[snap::TORQUE_STARTER_NM] = starter.torque_nm;
        s[snap::TORQUE_GENERATOR_NM] = generator.torque_nm;
        s[snap::TORQUE_NET_NM] = net_torque;
        s[snap::ELECTRIC_POWER_KW] = generator.electric_power_w * 1.0e-3;
        s[snap::REACTIVE_POWER_KVAR] = generator.reactive_power_var * 1.0e-3;
        s[snap::STATOR_CURRENT_A] = generator.line_current_a;
        s[snap::POWER_FACTOR] = generator.power_factor();
        s[snap::LOAD_ANGLE_DEG] = generator.load_angle_rad.to_degrees();
        s[snap::GENERATOR_VOLTAGE_V] = generator.terminal_voltage_v;
        s[snap::BUS_VOLTAGE_KV] = plant.bus_voltage_v() * 1.0e-3;
        s[snap::BUS_FREQUENCY_HZ] = operator.grid_frequency_hz;
        s[snap::GENERATOR_FREQUENCY_HZ] = pole_pairs * omega / (2.0 * PI);
        s[snap::SYNC_VOLTAGE_DIFFERENCE_PU] = check.voltage_difference_pu;
        s[snap::SYNC_FREQUENCY_DIFFERENCE_HZ] = check.frequency_difference_hz;
        s[snap::SYNC_PHASE_ANGLE_DEG] = check.phase_angle_rad.to_degrees();
        s[snap::SYNC_PERMITTED] = if check.outside == 0 { 1.0 } else { 0.0 };
        s[snap::FUEL_POWER_KW] = engine.fuel_power_w * 1.0e-3;
        s[snap::GAS_FLOW_STD_M3_H] = plant.standard_flow_m3_s() * 3_600.0;
        s[snap::GAS_FLOW_METER_M3_H] = plant.meter_flow_m3_s() * 3_600.0;
        s[snap::GAS_VALVE_LIFT] = valve.lift;
        s[snap::VALVE_DEMAND] = controller.valve_demand;
        s[snap::BRAKE_EFFICIENCY_PCT] = brake_efficiency;
        s[snap::ELECTRIC_EFFICIENCY_PCT] = electric_efficiency;
        // Heat recovery. The exhaust loss is what leaves by the stack after the exchanger.
        let kelvin_to_c = |kelvin: f64| if kelvin > 0.0 { kelvin - 273.15 } else { f64::NAN };
        s[snap::HEAT_TO_NETWORK_KW] = network.heat_w * 1.0e-3;
        s[snap::HEAT_DUMPED_KW] = dump.heat_w * 1.0e-3;
        s[snap::EXHAUST_LOSS_KW] = (exhaust.stack_loss_w + engine.unburned_w) * 1.0e-3;
        s[snap::EXHAUST_RECOVERED_KW] = exhaust.recovered_w * 1.0e-3;
        s[snap::STACK_TEMPERATURE_C] = kelvin_to_c(exhaust.stack_k);
        s[snap::NETWORK_SUPPLY_C] = kelvin_to_c(network.network_supply_k);
        s[snap::THERMOSTAT_OPENING] = network.thermostat_opening;
        s[snap::DUMP_VALVE_OPENING] = dump.valve_opening;
        s[snap::BLOCK_LOSS_KW] =
            plant.coolant_card.room_conductance_w_k * (coolant_k - crate::plant::ROOM_K) * 1.0e-3;
        s[snap::THERMAL_EFFICIENCY_PCT] = if fuel_flowing {
            100.0 * network.heat_w / engine.fuel_power_w
        } else {
            f64::NAN
        };
        s[snap::ENERGY_DUMPED_KWH] = plant.sink_heat_j(crate::plant::sink::DUMP) / 3.6e6;
        s[snap::RADIATION_LOSS_KW] = engine.radiation_w * 1.0e-3;
        s[snap::FRICTION_POWER_KW] = engine.friction_power_w * 1.0e-3;
        s[snap::GENERATOR_LOSS_KW] = generator.total_loss_w() * 1.0e-3;
        s[snap::JACKET_HEAT_KW] = engine.jacket_heat_w * 1.0e-3;
        // Supply is the coolant node's temperature (engine outlet). Return is what the pump
        // sends back into the engine: the supply less the rise across jacket and exhaust
        // exchanger at the pump's flow (a steady relation, shown for the page).
        s[snap::COOLANT_SUPPLY_C] = coolant_k - 273.15;
        s[snap::COOLANT_RETURN_C] = coolant_k
            - (engine.jacket_heat_w + exhaust.recovered_w) / plant.coolant_card.pump_capacity_rate_w_k
            - 273.15;
        s[snap::EXHAUST_TEMPERATURE_C] = kelvin_to_c(exhaust.exhaust_k);
        s[snap::LOAD_REFERENCE_PCT] = 100.0 * controller.load_reference;
        s[snap::SPEED_REFERENCE_RPM] = controller.speed_reference_rad_s * 30.0 / PI;
        s[snap::PASSPORT_LOAD_PCT] = 100.0 * engine.passport_electric_w / RATED_ELECTRIC_W;
        s[snap::LEDGER_RESIDUAL_J] = ledger.energy_residual_j();
        s[snap::LEDGER_BOUNDARY_ENERGY_MJ] = plant.boundary_energy_in_j("gas_valve.inlet") * 1.0e-6;
        s[snap::ENERGY_ELECTRIC_KWH] = -plant.boundary_energy_in_j("generator.grid") / 3.6e6;
        s[snap::ENERGY_HEAT_KWH] = plant.sink_heat_j(crate::plant::sink::NETWORK) / 3.6e6;
        s[snap::ENERGY_FUEL_KWH] = gas_std_m3 * plant.gas.lower_heating_value_j_m3 / 3.6e6;
        s[snap::GAS_STD_M3] = gas_std_m3;
        s[snap::GAS_METER_M3] = valve.meter_volume_m3;
        s[snap::ENGINE_HOURS] = controller.running_time_s / 3_600.0;
        s[snap::START_COUNT] = f64::from(controller.starts);
        s[snap::KINETIC_ENERGY_KJ] = 0.5 * SHAFT_INERTIA_KG_M2 * omega * omega * 1.0e-3;
        s[snap::NEWTON_ITERATIONS] = f64::from(plant.last_iterations);
        s[snap::LAST_CLOSE_REFUSAL_BITS] = f64::from(controller.last_close_refusal);

        // The bus's three phases, the generator's three currents, and the cabinet.
        let cabinet = plant.cabinet_state();
        let card = &plant.generator_card;
        let phase_volts = card.rated_phase_voltage_v();
        s[snap::PHASE_A_VOLTAGE_V] = operator.grid_phase_voltage_pu[0] * phase_volts;
        s[snap::PHASE_B_VOLTAGE_V] = operator.grid_phase_voltage_pu[1] * phase_volts;
        s[snap::PHASE_C_VOLTAGE_V] = operator.grid_phase_voltage_pu[2] * phase_volts;
        s[snap::PHASE_A_CURRENT_A] = generator.phase_current_a[0];
        s[snap::PHASE_B_CURRENT_A] = generator.phase_current_a[1];
        s[snap::PHASE_C_CURRENT_A] = generator.phase_current_a[2];
        s[snap::VOLTAGE_UNBALANCE_PCT] = 100.0 * operator.grid_sequences().unbalance();
        s[snap::NEGATIVE_SEQUENCE_CURRENT_PCT] =
            100.0 * generator.negative_sequence_current_a / card.rated_current_a();
        s[snap::ROTOR_THERMAL_USED_FRAC] =
            cabinet.rotor_heat_s / plant.cabinet_card.negative_sequence_limit_s;
        s[snap::PROTECTION_PICKUP_BITS] = f64::from(cabinet.pickup);
        s[snap::PROTECTION_TRIPPED_BITS] = f64::from(cabinet.tripped);
        s[snap::NEGATIVE_SEQUENCE_LOSS_KW] = (generator.negative_sequence_stator_loss_w
            + generator.negative_sequence_rotor_loss_w)
            * 1.0e-3;

        // Restoration after a grid event.
        let waiting = controller.sequence == crate::controller::Sequence::GridWait;
        let counting = waiting
            && operator.restoration_auto
            && controller.alarms & crate::controller::alarm::RESTORATION_LOCKOUT == 0;
        s[snap::RESTORATION_MODE] = if operator.restoration_auto { 1.0 } else { 0.0 };
        s[snap::GRID_HEALTHY_COUNTDOWN_S] = if counting {
            (plant.restoration_card.healthy_delay_s - controller.grid_healthy_time_s).max(0.0)
        } else {
            f64::NAN
        };
        s[snap::RESTORATION_ATTEMPTS] = f64::from(controller.restoration_attempts);
        s[snap::WAITING_FOR_GRID] = if waiting { 1.0 } else { 0.0 };

        // The power-quality analyser.
        s[snap::ANALYSER_CLOCK_S] = plant.analyser.clock_s(plant.system.time_s());
        s[snap::ANALYSER_OPEN_EVENTS] = f64::from(plant.analyser.open_events());
        s[snap::ANALYSER_RECORDED_EVENTS] = f64::from(plant.analyser.recorded_events());
    }
}
