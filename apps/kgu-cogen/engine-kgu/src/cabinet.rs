//! The generator breaker cabinet: the breaker's close path and the
//! protection device, as one `fs_ports` Component with signal ports only.
//!
//! The genset controller asks for the breaker to be closed; the cabinet
//! passes that request on unless one of its protection functions has
//! tripped. Each function is a setting of the relay
//! ([`BreakerCabinetCard`]): a level and a time, or for the negative-sequence
//! current a thermal replica `(I2 / I_rated)^2 t` against a limit. The
//! replica is the relay's estimate of the rotor's heating; the rotor's
//! temperature itself is not a state of this model.
//!
//! The device measures what a relay measures: the three phase voltages of
//! the bus (the scenario's input, so the present values), and the phase
//! currents, the negative-sequence current and the power of the generator as
//! they stood at the start of the step. Like the controller it has
//! thresholds and latches, so it stands outside the plant's Newton block and
//! reaches the generator through a signal.
//!
//! The cabinet's third instrument, the power-quality analyser, records and
//! switches nothing; it is in `analyser.rs`.

use fs_exec::Cx;
use fs_ports::component::{Class, Component, PortSpec, Proposal, Receipt};

use crate::cards::{BreakerCabinetCard, DefiniteTimeCard, RestorationCard};
use crate::components::{KguError, all_finite, check_cx, nz, sid};

/// Width of the `command` input: `[close request, reset]`.
pub const COMMAND_WIDTH: usize = 2;
/// Width of the `measure` input: three phase voltages [pu], their three
/// angle offsets [rad], three phase currents [A], the negative-sequence
/// current [A], the frequency [Hz], the power delivered [W], breaker closed.
pub const MEASURE_WIDTH: usize = 13;

/// Protection functions of the cabinet: an index each, and a bit `1 << index`.
pub mod protection {
    /// Under-voltage, per phase.
    pub const UNDERVOLTAGE: u32 = 0;
    /// Over-voltage, per phase.
    pub const OVERVOLTAGE: u32 = 1;
    /// Voltage unbalance, negative over positive sequence.
    pub const VOLTAGE_UNBALANCE: u32 = 2;
    /// Negative-sequence current against the rotor's thermal limit.
    pub const NEGATIVE_SEQUENCE: u32 = 3;
    /// Over-current, per phase, time-delayed stage.
    pub const OVERCURRENT: u32 = 4;
    /// Over-current, per phase, high-set stage.
    pub const OVERCURRENT_HIGH: u32 = 5;
    /// Under-frequency.
    pub const UNDERFREQUENCY: u32 = 6;
    /// Over-frequency.
    pub const OVERFREQUENCY: u32 = 7;
    /// Reverse power.
    pub const REVERSE_POWER: u32 = 8;
    /// Number of functions.
    pub const COUNT: usize = 9;

    /// The functions that answer an event on the grid: the breaker opens and
    /// the engine keeps running unloaded.
    pub const GRID_EVENTS: u32 = (1 << UNDERVOLTAGE)
        | (1 << OVERVOLTAGE)
        | (1 << VOLTAGE_UNBALANCE)
        | (1 << NEGATIVE_SEQUENCE)
        | (1 << UNDERFREQUENCY)
        | (1 << OVERFREQUENCY);
    /// The functions that point at a fault of the unit itself (a current no
    /// grid event of this model produces, an engine that no longer drives):
    /// the unit is stopped and waits for an acknowledgement.
    pub const UNIT_FAULTS: u32 =
        (1 << OVERCURRENT) | (1 << OVERCURRENT_HIGH) | (1 << REVERSE_POWER);
}

/// Positive- and negative-sequence phasors of three phase voltages, as
/// `[real, imaginary]` in the unit of the magnitudes, against the nominal
/// position of phase a.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sequences {
    /// Positive sequence.
    pub positive: [f64; 2],
    /// Negative sequence.
    pub negative: [f64; 2],
}

impl Sequences {
    /// Magnitude of the positive sequence.
    #[must_use]
    pub fn positive_magnitude(&self) -> f64 {
        self.positive[0].hypot(self.positive[1])
    }

    /// Magnitude of the negative sequence.
    #[must_use]
    pub fn negative_magnitude(&self) -> f64 {
        self.negative[0].hypot(self.negative[1])
    }

    /// Angle of the positive sequence [rad].
    #[must_use]
    pub fn positive_angle(&self) -> f64 {
        self.positive[1].atan2(self.positive[0])
    }

    /// Unbalance: negative over positive sequence; zero for a dead bus.
    #[must_use]
    pub fn unbalance(&self) -> f64 {
        let positive = self.positive_magnitude();
        if positive > 0.0 {
            self.negative_magnitude() / positive
        } else {
            0.0
        }
    }

    /// The negative sequence as a phasor against the positive one (turned
    /// back by the positive sequence's angle).
    #[must_use]
    pub fn negative_against_positive(&self) -> [f64; 2] {
        let magnitude = self.positive_magnitude();
        if !(magnitude > 0.0) {
            return self.negative;
        }
        let (cos, sin) = (self.positive[0] / magnitude, self.positive[1] / magnitude);
        [
            self.negative[0] * cos + self.negative[1] * sin,
            self.negative[1] * cos - self.negative[0] * sin,
        ]
    }
}

/// Symmetrical components of three phase voltages given by their magnitudes
/// and by how far each phasor is turned from its nominal position (phase b a
/// third of a turn behind phase a, phase c a third ahead). The zero sequence
/// is not returned: the generator's winding and the transformer give it no
/// path in this model.
///
/// Three equal magnitudes at their nominal positions give exactly the
/// magnitude and exactly zero: the sums are formed so that nothing is left
/// to rounding.
#[must_use]
pub fn sequence_components(magnitude: [f64; 3], angle_offset_rad: [f64; 3]) -> Sequences {
    let half_root_3 = 0.5 * 3.0_f64.sqrt();
    let phasor = |index: usize| -> [f64; 2] {
        let (sin, cos) = angle_offset_rad[index].sin_cos();
        [magnitude[index] * cos, magnitude[index] * sin]
    };
    let (a, b, c) = (phasor(0), phasor(1), phasor(2));
    // Positive: (Va + a Vb + a^2 Vc) / 3, with every phasor turned to phase a's position.
    let positive = [
        a[0] + ((b[0] - a[0]) + (c[0] - a[0])) / 3.0,
        a[1] + ((b[1] - a[1]) + (c[1] - a[1])) / 3.0,
    ];
    // Negative: (Va + a^2 Vb + a Vc) / 3; phase b lands a third ahead, phase c a third behind.
    let negative = [
        (a[0] - 0.5 * b[0] - 0.5 * c[0] - (half_root_3 * b[1] - half_root_3 * c[1])) / 3.0,
        (a[1] - 0.5 * b[1] - 0.5 * c[1] + (half_root_3 * b[0] - half_root_3 * c[0])) / 3.0,
    ];
    Sequences { positive, negative }
}

/// Whether a grid counts as healthy for the automatic return.
#[must_use]
pub fn grid_is_healthy(
    card: &RestorationCard,
    phase_voltage_pu: [f64; 3],
    unbalance: f64,
    frequency_hz: f64,
) -> bool {
    phase_voltage_pu.iter().all(|&voltage| {
        voltage >= card.healthy_voltage_low_pu && voltage <= card.healthy_voltage_high_pu
    }) && unbalance <= card.healthy_unbalance
        && frequency_hz >= card.healthy_frequency_low_hz
        && frequency_hz <= card.healthy_frequency_high_hz
}

/// Accepted state of the cabinet's protection device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CabinetState {
    /// The close request as the cabinet passes it to the breaker.
    pub closed: bool,
    /// Functions that have tripped and hold the breaker open until a reset,
    /// one bit per function.
    pub tripped: u32,
    /// Functions whose condition holds now, one bit per function.
    pub pickup: u32,
    /// Time each function's condition has held [s].
    pub timer_s: [f64; protection::COUNT],
    /// The rotor's thermal replica, `integral (I2 / I_rated)^2 dt` [s].
    pub rotor_heat_s: f64,
    /// Number of trips since the model started.
    pub trips: u32,
    /// The function that tripped last (its index).
    pub last_function: u32,
    /// The phase it tripped on: 1, 2, 3 for a, b, c; 0 when the function is
    /// not about one phase.
    pub last_phase: u32,
    /// The value it tripped at, in the function's unit: voltage [pu],
    /// unbalance [-], negative-sequence current [pu], current [pu],
    /// frequency [Hz], reverse power [W].
    pub last_value: f64,
    /// Whether the grid is healthy by the restoration card.
    pub grid_healthy: bool,
    /// Voltage unbalance, negative over positive sequence [-].
    pub unbalance: f64,
}

impl CabinetState {
    /// A cabinet with its breaker open and nothing tripped.
    #[must_use]
    pub fn open() -> Self {
        Self {
            closed: false,
            tripped: 0,
            pickup: 0,
            timer_s: [0.0; protection::COUNT],
            rotor_heat_s: 0.0,
            trips: 0,
            last_function: 0,
            last_phase: 0,
            last_value: 0.0,
            grid_healthy: false,
            unbalance: 0.0,
        }
    }
}

/// The generator breaker cabinet's protection device.
///
/// Ports: `command` (signal in, [`COMMAND_WIDTH`]), `measure` (signal in,
/// [`MEASURE_WIDTH`]), `breaker` (signal out: the close request the breaker
/// receives).
pub struct BreakerCabinetComponent {
    ports: [PortSpec; 3],
    card: BreakerCabinetCard,
    restoration: RestorationCard,
    rated_current_a: f64,
}

/// What one function sees in a step.
#[derive(Debug, Clone, Copy)]
struct Reading {
    function: u32,
    holds: bool,
    phase: u32,
    value: f64,
    delay_s: f64,
}

/// The phase (1, 2, 3) and value of the extreme of three readings.
fn extreme(values: [f64; 3], largest: bool) -> (u32, f64) {
    let mut best = (1, values[0]);
    for (index, &value) in values.iter().enumerate().skip(1) {
        if (largest && value > best.1) || (!largest && value < best.1) {
            best = (index as u32 + 1, value);
        }
    }
    best
}

impl BreakerCabinetComponent {
    /// A cabinet with the given settings for a generator of
    /// `rated_current_a`.
    #[must_use]
    pub fn new(card: BreakerCabinetCard, restoration: RestorationCard, rated_current_a: f64) -> Self {
        Self {
            ports: [
                PortSpec::signal_in(sid("command"), nz(COMMAND_WIDTH)),
                PortSpec::signal_in(sid("measure"), nz(MEASURE_WIDTH)),
                PortSpec::signal_out(sid("breaker"), nz(1)),
            ],
            card,
            restoration,
            rated_current_a,
        }
    }

    /// The protection settings.
    #[must_use]
    pub fn card(&self) -> &BreakerCabinetCard {
        &self.card
    }

    fn definite(
        function: u32,
        card: &DefiniteTimeCard,
        holds: bool,
        phase: u32,
        value: f64,
    ) -> Reading {
        Reading {
            function,
            holds,
            phase,
            value,
            delay_s: card.delay_s,
        }
    }
}

impl Component for BreakerCabinetComponent {
    type State = CabinetState;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        // Thresholds, timers and latches.
        Class::NonSmooth
    }

    fn stored_energy_j(&self, _state: &Self::State) -> Result<f64, Self::Error> {
        Ok(0.0)
    }

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
        all_finite(inputs, "cabinet inputs must be finite")?;
        // Inputs: command [close, reset] (0..2), measure (2..15).
        let close_request = inputs[0] >= 0.5;
        let reset = inputs[1] >= 0.5;
        let measure = &inputs[COMMAND_WIDTH..COMMAND_WIDTH + MEASURE_WIDTH];
        let voltage = [measure[0], measure[1], measure[2]];
        let angle = [measure[3], measure[4], measure[5]];
        let current_pu = [
            measure[6] / self.rated_current_a,
            measure[7] / self.rated_current_a,
            measure[8] / self.rated_current_a,
        ];
        let negative_pu = measure[9] / self.rated_current_a;
        let frequency = measure[10];
        let power = measure[11];
        let breaker_closed = measure[12] >= 0.5;
        let card = &self.card;

        let mut next = *old;
        let sequences = sequence_components(voltage, angle);
        next.unbalance = sequences.unbalance();
        next.grid_healthy = grid_is_healthy(&self.restoration, voltage, next.unbalance, frequency);

        // A reset clears the lock-out while the breaker is not asked to close.
        if reset && !close_request {
            next.tripped = 0;
        }

        // The functions watch a closed breaker: they measure the generator on the bus.
        let armed = breaker_closed && old.closed;
        let (low_phase, low) = extreme(voltage, false);
        let (high_phase, high) = extreme(voltage, true);
        let (current_phase, current) = extreme(current_pu, true);
        let readings = [
            Self::definite(
                protection::UNDERVOLTAGE,
                &card.undervoltage,
                low < card.undervoltage.pickup,
                low_phase,
                low,
            ),
            Self::definite(
                protection::OVERVOLTAGE,
                &card.overvoltage,
                high > card.overvoltage.pickup,
                high_phase,
                high,
            ),
            Self::definite(
                protection::VOLTAGE_UNBALANCE,
                &card.voltage_unbalance,
                next.unbalance > card.voltage_unbalance.pickup,
                0,
                next.unbalance,
            ),
            Self::definite(
                protection::OVERCURRENT,
                &card.overcurrent,
                current > card.overcurrent.pickup,
                current_phase,
                current,
            ),
            Self::definite(
                protection::OVERCURRENT_HIGH,
                &card.overcurrent_high,
                current > card.overcurrent_high.pickup,
                current_phase,
                current,
            ),
            Self::definite(
                protection::UNDERFREQUENCY,
                &card.underfrequency,
                frequency < card.underfrequency.pickup,
                0,
                frequency,
            ),
            Self::definite(
                protection::OVERFREQUENCY,
                &card.overfrequency,
                frequency > card.overfrequency.pickup,
                0,
                frequency,
            ),
            Self::definite(
                protection::REVERSE_POWER,
                &card.reverse_power,
                -power > card.reverse_power.pickup,
                0,
                -power,
            ),
        ];

        let mut pickup = 0_u32;
        let mut trip = 0_u32;
        let mut first: Option<Reading> = None;
        for reading in readings {
            let index = reading.function as usize;
            if armed && reading.holds {
                next.timer_s[index] = old.timer_s[index] + dt_s;
                pickup |= 1 << reading.function;
                // The delay counts whole steps; the margin keeps a sum of steps from
                // falling one rounding short of it.
                if next.timer_s[index] >= reading.delay_s * (1.0 - 1.0e-9) {
                    trip |= 1 << reading.function;
                    first.get_or_insert(reading);
                }
            } else {
                next.timer_s[index] = 0.0;
            }
        }

        // The rotor's thermal replica: it integrates above the continuous level and cools
        // below it, whether or not the breaker is closed.
        let thermal = protection::NEGATIVE_SEQUENCE as usize;
        if armed && negative_pu > card.negative_sequence_pickup_pu {
            next.rotor_heat_s = old.rotor_heat_s + negative_pu * negative_pu * dt_s;
            next.timer_s[thermal] = old.timer_s[thermal] + dt_s;
            pickup |= 1 << protection::NEGATIVE_SEQUENCE;
            if next.rotor_heat_s >= card.negative_sequence_limit_s {
                trip |= 1 << protection::NEGATIVE_SEQUENCE;
                first.get_or_insert(Reading {
                    function: protection::NEGATIVE_SEQUENCE,
                    holds: true,
                    phase: 0,
                    value: negative_pu,
                    delay_s: next.timer_s[thermal],
                });
            }
        } else {
            next.rotor_heat_s =
                (old.rotor_heat_s * (1.0 - dt_s / card.negative_sequence_cooling_s)).max(0.0);
            next.timer_s[thermal] = 0.0;
        }
        next.pickup = pickup;

        if let Some(reading) = first {
            next.tripped |= trip;
            next.trips = old.trips + 1;
            next.last_function = reading.function;
            next.last_phase = reading.phase;
            next.last_value = reading.value;
            next.timer_s = [0.0; protection::COUNT];
            next.pickup = 0;
        }
        next.closed = close_request && next.tripped == 0;

        outputs[0] = if next.closed { 1.0 } else { 0.0 };
        *new = next;
        Ok(Proposal::Step(Receipt::lossless(0.0, 1.0e-12)))
    }
}
