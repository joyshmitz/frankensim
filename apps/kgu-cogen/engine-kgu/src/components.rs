//! `fs_ports::component::Component` implementations of the physical parts of
//! the cogeneration unit (Component contract v0).
//!
//! Every part owns its ports and a pure `propose`; its state belongs to the
//! system. The parts are joined only through `fs_system::SystemBuilder`
//! (`plant.rs`). The shaft's inertia is the kernel's own
//! `fs_system::elements::Mass` in the rotational domain; here it is wrapped
//! in [`ShaftComponent`], which adds the one thing that element has not: a
//! shaft that comes to rest.
//!
//! 1. [`GasControlValveComponent`]: compressible orifice with a rate-limited
//!    actuator and a gas meter at supply conditions.
//! 2. [`GasEngineComponent`]: fuel power to indicated torque, friction, wall
//!    heat, exhaust enthalpy and radiation, from the fitted and assumed cards.
//! 3. [`StarterMotorComponent`]: series DC machine on the battery.
//! 4. [`SynchronousGeneratorComponent`]: classical machine on an infinite bus
//!    behind the transformer reactance, with the generator breaker; currents
//!    and torque from `fs_phs::circuit::synchronous::SynchronousMotor`.
//! 5. [`ShaftComponent`]: the kernel's inertia, and static friction that
//!    stops an undriven shaft at exactly zero speed.

use core::f64::consts::PI;
use core::num::NonZeroUsize;
use std::cell::RefCell;

use fs_exec::Cx;
use fs_phs::circuit::synchronous::{SynchronousMotor, SynchronousMotorParameters};
use fs_ports::component::{
    Class, Component, PortRole, PortSpec, PortTangent, Proposal, Receipt, STREAM_ENTHALPY_RATE,
    STREAM_MASS_RATE, STREAM_PRESSURE, STREAM_SPECIFIC_ENTHALPY, StreamBasis,
};
use fs_ports::{PortKind, StableId};
use fs_system::elements::{Mass, Motion};

use crate::cards::{
    CombustionLimitsCard, ExhaustGasCard, FrictionCard, FuelGasCard, GasValveCard, GeneratorCard,
    HeatSplitCard, IndicatedWorkCard, RATED_ELECTRIC_W, StandstillCard, StarterCard,
    orifice_mass_flux_kg_m2_s,
};

/// Typed refusal of the unit's components.
#[derive(Debug, Clone, PartialEq)]
pub enum KguError {
    /// The execution context was cancelled.
    Cancelled,
    /// An input or a state is outside what the component's law covers.
    Invalid(&'static str),
}

pub(crate) fn sid(name: &str) -> StableId {
    StableId::new(name).expect("valid static StableId")
}

pub(crate) fn nz(width: usize) -> NonZeroUsize {
    NonZeroUsize::new(width).expect("nonzero port width")
}

pub(crate) fn check_cx(cx: &Cx<'_>) -> Result<(), KguError> {
    if cx.checkpoint().is_err() {
        Err(KguError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn all_finite(values: &[f64], what: &'static str) -> Result<(), KguError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(KguError::Invalid(what))
    }
}

/// Stream basis of the fuel gas: mass and enthalpy only, with the enthalpy
/// measured from the products of complete combustion at the standard
/// temperature with water as vapour, so that the enthalpy rate of the stream
/// is the fuel's lower heating power plus its sensible heat.
#[must_use]
pub fn fuel_gas_basis() -> StreamBasis {
    StreamBasis::new(
        sid("natural-gas-passport-v1"),
        sid("combustion-products-293.15K-water-vapour"),
        Vec::new(),
    )
    .expect("valid fuel gas stream basis")
}

// ---------------------------------------------------------------------------
// 1. Gas control valve
// ---------------------------------------------------------------------------

/// Gas control valve with its actuator, the shut-off valve in series, and the
/// branch gas meter (which has no volume corrector, SPEC section 2: it counts
/// volume at the supply's pressure and temperature).
///
/// Ports: `inlet` and `outlet` (fuel gas stream, transport side), `command`
/// (signal: valve demand `[0, 1]`, shut-off valve open `0/1`).
pub struct GasControlValveComponent {
    ports: [PortSpec; 3],
    gas: FuelGasCard,
    card: GasValveCard,
}

/// Accepted state of the gas valve and the meter behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GasValveState {
    /// Lift of the control valve [0, 1].
    pub lift: f64,
    /// Mass flow of the last step [kg/s].
    pub mass_flow_kg_s: f64,
    /// Supply absolute pressure of the last step [Pa].
    pub supply_pressure_pa: f64,
    /// Supply temperature of the last step [K].
    pub supply_temperature_k: f64,
    /// Meter index: volume at supply conditions since the model started [m3].
    pub meter_volume_m3: f64,
}

impl GasValveState {
    /// A closed valve and a meter at zero.
    #[must_use]
    pub fn closed(gas: &FuelGasCard) -> Self {
        Self {
            lift: 0.0,
            mass_flow_kg_s: 0.0,
            supply_pressure_pa: gas.standard_pressure_pa,
            supply_temperature_k: gas.standard_temperature_k,
            meter_volume_m3: 0.0,
        }
    }
}

impl GasControlValveComponent {
    /// A valve for `gas` with the actuator and area of `card`.
    #[must_use]
    pub fn new(gas: FuelGasCard, card: GasValveCard) -> Self {
        Self {
            ports: [
                PortSpec::identified_stream(sid("inlet"), PortRole::EffortIn, fuel_gas_basis())
                    .expect("gas valve inlet"),
                PortSpec::identified_stream(sid("outlet"), PortRole::EffortIn, fuel_gas_basis())
                    .expect("gas valve outlet"),
                PortSpec::signal_in(sid("command"), nz(2)),
            ],
            gas,
            card,
        }
    }

    /// Volume flow at supply conditions [m3/s] for a state (the meter's rate).
    #[must_use]
    pub fn meter_flow_m3_s(&self, state: &GasValveState) -> f64 {
        state.mass_flow_kg_s * self.gas.specific_gas_constant() * state.supply_temperature_k
            / state.supply_pressure_pa
    }

    /// Volume flow at standard conditions [m3/s] for a state.
    #[must_use]
    pub fn standard_flow_m3_s(&self, state: &GasValveState) -> f64 {
        state.mass_flow_kg_s / self.gas.standard_density_kg_m3()
    }
}

impl Component for GasControlValveComponent {
    type State = GasValveState;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        Class::Smooth
    }

    fn stored_energy_j(&self, _state: &Self::State) -> Result<f64, Self::Error> {
        Ok(0.0)
    }

    fn stored_mass_kg(&self, _state: &Self::State) -> Result<Option<f64>, Self::Error> {
        Ok(Some(0.0))
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
        all_finite(inputs, "gas valve inputs must be finite")?;
        // Inputs: inlet [p, h] (0..2), outlet [p, h] (2..4), command [demand, shut-off open] (4..6).
        let p_up = inputs[STREAM_PRESSURE];
        let h_up = inputs[STREAM_SPECIFIC_ENTHALPY];
        let p_down = inputs[2 + STREAM_PRESSURE];
        let demand = inputs[4].clamp(0.0, 1.0);
        let shut_off_open = inputs[5] >= 0.5;
        if !(p_up > 0.0) {
            return Err(KguError::Invalid("gas supply pressure must be positive"));
        }
        let t_up = self.gas.temperature_k(h_up);
        if !(t_up > 0.0) {
            return Err(KguError::Invalid("gas supply temperature must be positive"));
        }

        // Rate-limited actuator; with the shut-off valve closed the control valve is driven shut.
        let target = if shut_off_open { demand } else { 0.0 };
        let travel = target - old.lift;
        let limit = if travel >= 0.0 {
            self.card.opening_rate_per_s * dt_s
        } else {
            self.card.closing_rate_per_s * dt_s
        };
        let lift = (old.lift + travel.clamp(-limit, limit)).clamp(0.0, 1.0);

        // Flow at the mean lift of the step; none through a closed shut-off valve.
        let area = if shut_off_open {
            self.card.full_area_m2 * 0.5 * (old.lift + lift)
        } else {
            0.0
        };
        let mass_flow = area * orifice_mass_flux_kg_m2_s(&self.gas, p_up, t_up, p_down);
        let enthalpy_rate = mass_flow * h_up;

        outputs[STREAM_MASS_RATE] = mass_flow;
        outputs[STREAM_ENTHALPY_RATE] = enthalpy_rate;
        outputs[2 + STREAM_MASS_RATE] = -mass_flow;
        outputs[2 + STREAM_ENTHALPY_RATE] = -enthalpy_rate;

        *new = GasValveState {
            lift,
            mass_flow_kg_s: mass_flow,
            supply_pressure_pa: p_up,
            supply_temperature_k: t_up,
            meter_volume_m3: old.meter_volume_m3
                + dt_s * mass_flow * self.gas.specific_gas_constant() * t_up / p_up,
        };
        Ok(Proposal::Step(Receipt::lossless(
            0.0,
            1.0e-12 * dt_s * enthalpy_rate.abs() + 1.0e-9,
        )))
    }
}

// ---------------------------------------------------------------------------
// 2. Gas engine (brake side)
// ---------------------------------------------------------------------------

/// The gas engine from the fuel it receives to the crankshaft and its heat
/// outlets. It stores nothing: the fuel that arrives in a step is burned in
/// that step.
///
/// Ports: `gas_in` (fuel gas stream, storage side: it sets the mixer pressure
/// the valve discharges into), `shaft` (torque and speed), `jacket_heat`,
/// `exhaust_heat`, `radiation_heat` (heat leaving at the temperature each
/// port receives), `ignition` (signal `0/1`), `conditions` (signal:
/// machine-condition factor, atmospheric pressure [Pa]), `exhaust_flow`
/// (signal out: heat-capacity rate of the exhaust gas [W/K]) and
/// `unburned_heat` (the enthalpy of gas that passed without ignition).
///
/// The laws are the cards': indicated power from the effective fuel power
/// ([`IndicatedWorkCard`], fitted to the passport), friction torque against
/// speed ([`FrictionCard`]), the split of the remaining heat
/// ([`HeatSplitCard`]). The exhaust enthalpy is the remainder of the first
/// law, so the balance closes by construction and the split is what is
/// assumed. Fuel that arrives without ignition leaves unburned, past the
/// exhaust exchanger.
pub struct GasEngineComponent {
    ports: [PortSpec; 9],
    gas: FuelGasCard,
    work: IndicatedWorkCard,
    friction: FrictionCard,
    heat: HeatSplitCard,
    limits: CombustionLimitsCard,
    exhaust: ExhaustGasCard,
}

/// What the engine did in the last accepted step. It has no dynamic state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GasEngineState {
    /// Fuel power on the lower heating value [W].
    pub fuel_power_w: f64,
    /// Electrical power the passport machine would deliver from the
    /// effective fuel power [W] (the card's argument; negative below the
    /// no-load fuel line).
    pub passport_electric_w: f64,
    /// Indicated torque on the crankshaft [N m].
    pub indicated_torque_nm: f64,
    /// Friction torque opposing rotation [N m].
    pub friction_torque_nm: f64,
    /// Heat into the jacket coolant: wall heat plus friction heat [W].
    pub jacket_heat_w: f64,
    /// Friction power, part of the jacket heat [W].
    pub friction_power_w: f64,
    /// Sensible exhaust enthalpy rate above the fuel stream's datum [W].
    pub exhaust_enthalpy_w: f64,
    /// Enthalpy rate of gas leaving unburned (no ignition) [W].
    pub unburned_w: f64,
    /// Heat radiated and convected to the room [W].
    pub radiation_w: f64,
    /// Heat-capacity rate of the exhaust gas [W/K]; zero when nothing burns.
    pub exhaust_capacity_rate_w_k: f64,
    /// Whether fuel was burned in the step.
    pub firing: bool,
}

impl GasEngineComponent {
    /// An engine burning `gas` with the given cards.
    #[must_use]
    pub fn new(
        gas: FuelGasCard,
        work: IndicatedWorkCard,
        heat: HeatSplitCard,
        limits: CombustionLimitsCard,
        exhaust: ExhaustGasCard,
    ) -> Self {
        let friction = work.friction();
        Self {
            ports: [
                PortSpec::identified_stream(sid("gas_in"), PortRole::FlowIn, fuel_gas_basis())
                    .expect("engine gas port"),
                PortSpec::power(
                    sid("shaft"),
                    PortKind::RotationalTorqueAngularVelocity,
                    PortRole::FlowIn,
                    nz(1),
                )
                .expect("engine shaft port"),
                PortSpec::heat(sid("jacket_heat"), PortRole::EffortIn).expect("engine jacket port"),
                PortSpec::heat(sid("exhaust_heat"), PortRole::EffortIn)
                    .expect("engine exhaust port"),
                PortSpec::heat(sid("radiation_heat"), PortRole::EffortIn)
                    .expect("engine radiation port"),
                PortSpec::signal_in(sid("ignition"), nz(1)),
                PortSpec::signal_in(sid("conditions"), nz(2)),
                PortSpec::signal_out(sid("exhaust_flow"), nz(1)),
                PortSpec::heat(sid("unburned_heat"), PortRole::EffortIn)
                    .expect("engine unburned port"),
            ],
            gas,
            work,
            friction,
            heat,
            limits,
            exhaust,
        }
    }

    /// The fitted indicated-work card.
    #[must_use]
    pub fn work_card(&self) -> &IndicatedWorkCard {
        &self.work
    }

    /// The engine's first-law split for a fuel mass flow [kg/s] arriving with
    /// `enthalpy_rate_w`, at shaft speed `omega` [rad/s]: the law `propose`
    /// applies, and the one the heat-recovery fit is made through.
    #[must_use]
    pub fn balance(
        &self,
        mass_flow_kg_s: f64,
        enthalpy_rate_w: f64,
        omega: f64,
        ignition: bool,
        condition: f64,
    ) -> GasEngineState {
        let fuel_power = mass_flow_kg_s.max(0.0) * self.gas.lower_heating_value_j_kg();
        // The law branches on the ignition signal alone, which is held over a step. It does
        // not branch on the speed or on whether fuel flows: the block solve iterates on those,
        // and a branch there (even a continuous one) leaves it a slope from the wrong side.
        // With ignition on and no fuel every term below is zero by itself.
        let firing = ignition;

        // Indicated work: the card's power, turned into torque at the shaft's speed and
        // limited by the charge the cylinders can take.
        let (passport_electric, indicated_torque) = if firing {
            let effective = condition * fuel_power;
            let power = self.work.indicated_power_w(effective);
            let torque = (power / omega.max(self.limits.torque_speed_floor_rad_s))
                .min(self.limits.largest_indicated_torque_nm);
            (self.work.passport_electric_w(effective), torque)
        } else {
            (0.0, 0.0)
        };
        let indicated_power = indicated_torque * omega;
        let friction_torque = self.friction.torque_nm(omega);
        let friction_power = friction_torque * omega;

        // Heat: burned fuel that is not work is split between walls, room and exhaust;
        // unburned fuel leaves with the exhaust. The exhaust is the first law's remainder.
        let (wall_heat, radiation) = if firing {
            (
                self.heat.wall_share * (fuel_power - indicated_power).max(0.0),
                self.heat.radiation_share * fuel_power,
            )
        } else {
            (0.0, 0.0)
        };
        GasEngineState {
            fuel_power_w: fuel_power,
            passport_electric_w: passport_electric,
            indicated_torque_nm: indicated_torque,
            friction_torque_nm: friction_torque,
            jacket_heat_w: wall_heat + friction_power,
            friction_power_w: friction_power,
            // Burned gas leaves as sensible exhaust enthalpy, the first law's remainder.
            // Gas that passes an engine without ignition leaves unburned, carrying its
            // heating value past the exhaust exchanger: it is not heat the exchanger can take.
            exhaust_enthalpy_w: if firing {
                enthalpy_rate_w - indicated_power - wall_heat - radiation
            } else {
                0.0
            },
            unburned_w: if firing { 0.0 } else { enthalpy_rate_w },
            radiation_w: radiation,
            exhaust_capacity_rate_w_k: if firing {
                self.exhaust.capacity_rate_w_k(mass_flow_kg_s)
            } else {
                0.0
            },
            firing: firing && fuel_power > 0.0,
        }
    }

    /// The friction card.
    #[must_use]
    pub fn friction_card(&self) -> &FrictionCard {
        &self.friction
    }
}

impl Component for GasEngineComponent {
    type State = GasEngineState;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        Class::Smooth
    }

    fn stored_energy_j(&self, _state: &Self::State) -> Result<f64, Self::Error> {
        Ok(0.0)
    }

    fn propose(
        &self,
        _old: &Self::State,
        dt_s: f64,
        inputs: &[f64],
        new: &mut Self::State,
        outputs: &mut [f64],
        cx: &Cx<'_>,
    ) -> Result<Proposal, Self::Error> {
        check_cx(cx)?;
        all_finite(inputs, "engine inputs must be finite")?;
        // Inputs: gas_in [mass rate, enthalpy rate] (0..2), shaft -omega (2), jacket T (3),
        // exhaust T (4), radiation T (5), ignition (6), conditions [k, p_atm] (7..9).
        let mass_flow = inputs[STREAM_MASS_RATE];
        let enthalpy_rate = inputs[STREAM_ENTHALPY_RATE];
        let omega = -inputs[2];
        let (t_jacket, t_exhaust, t_radiation) = (inputs[3], inputs[4], inputs[5]);
        let ignition = inputs[6] >= 0.5;
        let condition = inputs[7].clamp(0.0, 1.0);
        let p_atm = inputs[8];
        let t_unburned = inputs[9];
        if !(t_jacket > 0.0 && t_exhaust > 0.0 && t_radiation > 0.0 && t_unburned > 0.0) {
            return Err(KguError::Invalid("engine heat ports need positive temperatures"));
        }
        if !(p_atm > 0.0) {
            return Err(KguError::Invalid("atmospheric pressure must be positive"));
        }

        let state = self.balance(mass_flow, enthalpy_rate, omega, ignition, condition);
        let (jacket_heat, exhaust, radiation) =
            (state.jacket_heat_w, state.exhaust_enthalpy_w, state.radiation_w);
        let friction_power = state.friction_power_w;
        let indicated_power = state.indicated_torque_nm * omega;

        // The engine "holds" the mixer: atmospheric pressure, and gas at the datum's enthalpy.
        outputs[STREAM_PRESSURE] = p_atm;
        outputs[STREAM_SPECIFIC_ENTHALPY] = self.gas.lower_heating_value_j_kg();
        outputs[2] = state.indicated_torque_nm - state.friction_torque_nm;
        outputs[3] = -jacket_heat;
        outputs[4] = -exhaust;
        outputs[5] = -radiation;
        outputs[6] = state.exhaust_capacity_rate_w_k;
        outputs[7] = -state.unburned_w;
        *new = state;

        // Entropy leaving with the heats. The entropy of the fuel stream and of combustion
        // is not resolved (contract v0 carries no stream entropy), so this is the heat
        // outflow's entropy, never negative.
        let entropy = (jacket_heat / t_jacket
            + exhaust / t_exhaust
            + radiation / t_radiation
            + state.unburned_w / t_unburned)
            .max(0.0);
        let scale = dt_s
            * (enthalpy_rate.abs()
                + indicated_power.abs()
                + friction_power.abs()
                + jacket_heat.abs()
                + exhaust.abs()
                + radiation.abs());
        Ok(Proposal::Step(Receipt {
            stored_change_j: 0.0,
            dissipated_j: dt_s * friction_power.max(0.0),
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: dt_s * entropy,
            energy_tolerance_j: 1.0e-12 * scale + 1.0e-9,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}

// ---------------------------------------------------------------------------
// 3. Starter motor
// ---------------------------------------------------------------------------

/// Electric starter: an ideal series DC machine referred to the crankshaft,
/// with an overrunning clutch (no torque above its free speed).
///
/// Ports: `shaft`, `battery` (voltage in, current out), `heat` (its `I^2 R`
/// loss), `engage` (signal `0/1`).
pub struct StarterMotorComponent {
    ports: [PortSpec; 4],
    card: StarterCard,
}

/// What the starter did in the last accepted step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StarterState {
    /// Torque on the crankshaft [N m].
    pub torque_nm: f64,
    /// Battery current [A].
    pub current_a: f64,
    /// Whether it was engaged.
    pub engaged: bool,
}

impl StarterMotorComponent {
    /// A starter with the given card.
    #[must_use]
    pub fn new(card: StarterCard) -> Self {
        Self {
            ports: [
                PortSpec::power(
                    sid("shaft"),
                    PortKind::RotationalTorqueAngularVelocity,
                    PortRole::FlowIn,
                    nz(1),
                )
                .expect("starter shaft port"),
                PortSpec::power(
                    sid("battery"),
                    PortKind::ElectricalVoltageCurrent,
                    PortRole::EffortIn,
                    nz(1),
                )
                .expect("starter battery port"),
                PortSpec::heat(sid("heat"), PortRole::EffortIn).expect("starter heat port"),
                PortSpec::signal_in(sid("engage"), nz(1)),
            ],
            card,
        }
    }
}

impl Component for StarterMotorComponent {
    type State = StarterState;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        Class::Smooth
    }

    fn stored_energy_j(&self, _state: &Self::State) -> Result<f64, Self::Error> {
        Ok(0.0)
    }

    fn propose(
        &self,
        _old: &Self::State,
        dt_s: f64,
        inputs: &[f64],
        new: &mut Self::State,
        outputs: &mut [f64],
        cx: &Cx<'_>,
    ) -> Result<Proposal, Self::Error> {
        check_cx(cx)?;
        all_finite(inputs, "starter inputs must be finite")?;
        // Inputs: shaft -omega (0), battery voltage (1), heat port temperature (2), engage (3).
        let omega = -inputs[0];
        let voltage = inputs[1];
        let temperature = inputs[2];
        let engaged = inputs[3] >= 0.5;
        if !(temperature > 0.0) {
            return Err(KguError::Invalid("starter heat port needs a positive temperature"));
        }
        let constant = self.card.constant();
        let resistance = self.card.resistance_ohm();
        // V = I R + k omega; the clutch lets the engine overrun, so the current is never
        // negative. The corner at the free speed is rounded over 2 A (of some 2 700 A at
        // stall) so that the block solve has a slope on both sides of it.
        const CORNER_A: f64 = 2.0;
        let current = if engaged {
            let unclutched = (voltage - constant * omega) / resistance;
            0.5 * (unclutched + unclutched.hypot(CORNER_A))
        } else {
            0.0
        };
        let torque = constant * current;
        let loss = current * current * resistance;
        // First law to the bit of its terms: V I = torque omega + I^2 R, with V I formed from them.
        let electric_power = torque * omega + loss;
        outputs[0] = torque;
        outputs[1] = if voltage > 0.0 { electric_power / voltage } else { 0.0 };
        outputs[2] = -loss;
        *new = StarterState {
            torque_nm: torque,
            current_a: current,
            engaged,
        };
        let scale = dt_s * (electric_power.abs() + (torque * omega).abs() + loss);
        Ok(Proposal::Step(Receipt {
            stored_change_j: 0.0,
            dissipated_j: dt_s * loss,
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: dt_s * loss / temperature,
            energy_tolerance_j: 1.0e-12 * scale + 1.0e-9,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}

// ---------------------------------------------------------------------------
// 4. Synchronous generator, breaker and transformer reactance
// ---------------------------------------------------------------------------

/// Rotor inertia handed to the fs-phs machine [kg m2]. The shaft's inertia is
/// the kernel's `Mass`; the fs-phs machine is asked only for its operating
/// point, where this number converts the momentum it is given back to the
/// speed, and it drops out.
const PHS_ROTOR_INERTIA_KG_M2: f64 = 1.0;

/// Synchronous generator on an infinite bus, in the classical model: the EMF
/// `E = excitation * rated voltage * speed ratio` behind the transient
/// reactance plus the transformer's reactance, stator transients neglected.
///
/// The bus is the 10 kV bus referred to the generator's 0.4 kV side through
/// an ideal ratio. Each step integrates the angle between the EMF and the bus
/// (`d delta/dt = p omega - omega_grid`, also with the breaker open: it is
/// the angle the synchroniser watches), solves the stator voltage equations
/// at the mid-step angle
///
/// `v_d = R i_d - X i_q`, `v_q = R i_q + X i_d + omega_e psi`,
/// `(v_d, v_q) = V (sin delta, cos delta)`,
///
/// and takes torque and currents from
/// `fs_phs::circuit::synchronous::SynchronousMotor::operating_point` at the
/// resulting flux linkages. Electrical quantities are power invariant: the
/// d-q voltage magnitude is the line-to-line RMS voltage and the line current
/// is the d-q current over `sqrt 3`. The damper winding is one lumped torque
/// coefficient on the slip. A pole slip (`|delta| > pi`) opens the breaker
/// and latches until the breaker command is withdrawn.
///
/// The bus has three phase voltages. Their positive sequence is the bus
/// voltage of the equations above. Their negative sequence meets no EMF in
/// the machine and drives a current through the negative-sequence impedance
/// of the card, `I2 = V2 / (R2 + j X2)`: computed here, in the app, from
/// assumed values, because the `fs-phs` machine has no rotor circuit for the
/// double-frequency currents that this is about. It adds to the phase
/// currents, heats stator and rotor, and brakes the shaft by half the rotor's
/// share. The 100 Hz ripple of torque that goes with it is not modelled.
///
/// Ports: `shaft`; `grid` (electrical: positive-sequence bus voltage in,
/// current out; the pair is an accounting pair whose product is the active
/// power, not a phasor); `cooler_heat` (all of the machine's losses);
/// `breaker` (signal `0/1`: the position of the cabinet's contacts);
/// `field` (signal: excitation [pu], grid frequency [Hz]); `unbalance`
/// (signal: negative-sequence phase voltage [V], real and imaginary against
/// the positive sequence, and the positive sequence's angle [rad]).
pub struct SynchronousGeneratorComponent {
    ports: [PortSpec; 6],
    card: GeneratorCard,
    /// fs-phs machine for the last field linkage used; rebuilt when it changes.
    machine: RefCell<Option<(u64, SynchronousMotor)>>,
}

/// Accepted state of the generator and its breaker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneratorState {
    /// Angle of the EMF ahead of the bus voltage [rad, electrical]. With the
    /// breaker open it is wrapped to `(-pi, pi]`.
    pub load_angle_rad: f64,
    /// Whether the machine carries the bus: the breaker's contacts are
    /// closed (the cabinet's state, told by signal) and the bus has a
    /// voltage. On a bus without voltage the contacts may be closed and
    /// this false. The position of the contacts is the cabinet's.
    pub breaker_closed: bool,
    /// Latched by a pole slip; cleared when the contacts open.
    pub out_of_step: bool,
    /// Active power delivered to the bus [W].
    pub electric_power_w: f64,
    /// Reactive power delivered to the bus [var].
    pub reactive_power_var: f64,
    /// Stator line current [A, RMS].
    pub line_current_a: f64,
    /// Generator terminal voltage, behind the transformer [V, line to line].
    pub terminal_voltage_v: f64,
    /// Copper and stray loss [W].
    pub copper_loss_w: f64,
    /// Core loss [W].
    pub core_loss_w: f64,
    /// Damper-winding loss [W].
    pub damper_loss_w: f64,
    /// Total torque on the shaft [N m] (negative while generating).
    pub torque_nm: f64,
    /// Currents of phases a, b, c [A, RMS]: positive and negative sequence
    /// together.
    pub phase_current_a: [f64; 3],
    /// Negative-sequence current [A, RMS per phase].
    pub negative_sequence_current_a: f64,
    /// Heat the negative-sequence current leaves in the stator [W].
    pub negative_sequence_stator_loss_w: f64,
    /// Heat the negative-sequence current leaves in the rotor [W].
    pub negative_sequence_rotor_loss_w: f64,
}

impl GeneratorState {
    /// A de-energised machine with its breaker open.
    #[must_use]
    pub fn open() -> Self {
        Self {
            load_angle_rad: 0.0,
            breaker_closed: false,
            out_of_step: false,
            electric_power_w: 0.0,
            reactive_power_var: 0.0,
            line_current_a: 0.0,
            terminal_voltage_v: 0.0,
            copper_loss_w: 0.0,
            core_loss_w: 0.0,
            damper_loss_w: 0.0,
            torque_nm: 0.0,
            phase_current_a: [0.0; 3],
            negative_sequence_current_a: 0.0,
            negative_sequence_stator_loss_w: 0.0,
            negative_sequence_rotor_loss_w: 0.0,
        }
    }

    /// All heat the generator releases [W].
    #[must_use]
    pub fn total_loss_w(&self) -> f64 {
        self.copper_loss_w
            + self.core_loss_w
            + self.damper_loss_w
            + self.negative_sequence_stator_loss_w
            + self.negative_sequence_rotor_loss_w
    }

    /// Power factor at the bus, or NaN when no power flows.
    #[must_use]
    pub fn power_factor(&self) -> f64 {
        let apparent = self.electric_power_w.hypot(self.reactive_power_var);
        if self.breaker_closed && apparent > 1.0e-3 * RATED_ELECTRIC_W {
            self.electric_power_w / apparent
        } else {
            f64::NAN
        }
    }
}

/// Stator solution at one angle.
#[derive(Debug, Clone, Copy)]
struct StatorSolution {
    torque_nm: f64,
    current_dq_a: [f64; 2],
    voltage_dq_v: [f64; 2],
    copper_loss_w: f64,
}

/// Wrap an angle to `(-pi, pi]`.
#[must_use]
pub fn wrap_angle(angle_rad: f64) -> f64 {
    let wrapped = (angle_rad + PI).rem_euclid(2.0 * PI) - PI;
    if wrapped <= -PI { wrapped + 2.0 * PI } else { wrapped }
}

impl SynchronousGeneratorComponent {
    /// A generator with the given card.
    #[must_use]
    pub fn new(card: GeneratorCard) -> Self {
        Self {
            ports: [
                PortSpec::power(
                    sid("shaft"),
                    PortKind::RotationalTorqueAngularVelocity,
                    PortRole::FlowIn,
                    nz(1),
                )
                .expect("generator shaft port"),
                PortSpec::power(
                    sid("grid"),
                    PortKind::ElectricalVoltageCurrent,
                    PortRole::EffortIn,
                    nz(1),
                )
                .expect("generator grid port"),
                PortSpec::heat(sid("cooler_heat"), PortRole::EffortIn)
                    .expect("generator cooler port"),
                PortSpec::signal_in(sid("breaker"), nz(1)),
                PortSpec::signal_in(sid("field"), nz(2)),
                PortSpec::signal_in(sid("unbalance"), nz(3)),
            ],
            card,
            machine: RefCell::new(None),
        }
    }

    /// The machine's card.
    #[must_use]
    pub fn card(&self) -> &GeneratorCard {
        &self.card
    }

    /// Inductance from the EMF to the bus [H].
    fn inductance_h(&self) -> f64 {
        (self.card.transient_reactance_pu + self.card.transformer_reactance_pu)
            * self.card.base_impedance_ohm()
            / self.card.nominal_electrical_omega()
    }

    fn resistance_ohm(&self) -> f64 {
        self.card.loss_resistance_pu * self.card.base_impedance_ohm()
    }

    /// Field linkage [Wb, power invariant] giving an open-circuit EMF of
    /// `excitation_pu` times the rated voltage at nominal frequency.
    fn field_linkage_wb(&self, excitation_pu: f64) -> f64 {
        excitation_pu * self.card.rated_voltage_v / self.card.nominal_electrical_omega()
    }

    /// Core loss [W] at `excitation_pu` and electrical speed `omega_e`.
    fn core_loss_w(&self, excitation_pu: f64, omega_e: f64) -> f64 {
        let flux_and_speed = excitation_pu * omega_e / self.card.nominal_electrical_omega();
        self.card.rated_core_loss_w * flux_and_speed * flux_and_speed
    }

    /// Damper torque coefficient [N m s/rad, mechanical].
    fn damping_nm_s_per_rad(&self) -> f64 {
        let omega_m = self.card.nominal_electrical_omega() / f64::from(self.card.pole_pairs);
        self.card.damping_pu * self.card.rated_va / (omega_m * omega_m)
    }

    fn with_machine<R>(
        &self,
        field_linkage_wb: f64,
        ask: impl FnOnce(&SynchronousMotor) -> R,
    ) -> Result<R, KguError> {
        let mut slot = self.machine.borrow_mut();
        let key = field_linkage_wb.to_bits();
        if !matches!(&*slot, Some((known, _)) if *known == key) {
            let inductance = self.inductance_h();
            let machine = SynchronousMotor::new(SynchronousMotorParameters {
                d_inductance_h: inductance,
                q_inductance_h: inductance,
                winding_resistance_ohm: self.resistance_ohm(),
                pm_flux_linkage_wb: field_linkage_wb,
                pole_pairs: self.card.pole_pairs,
                rotor_inertia_kg_m2: PHS_ROTOR_INERTIA_KG_M2,
                stator_inertia_kg_m2: 1.0e9,
                bearing_drag_nm_s_per_rad: 0.0,
            })
            .map_err(|_| KguError::Invalid("fs-phs refused the generator parameters"))?;
            *slot = Some((key, machine));
        }
        Ok(ask(&slot.as_ref().expect("machine just built").1))
    }

    /// Quasi-steady stator solution at load angle `delta_rad`, shaft speed
    /// `omega_shaft` [rad/s, mechanical], bus voltage `v_bus` and excitation.
    fn solve_stator(
        &self,
        delta_rad: f64,
        omega_shaft: f64,
        v_bus: f64,
        excitation_pu: f64,
    ) -> Result<StatorSolution, KguError> {
        let omega_e = f64::from(self.card.pole_pairs) * omega_shaft;
        let inductance = self.inductance_h();
        let resistance = self.resistance_ohm();
        let reactance = omega_e * inductance;
        let psi = self.field_linkage_wb(excitation_pu);
        let emf = omega_e * psi;
        let (sin_delta, cos_delta) = delta_rad.sin_cos();
        let (v_d, v_q) = (v_bus * sin_delta, v_bus * cos_delta);
        let determinant = resistance * resistance + reactance * reactance;
        let i_d = (resistance * v_d + reactance * (v_q - emf)) / determinant;
        let i_q = (resistance * (v_q - emf) - reactance * v_d) / determinant;
        // fs-phs state: [lambda_d, lambda_q, theta, p_rotor, p_stator], rotor aligned.
        let state = [
            psi + inductance * i_d,
            inductance * i_q,
            0.0,
            PHS_ROTOR_INERTIA_KG_M2 * omega_shaft,
            0.0,
        ];
        let point = self
            .with_machine(psi, |machine| machine.operating_point(&state))?
            .map_err(|_| KguError::Invalid("fs-phs refused the generator operating point"))?;
        let [id, iq] = point.current_dq_a;
        Ok(StatorSolution {
            torque_nm: point.electromagnetic_torque_nm,
            current_dq_a: point.current_dq_a,
            voltage_dq_v: [v_d, v_q],
            copper_loss_w: resistance * (id * id + iq * iq),
        })
    }

    /// Losses [W] of the generator delivering `electric_w` to the nominal bus
    /// at nominal frequency and the reference excitation: copper plus core.
    /// This is the loss the indicated-work card is fitted through.
    ///
    /// # Errors
    /// An output beyond the machine's pull-out power at that excitation.
    pub fn reference_loss_w(&self, electric_w: f64) -> Result<f64, KguError> {
        let excitation = GeneratorCard::REFERENCE_EXCITATION_PU;
        let omega_shaft = self.card.nominal_electrical_omega() / f64::from(self.card.pole_pairs);
        let v_bus = self.card.rated_voltage_v;
        let core = self.core_loss_w(excitation, self.card.nominal_electrical_omega());
        let delivered = |delta: f64| -> Result<(f64, f64), KguError> {
            let solution = self.solve_stator(delta, omega_shaft, v_bus, excitation)?;
            let [id, iq] = solution.current_dq_a;
            let [vd, vq] = solution.voltage_dq_v;
            Ok((-(vd * id + vq * iq), solution.copper_loss_w))
        };
        let (mut low, mut high) = (0.0_f64, 0.5 * PI);
        if delivered(high)?.0 < electric_w {
            return Err(KguError::Invalid("output beyond pull-out at the reference excitation"));
        }
        for _ in 0..80 {
            let middle = 0.5 * (low + high);
            if delivered(middle)?.0 < electric_w {
                low = middle;
            } else {
                high = middle;
            }
        }
        Ok(delivered(0.5 * (low + high))?.1 + core)
    }

    /// Open-circuit EMF [pu of rated voltage] at `excitation_pu` and shaft
    /// speed `omega_shaft`.
    #[must_use]
    pub fn open_circuit_emf_pu(&self, excitation_pu: f64, omega_shaft: f64) -> f64 {
        let (low, high) = self.card.excitation_range_pu;
        excitation_pu.clamp(low, high) * f64::from(self.card.pole_pairs) * omega_shaft
            / self.card.nominal_electrical_omega()
    }
}

impl Component for SynchronousGeneratorComponent {
    type State = GeneratorState;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        Class::Smooth
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
        all_finite(inputs, "generator inputs must be finite")?;
        // Inputs: shaft -omega (0), positive-sequence bus voltage, line to line (1), cooler
        // temperature (2), breaker command (3), field [excitation, grid frequency] (4..6),
        // unbalance [negative-sequence phase voltage, real and imaginary against the
        // positive-sequence voltage; angle of the positive-sequence voltage] (6..9).
        let (v2_re, v2_im, v1_angle) = (inputs[6], inputs[7], inputs[8]);
        let omega_in = inputs[0];
        let omega_shaft = -omega_in;
        let v_bus = inputs[1].max(0.0);
        let t_cooler = inputs[2];
        let close_command = inputs[3] >= 0.5;
        let (exc_low, exc_high) = self.card.excitation_range_pu;
        let excitation = inputs[4].clamp(exc_low, exc_high);
        let grid_omega = 2.0 * PI * inputs[5];
        if !(t_cooler > 0.0) {
            return Err(KguError::Invalid("generator cooler needs a positive temperature"));
        }

        let pole_pairs = f64::from(self.card.pole_pairs);
        let omega_e = pole_pairs * omega_shaft;
        let slip_e = omega_e - grid_omega;
        let delta_new = old.load_angle_rad + dt_s * slip_e;
        let core_loss = self.core_loss_w(excitation, omega_e);
        // The core loss goes with the square of flux times speed, so its torque is linear in
        // the speed and needs no special case at standstill.
        let core_loss_per_speed_squared = self.core_loss_w(excitation, pole_pairs);
        let core_torque = -core_loss_per_speed_squared * omega_shaft;
        let core_heat = core_loss;

        let latched = close_command && old.out_of_step;
        let connected =
            close_command && !latched && omega_shaft > 0.0 && v_bus > 0.0 && grid_omega > 0.0;
        let state = if connected {
            // The load angle is against the positive-sequence voltage.
            let delta_mid = 0.5 * (old.load_angle_rad + delta_new) - v1_angle;
            let solution = self.solve_stator(delta_mid, omega_shaft, v_bus, excitation)?;
            let slip_m = slip_e / pole_pairs;
            let damping = self.damping_nm_s_per_rad();
            let damper_torque = -damping * slip_m;
            let damper_loss = damping * slip_m * slip_m;

            // Negative sequence. The machine has no EMF of that sequence, so the bus drives
            // the current through the negative-sequence impedance: I2 = V2 / (R2 + j X2).
            // The stator's resistance turns its share into heat; the rotor's circuits, which
            // see it at twice the line frequency, turn `R_rotor I2^2` into heat, half of it
            // drawn through the shaft as a braking torque at synchronous speed.
            let base = self.card.base_impedance_ohm();
            let r2 = self.card.negative_sequence_resistance_pu * base;
            let x2 = (self.card.negative_sequence_reactance_pu
                + self.card.transformer_reactance_pu)
                * base
                * grid_omega
                / self.card.nominal_electrical_omega();
            let z2_squared = r2 * r2 + x2 * x2;
            let i2 = [
                (v2_re * r2 + v2_im * x2) / z2_squared,
                (v2_im * r2 - v2_re * x2) / z2_squared,
            ];
            let i2_squared = i2[0] * i2[0] + i2[1] * i2[1];
            let stator_loss_2 = 3.0 * self.card.loss_resistance_pu * base * i2_squared;
            let rotor_loss_2 =
                3.0 * self.card.rotor_negative_sequence_resistance_pu() * base * i2_squared;
            let brake_torque = -0.5 * rotor_loss_2 * pole_pairs / grid_omega;

            let torque = solution.torque_nm + damper_torque + core_torque + brake_torque;
            let heat =
                solution.copper_loss_w + core_heat + damper_loss + stator_loss_2 + rotor_loss_2;
            let shaft_power_in = omega_in * torque;
            let export = shaft_power_in - heat;
            let [id, iq] = solution.current_dq_a;
            let [vd, vq] = solution.voltage_dq_v;
            // Terminal voltage: the bus voltage less the drop across the transformer reactance.
            let x_transformer = omega_e
                * self.card.transformer_reactance_pu
                * self.card.base_impedance_ohm()
                / self.card.nominal_electrical_omega();
            let terminal = (vd + x_transformer * iq).hypot(vq - x_transformer * id);
            let slipped = (delta_new - v1_angle).abs() > PI;
            // Phase currents: the positive-sequence current as a phasor against the
            // positive-sequence voltage, (i_d + j i_q) e^{-j (pi/2 - delta)} / sqrt 3, with the
            // negative-sequence phasor added in each phase's own rotation.
            let (sin_delta, cos_delta) = delta_mid.sin_cos();
            let root_3 = 3.0_f64.sqrt();
            let i1 = [
                (id * sin_delta + iq * cos_delta) / root_3,
                (iq * sin_delta - id * cos_delta) / root_3,
            ];
            let rotate = |phasor: [f64; 2], thirds: i32| -> [f64; 2] {
                let (sin, cos) = (f64::from(thirds) * 2.0 * PI / 3.0).sin_cos();
                [phasor[0] * cos - phasor[1] * sin, phasor[0] * sin + phasor[1] * cos]
            };
            let phase = |positive_thirds: i32| -> f64 {
                let positive = rotate(i1, positive_thirds);
                let negative = rotate(i2, -positive_thirds);
                (positive[0] + negative[0]).hypot(positive[1] + negative[1])
            };
            outputs[0] = torque;
            outputs[1] = -export / v_bus;
            outputs[2] = -heat;
            GeneratorState {
                load_angle_rad: if slipped { wrap_angle(delta_new) } else { delta_new },
                breaker_closed: !slipped,
                out_of_step: slipped,
                electric_power_w: export,
                // Delivered to the bus: the positive sequence's, less what the
                // negative-sequence current absorbs in its reactance, 3 X2 I2^2.
                reactive_power_var: vd * iq - vq * id - 3.0 * x2 * i2_squared,
                line_current_a: id.hypot(iq) / 3.0_f64.sqrt(),
                terminal_voltage_v: terminal,
                copper_loss_w: solution.copper_loss_w,
                core_loss_w: core_heat,
                damper_loss_w: damper_loss,
                torque_nm: torque,
                // Phase b lags phase a by a third of a turn, phase c leads it.
                phase_current_a: [phase(0), phase(-1), phase(1)],
                negative_sequence_current_a: i2_squared.sqrt(),
                negative_sequence_stator_loss_w: stator_loss_2,
                negative_sequence_rotor_loss_w: rotor_loss_2,
            }
        } else {
            // Breaker open: no stator current; the excited rotor still drags its core loss.
            outputs[0] = core_torque;
            outputs[1] = 0.0;
            outputs[2] = -core_heat;
            GeneratorState {
                load_angle_rad: wrap_angle(delta_new),
                breaker_closed: false,
                out_of_step: latched,
                electric_power_w: 0.0,
                reactive_power_var: 0.0,
                line_current_a: 0.0,
                terminal_voltage_v: self.card.rated_voltage_v
                    * self.open_circuit_emf_pu(excitation, omega_shaft.max(0.0)),
                copper_loss_w: 0.0,
                core_loss_w: core_heat,
                damper_loss_w: 0.0,
                torque_nm: core_torque,
                phase_current_a: [0.0; 3],
                negative_sequence_current_a: 0.0,
                negative_sequence_stator_loss_w: 0.0,
                negative_sequence_rotor_loss_w: 0.0,
            }
        };
        *new = state;

        let heat = state.total_loss_w();
        let scale = dt_s * ((omega_in * state.torque_nm).abs() + state.electric_power_w.abs() + heat);
        Ok(Proposal::Step(Receipt {
            stored_change_j: 0.0,
            dissipated_j: dt_s * heat,
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: dt_s * heat / t_cooler,
            energy_tolerance_j: 1.0e-10 * scale + 1.0e-9,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}

// ---------------------------------------------------------------------------
// 5. Shaft: the kernel's inertia, and rest
// ---------------------------------------------------------------------------

/// The shaft line's inertia with a state of rest.
///
/// While the shaft turns, or anything drives it, this is the kernel's
/// [`Mass`] and nothing else: its `propose` is called and its outputs passed
/// on unchanged.
///
/// The engine's friction law is regularised through zero speed, so below its
/// speed scale it is viscous and by itself never stops the shaft: the speed
/// decays for ever, into numbers the kernel can no longer difference. Static
/// friction ([`StandstillCard`]) ends that. A step in which
///
/// * nothing drives the shaft: the starter is not engaged (its command, held
///   over the step) and the torque the machines put on the shaft in the last
///   accepted step (the engine's indicated torque, the starter's, the
///   generator's when it motors) is no more than the static friction, and
/// * the speed at the start of the step is no more than the static friction
///   torque would remove within the step, `|omega| <= T_static dt / J`,
///
/// ends at exactly zero speed. The mean speed of that step is half the speed
/// it began with, whatever torque the machines on the shaft return; the
/// kinetic energy that was left, with the work those machines did on the
/// shaft in the step, leaves as heat through `friction_heat`. A shaft at rest
/// meets the same two conditions and stays at rest, releasing nothing, until
/// the starter is engaged.
///
/// Whether the engine drives is taken from what it did, not from the
/// ignition's command: an engine with its ignition on and no gas does not
/// drive, and its shaft comes to rest like any other. The decision uses the
/// torque of the last accepted step, never that of the step itself: that is
/// what the block solve iterates on, and a law that switched on it would
/// leave the solve no slope. So this is not a breakaway law within a step.
///
/// Ports: `p` (the kernel inertia's own: net torque in, mean speed of the
/// step out), `friction_heat` (heat of the stop), `starter` (signal: starter
/// engaged), `drive_torque` (signal: driving torque on the shaft in the last
/// accepted step [N m]).
pub struct ShaftComponent {
    inertia: Mass,
    inertia_kg_m2: f64,
    card: StandstillCard,
    ports: [PortSpec; 4],
}

impl ShaftComponent {
    /// A shaft line of `inertia_kg_m2` held at rest by the card's static
    /// friction.
    ///
    /// # Panics
    /// An inertia that is not positive and finite.
    #[must_use]
    pub fn new(inertia_kg_m2: f64, card: StandstillCard) -> Self {
        let inertia = Mass::in_domain(PortKind::RotationalTorqueAngularVelocity, inertia_kg_m2)
            .expect("shaft inertia");
        let power = inertia.ports()[0].clone();
        Self {
            inertia,
            inertia_kg_m2,
            card,
            ports: [
                power,
                PortSpec::heat(sid("friction_heat"), PortRole::EffortIn)
                    .expect("shaft friction heat port"),
                PortSpec::signal_in(sid("starter"), nz(1)),
                PortSpec::signal_in(sid("drive_torque"), nz(1)),
            ],
        }
    }

    /// The static friction card.
    #[must_use]
    pub fn card(&self) -> &StandstillCard {
        &self.card
    }

    /// Whether a step of `dt_s` from `old` under `inputs` ends at rest.
    fn comes_to_rest(&self, old: &Motion, dt_s: f64, inputs: &[f64]) -> bool {
        let driven = inputs[2] >= 0.5 || inputs[3] > self.card.static_friction_torque_nm;
        !driven && old.velocity_m_s.abs() <= self.card.rest_speed_rad_s(self.inertia_kg_m2, dt_s)
    }
}

impl Component for ShaftComponent {
    type State = Motion;
    type Error = KguError;

    fn ports(&self) -> &[PortSpec] {
        &self.ports
    }

    fn class(&self) -> Class {
        // Affine in the torque at a fixed state and fixed commands, but with a slope that
        // depends on both.
        Class::Smooth
    }

    fn stored_energy_j(&self, state: &Self::State) -> Result<f64, Self::Error> {
        Ok(0.5 * self.inertia_kg_m2 * state.velocity_m_s * state.velocity_m_s)
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
        all_finite(inputs, "shaft inputs must be finite")?;
        // Inputs: net torque (0), temperature the friction heat is released at (1), starter
        // engaged (2), driving torque of the last accepted step (3). Outputs: mean speed of
        // the step (0), heat (1).
        let (torque, temperature) = (inputs[0], inputs[1]);
        if !(temperature > 0.0) {
            return Err(KguError::Invalid("shaft heat port needs a positive temperature"));
        }
        if !self.comes_to_rest(old, dt_s, inputs) {
            let mut mean_speed = [0.0];
            let proposal = self
                .inertia
                .propose(old, dt_s, &inputs[..1], new, &mut mean_speed, cx)
                .map_err(|_| KguError::Invalid("the kernel's inertia refused the step"))?;
            outputs[0] = mean_speed[0];
            outputs[1] = 0.0;
            return Ok(proposal);
        }
        // Static friction stops the shaft within the step. The speed falls from where it
        // was to zero; its mean over the step is half of where it was.
        let mean_speed = 0.5 * old.velocity_m_s;
        *new = Motion {
            position_m: old.position_m + dt_s * mean_speed,
            velocity_m_s: 0.0,
        };
        let kinetic_j = self.stored_energy_j(old)?;
        // First law of the step: the work the shaft received and the kinetic energy it
        // had both leave as heat.
        let work_j = dt_s * torque * mean_speed;
        let heat_j = kinetic_j + work_j;
        outputs[0] = mean_speed;
        outputs[1] = -heat_j / dt_s;
        let released = heat_j.max(0.0);
        Ok(Proposal::Step(Receipt {
            stored_change_j: -kinetic_j,
            dissipated_j: released,
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: released / temperature,
            energy_tolerance_j: 64.0 * f64::EPSILON * (kinetic_j + work_j.abs()) + 1.0e-18,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}

impl PortTangent for ShaftComponent {
    fn jvp(
        &self,
        old: &Self::State,
        dt_s: f64,
        inputs: &[f64],
        d_inputs: &[f64],
        d_outputs: &mut [f64],
        _cx: &Cx<'_>,
    ) -> Result<(), Self::Error> {
        if self.comes_to_rest(old, dt_s, inputs) {
            // The mean speed is fixed by the state; the heat takes the work.
            d_outputs[0] = 0.0;
            d_outputs[1] = -0.5 * old.velocity_m_s * d_inputs[0];
        } else {
            d_outputs[0] = 0.5 * dt_s / self.inertia_kg_m2 * d_inputs[0];
            d_outputs[1] = 0.0;
        }
        Ok(())
    }

    fn vjp(
        &self,
        old: &Self::State,
        dt_s: f64,
        inputs: &[f64],
        bar_outputs: &[f64],
        bar_inputs: &mut [f64],
        _cx: &Cx<'_>,
    ) -> Result<(), Self::Error> {
        bar_inputs.fill(0.0);
        bar_inputs[0] = if self.comes_to_rest(old, dt_s, inputs) {
            -0.5 * old.velocity_m_s * bar_outputs[1]
        } else {
            0.5 * dt_s / self.inertia_kg_m2 * bar_outputs[0]
        };
        Ok(())
    }
}
