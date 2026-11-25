# Gas-Piston Cogeneration Unit (`apps/kgu-cogen`)

A physics engine for a 1 000 kW gas-piston cogeneration unit (CHP genset), stage 1: gas admission, engine (brake side), shaft, synchronous generator on an infinite bus, genset controller and heat recovery. It is a description on the FrankenSim composition kernel (`fs-ports` Component contract **v0**, `fs-system`), built natively and to WebAssembly. There is no page here: the engine is the package a teaching control console loads.

The machine is the training station's conventional unit (1 000 kW electric, 1 500 rpm, 12 cylinders, 0.4 kV generator, 0.4/10 kV unit transformer). Its passport numbers are themselves invented and plausible, as their source says; they belong to no real machine.

The plant advances in fixed 10 ms sub-steps, the governor's sample period. The swing of the generator against the bus is about 4.6 Hz with the assumed inertia and reactances, so one period takes 21 sub-steps. `step(dt)` takes whole sub-steps and carries the remainder; a run is the same at any calling rate, and two runs of the same commands agree to the bit. Measured on the built package under Node 25: 2.0 ms of wall time per simulated second (20 µs per sub-step), so a console has some 500 times real time in hand.

## Quick start

Builds go through `rch` from the repository root (the owner's rule for this host); the engine has its own `[workspace]`.

```bash
# native tests
RCH_REQUIRE_REMOTE=1 rch exec -- cargo test --manifest-path apps/kgu-cogen/engine-kgu/Cargo.toml -j 8

# WebAssembly package (rch syncs the artifact into target/ at the repository root)
RCH_REQUIRE_REMOTE=1 RCH_BUILD_TIMEOUT_SEC=1500 rch exec -- cargo build \
  --manifest-path apps/kgu-cogen/engine-kgu/Cargo.toml -j 8 --target wasm32-unknown-unknown --release
cd apps/kgu-cogen && npm run wasm:bindgen   # wasm-bindgen 0.2.128, --target web, into wasm-pkg/
npm test                                    # Node tests of the package
```

## The plant

```text
gas supply -> gas valve ==fuel stream==> engine --+
                                                  |  one shaft speed
                 starter (battery) ---------------+-- shaft inertia
                 generator (10 kV bus) -----------+
controller --signals--> starter, gas valve, ignition, breaker

engine jacket, exhaust exchanger, plate exchanger, dump cooler -- one coolant temperature
plate exchanger -> district heating     dump cooler -> ambient air     exhaust exchanger -> stack
```

What is a state and therefore emerges: shaft speed and angle, the generator's load angle (also with the breaker open: it is the phase the synchroniser watches), the coolant temperature, the gas valve's lift, the governor's integral, the controller's sequence step, latches and timers, the gas meter's index.

What a run shows without being told (measured on 2026-11-25, default cards):

| Quantity | Value | Where it comes from |
| --- | --- | --- |
| Start command to breaker closed | 91 s | 80 s of controller timers; the rest is cranking to the firing speed (0.6 s), the run-up (8.6 s) and the wait for the phase (2.4 s here, up to 20 s) |
| Gas burned before the first kWh | 0.62 m³ | the run-up and the warm-up at the no-load fuel |
| No-load fuel at rated speed | 374 kW | indicated torque against friction and the generator's core loss |
| Full-load rejection | peak 1 559 rpm, settles at 1 501.5 rpm | inertia, valve travel and governor; trip is at 1 725 rpm |
| Reactive power at 1.00 pu excitation, full load | −194 kvar (cos φ 0.982, under-excited) | the machine equations; about 1.09 pu gives 0.97 lagging |
| Droop | +50 kW for −0.1 Hz | 4 % droop on measured power |
| Coast-down from rated speed | about 11 s | friction against the inertia |
| Coolant at full load, full heat demand | 88.3 °C out, 68.2 °C back | thermostat band against the network exchanger |
| Coolant with no heat demand, fans running | 93.0 °C | the dump cooler's band and its capacity |
| No heat demand, fans failed, full load | trip after 15 to 28 s | 1 MW that cannot leave, into 3.1 MJ/K |
| Exhaust | 485 °C before, 215 °C after the exchanger at full load | exhaust enthalpy over its flow; fitted exchanger length |

## Origin of Each Subsystem

Per FrankenSim Operating Rule 2. "Fitted" and "assumed" numbers are named in `engine-kgu/src/cards.rs`, each with its origin.

| Subsystem | Origin | FrankenSim crates / implementation |
| --- | --- | --- |
| **Coupling, interface solve, atomic step, conservation audit** | `core solve` | `fs-ports::component::{Component, PortSpec, Receipt}`; `fs-system::SystemBuilder` (`bond`, `common_flow`, `common_effort`, `heat_route`, `signal`, `signal_map`, `boundary`), `System::step`, `System::ledger` (first-law residual, boundary energy and mass, heat per sink). 11 component instances in one description |
| **Shaft inertia** | `core solve` | `fs_system::elements::Mass` in the rotational domain: speed and angle are its state, its step is the kernel's |
| **Coolant node and block loss** | `core solve` | `fs_system::elements::HeatCapacity` (the coolant temperature is its state) and `Conductance` to the engine room |
| **Gas admission** | `in-app model` | `GasControlValveComponent`: isentropic orifice flow of an ideal gas (Saint-Venant and Wantzel), rate-limited actuator, shut-off valve, meter at supply conditions. The fuel enters as an `fs-ports` stream whose enthalpy is measured from combustion products at 20 °C with water as vapour, so the ledger books lower heating power plus sensible heat. Gas properties are an assumed card (methane as an ideal gas); `fs-thermochem` is not called: the passport's heating value is an input |
| **Engine, brake side** | `in-app model`, **fitted** | `GasEngineComponent`: indicated power from fuel power by `IndicatedWorkCard`, **fitted** to the passport's electrical efficiency (30 %: 30.0, 50 %: 35.0, 75 %: 38.0, 90 %: 39.3, 100 %: 40.0 %) times the machine-condition factor (default 0.955) through this plant's own generator losses at 1.00 pu excitation. Source: the training station's dataset passport and simulator specification (`felectra-eip/sim/virtual-kgu/SPEC.md` §3.2, `vkgu/station.py`). Below 30 % load the passport is silent: the fuel line of the 30–50 % segment is continued to zero load (an assumption). Friction torque: its power at rated speed is fitted (the no-load fuel turned into work), its split between a constant and a speed-proportional part is assumed. Heat split (30 % of the non-work heat to the jacket, 2.5 % of fuel to the room) is assumed; the exhaust enthalpy is the first law's remainder |
| **Starter** | `in-app model` | `StarterMotorComponent`: series DC machine referred to the crankshaft, with an overrunning clutch; assumed card |
| **Synchronous generator, breaker, transformer reactance** | `core solve` + `in-app model` | `fs_phs::circuit::synchronous::SynchronousMotor::operating_point` gives torque and d-q currents of the classical machine (EMF behind the transient reactance plus the transformer's, 4 poles) at the flux linkages the stator voltage equations give against the bus. In the app: the load angle's integration, reactive power, terminal voltage, a lumped damper coefficient, core loss, the out-of-step trip. Per-unit values are assumed |
| **Governor** | `core solve` + `in-app model` | `fs_control::speed::SpeedController` (sampled PI with tracking anti-windup). In the app: its reference (run-up ramp, synchronising slip, 4 % droop on measured power, load ramp) and the conversion of its torque request into valve demand |
| **Sequencer, synchroniser, protection** | `in-app model` | `GensetControllerComponent`, a Component with signal ports only. Every transition is taken on a measured condition or on one of the controller's own timers. The timers, ramps, thresholds and gains are **controller setpoints** (`ControllerSetpoints`): parameters of a device, not results. The synchronisation window (5 %, 0.10 Hz, 10°) is assumed: **to be verified against the applicable grid code**. Pre-lubrication (30 s), purge (5 s), warm-up (45 s), cool-down (300 s): **to be verified against the controller documentation of a real machine** |
| **Exhaust-gas heat exchanger** | `core solve` + `in-app model`, **one fitted length** | Gas-side film from `fs_convection::evaluate(CorrelationId::Gnielinski)`, tabulated once; effectiveness-NTU in `ExhaustHeatExchangerComponent`. Tube count and bore assumed; the tube length (1.74 m) is **fitted** so that at the rated point and 90 °C coolant the recovered heat plus the jacket heat is the passport's thermal recovery, 43 % of fuel (same source, `ETA_TH`). At other loads the recovery is computed |
| **Plate exchanger to the district-heating network** | `in-app model` | `NetworkHeatExchangerComponent`: counterflow effectiveness-NTU on an assumed `UA`, behind a thermostatic valve (84 to 88 °C, assumed). The demand input is the share of the network's rated water flow |
| **Emergency (dump) cooler** | `core solve` + `in-app model` | Air-side film from `fs_convection::evaluate(CorrelationId::ChurchillBernsteinCylinder)` at the fan's air speed and at the natural draught's; effectiveness-NTU in `DumpCoolerComponent`. Geometry and its valve band (91 to 94 °C) assumed |
| **Operator inputs, fault inputs** | `scripted` (inputs only) | Load setpoint, excitation, heat demand, network return temperature, gas pressure and temperature, bus voltage and frequency, fan failure, low oil pressure, condition factor. They enter through kernel boundaries between sub-steps; every response is computed |
| **Snapshot, limits, event log** | `display only` | Flat arrays read from the accepted states and the ledger. Counters (kWh, m³) are the ledger's own accounts; the gas meter's index and the engine hours are Component states |

### What is fitted, and what the tests then check

Fitted to the passport: the electrical efficiency against load (exactly, at 1.00 pu excitation), and the thermal recovery at rated load (through one tube length). Checked, with tolerances stated in the tests: electrical efficiency at 100 % and 50 % within 0.3 points of passport × 0.955 (the plant gives it to four digits); thermal recovery at 100 % within 1.5 points (42.75 % against 43.0 %).

Not fitted, and compared with the passport all the same: thermal recovery is 45.2 % against 45.0 % at 75 % load, 48.7 % against 47.0 % at 50 %, and **53.7 % against 49.0 % at 30 %**. The low-load overshoot is the model's defect: a constant wall-heat share and a friction power that does not fall with load put too much heat into the jacket there.

Away from 1.00 pu excitation the electrical efficiency moves with the generator's losses (more stator current, more core flux); that is computed.

## Not Modelled

- The in-cylinder cycle: no pressure trace, no combustion, no air path, turbocharger, mixture or knock. Brake behaviour is the fitted card. Cylinders are a number. Per-cylinder exhaust temperatures and misfire are absent.
- Mass balance. Air and exhaust are not streams of the description, so the kernel's mass account is not claimed (`mass_residual_kg()` is `None`); only the fuel's mass is counted, at the supply boundary. The energy account is complete.
- Entropy of the fuel stream and of combustion (contract v0 carries no stream entropy). Components book the entropy of the heat they release.
- The oil circuit. Low oil pressure is an input that trips after its delay; pre-lubrication is a timer.
- The standby heater that keeps the block at 60 °C: only the starting temperature.
- Thermal masses of the exchangers, the coolant's transport delay round the circuit, the heating network's own inertia. With them missing, a failed cooler trips the unit in seconds to a minute and a half where a real plant takes minutes.
- Stator and field transients, the voltage regulator, saturation, sub-transient currents, the transformer's losses and tap changer. Excitation is the EMF directly. The breaker closes and opens at once. A manual close at the edge of the window gives the jolt the equations give; no closing-time compensation.
- Grid protection beyond under-voltage: no frequency, vector-shift or reverse-power relays.
- Gas train beyond one valve: no regulator, no filter, no leak test. Compressibility is one.
- Events are taken at sub-step boundaries (10 ms); the kernel's event location is not used.
- Acoustics, vibration, fields, the design optimiser: later slices.

## Kernel limits met

- A component inside a looped block must be continuous and have a slope on both sides wherever the solve can land. A step function, or a corner with a flat side, stalls the kernel's Newton search (`did not converge`), because its backtracking cannot step across. Found three times here and removed in the app: an exchanger that switched on with the flow, an engine law that branched on "fuel flows", a piecewise-linear thermostat. The laws are now smooth where the solve iterates and branch only on signals held over a step.
- The controller is therefore outside the loop: it reads measurements sampled from the accepted state after each sub-step (start-of-step feedback) and drives the plant through signals. Wiring its inputs as kernel signals from the plant would put a state machine inside the shaft's Newton block.
- No synchronous-machine Component in `fs-phs` behind `fs-ports`, no droop, sequencer or synchroniser in `fs-control`, no signal-modulated conductance among the kernel's elements: those are the in-app models above.

## The WebAssembly boundary

`KguSim` (see `wasm-pkg/fs_kgu_cogen.d.ts`): `new()`; setters `set_load_setpoint_pct`, `set_excitation_pu`, `set_heat_demand_frac`, `set_network_return_temp_c`, `set_gas_gauge_kpa`, `set_gas_temp_c`, `set_grid_voltage_pu`, `set_grid_freq_hz`, `set_fan_failed`, `set_oil_pressure_low`, and beyond the first contract `set_auto_sync`, `set_atm_pressure_kpa`, `set_condition_factor`, `set_load_ramp_pct_per_min`; commands `cmd_start`, `cmd_stop`, `cmd_emergency_stop`, `cmd_ack`, `cmd_breaker_close` (returns 0 when accepted, else refusal bits), `cmd_breaker_open`; `step(dt_s)`; `snapshot()`, `limits()`, `events()`, `machine_facts()`, `last_error()`.

`snapshot()` is a flat `Float64Array`. Its layout is versioned in slot 0 and named by the engine (`snapshot_field_names()`); `snapshotLayout.mjs` mirrors it with index constants, state codes, alarm bits, refusal bits and event kinds, and a test fails when the two disagree. A slot with nothing to show holds NaN.

The engine returns numbers and field names only. The station controller's event codes (A-105 high coolant temperature, A-150 low gas pressure, A-160 low oil pressure, A-031 mains under-voltage, W-210 failed start, W-118 cooler fan) are bit positions in `ALARM_BITS` and appear as text in comments and in this file, never in a string the package returns; a Node test checks the package for them.

Protection as modelled: A-105 trips when the coolant state reaches 98 °C; A-150 when the gas valve is fully open, the power falls short of the load reference and the supply is below the pressure switch, for 5 s; A-160 and A-031 are input-driven with their delays; over-speed at 115 %; out-of-step on a pole slip; W-210 when a start does not reach rated speed in 30 s of cranking and run-up.

## Tests

Native (`engine-kgu/src/tests.rs`, `cards.rs`): 30 tests. Every in-app Component passes `fs_ports::component::conformance::check_component` at several operating points (purity, the first-law audit, admission, cancellation). Scenario tests each check a quantity that was not an input: steady 100 % and 50 % load; droop; full-load rejection; synchronisation refused outside the window and accepted inside; excitation against reactive power; start from standstill; a start without gas; normal stop (cool-down burns gas, coast-down burns none, to the bit); emergency stop and acknowledgement; low gas pressure; oil pressure and under-voltage; heat to the network; heat to the dump cooler; fan failure to the coolant trip; partial demand; bit-for-bit repeatability. The ledger's relative residual stays below 1e-9 in every scenario that checks it (measured: 1e-14).

Node (`test/kguEngine.test.mjs`): the same scenarios against the built package, plus the layout check and the check that no event code is in the package.
