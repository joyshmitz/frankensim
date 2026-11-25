//! The genset controller: governor, start/stop sequencer, synchroniser and
//! protection, as one `fs_ports` Component with signal ports only.
//!
//! It is a device, described by its setpoints ([`ControllerSetpoints`]). It
//! reads measurements sampled at the start of each step and operator inputs,
//! and drives the starter, the gas shut-off valve, the ignition, the gas
//! control valve and the generator breaker. Every transition is taken on a
//! measured condition (a speed, a power, the synchronisation window) or on one
//! of its own timers, which are settings of the device. What the plant does in
//! between is the physics of the other components.
//!
//! The speed loop is `fs_control::speed::SpeedController` (sampled PI with
//! tracking anti-windup); this module shapes its reference (run-up ramp,
//! synchronising slip, droop on measured power, load ramp) and turns its
//! torque request into a valve demand.
//!
//! The breaker itself and the protection against grid events are the
//! cabinet's (`cabinet.rs`): the controller asks the cabinet to close, and
//! learns of a protection trip from the cabinet's trip count. A unit fault
//! stops the engine. A grid event only takes the unit off the bus: the engine
//! runs on unloaded in [`Sequence::GridWait`], and comes back either by the
//! automation, once the grid has been healthy for the set time, or by the
//! operator's acknowledgement and close request ([`RestorationCard`]).

use core::f64::consts::PI;

use fs_control::speed::{
    ShaftPowerLimits, SpeedCommand, SpeedController, SpeedControllerParameters,
    SpeedControllerState,
};
use fs_exec::Cx;
use fs_ports::component::{Class, Component, PortSpec, Proposal, Receipt};

use crate::cabinet::protection;
use crate::cards::{
    ControllerSetpoints, NOMINAL_FREQUENCY_HZ, RATED_ELECTRIC_W, RestorationCard,
    SynchronisationWindowCard,
};
use crate::components::{KguError, all_finite, check_cx, nz, sid, wrap_angle};

/// Width of the `sense` input: `[omega, load angle, breaker closed, electric
/// power, out of step, coolant temperature, cabinet trip count, cabinet
/// tripped functions, grid healthy]`.
pub const SENSE_WIDTH: usize = 9;
/// Width of the `operator` input, see [`OperatorSignals`].
pub const OPERATOR_WIDTH: usize = 17;
/// Width of the `actuators` output: `[starter, gas shut-off open, ignition,
/// valve demand, breaker close request, cabinet reset]`.
pub const ACTUATOR_WIDTH: usize = 6;

/// Alarm and warning bits. Trips latch until acknowledged; warnings follow
/// their condition. Codes with a letter and a number are the station
/// controller's event codes (SPEC 3.4); the others are this model's own.
pub mod alarm {
    /// A-105: high coolant temperature (trip).
    pub const COOLANT_TEMPERATURE: u32 = 1 << 0;
    /// A-150: low gas pressure at the inlet (trip).
    pub const GAS_PRESSURE: u32 = 1 << 1;
    /// A-160: low oil pressure (trip).
    pub const OIL_PRESSURE: u32 = 1 << 2;
    /// A-031: mains protection, under-voltage (trip).
    pub const GRID_UNDERVOLTAGE: u32 = 1 << 3;
    /// Over-speed (trip).
    pub const OVERSPEED: u32 = 1 << 4;
    /// Emergency stop pressed (trip).
    pub const EMERGENCY_STOP: u32 = 1 << 5;
    /// W-210: failed start (latched, blocks the next start until acknowledged).
    pub const FAILED_START: u32 = 1 << 6;
    /// Generator out of step: pole slip (trip).
    pub const OUT_OF_STEP: u32 = 1 << 7;
    /// W-118: emergency cooler fan failure (warning).
    pub const COOLER_FAN: u32 = 1 << 8;
    /// Coolant temperature above the warning level (warning).
    pub const COOLANT_WARNING: u32 = 1 << 9;
    /// Gas supply pressure below the pressure switch (warning).
    pub const GAS_PRESSURE_WARNING: u32 = 1 << 10;
    /// Cabinet protection: voltage unbalance (grid event).
    pub const VOLTAGE_UNBALANCE: u32 = 1 << 11;
    /// Cabinet protection: negative-sequence current, rotor thermal limit
    /// (grid event).
    pub const NEGATIVE_SEQUENCE: u32 = 1 << 12;
    /// Cabinet protection: over-current (unit fault, trip).
    pub const OVERCURRENT: u32 = 1 << 13;
    /// Cabinet protection: over-voltage (grid event).
    pub const OVERVOLTAGE: u32 = 1 << 14;
    /// Cabinet protection: under- or over-frequency (grid event).
    pub const FREQUENCY: u32 = 1 << 15;
    /// Cabinet protection: reverse power (unit fault, trip).
    pub const REVERSE_POWER: u32 = 1 << 16;
    /// The automatic return is blocked after too many attempts (latched).
    pub const RESTORATION_LOCKOUT: u32 = 1 << 17;
    /// Number of bits in use.
    pub const COUNT: u32 = 18;
    /// Grid events: the cabinet opened the breaker, the engine runs on
    /// unloaded. `GRID_UNDERVOLTAGE` is the station controller's A-031.
    pub const GRID_EVENTS: u32 =
        GRID_UNDERVOLTAGE | VOLTAGE_UNBALANCE | NEGATIVE_SEQUENCE | OVERVOLTAGE | FREQUENCY;
    /// The bits that latch until acknowledged (or, for grid events, until
    /// the automation returns the unit to the bus by itself).
    pub const TRIPS: u32 = COOLANT_TEMPERATURE
        | GAS_PRESSURE
        | OIL_PRESSURE
        | OVERSPEED
        | EMERGENCY_STOP
        | FAILED_START
        | OUT_OF_STEP
        | OVERCURRENT
        | REVERSE_POWER
        | GRID_EVENTS
        | RESTORATION_LOCKOUT;

    /// The alarm bits for the cabinet's tripped protection functions
    /// (`cabinet::protection` bits).
    #[must_use]
    pub fn from_cabinet(functions: u32) -> u32 {
        use crate::cabinet::protection as p;
        let has = |function: u32| functions & (1 << function) != 0;
        let mut bits = 0;
        if has(p::UNDERVOLTAGE) {
            bits |= GRID_UNDERVOLTAGE;
        }
        if has(p::OVERVOLTAGE) {
            bits |= OVERVOLTAGE;
        }
        if has(p::VOLTAGE_UNBALANCE) {
            bits |= VOLTAGE_UNBALANCE;
        }
        if has(p::NEGATIVE_SEQUENCE) {
            bits |= NEGATIVE_SEQUENCE;
        }
        if has(p::OVERCURRENT) || has(p::OVERCURRENT_HIGH) {
            bits |= OVERCURRENT;
        }
        if has(p::UNDERFREQUENCY) || has(p::OVERFREQUENCY) {
            bits |= FREQUENCY;
        }
        if has(p::REVERSE_POWER) {
            bits |= REVERSE_POWER;
        }
        bits
    }
}

/// Why a breaker close request is refused, one bit per reason.
pub mod refusal {
    /// Voltage difference outside the window.
    pub const VOLTAGE: u32 = 1 << 0;
    /// Frequency difference outside the window.
    pub const FREQUENCY: u32 = 1 << 1;
    /// Phase angle outside the window.
    pub const PHASE: u32 = 1 << 2;
    /// Bus voltage too low to synchronise to.
    pub const DEAD_BUS: u32 = 1 << 3;
    /// The unit is not in its synchronising step.
    pub const NOT_SYNCHRONISING: u32 = 1 << 4;
    /// A trip is latched.
    pub const TRIP_LATCHED: u32 = 1 << 5;
}

/// Step of the controller's sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sequence {
    /// At standstill, ready to start.
    Ready,
    /// Start accepted: the pre-lubrication timer runs, nothing turns yet.
    PreLubrication,
    /// Starter engaged: purging, then waiting for the firing speed.
    Cranking,
    /// Firing, speed reference ramping to rated.
    RunUp,
    /// At rated speed with the breaker open, on the warm-up timer.
    WarmUp,
    /// At speed with the breaker open, waiting for the window.
    Synchronising,
    /// Breaker closed, following the load setpoint.
    OnLoad,
    /// Breaker closed, load reference ramping down before a stop.
    Unloading,
    /// Breaker open, running unloaded at rated speed on its cool-down timer.
    CoolDown,
    /// Gas shut, coasting to standstill.
    CoastDown,
    /// A trip is latched: gas shut, breaker open, start blocked.
    Tripped,
    /// The cabinet's protection opened the breaker on a grid event: the
    /// engine runs unloaded at rated speed and waits for the grid (automatic
    /// return) or for the operator (manual return).
    GridWait,
}

impl Sequence {
    /// Index of the step, in declaration order.
    #[must_use]
    pub fn index(self) -> u32 {
        match self {
            Self::Ready => 0,
            Self::PreLubrication => 1,
            Self::Cranking => 2,
            Self::RunUp => 3,
            Self::WarmUp => 4,
            Self::Synchronising => 5,
            Self::OnLoad => 6,
            Self::Unloading => 7,
            Self::CoolDown => 8,
            Self::CoastDown => 9,
            Self::Tripped => 10,
            Self::GridWait => 11,
        }
    }

    /// The unit state of the station's dictionary (SPEC 3.4): 0 ready,
    /// 1 starting, 2 synchronizing, 3 running, 4 stopping, 5 trip. Unloading
    /// is still "running": the breaker is closed.
    #[must_use]
    pub fn state_code(self) -> u32 {
        match self {
            Self::Ready => 0,
            Self::PreLubrication | Self::Cranking | Self::RunUp | Self::WarmUp => 1,
            // Waiting for the grid is "synchronizing" in the dictionary's six states: at
            // speed, breaker open, not stopped. The alarm bits say why.
            Self::Synchronising | Self::GridWait => 2,
            Self::OnLoad | Self::Unloading => 3,
            Self::CoolDown | Self::CoastDown => 4,
            Self::Tripped => 5,
        }
    }
}

/// What the synchroniser sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynchroCheck {
    /// Generator open-circuit EMF minus bus voltage [pu].
    pub voltage_difference_pu: f64,
    /// Generator frequency minus bus frequency [Hz].
    pub frequency_difference_hz: f64,
    /// Angle of the EMF ahead of the bus voltage [rad], in `(-pi, pi]`.
    pub phase_angle_rad: f64,
    /// Reasons the window is not met ([`refusal`] bits `VOLTAGE`,
    /// `FREQUENCY`, `PHASE`, `DEAD_BUS`); zero when the breaker may close.
    pub outside: u32,
}

impl SynchroCheck {
    /// The check with nothing measured yet.
    pub const NONE: Self = Self {
        voltage_difference_pu: 0.0,
        frequency_difference_hz: 0.0,
        phase_angle_rad: 0.0,
        outside: refusal::DEAD_BUS,
    };
}

/// Compare the generator's open-circuit EMF with the bus against `window`.
///
/// `emf_pu` is the open-circuit EMF [pu], `generator_hz` its frequency,
/// `phase_rad` the angle of the EMF ahead of the bus.
#[must_use]
pub fn synchro_check(
    window: &SynchronisationWindowCard,
    emf_pu: f64,
    generator_hz: f64,
    phase_rad: f64,
    bus_pu: f64,
    bus_hz: f64,
) -> SynchroCheck {
    let voltage_difference_pu = emf_pu - bus_pu;
    let frequency_difference_hz = generator_hz - bus_hz;
    let phase_angle_rad = wrap_angle(phase_rad);
    let mut outside = 0;
    if !(voltage_difference_pu.abs() <= window.voltage_difference_pu) {
        outside |= refusal::VOLTAGE;
    }
    if !(frequency_difference_hz.abs() <= window.frequency_difference_hz) {
        outside |= refusal::FREQUENCY;
    }
    if !(phase_angle_rad.abs() <= window.phase_angle_rad) {
        outside |= refusal::PHASE;
    }
    if !(bus_pu >= window.live_bus_pu) {
        outside |= refusal::DEAD_BUS;
    }
    SynchroCheck {
        voltage_difference_pu,
        frequency_difference_hz,
        phase_angle_rad,
        outside,
    }
}

/// Reasons a close request would be refused now ([`refusal`] bits), zero when
/// it is accepted. One law for the controller's own step and for the answer
/// given to the operator at once.
#[must_use]
pub fn close_refusal(sequence: Sequence, alarms: u32, check: &SynchroCheck) -> u32 {
    let mut bits = check.outside;
    if sequence != Sequence::Synchronising {
        bits |= refusal::NOT_SYNCHRONISING;
    }
    if alarms & alarm::TRIPS != 0 {
        bits |= refusal::TRIP_LATCHED;
    }
    bits
}

/// Reasons a command to synchronise would be refused now ([`refusal`] bits),
/// zero when it is accepted. The command hands the closing to the automatic
/// synchroniser, which waits for the window by itself, so only the step of
/// the sequence and a latched trip can refuse it.
#[must_use]
pub fn synchronise_refusal(sequence: Sequence, alarms: u32) -> u32 {
    let mut bits = 0;
    if sequence != Sequence::Synchronising {
        bits |= refusal::NOT_SYNCHRONISING;
    }
    if alarms & alarm::TRIPS != 0 {
        bits |= refusal::TRIP_LATCHED;
    }
    bits
}

/// The operator's side of the controller, in the order of the `operator`
/// signal port. Pulses are one for exactly one step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OperatorSignals {
    /// Load setpoint [fraction of rated].
    pub load_setpoint: f64,
    /// Start command (pulse).
    pub start: bool,
    /// Normal stop command (pulse).
    pub stop: bool,
    /// Emergency stop (pulse).
    pub emergency_stop: bool,
    /// Alarm acknowledgement (pulse).
    pub acknowledge: bool,
    /// Manual breaker close request (pulse).
    pub breaker_close: bool,
    /// Manual breaker open request (pulse).
    pub breaker_open: bool,
    /// Whether the synchroniser closes the breaker by itself.
    pub auto_synchronise: bool,
    /// Gas supply gauge pressure as the pressure transmitter reads it [Pa].
    pub gas_gauge_pa: f64,
    /// Low oil pressure switch.
    pub oil_pressure_low: bool,
    /// Bus voltage as the mains protection reads it [pu].
    pub grid_voltage_pu: f64,
    /// Bus frequency [Hz].
    pub grid_frequency_hz: f64,
    /// Excitation setting [pu].
    pub excitation_pu: f64,
    /// Emergency cooler fan failure signal.
    pub fan_failed: bool,
    /// Load ramp setting [fraction of rated per second].
    pub load_ramp_per_s: f64,
    /// Whether the unit returns to the bus by itself after a grid event
    /// (automatic restoration) or waits for the operator (manual).
    pub restoration_auto: bool,
    /// Command to synchronise (pulse): the operator decides, the automatic
    /// synchroniser closes the breaker for this one attempt.
    pub synchronise: bool,
}

impl OperatorSignals {
    /// Write the signals in port order.
    pub fn write(&self, values: &mut [f64]) {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        values[0] = self.load_setpoint;
        values[1] = flag(self.start);
        values[2] = flag(self.stop);
        values[3] = flag(self.emergency_stop);
        values[4] = flag(self.acknowledge);
        values[5] = flag(self.breaker_close);
        values[6] = flag(self.breaker_open);
        values[7] = flag(self.auto_synchronise);
        values[8] = self.gas_gauge_pa;
        values[9] = flag(self.oil_pressure_low);
        values[10] = self.grid_voltage_pu;
        values[11] = self.grid_frequency_hz;
        values[12] = self.excitation_pu;
        values[13] = flag(self.fan_failed);
        values[14] = self.load_ramp_per_s;
        values[15] = flag(self.restoration_auto);
        values[16] = flag(self.synchronise);
    }

    fn read(values: &[f64]) -> Self {
        let flag = |value: f64| value >= 0.5;
        Self {
            load_setpoint: values[0].clamp(0.0, 1.0),
            start: flag(values[1]),
            stop: flag(values[2]),
            emergency_stop: flag(values[3]),
            acknowledge: flag(values[4]),
            breaker_close: flag(values[5]),
            breaker_open: flag(values[6]),
            auto_synchronise: flag(values[7]),
            gas_gauge_pa: values[8],
            oil_pressure_low: flag(values[9]),
            grid_voltage_pu: values[10],
            grid_frequency_hz: values[11],
            excitation_pu: values[12],
            fan_failed: flag(values[13]),
            load_ramp_per_s: values[14].max(0.0),
            restoration_auto: flag(values[15]),
            synchronise: flag(values[16]),
        }
    }
}

/// Accepted state of the controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerState {
    /// Step of the sequence.
    pub sequence: Sequence,
    /// Latched trip bits ([`alarm`]).
    pub alarms: u32,
    /// Warning bits that follow their condition ([`alarm`]).
    pub warnings: u32,
    /// Speed reference of the governor [rad/s].
    pub speed_reference_rad_s: f64,
    /// Load reference after the ramp [fraction of rated].
    pub load_reference: f64,
    /// Integral state of the speed loop.
    pub governor: SpeedControllerState,
    /// Demand to the gas control valve [0, 1].
    pub valve_demand: f64,
    /// Starter engaged.
    pub starter: bool,
    /// Gas shut-off valve open.
    pub gas_open: bool,
    /// Ignition on.
    pub ignition: bool,
    /// Breaker close command.
    pub breaker_command: bool,
    /// The operator opened the breaker: no automatic reclosing until a manual
    /// close request or a new start.
    pub resynchronise_inhibited: bool,
    /// Time in the current step of the sequence [s].
    pub step_time_s: f64,
    /// Time since the start command, while starting [s].
    pub start_time_s: f64,
    /// Cabinet trips already answered (the cabinet's own count).
    pub seen_cabinet_trips: u32,
    /// Reset request to the cabinet's protection, held for one step.
    pub cabinet_reset: bool,
    /// Time the grid has been healthy without a break while waiting for it [s].
    pub grid_healthy_time_s: f64,
    /// Automatic returns made inside the present counting window.
    pub restoration_attempts: u32,
    /// Engine running time at which the counting window began [s].
    pub attempt_window_start_s: f64,
    /// The automation is on its way back to the bus: the synchroniser closes
    /// by itself whatever the operator's auto-synchronise setting.
    pub returning: bool,
    /// The operator has commanded this synchronisation: the automatic
    /// synchroniser closes the breaker for this one attempt, whatever holds
    /// it back otherwise. Cleared when the unit leaves the synchronising step.
    pub synchroniser_armed: bool,
    /// Time the power has fallen short with the valve fully open [s].
    pub gas_shortfall_time_s: f64,
    /// Time the low-oil-pressure input has been on with the engine firing [s].
    pub oil_pressure_time_s: f64,
    /// Time the engine has run with ignition on [s].
    pub running_time_s: f64,
    /// Number of start commands accepted.
    pub starts: u32,
    /// Number of refused commands (close or start), for the event log.
    pub refusals: u32,
    /// Reasons of the last refused close request ([`refusal`] bits).
    pub last_close_refusal: u32,
    /// The synchroniser's view at the start of the last step.
    pub synchronism: SynchroCheck,
}

impl ControllerState {
    /// A unit at standstill, ready to start.
    #[must_use]
    pub fn ready() -> Self {
        Self {
            sequence: Sequence::Ready,
            alarms: 0,
            warnings: 0,
            speed_reference_rad_s: 0.0,
            load_reference: 0.0,
            governor: SpeedControllerState::default(),
            valve_demand: 0.0,
            starter: false,
            gas_open: false,
            ignition: false,
            breaker_command: false,
            resynchronise_inhibited: false,
            step_time_s: 0.0,
            start_time_s: 0.0,
            seen_cabinet_trips: 0,
            cabinet_reset: false,
            grid_healthy_time_s: 0.0,
            restoration_attempts: 0,
            attempt_window_start_s: 0.0,
            returning: false,
            synchroniser_armed: false,
            gas_shortfall_time_s: 0.0,
            oil_pressure_time_s: 0.0,
            running_time_s: 0.0,
            starts: 0,
            refusals: 0,
            last_close_refusal: 0,
            synchronism: SynchroCheck::NONE,
        }
    }
}

/// The genset controller.
///
/// Ports: `sense` (signal in, [`SENSE_WIDTH`]), `operator` (signal in,
/// [`OPERATOR_WIDTH`]), `actuators` (signal out, [`ACTUATOR_WIDTH`]).
pub struct GensetControllerComponent {
    ports: [PortSpec; 3],
    setpoints: ControllerSetpoints,
    window: SynchronisationWindowCard,
    restoration: RestorationCard,
    speed_loop: SpeedController,
    /// Pole pairs of the generator, to turn shaft speed into frequency.
    pole_pairs: f64,
    /// Excitation range the field system accepts [pu].
    excitation_range_pu: (f64, f64),
    /// Indicated torque the engine needs at rated speed with no load [N m]: the
    /// governor's feed-forward calibration.
    no_load_torque_nm: f64,
}

impl GensetControllerComponent {
    /// A controller with the given settings for a generator of `pole_pairs`.
    ///
    /// # Panics
    /// If the speed-loop settings are not valid for `fs-control`.
    #[must_use]
    pub fn new(
        setpoints: ControllerSetpoints,
        window: SynchronisationWindowCard,
        restoration: RestorationCard,
        pole_pairs: u32,
        excitation_range_pu: (f64, f64),
        no_load_torque_nm: f64,
    ) -> Self {
        let speed_loop = SpeedController::new(SpeedControllerParameters {
            sample_period_s: setpoints.sample_period_s,
            proportional_gain_nm_s_per_rad: setpoints.proportional_gain_nm_s_per_rad,
            integral_gain_nm_per_rad: setpoints.integral_gain_nm_per_rad,
            anti_windup_time_s: setpoints.anti_windup_time_s,
        })
        .expect("valid speed-loop settings");
        Self {
            ports: [
                PortSpec::signal_in(sid("sense"), nz(SENSE_WIDTH)),
                PortSpec::signal_in(sid("operator"), nz(OPERATOR_WIDTH)),
                PortSpec::signal_out(sid("actuators"), nz(ACTUATOR_WIDTH)),
            ],
            setpoints,
            window,
            restoration,
            speed_loop,
            pole_pairs: f64::from(pole_pairs),
            excitation_range_pu,
            no_load_torque_nm,
        }
    }

    /// The controller's settings.
    #[must_use]
    pub fn setpoints(&self) -> &ControllerSetpoints {
        &self.setpoints
    }

    /// The synchronisation window.
    #[must_use]
    pub fn window(&self) -> &SynchronisationWindowCard {
        &self.window
    }

    /// The restoration settings.
    #[must_use]
    pub fn restoration(&self) -> &RestorationCard {
        &self.restoration
    }

    /// Rated shaft speed at nominal frequency [rad/s].
    #[must_use]
    pub fn nominal_omega(&self) -> f64 {
        2.0 * PI * NOMINAL_FREQUENCY_HZ / self.pole_pairs
    }

    /// The synchroniser's view for a shaft speed, an EMF angle and the
    /// operator's excitation and bus readings.
    #[must_use]
    pub fn check(
        &self,
        omega_rad_s: f64,
        load_angle_rad: f64,
        excitation_pu: f64,
        bus_pu: f64,
        bus_hz: f64,
    ) -> SynchroCheck {
        let (low, high) = self.excitation_range_pu;
        let generator_hz = self.pole_pairs * omega_rad_s / (2.0 * PI);
        let emf_pu = excitation_pu.clamp(low, high) * generator_hz / NOMINAL_FREQUENCY_HZ;
        synchro_check(&self.window, emf_pu, generator_hz, load_angle_rad, bus_pu, bus_hz)
    }

    fn trip(next: &mut ControllerState) {
        next.sequence = Sequence::Tripped;
        next.step_time_s = 0.0;
        Self::everything_off(next);
    }

    fn everything_off(next: &mut ControllerState) {
        next.starter = false;
        next.gas_open = false;
        next.ignition = false;
        next.breaker_command = false;
        next.valve_demand = 0.0;
        next.load_reference = 0.0;
        next.speed_reference_rad_s = 0.0;
        next.governor = SpeedControllerState::default();
        next.returning = false;
    }

    fn enter(next: &mut ControllerState, sequence: Sequence) {
        next.sequence = sequence;
        next.step_time_s = 0.0;
    }
}

fn approach(value: f64, target: f64, largest_change: f64) -> f64 {
    value + (target - value).clamp(-largest_change, largest_change)
}

impl Component for GensetControllerComponent {
    type State = ControllerState;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        // Thresholds, latches and a saturating speed loop.
        Class::NonSmooth
    }

    fn stored_energy_j(&self, _state: &Self::State) -> Result<f64, Self::Error> {
        Ok(0.0)
    }

    #[allow(clippy::too_many_lines)]
    fn propose(
        &self,
        old: &Self::State,
        dt_s: f64,
        inputs: &[f64],
        new: &mut Self::State,
        outputs: &mut [f64],
        cx: &Cx<'_>,
    ) -> Result<Proposal, Self::Error> {
        check_cx(cx)?;
        all_finite(inputs, "controller inputs must be finite")?;
        let set = &self.setpoints;
        let omega = inputs[0];
        let load_angle = inputs[1];
        let breaker_closed = inputs[2] >= 0.5;
        let electric_power = inputs[3];
        let out_of_step = inputs[4] >= 0.5;
        let coolant_k = inputs[5];
        let cabinet_trips = inputs[6] as u32;
        let cabinet_functions = inputs[7] as u32;
        let grid_healthy = inputs[8] >= 0.5;
        let operator = OperatorSignals::read(&inputs[SENSE_WIDTH..SENSE_WIDTH + OPERATOR_WIDTH]);
        let nominal_omega = self.nominal_omega();
        let restore = &self.restoration;

        let mut next = *old;
        next.step_time_s = old.step_time_s + dt_s;
        next.cabinet_reset = false;
        let check = self.check(
            omega,
            load_angle,
            operator.excitation_pu,
            operator.grid_voltage_pu,
            operator.grid_frequency_hz,
        );
        next.synchronism = check;

        // --- Acknowledgement: clears the latches whose cause is gone. -------------------
        if operator.acknowledge && old.sequence == Sequence::Tripped {
            let mut still = 0;
            if coolant_k >= set.coolant_trip_k {
                still |= alarm::COOLANT_TEMPERATURE;
            }
            if operator.oil_pressure_low && omega > set.standstill_rad_s {
                still |= alarm::OIL_PRESSURE;
            }
            next.alarms = old.alarms & still;
            next.cabinet_reset = true;
            if next.alarms == 0 {
                Self::enter(
                    &mut next,
                    if omega > set.standstill_rad_s {
                        Sequence::CoastDown
                    } else {
                        Sequence::Ready
                    },
                );
            }
        }
        // Away from the bus and not waiting for the grid, an acknowledgement also clears
        // what a grid event left latched (after the automation gave up and stopped the unit).
        if operator.acknowledge
            && matches!(
                old.sequence,
                Sequence::Ready | Sequence::CoolDown | Sequence::CoastDown
            )
            && old.alarms & (alarm::GRID_EVENTS | alarm::RESTORATION_LOCKOUT) != 0
        {
            next.alarms = old.alarms & !(alarm::GRID_EVENTS | alarm::RESTORATION_LOCKOUT);
            next.restoration_attempts = 0;
            next.cabinet_reset = true;
        }

        // --- Protection. -----------------------------------------------------------------
        let on_bus = matches!(old.sequence, Sequence::OnLoad | Sequence::Unloading);
        let mut tripped = 0;
        if operator.emergency_stop {
            tripped |= alarm::EMERGENCY_STOP;
        }
        if omega >= set.overspeed_trip_rad_s {
            tripped |= alarm::OVERSPEED;
        }
        if out_of_step {
            tripped |= alarm::OUT_OF_STEP;
        }
        if old.ignition && coolant_k >= set.coolant_trip_k {
            tripped |= alarm::COOLANT_TEMPERATURE;
        }
        // The cabinet's protection reports a trip by its count. A unit fault stops the
        // engine like the controller's own protection; a grid event only takes the unit
        // off the bus (below).
        let mut grid_event = 0;
        if cabinet_trips != old.seen_cabinet_trips {
            next.seen_cabinet_trips = cabinet_trips;
            if cabinet_functions & protection::UNIT_FAULTS != 0 {
                tripped |= alarm::from_cabinet(cabinet_functions);
            } else {
                grid_event = alarm::from_cabinet(cabinet_functions);
            }
        }
        next.oil_pressure_time_s = if old.ignition && operator.oil_pressure_low {
            old.oil_pressure_time_s + dt_s
        } else {
            0.0
        };
        if next.oil_pressure_time_s >= set.oil_pressure_delay_s {
            tripped |= alarm::OIL_PRESSURE;
        }
        let gas_low = operator.gas_gauge_pa < set.gas_low_pressure_pa;
        let short = RATED_ELECTRIC_W * old.load_reference - electric_power > set.power_shortfall_w;
        next.gas_shortfall_time_s =
            if on_bus && gas_low && short && old.valve_demand >= set.valve_saturated_demand {
                old.gas_shortfall_time_s + dt_s
            } else {
                0.0
            };
        if next.gas_shortfall_time_s >= set.gas_shortfall_delay_s {
            tripped |= alarm::GAS_PRESSURE;
        }
        let starting = matches!(old.sequence, Sequence::Cranking | Sequence::RunUp);
        next.start_time_s = if starting { old.start_time_s + dt_s } else { 0.0 };
        if starting && next.start_time_s >= set.start_time_limit_s {
            tripped |= alarm::FAILED_START;
        }
        if tripped != 0 {
            next.alarms |= tripped;
            Self::trip(&mut next);
        } else if grid_event != 0 && on_bus {
            // The breaker is open; the engine stays at rated speed, unloaded, burning its
            // no-load gas. A stop that was already under way goes on to its cool-down.
            next.alarms |= grid_event;
            next.breaker_command = false;
            next.load_reference = 0.0;
            next.grid_healthy_time_s = 0.0;
            next.returning = false;
            if old.sequence == Sequence::Unloading {
                Self::enter(&mut next, Sequence::CoolDown);
            } else {
                Self::enter(&mut next, Sequence::GridWait);
                if old.running_time_s - old.attempt_window_start_s > restore.attempt_window_s {
                    next.restoration_attempts = 0;
                }
                if operator.restoration_auto
                    && next.restoration_attempts >= restore.largest_attempts
                {
                    next.alarms |= alarm::RESTORATION_LOCKOUT;
                }
            }
        }

        let mut warnings = 0;
        if operator.fan_failed {
            warnings |= alarm::COOLER_FAN;
        }
        if coolant_k >= set.coolant_warning_k {
            warnings |= alarm::COOLANT_WARNING;
        }
        if old.ignition && gas_low {
            warnings |= alarm::GAS_PRESSURE_WARNING;
        }
        next.warnings = warnings;

        // --- Sequence. -------------------------------------------------------------------
        // A step entered above (trip, acknowledgement) is not left in the same sample.
        if next.sequence == old.sequence {
            match old.sequence {
                Sequence::Ready => {
                    Self::everything_off(&mut next);
                    if operator.start {
                        if old.alarms == 0 {
                            Self::enter(&mut next, Sequence::PreLubrication);
                            next.starts = old.starts + 1;
                            next.resynchronise_inhibited = false;
                            next.returning = false;
                            next.restoration_attempts = 0;
                            next.cabinet_reset = true;
                        } else {
                            next.refusals = old.refusals + 1;
                        }
                    }
                }
                Sequence::PreLubrication => {
                    // The oil pump runs on its timer; the oil circuit itself is not modelled.
                    Self::everything_off(&mut next);
                    if operator.stop {
                        Self::enter(&mut next, Sequence::Ready);
                    } else if next.step_time_s >= set.pre_lubrication_time_s {
                        Self::enter(&mut next, Sequence::Cranking);
                        next.starter = true;
                        next.start_time_s = 0.0;
                    }
                }
                Sequence::Cranking => {
                    next.starter = true;
                    if operator.stop {
                        Self::enter(&mut next, Sequence::CoastDown);
                        Self::everything_off(&mut next);
                    } else if omega >= set.firing_speed_rad_s
                        && next.step_time_s >= set.purge_time_s
                    {
                        // Purged: the engine has turned without gas for the purge time.
                        Self::enter(&mut next, Sequence::RunUp);
                        next.gas_open = true;
                        next.ignition = true;
                        next.speed_reference_rad_s = omega;
                    }
                }
                Sequence::RunUp => {
                    next.starter = omega < set.starter_release_rad_s;
                    next.speed_reference_rad_s = approach(
                        old.speed_reference_rad_s,
                        nominal_omega,
                        set.run_up_rate_rad_s2 * dt_s,
                    );
                    if operator.stop {
                        Self::enter(&mut next, Sequence::CoastDown);
                        Self::everything_off(&mut next);
                    } else if omega >= set.at_speed_share * nominal_omega {
                        Self::enter(&mut next, Sequence::WarmUp);
                        next.starter = false;
                    }
                }
                Sequence::WarmUp => {
                    next.starter = false;
                    next.speed_reference_rad_s = approach(
                        old.speed_reference_rad_s,
                        nominal_omega,
                        set.run_up_rate_rad_s2 * dt_s,
                    );
                    if operator.stop {
                        Self::enter(&mut next, Sequence::CoolDown);
                    } else if next.step_time_s >= set.warm_up_time_s {
                        Self::enter(&mut next, Sequence::Synchronising);
                    }
                }
                Sequence::Synchronising => {
                    next.starter = false;
                    // Hold a small slip above the bus so that the phase comes round.
                    let target = 2.0 * PI
                        * (operator.grid_frequency_hz + set.synchronising_slip_hz)
                        / self.pole_pairs;
                    next.speed_reference_rad_s = approach(
                        old.speed_reference_rad_s,
                        target,
                        set.run_up_rate_rad_s2 * dt_s,
                    );
                    let refused = close_refusal(old.sequence, old.alarms, &check);
                    // Who lets the automatic synchroniser close: the start's own setting
                    // or the automatic return, unless a manual opening or a manual return
                    // holds it back; or the operator's command, for this attempt.
                    next.synchroniser_armed = old.synchroniser_armed || operator.synchronise;
                    let automatic = ((operator.auto_synchronise || old.returning)
                        && !old.resynchronise_inhibited)
                        || next.synchroniser_armed;
                    if operator.stop {
                        Self::enter(&mut next, Sequence::CoolDown);
                        next.returning = false;
                    } else if operator.breaker_close && refused != 0 {
                        next.last_close_refusal = refused;
                        next.refusals = old.refusals + 1;
                    } else if refused == 0
                        && (operator.breaker_close
                            || (automatic
                                && check.phase_angle_rad.abs() <= set.auto_close_phase_rad))
                    {
                        Self::enter(&mut next, Sequence::OnLoad);
                        next.breaker_command = true;
                        next.resynchronise_inhibited = false;
                        next.returning = false;
                        next.load_reference = 0.0;
                        next.last_close_refusal = 0;
                    }
                }
                Sequence::GridWait => {
                    next.starter = false;
                    next.breaker_command = false;
                    next.load_reference = 0.0;
                    next.speed_reference_rad_s = approach(
                        old.speed_reference_rad_s,
                        nominal_omega,
                        set.run_up_rate_rad_s2 * dt_s,
                    );
                    next.grid_healthy_time_s = if grid_healthy {
                        old.grid_healthy_time_s + dt_s
                    } else {
                        0.0
                    };
                    let waiting_bits = alarm::GRID_EVENTS | alarm::RESTORATION_LOCKOUT;
                    if operator.stop {
                        // The operator decides to stop: the normal cool-down.
                        Self::enter(&mut next, Sequence::CoolDown);
                    } else if operator.restoration_auto {
                        if operator.acknowledge {
                            // An acknowledgement releases a blocked automation.
                            next.alarms = old.alarms & !alarm::RESTORATION_LOCKOUT;
                            next.restoration_attempts = 0;
                        }
                        let blocked = next.alarms & alarm::RESTORATION_LOCKOUT != 0;
                        if !blocked
                            && next.grid_healthy_time_s >= restore.healthy_delay_s * (1.0 - 1.0e-9)
                        {
                            // The grid has been healthy for the whole delay: back to the
                            // synchroniser, which closes by itself.
                            if next.restoration_attempts == 0 {
                                next.attempt_window_start_s = old.running_time_s;
                            }
                            next.restoration_attempts += 1;
                            next.alarms &= !waiting_bits;
                            next.cabinet_reset = true;
                            next.returning = true;
                            next.resynchronise_inhibited = false;
                            Self::enter(&mut next, Sequence::Synchronising);
                        } else if next.step_time_s >= restore.longest_wait_s {
                            // Waited long enough at no load: a normal stop. What the grid
                            // event latched stays latched until it is acknowledged.
                            Self::enter(&mut next, Sequence::CoolDown);
                        }
                    } else if operator.acknowledge {
                        // Manual return: the operator has seen the cause. The unit goes back
                        // to the synchroniser and waits there for a close request.
                        next.alarms = old.alarms & !waiting_bits;
                        next.restoration_attempts = 0;
                        next.cabinet_reset = true;
                        next.returning = false;
                        next.resynchronise_inhibited = true;
                        Self::enter(&mut next, Sequence::Synchronising);
                    }
                }
                Sequence::OnLoad | Sequence::Unloading => {
                    let unloading = old.sequence == Sequence::Unloading;
                    next.breaker_command = true;
                    if !breaker_closed && old.step_time_s > 2.0 * set.sample_period_s {
                        // The breaker is open although commanded closed: back to the synchroniser.
                        Self::enter(&mut next, Sequence::Synchronising);
                        next.breaker_command = false;
                        next.resynchronise_inhibited = true;
                        next.load_reference = 0.0;
                    } else if operator.breaker_open {
                        Self::enter(&mut next, Sequence::Synchronising);
                        next.breaker_command = false;
                        next.resynchronise_inhibited = true;
                        next.load_reference = 0.0;
                    } else if unloading {
                        next.load_reference =
                            approach(old.load_reference, 0.0, set.unload_ramp_per_s * dt_s);
                        if operator.start {
                            Self::enter(&mut next, Sequence::OnLoad);
                        } else if next.load_reference <= 0.0
                            && electric_power < set.breaker_open_power_w
                        {
                            Self::enter(&mut next, Sequence::CoolDown);
                            next.breaker_command = false;
                        }
                    } else {
                        next.load_reference = approach(
                            old.load_reference,
                            operator.load_setpoint,
                            operator.load_ramp_per_s * dt_s,
                        );
                        if operator.stop {
                            Self::enter(&mut next, Sequence::Unloading);
                        }
                    }
                }
                Sequence::CoolDown => {
                    next.breaker_command = false;
                    next.load_reference = 0.0;
                    next.speed_reference_rad_s = approach(
                        old.speed_reference_rad_s,
                        nominal_omega,
                        set.run_up_rate_rad_s2 * dt_s,
                    );
                    if next.step_time_s >= set.cool_down_time_s {
                        Self::enter(&mut next, Sequence::CoastDown);
                        Self::everything_off(&mut next);
                    }
                }
                Sequence::CoastDown => {
                    Self::everything_off(&mut next);
                    if omega < set.standstill_rad_s {
                        Self::enter(&mut next, Sequence::Ready);
                    } else if operator.start {
                        next.refusals = old.refusals + 1;
                    }
                }
                Sequence::Tripped => {
                    Self::everything_off(&mut next);
                    if operator.start {
                        next.refusals = old.refusals + 1;
                    }
                }
            }
        }

        // The operator's command to synchronise is for one attempt: it does not outlast
        // the synchronising step, however that step is left.
        if next.sequence != Sequence::Synchronising {
            next.synchroniser_armed = false;
        }

        // --- Governor. -------------------------------------------------------------------
        if next.ignition {
            let on_bus_now = matches!(next.sequence, Sequence::OnLoad | Sequence::Unloading);
            let (reference, feed_forward, demand_limit) = if on_bus_now {
                // Droop on measured power around the nominal speed setting.
                let reference = nominal_omega
                    * (1.0 + set.droop * (next.load_reference - electric_power / RATED_ELECTRIC_W));
                let load_torque = next.load_reference * RATED_ELECTRIC_W / nominal_omega;
                (reference, self.no_load_torque_nm + load_torque, 1.0)
            } else {
                let limit = if next.sequence == Sequence::RunUp {
                    set.start_fuel_limit
                } else {
                    1.0
                };
                (next.speed_reference_rad_s, self.no_load_torque_nm, limit)
            };
            let demand = self
                .speed_loop
                .demand_controlled(
                    &old.governor,
                    [omega, 0.0],
                    SpeedCommand {
                        relative_speed_rad_s: reference,
                        feedforward_torque_nm: feed_forward,
                    },
                    ShaftPowerLimits {
                        positive_torque_limit_nm: demand_limit * set.torque_per_valve_demand_nm,
                        negative_torque_limit_nm: 0.0,
                        motoring_power_w: 1.0e12,
                        regeneration_power_w: 0.0,
                    },
                    || false,
                )
                .map_err(|_| KguError::Invalid("fs-control refused the speed sample"))?;
            let request = demand.requested_torque_nm.max(0.0);
            let proposal = demand
                .finish_controlled(request, || false)
                .map_err(|_| KguError::Invalid("fs-control refused the torque request"))?;
            next.governor = proposal.next_state;
            next.valve_demand = (request / set.torque_per_valve_demand_nm).clamp(0.0, 1.0);
            next.running_time_s = old.running_time_s + dt_s;
        } else {
            next.governor = SpeedControllerState::default();
            next.valve_demand = 0.0;
        }

        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        outputs[0] = flag(next.starter);
        outputs[1] = flag(next.gas_open);
        outputs[2] = flag(next.ignition);
        outputs[3] = next.valve_demand;
        outputs[4] = flag(next.breaker_command);
        outputs[5] = flag(next.cabinet_reset);
        *new = next;
        Ok(Proposal::Step(Receipt::lossless(0.0, 1.0e-12)))
    }
}
