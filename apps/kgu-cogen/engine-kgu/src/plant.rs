//! The cogeneration unit as a description on the composition kernel:
//! components, connections and boundaries handed to
//! `fs_system::SystemBuilder`, stepped by `fs_system::System::step` and
//! audited by `fs_system::System::ledger`.
//!
//! ```text
//! gas supply (boundary) -> gas_valve ==stream==> engine --+
//!                                                         |  common flow (shaft speed)
//!        starter (battery boundary) ----------------------+-- shaft (kernel Mass)
//!        generator (bus boundary) ------------------------+
//! controller --signals--> starter, gas_valve, engine, cabinet
//! cabinet (protection) --signal--> generator breaker
//! bus (boundary): three phase voltages -> positive sequence to the generator's
//!                 grid port, negative sequence to its unbalance port
//!
//! engine jacket ----------+
//! exhaust_hx (coolant) ---+-- common effort (coolant temperature)
//! network_hx (coolant) ---+-- coolant (kernel HeatCapacity)
//! dump_cooler (coolant) --+
//! block_loss (kernel Conductance, to the engine room)
//! engine exhaust --heat route--> exhaust_hx (gas), with its flow as a signal
//! network_hx, dump_cooler, exhaust_hx (stack), engine (radiation, unburned gas),
//! generator, starter --heat routes--> named heat sinks
//! ```
//!
//! The controller's measurements are sampled from the accepted state after
//! every sub-step and handed to it as a boundary of the next one, so it acts
//! on start-of-step values, as a sampled controller does. The operator's
//! inputs reach the plant only through boundaries, and change only between
//! sub-steps.

use std::cell::RefCell;
use std::rc::Rc;

use fs_exec::Cx;
use fs_ports::PortKind;
use fs_system::elements::{Conductance, HeatCapacity, Mass, Motion};
use fs_system::{NodeId, RouteTemperature, SolveConfig, System, SystemBuilder, heat_sink};

use crate::analyser::PowerQualityAnalyser;
use crate::cabinet::{
    BreakerCabinetComponent, CabinetState, MEASURE_WIDTH, Sequences, sequence_components,
};
use crate::cards::{
    BUS_VOLTAGE_V, BreakerCabinetCard, CombustionLimitsCard, ControllerSetpoints, CoolantCard,
    DEFAULT_CONDITION_FACTOR, DumpCoolerCard, ExhaustExchangerCard, ExhaustGasCard, FuelGasCard,
    GasValveCard, GeneratorCard, HeatSplitCard, IndicatedWorkCard, NOMINAL_FREQUENCY_HZ,
    MeterCard, NetworkExchangerCard, PassportCard, PowerQualityCard, RATED_ELECTRIC_W,
    RATED_OMEGA_RAD_S, RestorationCard, SHAFT_INERTIA_KG_M2, StarterCard,
    SynchronisationWindowCard,
};
use crate::meter::RevenueMeter;
use crate::components::{
    GasControlValveComponent, GasEngineComponent, GasEngineState, GasValveState, GeneratorState,
    StarterMotorComponent, StarterState, SynchronousGeneratorComponent,
};
use crate::heat::{
    DumpCoolerComponent, DumpCoolerState, ExhaustExchangerState, ExhaustHeatExchangerComponent,
    NetworkExchangerState, NetworkHeatExchangerComponent,
};
use crate::controller::{
    ControllerState, GensetControllerComponent, OPERATOR_WIDTH, OperatorSignals, SENSE_WIDTH,
    SynchroCheck, close_refusal, synchronise_refusal,
};

/// The plant's fixed sub-step [s]: the governor's sample period. The
/// electromechanical swing of the generator against the bus (about 4.6 Hz
/// with the assumed inertia and reactances) is resolved by 21 sub-steps.
pub const SUB_STEP_S: f64 = 0.01;

/// Temperature at which the heat delivered to the network is booked [K]: the
/// mean of a 65/85 C network. It enters only the entropy account.
pub const NETWORK_SINK_K: f64 = 273.15 + 75.0;
/// Temperature at which the stack loss is booked [K]. Entropy account only.
pub const STACK_SINK_K: f64 = 273.15 + 120.0;
/// Temperature of the engine room [K]: radiation, generator cooling air,
/// starter losses and the block's own loss are booked there.
pub const ROOM_K: f64 = 273.15 + 30.0;

/// Names of the heat sinks in the ledger.
pub mod sink {
    /// Heat delivered to the district-heating network.
    pub const NETWORK: &str = "district_heating";
    /// Heat rejected by the emergency cooler.
    pub const DUMP: &str = "dump_cooler_air";
    /// Exhaust enthalpy leaving by the stack, after the exhaust exchanger.
    pub const EXHAUST: &str = "exhaust_stack";
    /// Engine surface radiation and convection.
    pub const RADIATION: &str = "engine_radiation";
    /// Generator losses.
    pub const GENERATOR: &str = "generator_cooling_air";
    /// Starter losses.
    pub const STARTER: &str = "starter_losses";
}

/// Operator inputs, boundary conditions and injectable faults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OperatorInputs {
    /// Load setpoint [fraction of rated].
    pub load_setpoint: f64,
    /// Load ramp setting [fraction of rated per second].
    pub load_ramp_per_s: f64,
    /// Excitation setting [pu].
    pub excitation_pu: f64,
    /// Whether the synchroniser closes the breaker by itself.
    pub auto_synchronise: bool,
    /// Heat demand of the district-heating network [fraction of available].
    pub heat_demand: f64,
    /// Network return temperature [K].
    pub network_return_k: f64,
    /// Gas supply gauge pressure [Pa].
    pub gas_gauge_pa: f64,
    /// Gas supply temperature [K].
    pub gas_temperature_k: f64,
    /// Atmospheric pressure [Pa].
    pub atmospheric_pa: f64,
    /// Magnitudes of the three phase voltages of the bus [pu of the rated
    /// phase voltage], phases a, b, c.
    pub grid_phase_voltage_pu: [f64; 3],
    /// How far each phase voltage is turned from its nominal position [rad].
    pub grid_phase_angle_rad: [f64; 3],
    /// Whether the unit returns to the bus by itself after a grid event.
    pub restoration_auto: bool,
    /// Bus frequency [Hz].
    pub grid_frequency_hz: f64,
    /// Machine-condition factor [-].
    pub condition_factor: f64,
    /// Fault: emergency cooler fan failed.
    pub fan_failed: bool,
    /// Fault: low oil pressure switch.
    pub oil_pressure_low: bool,
    /// Command pulses, consumed by the next sub-step.
    pub start: bool,
    /// Normal stop pulse.
    pub stop: bool,
    /// Emergency stop pulse.
    pub emergency_stop: bool,
    /// Acknowledge pulse.
    pub acknowledge: bool,
    /// Manual breaker close pulse.
    pub breaker_close: bool,
    /// Manual breaker open pulse.
    pub breaker_open: bool,
    /// Command to synchronise (pulse): the automatic synchroniser closes the
    /// breaker for this one attempt.
    pub synchronise: bool,
}

impl Default for OperatorInputs {
    fn default() -> Self {
        Self {
            load_setpoint: 0.0,
            load_ramp_per_s: ControllerSetpoints::DEFAULT.load_ramp_per_s,
            excitation_pu: GeneratorCard::REFERENCE_EXCITATION_PU,
            auto_synchronise: true,
            heat_demand: 1.0,
            network_return_k: 273.15 + 65.0,
            gas_gauge_pa: GasValveCard::SIZING_GAUGE_PA,
            gas_temperature_k: GasValveCard::SIZING_TEMPERATURE_K,
            atmospheric_pa: FuelGasCard::STATION.standard_pressure_pa,
            grid_phase_voltage_pu: [1.0; 3],
            grid_phase_angle_rad: [0.0; 3],
            restoration_auto: false,
            grid_frequency_hz: NOMINAL_FREQUENCY_HZ,
            condition_factor: DEFAULT_CONDITION_FACTOR,
            fan_failed: false,
            oil_pressure_low: false,
            start: false,
            stop: false,
            emergency_stop: false,
            acknowledge: false,
            breaker_close: false,
            breaker_open: false,
            synchronise: false,
        }
    }
}

impl OperatorInputs {
    /// Positive and negative sequence of the bus's phase voltages [pu].
    #[must_use]
    pub fn grid_sequences(&self) -> Sequences {
        sequence_components(self.grid_phase_voltage_pu, self.grid_phase_angle_rad)
    }
}

/// Measurements sampled from the accepted state after each sub-step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sensed {
    /// Shaft speed [rad/s].
    pub omega_rad_s: f64,
    /// Angle of the generator EMF ahead of the bus [rad].
    pub load_angle_rad: f64,
    /// Breaker closed.
    pub breaker_closed: bool,
    /// Electrical power [W].
    pub electric_power_w: f64,
    /// Generator out of step.
    pub out_of_step: bool,
    /// Coolant temperature at the engine outlet [K].
    pub coolant_k: f64,
    /// Generator phase currents [A].
    pub phase_current_a: [f64; 3],
    /// Generator negative-sequence current [A].
    pub negative_sequence_current_a: f64,
    /// The cabinet's trip count.
    pub cabinet_trips: u32,
    /// The cabinet's tripped functions, one bit each.
    pub cabinet_tripped: u32,
    /// Whether the cabinet's protection finds the grid healthy.
    pub grid_healthy: bool,
}

struct Shared {
    operator: OperatorInputs,
    sensed: Sensed,
}

/// One entry of the event log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Time of the sub-step that ended with the change [s].
    pub time_s: f64,
    /// What changed, see [`event_kind`].
    pub kind: u32,
    /// State code, sequence step, breaker position, alarm bit index or refusal bits.
    pub code: u32,
    /// A value that goes with it (shaft speed [rpm] for state changes,
    /// electrical power [kW] for breaker changes), else zero.
    pub value: f64,
}

/// Kinds of [`Event`].
pub mod event_kind {
    /// The unit state changed; `code` is the new state code.
    pub const STATE: u32 = 1;
    /// The breaker changed; `code` is 1 for closed.
    pub const BREAKER: u32 = 2;
    /// An alarm or warning was raised; `code` is its bit index.
    pub const ALARM_RAISED: u32 = 3;
    /// An alarm or warning cleared; `code` is its bit index.
    pub const ALARM_CLEARED: u32 = 4;
    /// A command was refused; `code` holds the close refusal bits, or zero
    /// for a refused start.
    pub const REFUSED: u32 = 5;
    /// The sequence step changed; `code` is the new step index.
    pub const SEQUENCE: u32 = 6;
    /// A protection function of the breaker cabinet tripped. `code` is ten
    /// times the function's index (`cabinet::protection`) plus the phase (1,
    /// 2, 3 for a, b, c; 0 when the function is not about one phase); `value`
    /// is what it tripped at, in the function's unit: phase voltage [V],
    /// unbalance [%], negative-sequence current [% of rated], phase current
    /// [A], frequency [Hz], reverse power [kW].
    pub const PROTECTION: u32 = 7;
    /// The operator commanded a synchronisation. `code` is zero when the
    /// command was accepted (the automatic synchroniser will close the
    /// breaker), otherwise the refusal bits; `value` is the phase angle at
    /// the command [degrees]. A closing with no such event before it was
    /// the automation's own (automatic start or automatic return) or a
    /// manual close request.
    pub const SYNCHRONISE_COMMAND: u32 = 8;
}

/// Handles of the plant's nodes.
#[derive(Debug, Clone, Copy)]
pub struct KguNodes {
    /// Gas control valve.
    pub gas_valve: NodeId,
    /// Engine.
    pub engine: NodeId,
    /// Shaft inertia (kernel `Mass`).
    pub shaft: NodeId,
    /// Starter motor.
    pub starter: NodeId,
    /// Generator.
    pub generator: NodeId,
    /// Genset controller.
    pub controller: NodeId,
    /// Jacket coolant node (kernel `HeatCapacity`).
    pub coolant: NodeId,
    /// Exhaust-gas heat exchanger.
    pub exhaust_hx: NodeId,
    /// Plate heat exchanger to the network.
    pub network_hx: NodeId,
    /// Emergency cooler.
    pub dump_cooler: NodeId,
    /// Generator breaker cabinet (protection device).
    pub cabinet: NodeId,
}

/// The sealed plant with its operator registers and event log.
pub struct KguPlant {
    /// The sealed system.
    pub system: System,
    /// Node handles.
    pub nodes: KguNodes,
    shared: Rc<RefCell<Shared>>,
    events: Vec<Event>,
    /// The cabinet's power-quality analyser.
    pub analyser: PowerQualityAnalyser,
    /// The cabinet's revenue meter.
    pub meter: RevenueMeter,
    /// The cabinet's protection settings.
    pub cabinet_card: BreakerCabinetCard,
    /// The restoration settings.
    pub restoration_card: RestorationCard,
    /// The coolant circuit's card.
    pub coolant_card: CoolantCard,
    /// The exhaust exchanger's card, with its fitted tube length.
    pub exhaust_card: ExhaustExchangerCard,
    /// The gas card the plant was built with.
    pub gas: FuelGasCard,
    /// The controller's settings.
    pub setpoints: ControllerSetpoints,
    /// The synchronisation window.
    pub window: SynchronisationWindowCard,
    /// The generator card.
    pub generator_card: GeneratorCard,
    /// Friction power at rated speed [W] (fitted).
    pub rated_friction_w: f64,
    /// Rated fuel mass flow the valve was sized for [kg/s].
    pub rated_fuel_kg_s: f64,
    /// Newton iterations of the last sub-step.
    pub last_iterations: u32,
    check_controller: GensetControllerComponent,
    meter_valve: GasControlValveComponent,
    time_debt_s: f64,
}

/// Longest event log kept; older entries are dropped.
const EVENT_LOG_CAPACITY: usize = 512;

impl KguPlant {
    /// Build and seal the plant at standstill, ready to start.
    ///
    /// # Panics
    /// If the description is refused by the kernel: that is a defect of this
    /// crate, not a run-time condition.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn build() -> Self {
        let gas = FuelGasCard::STATION;
        let passport = PassportCard::STATION;
        let generator_card = GeneratorCard::ASSUMED;
        let setpoints = ControllerSetpoints::DEFAULT;
        let window = SynchronisationWindowCard::ASSUMED;

        // The indicated-work card is fitted through this plant's own generator model.
        let generator_c = SynchronousGeneratorComponent::new(generator_card);
        let work = IndicatedWorkCard::fitted(passport, |electric_w| {
            generator_c
                .reference_loss_w(electric_w)
                .expect("generator reference loss inside the fitted range")
        });
        let rated_friction_w = work.rated_friction_w;
        let rated_indicated_w = work.indicated_power_w(passport.fuel_for_electric_w(RATED_ELECTRIC_W));
        let no_load_torque_nm =
            (rated_friction_w + work.reference_generator_loss_w(0.0)) / RATED_OMEGA_RAD_S;
        let limits = CombustionLimitsCard {
            largest_indicated_torque_nm: 1.15 * rated_indicated_w / RATED_OMEGA_RAD_S,
            torque_speed_floor_rad_s: setpoints.firing_speed_rad_s,
        };
        // Rated fuel flow of the machine in its default condition.
        let rated_fuel_kg_s = passport.fuel_for_electric_w(RATED_ELECTRIC_W)
            / DEFAULT_CONDITION_FACTOR
            / gas.lower_heating_value_j_kg();
        let valve_card = GasValveCard::sized_for(&gas, rated_fuel_kg_s);

        let gas_valve_c = GasControlValveComponent::new(gas, valve_card);
        let exhaust_gas = ExhaustGasCard::ASSUMED;
        let engine_c =
            GasEngineComponent::new(gas, work, HeatSplitCard::ASSUMED, limits, exhaust_gas);

        // Heat recovery. The exhaust exchanger's tube length is fitted at the passport's
        // rated point: what it recovers there plus the jacket heat is the passport's thermal
        // recovery. Everything else about the heat side is computed.
        let coolant_card = CoolantCard::ASSUMED;
        let rated = engine_c.balance(
            rated_fuel_kg_s,
            rated_fuel_kg_s * gas.specific_enthalpy_j_kg(GasValveCard::SIZING_TEMPERATURE_K),
            RATED_OMEGA_RAD_S,
            true,
            DEFAULT_CONDITION_FACTOR,
        );
        let rated_recovery_w = passport.thermal_recovery(1.0) * rated.fuel_power_w;
        let exhaust_card = ExhaustHeatExchangerComponent::fitted_card(
            &exhaust_gas,
            &gas,
            rated.exhaust_enthalpy_w,
            rated.exhaust_capacity_rate_w_k,
            rated_recovery_w - rated.jacket_heat_w,
        )
        .expect("exhaust exchanger fit at the rated point");
        let exhaust_hx_c = ExhaustHeatExchangerComponent::new(exhaust_gas, exhaust_card, &gas)
            .expect("exhaust exchanger inside its correlation card");
        let network_hx_c = NetworkHeatExchangerComponent::new(
            NetworkExchangerCard::ASSUMED,
            coolant_card.pump_capacity_rate_w_k,
        );
        let dump_card = DumpCoolerCard::ASSUMED;
        let dump_cooler_c =
            DumpCoolerComponent::new(dump_card).expect("dump cooler inside its correlation card");
        let coolant_c = HeatCapacity::new(coolant_card.capacity_j_k).expect("coolant capacity");
        let block_loss_c =
            Conductance::new(coolant_card.room_conductance_w_k).expect("block conductance");
        let shaft_c = Mass::in_domain(PortKind::RotationalTorqueAngularVelocity, SHAFT_INERTIA_KG_M2)
            .expect("shaft inertia");
        let starter_c = StarterMotorComponent::new(StarterCard::ASSUMED);
        let cabinet_card = BreakerCabinetCard::DEFAULT;
        let restoration_card = RestorationCard::DEFAULT;
        let cabinet_c = BreakerCabinetComponent::new(
            cabinet_card,
            restoration_card,
            generator_card.rated_current_a(),
        );
        let new_controller = || {
            GensetControllerComponent::new(
                setpoints,
                window,
                restoration_card,
                generator_card.pole_pairs,
                generator_card.excitation_range_pu,
                no_load_torque_nm,
            )
        };

        let operator = OperatorInputs::default();
        let shared = Rc::new(RefCell::new(Shared {
            operator,
            sensed: Sensed {
                omega_rad_s: 0.0,
                load_angle_rad: 0.0,
                breaker_closed: false,
                electric_power_w: 0.0,
                out_of_step: false,
                coolant_k: coolant_card.standby_k,
                phase_current_a: [0.0; 3],
                negative_sequence_current_a: 0.0,
                cabinet_trips: 0,
                cabinet_tripped: 0,
                grid_healthy: false,
            },
        }));

        let mut b = SystemBuilder::new();
        let controller = b
            .add("controller", new_controller(), ControllerState::ready())
            .expect("add controller");
        let gas_valve = b
            .add("gas_valve", gas_valve_c, GasValveState::closed(&gas))
            .expect("add gas_valve");
        let engine = b
            .add("engine", engine_c, GasEngineState::default())
            .expect("add engine");
        let shaft = b
            .add_with_tangent(
                "shaft",
                shaft_c,
                Motion {
                    position_m: 0.0,
                    velocity_m_s: 0.0,
                },
            )
            .expect("add shaft");
        let starter = b
            .add("starter", starter_c, StarterState::default())
            .expect("add starter");
        let generator = b
            .add("generator", generator_c, GeneratorState::open())
            .expect("add generator");
        let coolant = b
            .add_with_tangent("coolant", coolant_c, coolant_card.standby_k)
            .expect("add coolant");
        let block_loss = b
            .add_with_tangent("block_loss", block_loss_c, ())
            .expect("add block_loss");
        let exhaust_hx = b
            .add("exhaust_hx", exhaust_hx_c, ExhaustExchangerState::default())
            .expect("add exhaust_hx");
        let network_hx = b
            .add("network_hx", network_hx_c, NetworkExchangerState::default())
            .expect("add network_hx");
        let dump_cooler = b
            .add("dump_cooler", dump_cooler_c, DumpCoolerState::default())
            .expect("add dump_cooler");
        let cabinet = b
            .add("cabinet", cabinet_c, CabinetState::open())
            .expect("add cabinet");

        let port = |b: &SystemBuilder, node: NodeId, id: &str| b.port(node, id).expect("known port");
        let valve_inlet = port(&b, gas_valve, "inlet");
        let valve_outlet = port(&b, gas_valve, "outlet");
        let valve_command = port(&b, gas_valve, "command");
        let engine_gas = port(&b, engine, "gas_in");
        let engine_shaft = port(&b, engine, "shaft");
        let engine_jacket = port(&b, engine, "jacket_heat");
        let engine_exhaust = port(&b, engine, "exhaust_heat");
        let engine_radiation = port(&b, engine, "radiation_heat");
        let engine_ignition = port(&b, engine, "ignition");
        let engine_conditions = port(&b, engine, "conditions");
        let shaft_p = port(&b, shaft, "p");
        let starter_shaft = port(&b, starter, "shaft");
        let starter_battery = port(&b, starter, "battery");
        let starter_heat = port(&b, starter, "heat");
        let starter_engage = port(&b, starter, "engage");
        let generator_shaft = port(&b, generator, "shaft");
        let generator_grid = port(&b, generator, "grid");
        let generator_heat = port(&b, generator, "cooler_heat");
        let generator_breaker = port(&b, generator, "breaker");
        let generator_field = port(&b, generator, "field");
        let controller_sense = port(&b, controller, "sense");
        let controller_operator = port(&b, controller, "operator");
        let controller_actuators = port(&b, controller, "actuators");

        // --- Fuel gas: valve (transport) into the engine (which sets the mixer pressure).
        b.bond(engine_gas, valve_outlet).expect("bond gas valve to engine");

        // --- One shaft speed: the inertia sets it, every machine on it returns a torque.
        b.common_flow(shaft_p, &[engine_shaft, starter_shaft, generator_shaft])
            .expect("shaft junction");

        // --- Controller outputs: [starter, gas shut-off, ignition, valve demand, breaker
        //     close request, cabinet reset]. The close request goes to the cabinet, whose
        //     protection decides what the breaker receives.
        let cabinet_command = port(&b, cabinet, "command");
        let cabinet_measure = port(&b, cabinet, "measure");
        let cabinet_breaker = port(&b, cabinet, "breaker");
        let generator_unbalance = port(&b, generator, "unbalance");
        b.signal_map(controller_actuators, starter_engage, vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
            .expect("starter command");
        b.signal_map(
            controller_actuators,
            valve_command,
            vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        )
        .expect("valve command");
        b.signal_map(controller_actuators, engine_ignition, vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
            .expect("ignition command");
        b.signal_map(
            controller_actuators,
            cabinet_command,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        )
        .expect("cabinet command");
        b.signal(cabinet_breaker, generator_breaker)
            .expect("breaker close path");

        // --- Heat recovery: one coolant temperature (the kernel's heat capacity sets it);
        //     the engine's jacket, the three exchangers and the block's loss exchange heat
        //     with it.
        let coolant_heat = port(&b, coolant, "heat");
        let block_a = port(&b, block_loss, "a");
        let block_b = port(&b, block_loss, "b");
        let exhaust_gas_port = port(&b, exhaust_hx, "gas");
        let exhaust_coolant = port(&b, exhaust_hx, "coolant");
        let exhaust_stack = port(&b, exhaust_hx, "stack");
        let exhaust_flow = port(&b, exhaust_hx, "flow");
        let network_coolant = port(&b, network_hx, "coolant");
        let network_out = port(&b, network_hx, "network");
        let network_demand = port(&b, network_hx, "demand");
        let dump_coolant = port(&b, dump_cooler, "coolant");
        let dump_air = port(&b, dump_cooler, "air");
        let dump_fan = port(&b, dump_cooler, "fan");
        b.common_effort(
            coolant_heat,
            &[engine_jacket, exhaust_coolant, network_coolant, dump_coolant, block_a],
        )
        .expect("coolant junction");
        // The engine's exhaust enthalpy goes to the exhaust exchanger, which answers with
        // the temperature that enthalpy arrives at; its capacity rate is a signal.
        b.heat_route(
            engine_exhaust,
            &[(exhaust_gas_port, 1.0)],
            None,
            RouteTemperature::SinkEquivalent,
        )
        .expect("exhaust route");
        let engine_exhaust_flow = port(&b, engine, "exhaust_flow");
        let engine_unburned = port(&b, engine, "unburned_heat");
        b.signal(engine_exhaust_flow, exhaust_flow)
            .expect("exhaust flow signal");
        b.boundary(block_b, Box::new(|_t0, _dt, values| values[0] = ROOM_K))
            .expect("engine room boundary");
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                network_demand,
                Box::new(move |_t0, _dt, values| {
                    let operator = shared.borrow().operator;
                    values[0] = operator.heat_demand;
                    values[1] = operator.network_return_k;
                }),
            )
            .expect("network demand boundary");
        }
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                dump_fan,
                Box::new(move |_t0, _dt, values| {
                    values[0] = if shared.borrow().operator.fan_failed { 1.0 } else { 0.0 };
                }),
            )
            .expect("dump fan boundary");
        }

        // --- Heat leaves the system to named sinks.
        for (source, name, kelvin) in [
            (network_out, sink::NETWORK, NETWORK_SINK_K),
            (dump_air, sink::DUMP, dump_card.ambient_k),
            (exhaust_stack, sink::EXHAUST, STACK_SINK_K),
            (engine_unburned, sink::EXHAUST, STACK_SINK_K),
            (engine_radiation, sink::RADIATION, ROOM_K),
            (generator_heat, sink::GENERATOR, ROOM_K),
            (starter_heat, sink::STARTER, ROOM_K),
        ] {
            b.heat_route(
                source,
                &[],
                Some(heat_sink(name, kelvin).expect("sink name")),
                RouteTemperature::Fixed(kelvin),
            )
            .expect("heat route");
        }

        // --- Boundaries: everything the operator and the surroundings prescribe.
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                valve_inlet,
                Box::new(move |_t0, _dt, values| {
                    let operator = shared.borrow().operator;
                    values[0] = operator.atmospheric_pa + operator.gas_gauge_pa;
                    values[1] = gas.specific_enthalpy_j_kg(operator.gas_temperature_k);
                }),
            )
            .expect("gas supply boundary");
        }
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                engine_conditions,
                Box::new(move |_t0, _dt, values| {
                    let operator = shared.borrow().operator;
                    values[0] = operator.condition_factor;
                    values[1] = operator.atmospheric_pa;
                }),
            )
            .expect("engine conditions boundary");
        }
        b.boundary(
            starter_battery,
            Box::new(|_t0, _dt, values| values[0] = StarterCard::ASSUMED.battery_voltage_v),
        )
        .expect("battery boundary");
        {
            let shared = Rc::clone(&shared);
            let rated_voltage = generator_card.rated_voltage_v;
            b.boundary(
                generator_grid,
                Box::new(move |_t0, _dt, values| {
                    // The 10 kV bus referred to the generator side through the ideal ratio:
                    // the positive sequence of its three phase voltages, as a line voltage.
                    let sequences = shared.borrow().operator.grid_sequences();
                    values[0] = sequences.positive_magnitude() * rated_voltage;
                }),
            )
            .expect("bus boundary");
        }
        {
            let shared = Rc::clone(&shared);
            let rated_phase_voltage = generator_card.rated_phase_voltage_v();
            b.boundary(
                generator_unbalance,
                Box::new(move |_t0, _dt, values| {
                    // The negative sequence [V per phase] against the positive one, and
                    // the positive sequence's own angle.
                    let sequences = shared.borrow().operator.grid_sequences();
                    let negative = sequences.negative_against_positive();
                    values[0] = negative[0] * rated_phase_voltage;
                    values[1] = negative[1] * rated_phase_voltage;
                    values[2] = sequences.positive_angle();
                }),
            )
            .expect("unbalance boundary");
        }
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                cabinet_measure,
                Box::new(move |_t0, _dt, values| {
                    debug_assert_eq!(values.len(), MEASURE_WIDTH);
                    let shared = shared.borrow();
                    let (operator, sensed) = (&shared.operator, &shared.sensed);
                    values[0..3].copy_from_slice(&operator.grid_phase_voltage_pu);
                    values[3..6].copy_from_slice(&operator.grid_phase_angle_rad);
                    values[6..9].copy_from_slice(&sensed.phase_current_a);
                    values[9] = sensed.negative_sequence_current_a;
                    values[10] = operator.grid_frequency_hz;
                    values[11] = sensed.electric_power_w;
                    values[12] = if sensed.breaker_closed { 1.0 } else { 0.0 };
                }),
            )
            .expect("cabinet measure boundary");
        }
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                generator_field,
                Box::new(move |_t0, _dt, values| {
                    let operator = shared.borrow().operator;
                    values[0] = operator.excitation_pu;
                    values[1] = operator.grid_frequency_hz;
                }),
            )
            .expect("field boundary");
        }
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                controller_sense,
                Box::new(move |_t0, _dt, values| {
                    let shared = shared.borrow();
                    let sensed = shared.sensed;
                    let flag = |on: bool| if on { 1.0 } else { 0.0 };
                    values[0] = sensed.omega_rad_s;
                    // The synchroniser compares against the positive-sequence voltage.
                    values[1] =
                        sensed.load_angle_rad - shared.operator.grid_sequences().positive_angle();
                    values[2] = flag(sensed.breaker_closed);
                    values[3] = sensed.electric_power_w;
                    values[4] = flag(sensed.out_of_step);
                    values[5] = sensed.coolant_k;
                    values[6] = f64::from(sensed.cabinet_trips);
                    values[7] = f64::from(sensed.cabinet_tripped);
                    values[8] = flag(sensed.grid_healthy);
                    debug_assert_eq!(values.len(), SENSE_WIDTH);
                }),
            )
            .expect("controller sense boundary");
        }
        {
            let shared = Rc::clone(&shared);
            b.boundary(
                controller_operator,
                Box::new(move |_t0, _dt, values| {
                    debug_assert_eq!(values.len(), OPERATOR_WIDTH);
                    operator_signals(&shared.borrow().operator).write(values);
                }),
            )
            .expect("controller operator boundary");
        }

        let mut system = b.seal().expect("seal the cogeneration unit");
        system.set_config(SolveConfig {
            relative_tolerance: 1.0e-9,
            absolute_tolerance: 1.0e-9,
            max_iterations: 40,
            max_shortenings: 16,
            interface_energy_tolerance: 1.0e-6,
        });
        // First guesses for the temperatures the solve iterates on: a component refuses a
        // heat port at zero kelvin.
        for wire in [
            engine_jacket,
            engine_exhaust,
            exhaust_coolant,
            network_coolant,
            dump_coolant,
            block_a,
        ] {
            let _ = system.guess(wire, &[coolant_card.standby_k]);
        }

        Self {
            system,
            nodes: KguNodes {
                gas_valve,
                engine,
                shaft,
                starter,
                generator,
                controller,
                coolant,
                exhaust_hx,
                network_hx,
                dump_cooler,
                cabinet,
            },
            analyser: PowerQualityAnalyser::new(
                PowerQualityCard::DEFAULT,
                generator_card.rated_phase_voltage_v(),
            ),
            meter: RevenueMeter::new(MeterCard::ASSUMED),
            cabinet_card,
            restoration_card,
            coolant_card,
            exhaust_card,
            shared,
            events: Vec::new(),
            gas,
            setpoints,
            window,
            generator_card,
            rated_friction_w,
            rated_fuel_kg_s,
            last_iterations: 0,
            check_controller: new_controller(),
            meter_valve: GasControlValveComponent::new(gas, valve_card),
            time_debt_s: 0.0,
        }
    }

    /// Operator inputs as they stand.
    #[must_use]
    pub fn operator(&self) -> OperatorInputs {
        self.shared.borrow().operator
    }

    /// Change the operator inputs; takes effect at the next sub-step.
    pub fn set_operator(&mut self, change: impl FnOnce(&mut OperatorInputs)) {
        change(&mut self.shared.borrow_mut().operator);
    }

    /// The measurements the controller will read at the next sub-step.
    #[must_use]
    pub fn sensed(&self) -> Sensed {
        self.shared.borrow().sensed
    }

    /// The synchroniser's view of the accepted state under the operator's
    /// present excitation and bus settings.
    #[must_use]
    pub fn synchro_check(&self) -> SynchroCheck {
        let shared = self.shared.borrow();
        let sequences = shared.operator.grid_sequences();
        self.check_controller.check(
            shared.sensed.omega_rad_s,
            shared.sensed.load_angle_rad - sequences.positive_angle(),
            shared.operator.excitation_pu,
            sequences.positive_magnitude(),
            shared.operator.grid_frequency_hz,
        )
    }

    /// Reasons a breaker close request would be refused now; zero when the
    /// controller will accept it at the next sub-step.
    #[must_use]
    pub fn close_refusal(&self) -> u32 {
        let controller = self.controller_state();
        close_refusal(controller.sequence, controller.alarms, &self.synchro_check())
    }

    /// Reasons a command to synchronise would be refused now; zero when the
    /// controller will arm its synchroniser at the next sub-step.
    #[must_use]
    pub fn synchronise_refusal(&self) -> u32 {
        let controller = self.controller_state();
        synchronise_refusal(controller.sequence, controller.alarms)
    }

    /// Advance by `dt_s`, in whole sub-steps of [`SUB_STEP_S`]. Time that
    /// does not fill a sub-step is carried to the next call.
    ///
    /// # Errors
    /// The kernel's refusal of a sub-step, as text.
    pub fn step(&mut self, dt_s: f64, cx: &Cx<'_>) -> Result<(), String> {
        self.time_debt_s += dt_s;
        while self.time_debt_s >= SUB_STEP_S * (1.0 - 1.0e-9) {
            self.sub_step(cx)?;
            self.time_debt_s -= SUB_STEP_S;
        }
        if self.time_debt_s < 0.0 {
            self.time_debt_s = 0.0;
        }
        Ok(())
    }

    fn sub_step(&mut self, cx: &Cx<'_>) -> Result<(), String> {
        let before_controller = self.controller_state();
        let before_generator = self.generator_state();
        let before_cabinet = self.cabinet_state();

        // `System::step` takes one step of at most the sub-step; a component may shorten it.
        let mut remaining = SUB_STEP_S;
        let mut taken = 0_u32;
        while remaining > 1.0e-12 * SUB_STEP_S {
            taken += 1;
            if taken > 64 {
                return Err(format!(
                    "fs_system::System::step kept shortening: {remaining:.3e} s of a sub-step left"
                ));
            }
            let grid_energy_before_j = self.boundary_energy_in_j("generator.grid");
            let report = self
                .system
                .step(remaining, cx)
                .map_err(|error| format!("fs_system::System::step refused: {error}"))?;
            remaining -= report.dt_s;
            self.last_iterations = report.iterations;
            // The analyser reads the bus voltages that held over the step just taken.
            let voltages = self.operator().grid_phase_voltage_pu;
            let step_start_s = self.system.time_s() - report.dt_s;
            self.analyser.observe(step_start_s, report.dt_s, voltages);
            // The meter reads the same voltages, the energy the ledger booked at the
            // generator's bus port over the step, and the reactive power.
            let exported_j = grid_energy_before_j - self.boundary_energy_in_j("generator.grid");
            let reactive_var = self.generator_state().reactive_power_var;
            self.meter
                .observe(step_start_s, report.dt_s, voltages, exported_j, reactive_var);
            self.sample();
        }

        self.log_changes(&before_controller, &before_generator, &before_cabinet);
        Ok(())
    }

    /// Sample the accepted state for the controller and consume the pulses.
    fn sample(&mut self) {
        let shaft = self.shaft_state();
        let generator = self.generator_state();
        let cabinet = self.cabinet_state();
        let coolant_k = self.coolant_k();
        let mut shared = self.shared.borrow_mut();
        shared.sensed = Sensed {
            omega_rad_s: shaft.velocity_m_s,
            load_angle_rad: generator.load_angle_rad,
            breaker_closed: generator.breaker_closed,
            electric_power_w: generator.electric_power_w,
            out_of_step: generator.out_of_step,
            coolant_k,
            phase_current_a: generator.phase_current_a,
            negative_sequence_current_a: generator.negative_sequence_current_a,
            cabinet_trips: cabinet.trips,
            cabinet_tripped: cabinet.tripped,
            grid_healthy: cabinet.grid_healthy,
        };
        let operator = &mut shared.operator;
        operator.start = false;
        operator.stop = false;
        operator.emergency_stop = false;
        operator.acknowledge = false;
        operator.breaker_close = false;
        operator.breaker_open = false;
        operator.synchronise = false;
    }

    fn push_event(&mut self, kind: u32, code: u32, value: f64) {
        if self.events.len() == EVENT_LOG_CAPACITY {
            self.events.remove(0);
        }
        self.events.push(Event {
            time_s: self.system.time_s(),
            kind,
            code,
            value,
        });
    }

    fn log_changes(
        &mut self,
        before: &ControllerState,
        before_generator: &GeneratorState,
        before_cabinet: &CabinetState,
    ) {
        let after = self.controller_state();
        let generator = self.generator_state();
        let cabinet = self.cabinet_state();
        let rpm = self.shaft_state().velocity_m_s * 30.0 / core::f64::consts::PI;
        if cabinet.trips != before_cabinet.trips {
            // The protection's own account of its trip, in the units an operator reads.
            use crate::cabinet::protection as p;
            let value = match cabinet.last_function {
                p::UNDERVOLTAGE | p::OVERVOLTAGE => {
                    cabinet.last_value * self.generator_card.rated_phase_voltage_v()
                }
                p::VOLTAGE_UNBALANCE | p::NEGATIVE_SEQUENCE => 100.0 * cabinet.last_value,
                p::OVERCURRENT | p::OVERCURRENT_HIGH => {
                    cabinet.last_value * self.generator_card.rated_current_a()
                }
                p::REVERSE_POWER => cabinet.last_value * 1.0e-3,
                _ => cabinet.last_value,
            };
            self.push_event(
                event_kind::PROTECTION,
                10 * cabinet.last_function + cabinet.last_phase,
                value,
            );
        }
        if after.sequence != before.sequence {
            self.push_event(event_kind::SEQUENCE, after.sequence.index(), rpm);
        }
        if after.sequence.state_code() != before.sequence.state_code() {
            self.push_event(event_kind::STATE, after.sequence.state_code(), rpm);
        }
        if generator.breaker_closed != before_generator.breaker_closed {
            self.push_event(
                event_kind::BREAKER,
                u32::from(generator.breaker_closed),
                before_generator.electric_power_w * 1.0e-3,
            );
        }
        let (old_bits, new_bits) = (before.alarms | before.warnings, after.alarms | after.warnings);
        for bit in 0..crate::controller::alarm::COUNT {
            let mask = 1_u32 << bit;
            if new_bits & mask != 0 && old_bits & mask == 0 {
                self.push_event(event_kind::ALARM_RAISED, bit, 0.0);
            } else if new_bits & mask == 0 && old_bits & mask != 0 {
                self.push_event(event_kind::ALARM_CLEARED, bit, 0.0);
            }
        }
        if after.refusals != before.refusals {
            // Close requests are refused only while synchronising; elsewhere it was a start.
            let code = if before.sequence == crate::controller::Sequence::Synchronising {
                after.last_close_refusal
            } else {
                0
            };
            self.push_event(event_kind::REFUSED, code, 0.0);
        }
    }

    /// Record that a close request was refused before it reached the controller.
    pub fn log_refused_close(&mut self, bits: u32) {
        self.push_event(event_kind::REFUSED, bits, 0.0);
    }

    /// Record the operator's command to synchronise: accepted (`bits` zero)
    /// or refused, with the phase angle at that moment.
    pub fn log_synchronise_command(&mut self, bits: u32) {
        let phase_deg = self.synchro_check().phase_angle_rad.to_degrees();
        self.push_event(event_kind::SYNCHRONISE_COMMAND, bits, phase_deg);
    }

    /// The event log, oldest first.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Empty the event log.
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Accepted state of the gas valve.
    #[must_use]
    pub fn gas_valve_state(&self) -> GasValveState {
        *self
            .system
            .state::<GasValveState>(self.nodes.gas_valve)
            .expect("gas valve state")
    }

    /// Accepted state of the engine.
    #[must_use]
    pub fn engine_state(&self) -> GasEngineState {
        *self
            .system
            .state::<GasEngineState>(self.nodes.engine)
            .expect("engine state")
    }

    /// Accepted state of the shaft.
    #[must_use]
    pub fn shaft_state(&self) -> Motion {
        *self.system.state::<Motion>(self.nodes.shaft).expect("shaft state")
    }

    /// Accepted state of the starter.
    #[must_use]
    pub fn starter_state(&self) -> StarterState {
        *self
            .system
            .state::<StarterState>(self.nodes.starter)
            .expect("starter state")
    }

    /// Accepted state of the generator.
    #[must_use]
    pub fn generator_state(&self) -> GeneratorState {
        *self
            .system
            .state::<GeneratorState>(self.nodes.generator)
            .expect("generator state")
    }

    /// Accepted state of the controller.
    #[must_use]
    pub fn controller_state(&self) -> ControllerState {
        *self
            .system
            .state::<ControllerState>(self.nodes.controller)
            .expect("controller state")
    }

    /// Accepted state of the breaker cabinet's protection device.
    #[must_use]
    pub fn cabinet_state(&self) -> CabinetState {
        *self
            .system
            .state::<CabinetState>(self.nodes.cabinet)
            .expect("cabinet state")
    }

    /// Coolant temperature at the engine outlet [K]: the coolant node's state.
    #[must_use]
    pub fn coolant_k(&self) -> f64 {
        *self.system.state::<f64>(self.nodes.coolant).expect("coolant state")
    }

    /// Accepted state of the exhaust-gas heat exchanger.
    #[must_use]
    pub fn exhaust_hx_state(&self) -> ExhaustExchangerState {
        *self
            .system
            .state::<ExhaustExchangerState>(self.nodes.exhaust_hx)
            .expect("exhaust exchanger state")
    }

    /// Accepted state of the network heat exchanger.
    #[must_use]
    pub fn network_hx_state(&self) -> NetworkExchangerState {
        *self
            .system
            .state::<NetworkExchangerState>(self.nodes.network_hx)
            .expect("network exchanger state")
    }

    /// Accepted state of the emergency cooler.
    #[must_use]
    pub fn dump_cooler_state(&self) -> DumpCoolerState {
        *self
            .system
            .state::<DumpCoolerState>(self.nodes.dump_cooler)
            .expect("dump cooler state")
    }

    /// Gas volume flow at the meter's conditions [m3/s].
    #[must_use]
    pub fn meter_flow_m3_s(&self) -> f64 {
        self.meter_valve.meter_flow_m3_s(&self.gas_valve_state())
    }

    /// Gas volume flow at standard conditions [m3/s].
    #[must_use]
    pub fn standard_flow_m3_s(&self) -> f64 {
        self.meter_valve.standard_flow_m3_s(&self.gas_valve_state())
    }

    /// Energy [J] that entered through the boundary of `port` (`node.port`)
    /// since the model started, from the kernel's ledger.
    #[must_use]
    pub fn boundary_energy_in_j(&self, port: &str) -> f64 {
        self.system
            .ledger()
            .boundaries
            .iter()
            .filter(|account| account.port == port)
            .map(|account| account.energy_in_j.value())
            .sum()
    }

    /// Mass [kg] that entered through the stream boundary of `port`.
    #[must_use]
    pub fn boundary_mass_in_kg(&self, port: &str) -> f64 {
        self.system
            .ledger()
            .boundaries
            .iter()
            .filter(|account| account.port == port)
            .map(|account| account.mass_in_kg.value())
            .sum()
    }

    /// Heat [J] delivered to the named sink since the model started.
    #[must_use]
    pub fn sink_heat_j(&self, name: &str) -> f64 {
        self.system
            .ledger()
            .sinks
            .iter()
            .filter(|account| account.name == name)
            .map(|account| account.heat_j.value())
            .sum()
    }

    /// Bus voltage [V] on the 10 kV side.
    #[must_use]
    pub fn bus_voltage_v(&self) -> f64 {
        self.operator().grid_sequences().positive_magnitude() * BUS_VOLTAGE_V
    }
}

fn operator_signals(operator: &OperatorInputs) -> OperatorSignals {
    OperatorSignals {
        load_setpoint: operator.load_setpoint,
        start: operator.start,
        stop: operator.stop,
        emergency_stop: operator.emergency_stop,
        acknowledge: operator.acknowledge,
        breaker_close: operator.breaker_close,
        breaker_open: operator.breaker_open,
        auto_synchronise: operator.auto_synchronise,
        gas_gauge_pa: operator.gas_gauge_pa,
        oil_pressure_low: operator.oil_pressure_low,
        // The synchroniser's voltage is the positive sequence of the bus.
        grid_voltage_pu: operator.grid_sequences().positive_magnitude(),
        grid_frequency_hz: operator.grid_frequency_hz,
        excitation_pu: operator.excitation_pu,
        fan_failed: operator.fan_failed,
        load_ramp_per_s: operator.load_ramp_per_s,
        restoration_auto: operator.restoration_auto,
        synchronise: operator.synchronise,
    }
}
