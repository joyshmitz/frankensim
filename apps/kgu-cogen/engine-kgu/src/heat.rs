//! Heat recovery: the exchangers around the jacket-coolant node.
//!
//! The coolant node itself is the kernel's `fs_system::elements::HeatCapacity`
//! (its temperature is a state), and the block's loss to the engine room is
//! the kernel's `Conductance`. The three exchangers here are stateless
//! Components that move heat between that node and the outside:
//!
//! 1. [`ExhaustHeatExchangerComponent`]: exhaust gas to coolant, with the
//!    remainder to the stack. Gas-side film from Gnielinski (`fs-convection`).
//! 2. [`NetworkHeatExchangerComponent`]: coolant to the district-heating
//!    network through a thermostatic valve; the network's flow is the demand.
//! 3. [`DumpCoolerComponent`]: coolant to ambient air through a radiator with
//!    fans; film coefficient from Churchill-Bernstein (`fs-convection`).
//!
//! Each is an effectiveness-NTU exchanger: a steady relation between the two
//! inlet temperatures and the two flows. Their own thermal masses and the
//! transport delay of the coolant round the circuit are not modelled.

use fs_convection::{CorrelationId, CorrelationInputs};
use fs_exec::Cx;
use fs_ports::component::{Class, Component, PortRole, PortSpec, Proposal, Receipt};

use crate::cards::{
    DumpCoolerCard, ExhaustExchangerCard, ExhaustGasCard, FuelGasCard, NetworkExchangerCard,
};
use crate::components::{KguError, all_finite, check_cx, nz, sid};

/// Effectiveness of a counterflow exchanger with `ntu` transfer units and
/// capacity-rate ratio `ratio = C_min / C_max` in `[0, 1]`.
#[must_use]
pub fn counterflow_effectiveness(ntu: f64, ratio: f64) -> f64 {
    if !(ntu > 0.0) {
        return 0.0;
    }
    if (1.0 - ratio).abs() < 1.0e-9 {
        ntu / (1.0 + ntu)
    } else {
        let decay = (-ntu * (1.0 - ratio)).exp();
        (1.0 - decay) / (1.0 - ratio * decay)
    }
}

/// Opening of a thermostatic valve: zero at or below `opens`, one at or
/// above `open`, and the smooth S-curve `3 x^2 - 2 x^3` between.
///
/// The curve has no corner at either end. A real wax element has none
/// either, and the block solve needs it: at a corner whose one side is flat
/// the kernel's Newton search stalls (found when the coolant crossed the
/// opening point of a piecewise-linear characteristic during cool-down).
#[must_use]
pub fn valve_opening(temperature_k: f64, opens_k: f64, open_k: f64) -> f64 {
    let x = ((temperature_k - opens_k) / (open_k - opens_k)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn nusselt(id: CorrelationId, inputs: CorrelationInputs) -> Result<f64, KguError> {
    fs_convection::evaluate(id, inputs)
        .map(|evaluation| evaluation.evidence().value)
        .map_err(|_| KguError::Invalid("fs-convection refused the correlation point"))
}

// ---------------------------------------------------------------------------
// 1. Exhaust-gas heat exchanger
// ---------------------------------------------------------------------------

/// Lowest Reynolds number of the Gnielinski card. Below it (less exhaust than
/// at no load) the film coefficient is held at this point's; the exchanger is
/// then far over-sized for the flow and the choice does not move the result.
const GNIELINSKI_LOWEST_REYNOLDS: f64 = 3_000.0;
/// Points of the tabulated gas-side Nusselt number.
const NUSSELT_POINTS: usize = 33;
/// Largest Reynolds number tabulated.
const NUSSELT_LARGEST_REYNOLDS: f64 = 80_000.0;

/// Exhaust-gas heat exchanger.
///
/// Ports: `gas` (heat, storage side: receives the engine's exhaust enthalpy
/// rate and returns the exhaust temperature it corresponds to), `coolant`
/// (heat into the coolant node), `stack` (the rest, to the stack), `flow`
/// (signal: exhaust heat-capacity rate [W/K]).
///
/// The exhaust enthalpy is sensible heat above the fuel stream's datum, so
/// the gas arrives at `T_datum + H / C`. The gas side is the smaller capacity
/// rate and the controlling film: `NTU = h A / C`, `eps = 1 - exp(-NTU)`,
/// `Q = eps C (T_gas - T_coolant)`.
pub struct ExhaustHeatExchangerComponent {
    ports: [PortSpec; 4],
    gas: ExhaustGasCard,
    card: ExhaustExchangerCard,
    datum_k: f64,
    /// Gnielinski Nusselt numbers, equally spaced in Reynolds number from
    /// [`GNIELINSKI_LOWEST_REYNOLDS`] to [`NUSSELT_LARGEST_REYNOLDS`].
    nusselt: Vec<f64>,
}

/// What the exhaust exchanger did in the last accepted step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ExhaustExchangerState {
    /// Exhaust temperature at the engine outlet [K]; zero with no flow.
    pub exhaust_k: f64,
    /// Exhaust temperature after the exchanger [K]; zero with no flow.
    pub stack_k: f64,
    /// Heat recovered into the coolant [W].
    pub recovered_w: f64,
    /// Enthalpy rate leaving by the stack, above the datum [W].
    pub stack_loss_w: f64,
    /// Effectiveness of the exchanger [-].
    pub effectiveness: f64,
}

impl ExhaustHeatExchangerComponent {
    /// An exchanger of the given geometry for `gas`, with the fuel's datum
    /// temperature as the zero of exhaust enthalpy.
    ///
    /// # Errors
    /// A geometry outside the Gnielinski card (`L / D` below 10).
    pub fn new(
        gas: ExhaustGasCard,
        card: ExhaustExchangerCard,
        fuel: &FuelGasCard,
    ) -> Result<Self, KguError> {
        let length_ratio = card.length_m / card.bore_m;
        let nusselt = (0..NUSSELT_POINTS)
            .map(|index| {
                let reynolds = GNIELINSKI_LOWEST_REYNOLDS
                    + (NUSSELT_LARGEST_REYNOLDS - GNIELINSKI_LOWEST_REYNOLDS) * index as f64
                        / (NUSSELT_POINTS - 1) as f64;
                nusselt(
                    CorrelationId::Gnielinski,
                    CorrelationInputs::forced(reynolds, gas.prandtl).with_length_ratio(length_ratio),
                )
            })
            .collect::<Result<Vec<f64>, KguError>>()?;
        Ok(Self {
            ports: [
                PortSpec::heat(sid("gas"), PortRole::FlowIn).expect("exhaust gas port"),
                PortSpec::heat(sid("coolant"), PortRole::EffortIn).expect("exhaust coolant port"),
                PortSpec::heat(sid("stack"), PortRole::EffortIn).expect("exhaust stack port"),
                PortSpec::signal_in(sid("flow"), nz(1)),
            ],
            gas,
            card,
            datum_k: fuel.standard_temperature_k,
            nusselt,
        })
    }

    /// The geometry whose tube length makes the exchanger recover
    /// `target_w` from an exhaust enthalpy rate `exhaust_w` at capacity rate
    /// `capacity_rate_w_k` into coolant at
    /// [`ExhaustExchangerCard::FIT_COOLANT_K`] (the fit of the card).
    ///
    /// # Errors
    /// A target the gas cannot give (more than its heat above the coolant).
    pub fn fitted_card(
        gas: &ExhaustGasCard,
        fuel: &FuelGasCard,
        exhaust_w: f64,
        capacity_rate_w_k: f64,
        target_w: f64,
    ) -> Result<ExhaustExchangerCard, KguError> {
        let mut card = ExhaustExchangerCard {
            tubes: ExhaustExchangerCard::TUBES,
            bore_m: ExhaustExchangerCard::BORE_M,
            length_m: 1.0,
        };
        let gas_k = fuel.standard_temperature_k + exhaust_w / capacity_rate_w_k;
        let available = capacity_rate_w_k * (gas_k - ExhaustExchangerCard::FIT_COOLANT_K);
        let effectiveness = target_w / available;
        if !(effectiveness > 0.0 && effectiveness < 1.0) {
            return Err(KguError::Invalid("exhaust exchanger target outside (0, 1) effectiveness"));
        }
        let ntu = -(1.0 - effectiveness).ln();
        let reynolds = card.reynolds(gas, capacity_rate_w_k);
        let film = nusselt(
            CorrelationId::Gnielinski,
            CorrelationInputs::forced(reynolds, gas.prandtl).with_length_ratio(100.0),
        )? * gas.conductivity_w_m_k
            / card.bore_m;
        let surface = ntu * capacity_rate_w_k / film;
        card.length_m = surface / (card.tubes * core::f64::consts::PI * card.bore_m);
        Ok(card)
    }

    /// The card this exchanger was built with.
    #[must_use]
    pub fn card(&self) -> &ExhaustExchangerCard {
        &self.card
    }

    fn film_w_m2_k(&self, capacity_rate_w_k: f64) -> f64 {
        let reynolds = self
            .card
            .reynolds(&self.gas, capacity_rate_w_k)
            .clamp(GNIELINSKI_LOWEST_REYNOLDS, NUSSELT_LARGEST_REYNOLDS);
        let last = NUSSELT_POINTS - 1;
        let position = (reynolds - GNIELINSKI_LOWEST_REYNOLDS)
            / (NUSSELT_LARGEST_REYNOLDS - GNIELINSKI_LOWEST_REYNOLDS)
            * last as f64;
        let index = (position.floor() as usize).min(last - 1);
        let weight = position - index as f64;
        let nusselt = self.nusselt[index] * (1.0 - weight) + self.nusselt[index + 1] * weight;
        nusselt * self.gas.conductivity_w_m_k / self.card.bore_m
    }
}

impl Component for ExhaustHeatExchangerComponent {
    type State = ExhaustExchangerState;
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
        all_finite(inputs, "exhaust exchanger inputs must be finite")?;
        // Inputs: gas enthalpy rate (0), coolant temperature (1), stack port temperature (2),
        // exhaust capacity rate (3).
        let exhaust_w = inputs[0];
        let coolant_k = inputs[1];
        let stack_port_k = inputs[2];
        let capacity_rate = inputs[3].max(0.0);
        if !(coolant_k > 0.0 && stack_port_k > 0.0) {
            return Err(KguError::Invalid("exhaust exchanger needs positive temperatures"));
        }
        // Q = eps (H - C (T_coolant - T_datum)), which is eps C (T_gas - T_coolant) written
        // without the gas temperature: it stays finite and continuous as the flow goes to
        // zero (the exchanger is then far over-sized: eps -> 1 and Q -> H). The block solve
        // needs that continuity; it cannot step across a jump.
        let effectiveness = if capacity_rate > 0.0 {
            let ntu = self.film_w_m2_k(capacity_rate) * self.card.surface_m2() / capacity_rate;
            1.0 - (-ntu).exp()
        } else {
            1.0
        };
        let recovered =
            effectiveness * (exhaust_w - capacity_rate * (coolant_k - self.datum_k));
        // Gas temperatures are read-outs; with next to no flow there is no gas to speak of.
        let flowing = capacity_rate > 1.0;
        let gas_k = if flowing { self.datum_k + exhaust_w / capacity_rate } else { 0.0 };
        let state = ExhaustExchangerState {
            exhaust_k: gas_k,
            stack_k: if flowing { gas_k - recovered / capacity_rate } else { 0.0 },
            recovered_w: recovered,
            stack_loss_w: exhaust_w - recovered,
            effectiveness,
        };
        // The gas port returns the temperature the exhaust enthalpy arrives at. It enters
        // only the engine's entropy account, and it must be smooth through zero flow for the
        // block solve: the gas temperature weighted by its capacity rate against the
        // coolant's weighted by 1 W/K. Above a few W/K it is the gas temperature.
        const NO_FLOW_WEIGHT_W_K: f64 = 1.0;
        let gas_port_k = ((capacity_rate * self.datum_k + exhaust_w
            + NO_FLOW_WEIGHT_W_K * coolant_k)
            / (capacity_rate + NO_FLOW_WEIGHT_W_K))
            .clamp(200.0, 3_000.0);
        outputs[0] = gas_port_k;
        outputs[1] = -state.recovered_w;
        outputs[2] = -state.stack_loss_w;
        *new = state;
        // Entropy: heat arrives at the gas temperature and leaves at the coolant's and the
        // stack port's. Never booked negative (a cold start can run the terms the other way
        // by less than the model resolves).
        let entropy = (state.recovered_w / coolant_k + state.stack_loss_w / stack_port_k
            - exhaust_w / gas_port_k)
            .max(0.0);
        let scale = dt_s * (exhaust_w.abs() + state.recovered_w.abs() + state.stack_loss_w.abs());
        Ok(Proposal::Step(Receipt {
            stored_change_j: 0.0,
            dissipated_j: 0.0,
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: dt_s * entropy,
            energy_tolerance_j: 1.0e-12 * scale + 1.0e-9,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}

// ---------------------------------------------------------------------------
// 2. Plate heat exchanger to the district-heating network
// ---------------------------------------------------------------------------

/// Plate heat exchanger to the district-heating network, behind the
/// thermostatic valve.
///
/// Ports: `coolant` (heat from the coolant node), `network` (heat to the
/// network), `demand` (signal: demand `[0, 1]`, network return temperature
/// [K]).
///
/// The network's capacity rate is the demand times its rated value; the
/// coolant's is the pump's times the thermostat's opening. Counterflow
/// effectiveness on a constant `UA`. A network that returns hotter than the
/// coolant gives heat to it through the open thermostat: the heat is then
/// negative.
pub struct NetworkHeatExchangerComponent {
    ports: [PortSpec; 3],
    card: NetworkExchangerCard,
    pump_capacity_rate_w_k: f64,
}

/// What the network exchanger did in the last accepted step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NetworkExchangerState {
    /// Heat delivered to the network [W].
    pub heat_w: f64,
    /// Opening of the thermostatic valve [-].
    pub thermostat_opening: f64,
    /// Temperature of the water leaving to the network [K]; zero with no flow.
    pub network_supply_k: f64,
}

impl NetworkHeatExchangerComponent {
    /// An exchanger with the given card on a coolant pump of
    /// `pump_capacity_rate_w_k`.
    #[must_use]
    pub fn new(card: NetworkExchangerCard, pump_capacity_rate_w_k: f64) -> Self {
        Self {
            ports: [
                PortSpec::heat(sid("coolant"), PortRole::EffortIn).expect("network coolant port"),
                PortSpec::heat(sid("network"), PortRole::EffortIn).expect("network port"),
                PortSpec::signal_in(sid("demand"), nz(2)),
            ],
            card,
            pump_capacity_rate_w_k,
        }
    }
}

impl Component for NetworkHeatExchangerComponent {
    type State = NetworkExchangerState;
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
        all_finite(inputs, "network exchanger inputs must be finite")?;
        // Inputs: coolant temperature (0), network port temperature (1), demand (2),
        // network return temperature (3).
        let coolant_k = inputs[0];
        let network_port_k = inputs[1];
        let demand = inputs[2].clamp(0.0, 1.0);
        let return_k = inputs[3];
        if !(coolant_k > 0.0 && network_port_k > 0.0 && return_k > 0.0) {
            return Err(KguError::Invalid("network exchanger needs positive temperatures"));
        }
        let opening =
            valve_opening(coolant_k, self.card.thermostat_opens_k, self.card.thermostat_open_k);
        let coolant_rate = self.pump_capacity_rate_w_k * opening;
        let network_rate = self.card.network_capacity_rate_w_k * demand;
        let smaller = coolant_rate.min(network_rate);
        let larger = coolant_rate.max(network_rate);
        // The exchanger moves heat whichever way the temperatures stand: return water
        // hotter than the coolant gives heat to the coolant, and the heat is negative. A
        // law cut off at equal temperatures would have a corner with one flat side there,
        // which the block solve cannot step across (found by the random walk of the tests:
        // a network return set above the coolant's temperature stopped the model).
        let heat = if smaller > 0.0 {
            let effectiveness =
                counterflow_effectiveness(self.card.ua_w_k / smaller, smaller / larger);
            effectiveness * smaller * (coolant_k - return_k)
        } else {
            0.0
        };
        outputs[0] = heat;
        outputs[1] = -heat;
        *new = NetworkExchangerState {
            heat_w: heat,
            thermostat_opening: opening,
            network_supply_k: if network_rate > 0.0 {
                return_k + heat / network_rate
            } else {
                0.0
            },
        };
        let entropy = (heat / network_port_k - heat / coolant_k).max(0.0);
        Ok(Proposal::Step(Receipt {
            stored_change_j: 0.0,
            dissipated_j: 0.0,
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: dt_s * entropy,
            energy_tolerance_j: 1.0e-12 * dt_s * heat.abs() + 1.0e-9,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}

// ---------------------------------------------------------------------------
// 3. Emergency (dump) cooler
// ---------------------------------------------------------------------------

/// Emergency cooler: a radiator the coolant is routed through as it warms
/// past the cooler's band, with fans.
///
/// Ports: `coolant` (heat from the coolant node), `air` (heat to ambient),
/// `fan` (signal: fan failed `0/1`).
///
/// The air's capacity rate is the smaller one: `NTU = h A / C_air`,
/// `eps = 1 - exp(-NTU)`, `Q = opening * eps * C_air * (T_coolant - T_air)`.
/// With the fans failed the air speed is the natural draught's, which lowers
/// both the film coefficient and the air's capacity rate.
pub struct DumpCoolerComponent {
    ports: [PortSpec; 3],
    card: DumpCoolerCard,
    /// `(air capacity rate [W/K], effectiveness)` with the fans running.
    fans_on: (f64, f64),
    /// The same with the fans failed.
    fans_failed: (f64, f64),
}

/// What the dump cooler did in the last accepted step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DumpCoolerState {
    /// Heat rejected to the air [W].
    pub heat_w: f64,
    /// Opening of the three-way valve [-].
    pub valve_opening: f64,
    /// Whether the fans were running.
    pub fans_running: bool,
}

impl DumpCoolerComponent {
    /// A cooler with the given card.
    ///
    /// # Errors
    /// An air speed outside the Churchill-Bernstein card.
    pub fn new(card: DumpCoolerCard) -> Result<Self, KguError> {
        let (density, cp, viscosity, conductivity, prandtl) = DumpCoolerCard::AIR;
        let at_speed = |speed: f64| -> Result<(f64, f64), KguError> {
            let reynolds = speed * card.tube_diameter_m / viscosity;
            let film = nusselt(
                CorrelationId::ChurchillBernsteinCylinder,
                CorrelationInputs::forced(reynolds, prandtl),
            )? * conductivity
                / card.tube_diameter_m;
            let capacity_rate = density * cp * speed * card.face_area_m2;
            Ok((capacity_rate, 1.0 - (-film * card.surface_m2 / capacity_rate).exp()))
        };
        Ok(Self {
            ports: [
                PortSpec::heat(sid("coolant"), PortRole::EffortIn).expect("dump coolant port"),
                PortSpec::heat(sid("air"), PortRole::EffortIn).expect("dump air port"),
                PortSpec::signal_in(sid("fan"), nz(1)),
            ],
            fans_on: at_speed(card.fan_air_speed_m_s)?,
            fans_failed: at_speed(card.draught_air_speed_m_s)?,
            card,
        })
    }

    /// Largest heat [W] the cooler rejects from coolant at `coolant_k` with
    /// its valve fully open.
    #[must_use]
    pub fn capacity_w(&self, coolant_k: f64, fans_running: bool) -> f64 {
        let (rate, effectiveness) = if fans_running { self.fans_on } else { self.fans_failed };
        effectiveness * rate * (coolant_k - self.card.ambient_k).max(0.0)
    }
}

impl Component for DumpCoolerComponent {
    type State = DumpCoolerState;
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
        all_finite(inputs, "dump cooler inputs must be finite")?;
        // Inputs: coolant temperature (0), air port temperature (1), fan failed (2).
        let coolant_k = inputs[0];
        let air_port_k = inputs[1];
        let fans_running = inputs[2] < 0.5;
        if !(coolant_k > 0.0 && air_port_k > 0.0) {
            return Err(KguError::Invalid("dump cooler needs positive temperatures"));
        }
        let opening = valve_opening(coolant_k, self.card.valve_opens_k, self.card.valve_open_k);
        let heat = opening * self.capacity_w(coolant_k, fans_running);
        outputs[0] = heat;
        outputs[1] = -heat;
        *new = DumpCoolerState {
            heat_w: heat,
            valve_opening: opening,
            fans_running,
        };
        let entropy = (heat / air_port_k - heat / coolant_k).max(0.0);
        Ok(Proposal::Step(Receipt {
            stored_change_j: 0.0,
            dissipated_j: 0.0,
            numerical_loss_j: 0.0,
            entropy_generated_j_per_k: dt_s * entropy,
            energy_tolerance_j: 1.0e-12 * dt_s * heat + 1.0e-9,
            residual: 0.0,
            tolerance: 0.0,
        }))
    }
}
