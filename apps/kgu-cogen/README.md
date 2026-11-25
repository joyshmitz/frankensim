# Gas-Piston Cogeneration Unit (`apps/kgu-cogen`)

A physics engine for a 1 000 kW gas-piston cogeneration unit (CHP genset), stage 1: gas admission, engine (brake side), shaft, synchronous generator on an infinite three-phase bus, the generator-breaker cabinet (breaker, protection, power-quality analyser), genset controller and heat recovery. It is a description on the FrankenSim composition kernel (`fs-ports` Component contract **v0**, `fs-system`), built natively and to WebAssembly. There is no page here: the engine is the package a teaching control console loads.

The machine is the training station's conventional unit (1 000 kW electric, 1 500 rpm, 12 cylinders, 0.4 kV generator, 0.4/10 kV unit transformer). Its passport numbers are themselves invented and plausible, as their source says; they belong to no real machine.

The plant advances in fixed 10 ms sub-steps, the governor's sample period. The swing of the generator against the bus is about 4.6 Hz with the assumed inertia and reactances, so one period takes 21 sub-steps. `step(dt)` takes whole sub-steps and carries the remainder; a run is the same at any calling rate, and two runs of the same commands agree to the bit. Measured on the built package under Node 25: 2.0 ms of wall time per simulated second (20 µs per sub-step), so a console has some 500 times real time in hand.

## Quick start

Builds go through `rch` from the repository root (the owner's rule for this host); the engine has its own `[workspace]`.

```bash
# native tests
RCH_REQUIRE_REMOTE=1 rch exec -- cargo test --locked --manifest-path apps/kgu-cogen/engine-kgu/Cargo.toml -j 8

# WebAssembly package (rch syncs the artifact into target/ at the repository root)
RCH_REQUIRE_REMOTE=1 RCH_BUILD_TIMEOUT_SEC=1500 rch exec -- cargo build --locked \
  --manifest-path apps/kgu-cogen/engine-kgu/Cargo.toml -j 8 --target wasm32-unknown-unknown --release
cd apps/kgu-cogen && npm run wasm:bindgen   # wasm-bindgen 0.2.128, --target web, into wasm-pkg/
npm test                                    # Node tests of the package
```

## The plant

```text
gas supply -> gas valve ==fuel stream==> engine --+
                                                  |  one shaft speed
                 starter (battery) ---------------+-- shaft inertia
                 generator (stiff bus, three phase voltages) ---+
controller --signals--> starter, gas valve, ignition, close request and reset to the cabinet
cabinet (protection) --signal--> generator breaker      analyser, meter: read the bus, switch nothing

engine jacket, exhaust exchanger, plate exchanger, dump cooler -- one coolant temperature
plate exchanger -> district heating     dump cooler -> ambient air     exhaust exchanger -> stack
```

What is a state and therefore emerges: shaft speed and angle, the generator's load angle (also with the breaker open: it is the phase the synchroniser watches), the coolant temperature, the gas valve's lift, the governor's integral, the controller's sequence step, latches and timers, the gas meter's index, the cabinet's timers, latches and the rotor's thermal replica.

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

With an unbalanced bus (measured on 2026-11-26 on the built package, default cards; every setting named here is a card value to be verified, see below):

| Quantity | Value | Where it comes from |
| --- | --- | --- |
| One phase at 0.73 pu (168.6 V), unit exporting 900 kW | U2/U1 9.89 %; I2 34.0 % of rated; phase currents 1 922 / 1 785 / 874 A against 1 320 A before; generator losses 42.2 kW against 27.4 kW, of which 11.6 kW from the negative sequence | symmetrical components of the three voltages; the negative-sequence voltage over the assumed `R2 + jX2` |
| The same case: what trips | phase under-voltage on phase b, 0.15 s after the phase fell, at 168.6 V; unbalance and the rotor's replica had picked up too and were slower | the cabinet's functions against their settings |
| After that trip | breaker open, 1 500 rpm, 374 kW of fuel, waiting | grid events do not stop the engine |
| Phases 1.00 / 1.00 / 0.88 at 800 kW | U2/U1 4.17 %; I2 15.1 %; trip by the rotor's limit after 876.3 s | `(I2/In)² t = 20 s` integrated: 20 / 0.151² = 876.25 s |
| 1.00 / 1.00 / 0.90 and 1.00 / 1.00 / 0.86 | I2 12.6 % and 17.6 %; trip after 1 261.8 s and 643.8 s | the same integral |
| 1.10 / 1.00 / 0.88 | U2/U1 6.40 %; the unbalance function trips after 5.0 s | its own setting; the rotor's replica would have needed 347 s |
| 1.00 / 1.00 / 0.94 at 800 kW | U2/U1 2.04 %; I2 7.55 % (below the 8 % continuous level); currents 1 058 / 1 293 / 1 206 A; 0.57 kW of extra heat; runs on | as above |
| Automatic return after a 2 s disturbance, from 500 kW | back on the bus 60.15 s after the grid recovered, at the setpoint 10 s later (ramp 300 %/min); 0.67 m³ of gas burned unloaded | 60 s healthy-grid delay; then the synchroniser needs the phase, which the load rejection left where it left it: 0.15 s from 30 % and 50 % load, 11.2 s from 90 % (158° to come round at 0.05 Hz slip) |
| Automatic mode, disturbance stays | normal stop begins 600.2 s after the phase fell; 6.70 m³ burned waiting; ready 312 s later | longest wait, then cool-down and coast-down |
| Automatic mode, grid falls again after each return | three returns, the fourth trip blocks the automation until acknowledged | attempts within the window |
| Weak field (0.50 pu) at full load | 3 811 A, 2 445 kvar absorbed; over-current trips after 10 s and stops the engine | the machine equations against the 120 % stage |

Two instruments, one event (measured on 2026-11-26 on the built package; the meter's card is assumed, see below):

| Quantity | Value | Where it comes from |
| --- | --- | --- |
| A 130 ms dip to 160 V on one phase, unit exporting 500 kW, the meter's look half a second away | analyser: one dip, 0.13 s, 160 V. Meter: no record. Breaker stays closed (the protection needs 0.15 s) | the meter looks at the voltages once per second of its own clock and at nothing in between |
| The same dip, the meter's look 65 ms into it | analyser: the same. Meter: one low-voltage record on that phase, seen at its look, seen gone at the next look one second later; nothing in it says 160 V or 130 ms | the same mechanism |
| The same dip at a hundred places of the look within the second | in the journal at 13 of them | 0.13 s of every second |
| A 3 s dip, analyser's clock 100 s and meter's clock 250 s ahead, looks at 0.4 s past the second | analyser stamps the start (230.98); the meter its first look after it (381.40, which is 0.42 s late by model time) and its fourth (384.40) | each instrument's own clock and way of measuring |
| 101 low-voltage events | the journal holds 100; the first record is displaced; overflow flag set. The analyser has 101 | capacity and overflow behaviour of the card |
| A quarter of an hour at 1 000 kW, 1.00 pu excitation | active export 250.000 kWh, the engine's counter to the last digit; active import unchanged; reactive import 48.4 kvarh, reactive export unchanged. At 1.20 pu (646 kvar delivered) the reactive energy goes to the export register | the ledger's energy at the bus port by its sign; the machine's reactive power by its sign |
| From the start until the unit has settled at full load | 0.0002 kWh in the import register, 0.0001 kvarh in the reactive export register | sub-steps in which the power was the other way; presumably at the closing, not traced further |
| Command to synchronise at 88° of phase, manual return | breaker closes by itself 15.1 s later at 1.0° | the phase comes round at the 0.05 Hz slip; the synchroniser closes inside 1° |

## Origin of Each Subsystem

Per FrankenSim Operating Rule 2. "Fitted" and "assumed" numbers are named in `engine-kgu/src/cards.rs`, each with its origin.

| Subsystem | Origin | FrankenSim crates / implementation |
| --- | --- | --- |
| **Coupling, interface solve, atomic step, conservation audit** | `core solve` | `fs-ports::component::{Component, PortSpec, Receipt}`; `fs-system::SystemBuilder` (`bond`, `common_flow`, `common_effort`, `heat_route`, `signal`, `signal_map`, `boundary`), `System::step`, `System::ledger` (first-law residual, boundary energy and mass, heat per sink). 12 component instances in one description |
| **Shaft inertia** | `core solve` | `fs_system::elements::Mass` in the rotational domain: speed and angle are its state, its step is the kernel's |
| **Coolant node and block loss** | `core solve` | `fs_system::elements::HeatCapacity` (the coolant temperature is its state) and `Conductance` to the engine room |
| **Gas admission** | `in-app model` | `GasControlValveComponent`: isentropic orifice flow of an ideal gas (Saint-Venant and Wantzel), rate-limited actuator, shut-off valve, meter at supply conditions. The fuel enters as an `fs-ports` stream whose enthalpy is measured from combustion products at 20 °C with water as vapour, so the ledger books lower heating power plus sensible heat. Gas properties are an assumed card (methane as an ideal gas); `fs-thermochem` is not called: the passport's heating value is an input |
| **Engine, brake side** | `in-app model`, **fitted** | `GasEngineComponent`: indicated power from fuel power by `IndicatedWorkCard`, **fitted** to the passport's electrical efficiency (30 %: 30.0, 50 %: 35.0, 75 %: 38.0, 90 %: 39.3, 100 %: 40.0 %) times the machine-condition factor (default 0.955) through this plant's own generator losses at 1.00 pu excitation. Source: the training station's dataset passport and simulator specification (`felectra-eip/sim/virtual-kgu/SPEC.md` §3.2, `vkgu/station.py`). Below 30 % load the passport is silent: the fuel line of the 30–50 % segment is continued to zero load (an assumption). Friction torque: its power at rated speed is fitted (the no-load fuel turned into work), its split between a constant and a speed-proportional part is assumed. Heat split (30 % of the non-work heat to the jacket, 2.5 % of fuel to the room) is assumed; the exhaust enthalpy is the first law's remainder |
| **Starter** | `in-app model` | `StarterMotorComponent`: series DC machine referred to the crankshaft, with an overrunning clutch; assumed card |
| **Synchronous generator, breaker, transformer reactance** | `core solve` + `in-app model` | `fs_phs::circuit::synchronous::SynchronousMotor::operating_point` gives torque and d-q currents of the classical machine (EMF behind the transient reactance plus the transformer's, 4 poles) at the flux linkages the stator voltage equations give against the bus. In the app: the load angle's integration, reactive power, terminal voltage, a lumped damper coefficient, core loss, the out-of-step trip. Per-unit values are assumed |
| **Three-phase bus** | `scripted` (inputs only) | Three phase voltages (and, by a separate call, three angle offsets) are a scenario input: a grid disturbance is an outside circumstance, not something the plant computes. `cabinet::sequence_components` turns them into positive and negative sequence; the positive sequence is the bus voltage of the machine equations, so three equal phases are the earlier single-voltage model to the bit |
| **Negative sequence in the generator** | `in-app model`, **assumed** values | `I2 = V2 / (R2 + jX2)` with `X2` 0.20 pu and `R2` 0.05 pu of the generator card (assumed, typical of a salient-pole machine with a damper cage; **to be verified against the generator's data sheet**) plus the transformer's reactance. It gives the three phase currents, `3 R I2²` of stator heat and `3 Rr I2²` of rotor heat with `Rr = 2 (R2 − R)`; half of the rotor's heat comes from the bus and half from the shaft as a braking torque (the textbook split at slip 2). All of it leaves through the generator's heat port, so the ledger books it. Why not the kernel: `fs_phs::circuit::synchronous` has stator and field only, no damper or rotor-body circuit, and the double-frequency rotor currents are the whole subject; its α-β step would also need sub-steps a hundred times shorter than the plant's 10 ms and carries its own rotor inertia, which the kernel's shaft already owns |
| **Breaker cabinet: protection** | `in-app model` | `BreakerCabinetComponent`, a Component with signal ports only, between the controller's close request and the generator's breaker. Functions: phase under-voltage, phase over-voltage, voltage unbalance (U2/U1), negative-sequence current as a thermal replica `(I2/In)² t` with cooling, over-current in two stages, under- and over-frequency, reverse power. Each is a level and a delay in `BreakerCabinetCard`: **settings of a device, to be verified against the cabinet documentation and the grid code**. A trip latches until reset; the event names function, phase and value as numbers |
| **Restoration after a grid event** | `in-app model` | In the controller (`Sequence::GridWait`). Grid events (voltage, unbalance, negative sequence, frequency) open the breaker and leave the engine running unloaded; unit faults (over-current, reverse power) stop it. Manual mode (default): nothing happens until the operator acknowledges; then the unit stands at the synchroniser and the operator stops it or brings it back in one of two ways. *Manual synchronisation*: the operator closes the breaker with `cmd_breaker_close()` inside the window, by the synchroscope; outside the window the request is refused with the reasons. *Synchronisation on command*: `cmd_synchronise()` — the person decides, the automatic synchroniser carries it out and closes near zero phase, as in an automatic start, for that one attempt; a stop cancels the wait. The command is refused (same bits) when the unit is not at the synchroniser or a trip is latched, so not before the acknowledgement. Accepted or refused, it is an event of its own in the log: a closing with a command before it was a person's decision, one without was the automation's (automatic return, automatic start) or a manual close request. The same command serves a start with the automatic synchronisation switched off, and a unit whose breaker was opened by hand. Automatic mode: the grid must be healthy for a delay without a break, then the synchroniser closes and the load returns on its ramp; after the longest wait a normal stop; after too many returns in a window a lockout until acknowledged. `RestorationCard`: **to be verified** likewise |
| **Power-quality analyser** | `display only` | `PowerQualityAnalyser`: an instrument of the plant, not a Component. It reads the bus's phase voltages after each accepted sub-step, switches nothing and feeds nothing back. It keeps its own record of dips, swells and interruptions per phase (start, duration, residual or highest voltage) by its own clock, which may be set apart from model time. Thresholds (90 %, 110 %, 5 %, hysteresis 2 %) in `PowerQualityCard`: **to be verified against the current edition of the power-quality standard** |
| **Revenue meter** | `display only` | `RevenueMeter`: a revenue meter with one-second threshold supervision and capacity-limited journals; like the analyser an instrument of the plant, not a Component. *Energy*: four registers (active exported and imported, reactive exported and imported). The active ones take the energy the kernel's ledger books at the generator's bus port over each sub-step, by its sign; the reactive ones the generator's reactive power times the sub-step. *Voltage supervision*: at `n × period + phase` of its own clock the meter reads the three phase voltages as they are in the sub-step that holds that instant, and nothing in between. A phase below the low level, above the high level or below the loss level opens a record in that journal; the first later look with no phase in the condition closes it. A record is: journal, phases seen, the look that saw it, the look that saw it gone. Whether a short disturbance is recorded follows from where the looks fall, not from any rule about its length. `MeterCard` (period 1 s; levels 90 %, 110 %, 5 %; 100, 100 and 20 records; a full journal displaces its oldest record): **assumed; to be verified against the meter's documentation** |
| **Governor** | `core solve` + `in-app model` | `fs_control::speed::SpeedController` (sampled PI with tracking anti-windup). In the app: its reference (run-up ramp, synchronising slip, 4 % droop on measured power, load ramp) and the conversion of its torque request into valve demand |
| **Sequencer, synchroniser, engine protection** | `in-app model` | `GensetControllerComponent`, a Component with signal ports only. It keeps the protections that stop the engine (coolant, oil, gas pressure, over-speed, out-of-step, emergency stop); the grid-side functions are the cabinet's. Every transition is taken on a measured condition or on one of the controller's own timers. The timers, ramps, thresholds and gains are **controller setpoints** (`ControllerSetpoints`): parameters of a device, not results. The synchronisation window (5 %, 0.10 Hz, 10°) is assumed: **to be verified against the applicable grid code**. Pre-lubrication (30 s), purge (5 s), warm-up (45 s), cool-down (300 s): **to be verified against the controller documentation of a real machine** |
| **Exhaust-gas heat exchanger** | `core solve` + `in-app model`, **one fitted length** | Gas-side film from `fs_convection::evaluate(CorrelationId::Gnielinski)`, tabulated once; effectiveness-NTU in `ExhaustHeatExchangerComponent`. Tube count and bore assumed; the tube length (1.74 m) is **fitted** so that at the rated point and 90 °C coolant the recovered heat plus the jacket heat is the passport's thermal recovery, 43 % of fuel (same source, `ETA_TH`). At other loads the recovery is computed |
| **Plate exchanger to the district-heating network** | `in-app model` | `NetworkHeatExchangerComponent`: counterflow effectiveness-NTU on an assumed `UA`, behind a thermostatic valve (84 to 88 °C, assumed). The demand input is the share of the network's rated water flow |
| **Emergency (dump) cooler** | `core solve` + `in-app model` | Air-side film from `fs_convection::evaluate(CorrelationId::ChurchillBernsteinCylinder)` at the fan's air speed and at the natural draught's; effectiveness-NTU in `DumpCoolerComponent`. Geometry and its valve band (91 to 94 °C) assumed |
| **Operator inputs, fault inputs** | `scripted` (inputs only) | Load setpoint, excitation, heat demand, network return temperature, gas pressure and temperature, the bus's phase voltages and frequency, fan failure, low oil pressure, condition factor. They enter through kernel boundaries between sub-steps; every response is computed |
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
- Where the three voltages are. They are the voltages of the stiff bus, in per unit of 230.9 V phase to neutral. The unit transformer is a reactance lumped into the machine and an ideal ratio: its winding connection, which on a real plant changes how a single-phase fault upstream looks on the 0.4 kV side, is absent, and the cabinet measures the bus, not a terminal voltage that differs from it by the drop over the transformer.
- Zero sequence: no neutral current, no earth fault, no short circuit of any kind. The three inputs are voltages, never faults; the over-current stages answer only to what the machine equations give (a weak field, an overload).
- The 100 Hz torque ripple and vibration that go with a negative-sequence current; the rotor's temperature itself (the replica counts `(I2/In)² t` against a limit, as the relay does); the change of `R2` and `X2` with frequency and saturation.
- Protection functions not in the cabinet: vector shift, rate of change of frequency, differential, earth fault, voltage-dependent over-current, loss of excitation as a function of its own (a weak field shows up as over-current). Definite times only, no inverse curves. No breaker-failure logic; the breaker opens in the sub-step of the trip.
- Which cabinet functions stop the engine: over-current (both stages) and reverse power are treated as faults of the unit and stop it; voltage, unbalance, negative sequence and frequency are grid events and leave it running unloaded. A real over-current can also be a through-fault of the grid; that distinction needs fault modelling, which is absent.
- In the analyser: harmonics, flicker, transients shorter than the 10 ms sub-step, RMS over the standard's half-cycle windows and its averaging intervals (10 min and the like), frequency and unbalance statistics. Harmonics and the standard's weekly assessment, in both directions (what the unit delivers and what it receives), are planned as separate models; the engine solves the fundamental only. An event's start and duration are whole sub-steps. Its measurements of current, power and frequency are the snapshot's own; it adds only the event record and its clock.
- In the meter: its accuracy class and any error of measurement (its energy is the engine's own, and its voltage at a look is the bus voltage exactly); current and voltage transformers; tariff zones, load profiles, demand maxima; the minimum, maximum and exact duration of a voltage event, which such a meter does not know; hysteresis on its levels (none); apparent energy. The closing instant of a record is kept, as a look of the meter's clock; a meter that keeps only the opening instant would show less. What a full journal does (displace the oldest record) is an assumption of the card. The meter reads the bus side of the breaker, like the analyser.
- Gas train beyond one valve: no regulator, no filter, no leak test. Compressibility is one.
- Events are taken at sub-step boundaries (10 ms); the kernel's event location is not used.
- Acoustics, vibration, fields, the design optimiser: later slices.

## Kernel limits met

- A component inside a looped block must be continuous and have a slope on both sides wherever the solve can land. A step function, or a corner with a flat side, stalls the kernel's Newton search (`did not converge`), because its backtracking cannot step across. Found three times here and removed in the app: an exchanger that switched on with the flow, an engine law that branched on "fuel flows", a piecewise-linear thermostat. The laws are now smooth where the solve iterates and branch only on signals held over a step.
- The controller is therefore outside the loop: it reads measurements sampled from the accepted state after each sub-step (start-of-step feedback) and drives the plant through signals. Wiring its inputs as kernel signals from the plant would put a state machine inside the shaft's Newton block.
- The cabinet's protection is outside the loop for the same reason: thresholds and latches. It reads the bus voltages and frequency as set, and the currents and power of the last accepted sub-step, and reaches the breaker by a forward signal in the sub-step in which a timer runs out. The controller learns of the trip one sub-step (10 ms) later.
- No rotor circuit in the `fs-phs` synchronous machine (stator and field only), so nothing in the kernel computes negative-sequence rotor heating; no protection relay or sequence-component element in `fs-control`.
- No synchronous-machine Component in `fs-phs` behind `fs-ports`, no droop, sequencer or synchroniser in `fs-control`, no signal-modulated conductance among the kernel's elements: those are the in-app models above.

## The WebAssembly boundary

`KguSim` (see `wasm-pkg/fs_kgu_cogen.d.ts`): `new()`; setters `set_load_setpoint_pct`, `set_excitation_pu`, `set_heat_demand_frac`, `set_network_return_temp_c`, `set_gas_gauge_kpa`, `set_gas_temp_c`, `set_grid_voltage_pu`, `set_grid_freq_hz`, `set_fan_failed`, `set_oil_pressure_low`, and beyond the first contract `set_auto_sync`, `set_atm_pressure_kpa`, `set_condition_factor`, `set_load_ramp_pct_per_min`; commands `cmd_start`, `cmd_stop`, `cmd_emergency_stop`, `cmd_ack`, `cmd_breaker_close` (returns 0 when accepted, else refusal bits), `cmd_breaker_open`; `step(dt_s)`; `snapshot()`, `limits()`, `events()`, `machine_facts()`, `last_error()`.

For the three-phase bus and the cabinet: `set_grid_phase_voltages_pu(a, b, c)` (each 0 to 1.3 of the rated phase voltage; `set_grid_voltage_pu(x)` sets all three to `x`), `set_grid_phase_angles_deg(a, b, c)` (offsets from the balanced positions), `set_restoration_mode(0 | 1)` (manual, the default, or automatic), `set_analyser_clock_offset_s(s)`, `analyser_events()` (five numbers per closed event: kind, phase, start by the analyser's clock, duration, residual or highest voltage in volts) and `clear_analyser_events()`. A cabinet trip is an event of kind 7 in `events()`, in model time: its code is ten times the function's index plus the phase (1 to 3, or 0), its value what it tripped at. `cmd_ack()` also resets the cabinet's latch. The 19 snapshot slots and 34 limits added for this are appended after the earlier ones, whose order is unchanged; the layout version stays 1.

For the meter: `set_meter_clock_offset_s(s)` (how far its clock is ahead of model time), `set_meter_sample_phase_s(s)` (where within its period it looks at the voltages; taken modulo the period), `meter_events()` (four numbers per record: journal 1 low voltage, 2 high voltage, 3 loss of voltage; phases seen as bits; the look that saw the condition and the look that saw it gone, NaN until then, both by the meter's clock; the low-voltage journal first, then high, then loss, oldest first within each) and `clear_meter_events()` (journals and overflow flags, not the registers). For synchronisation on command: `cmd_synchronise()` (returns 0 when accepted, else the refusal bits of `cmd_breaker_close`: 16 not synchronising, 32 a trip is latched); it is an event of kind 8 in `events()` whose code is that return value and whose value is the phase angle at the command. Ten more snapshot slots (four registers, the meter's clock, the three journals' record counts, the overflow bits, `SYNCHRONISER_ARMED_BY_OPERATOR`) and eight more limits (the meter's card) are appended in the same way.

`snapshot()` is a flat `Float64Array`. Its layout is versioned in slot 0 and named by the engine (`snapshot_field_names()`); `snapshotLayout.mjs` mirrors it with index constants, state codes, alarm bits, refusal bits, event kinds, protection functions, the meter's journals and decoders for the three records, and a test fails when the two disagree. A slot with nothing to show holds NaN.

The engine returns numbers and field names only. The station controller's event codes (A-105 high coolant temperature, A-150 low gas pressure, A-160 low oil pressure, A-031 mains under-voltage, W-210 failed start, W-118 cooler fan) are bit positions in `ALARM_BITS` and appear as text in comments and in this file, never in a string the package returns; a Node test checks the package for them.

Protection as modelled: A-105 trips when the coolant state reaches 98 °C; A-150 when the gas valve is fully open, the power falls short of the load reference and the supply is below the pressure switch, for 5 s; A-160 is input-driven with its delay; A-031 is the cabinet's phase under-voltage (any phase below 0.85 pu for 0.15 s), which since slice 1c opens the breaker and no longer stops the engine; over-speed at 115 %; out-of-step on a pole slip; W-210 when a start does not reach rated speed in 30 s of cranking and run-up.

## Tests

Native (`engine-kgu/src/tests.rs`, `cards.rs`, `analyser.rs`, `meter.rs`): 55 tests. Every in-app Component passes `fs_ports::component::conformance::check_component` at several operating points (purity, the first-law audit, admission, cancellation). Scenario tests each check a quantity that was not an input: steady 100 % and 50 % load; droop; full-load rejection; synchronisation refused outside the window and accepted inside; excitation against reactive power; start from standstill; a start without gas; normal stop (cool-down burns gas, coast-down burns none, to the bit); emergency stop and acknowledgement; low gas pressure; oil pressure and under-voltage; heat to the network; heat to the dump cooler; fan failure to the coolant trip; partial demand; bit-for-bit repeatability. The ledger's relative residual stays below 1e-9 in every scenario that checks it (measured: 1e-14).

For the three-phase bus and the cabinet: sequence components; the generator's negative-sequence current and where its heat comes from; every protection function at its setting and the `1 / I2²` law of the rotor's replica; a balanced bus against the numbers recorded before the bus had three phases; one phase at 0.73 pu; all three at 0.73 pu; unbalance without low voltage; a small unbalance that runs on; automatic restoration (return, lasting disturbance, flickering grid); manual restoration; over-current as a unit fault; the analyser's record and clock.

For the meter and the commanded synchronisation: a 130 ms dip between two looks (analyser one event, meter none, breaker closed) and with a look inside it (a record without depth or duration); the share of a hundred sample phases that catch it (13); a 3 s dip in both records by two clocks; the 101st event against the journal's capacity, and both overflow behaviours at the instrument; the four registers against the engine's counter and the sign of the reactive power; manual return with no command, with the command, with the command before the acknowledgement, with a stop after the command; a start without automatic synchronisation, with the command and with a manual closing.

Node (`test/kguEngine.test.mjs`): 29 tests, the same scenarios against the built package, plus the layout check and the check that no event code is in the package. The balanced-bus test holds the package to the recorded numbers to 1e-12 (they are reproduced digit for digit); natively the same numbers are met to 1e-7: the native build differs from the package in the ninth digit (1000.000003 kW against 1000.000001 kW), which was not investigated and is presumed to come from the two targets' elementary functions.
