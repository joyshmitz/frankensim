// Node tests of the compiled release WebAssembly package (wasm-pkg/). Each scenario
// checks at least one quantity that was not an input. The same scenarios run natively
// in engine-kgu/src/tests.rs; here they prove the package the console loads.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import initWasm, { KguSim } from "../wasm-pkg/fs_kgu_cogen.js";
import {
  ALARM,
  CLOSE_REFUSAL,
  EVENT_KIND,
  LIMIT,
  LIMIT_FIELDS,
  METER_EVENT_STRIDE,
  METER_JOURNAL,
  PROTECTION,
  QUALITY_KIND,
  RESTORATION,
  SEQUENCE,
  SNAP,
  SNAPSHOT_FIELDS,
  SNAPSHOT_LAYOUT_VERSION,
  STATE,
  decodeAnalyserEvents,
  decodeEvents,
  decodeMeterEvents,
  decodeProtectionCode,
} from "../snapshotLayout.mjs";

const here = dirname(fileURLToPath(import.meta.url));
await initWasm({ module_or_path: readFileSync(resolve(here, "../wasm-pkg/fs_kgu_cogen_bg.wasm")) });

function advance(sim, seconds) {
  assert.equal(sim.step(seconds), true, `step refused: ${sim.last_error()}`);
}

function advanceUntil(sim, limitS, what, done) {
  const start = sim.snapshot()[SNAP.TIME_S];
  for (;;) {
    const s = sim.snapshot();
    if (done(s)) return s[SNAP.TIME_S] - start;
    assert.ok(
      s[SNAP.TIME_S] - start < limitS,
      `${what}: not reached in ${limitS} s (state ${s[SNAP.STATE_CODE]}, step ${s[SNAP.SEQUENCE_STEP]}, ` +
        `${s[SNAP.SPEED_RPM].toFixed(1)} rpm, ${s[SNAP.ELECTRIC_POWER_KW].toFixed(1)} kW, alarms ${s[SNAP.ALARM_BITS]})`,
    );
    advance(sim, 0.1);
  }
}

// A unit started, synchronised and settled at `loadPct`. The load ramp is quickened
// (a controller setting); the steady state it ends in does not depend on it.
function onLoad(loadPct, rampPctPerMin = 300) {
  const sim = new KguSim();
  sim.set_load_ramp_pct_per_min(rampPctPerMin);
  sim.set_load_setpoint_pct(loadPct);
  sim.cmd_start();
  advanceUntil(sim, 200, "breaker closed", (s) => s[SNAP.BREAKER_CLOSED] === 1);
  advanceUntil(sim, (60 * 100) / rampPctPerMin + 60, "load reference at setpoint", (s) =>
    Math.abs(s[SNAP.LOAD_REFERENCE_PCT] - loadPct) < 1e-9,
  );
  advance(sim, 30);
  return sim;
}

const eventsOf = (sim, kind) => decodeEvents(sim.events()).filter((e) => e.kind === kind);
const relativeResidual = (s) =>
  Math.abs(s[SNAP.LEDGER_RESIDUAL_J]) / (s[SNAP.LEDGER_BOUNDARY_ENERGY_MJ] * 1e6);

test("snapshotLayout.mjs agrees with the engine's own layout", () => {
  const sim = new KguSim();
  assert.deepEqual(sim.snapshot_field_names().split(","), [...SNAPSHOT_FIELDS]);
  assert.deepEqual(sim.limit_field_names().split(","), [...LIMIT_FIELDS]);
  const s = sim.snapshot();
  assert.ok(s instanceof Float64Array);
  assert.equal(s.length, SNAPSHOT_FIELDS.length);
  assert.equal(s[SNAP.LAYOUT_VERSION], SNAPSHOT_LAYOUT_VERSION);
  const limits = sim.limits();
  assert.equal(limits[LIMIT.RATED_ELECTRIC_KW], 1000);
  assert.equal(limits[LIMIT.RATED_SPEED_RPM], 1500);
  assert.ok(Math.abs(limits[LIMIT.OVERSPEED_TRIP_RPM] - 1725) < 1e-9);
  assert.ok(Math.abs(limits[LIMIT.COOLANT_TRIP_C] - 98) < 1e-9);
  assert.equal(sim.sub_step_s(), 0.01);
  // The cabinet's settings are in the limits, in operator units.
  assert.ok(Math.abs(limits[LIMIT.RATED_PHASE_VOLTAGE_V] - 400 / Math.sqrt(3)) < 1e-9);
  assert.ok(Math.abs(limits[LIMIT.RATED_CURRENT_A] - 1.25e6 / (Math.sqrt(3) * 400)) < 1e-9);
  assert.equal(limits[LIMIT.PHASE_UNDERVOLTAGE_PU], limits[LIMIT.GRID_UNDERVOLTAGE_PU]);
  assert.equal(limits[LIMIT.PHASE_UNDERVOLTAGE_DELAY_S], 0.15);
  assert.ok(limits[LIMIT.HEALTHY_GRID_DELAY_S] > 0 && limits[LIMIT.LONGEST_GRID_WAIT_S] > 0);
  assert.ok(limits[LIMIT.ANALYSER_DIP_PU] < 1 && limits[LIMIT.ANALYSER_SWELL_PU] > 1);
  assert.equal(s[SNAP.RESTORATION_MODE], RESTORATION.MANUAL, "manual is the default");
  sim.free();
});

test("start from standstill reaches the bus, burning gas before the first kWh", () => {
  const sim = new KguSim();
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.READY);
  sim.set_load_setpoint_pct(50);
  sim.cmd_start();
  advance(sim, 0.5);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.STARTING);
  assert.equal(sim.snapshot()[SNAP.SEQUENCE_STEP], SEQUENCE.PRE_LUBRICATION);
  assert.equal(sim.snapshot()[SNAP.SPEED_RPM], 0, "nothing turns while pre-lubricating");
  const limits = sim.limits();
  const timers =
    limits[LIMIT.PRE_LUBRICATION_TIME_S] + limits[LIMIT.PURGE_TIME_S] + limits[LIMIT.WARM_UP_TIME_S];
  advance(sim, limits[LIMIT.PRE_LUBRICATION_TIME_S] + 1);
  assert.equal(sim.snapshot()[SNAP.SEQUENCE_STEP], SEQUENCE.CRANKING);
  assert.ok(sim.snapshot()[SNAP.TORQUE_STARTER_NM] > 0);
  assert.equal(sim.snapshot()[SNAP.GAS_STD_M3], 0, "no gas while purging");
  advanceUntil(sim, 200, "breaker closed", (x) => x[SNAP.BREAKER_CLOSED] === 1);
  const s = sim.snapshot();
  // Beyond the controller's three timers: cranking, the run-up and the wait for the phase.
  const toBus = s[SNAP.TIME_S];
  assert.ok(toBus > timers + 8 && toBus < timers + 40, `time to the bus ${toBus} s, timers ${timers} s`);
  assert.ok(s[SNAP.GAS_STD_M3] > 0.5 && s[SNAP.GAS_STD_M3] < 1.2, `gas before the bus ${s[SNAP.GAS_STD_M3]} m3`);
  assert.ok(s[SNAP.GAS_METER_M3] > 0 && s[SNAP.GAS_METER_M3] < s[SNAP.GAS_STD_M3]);
  assert.ok(Math.abs(s[SNAP.ENERGY_ELECTRIC_KWH]) < 0.05, `kWh at closing ${s[SNAP.ENERGY_ELECTRIC_KWH]}`);
  assert.deepEqual(
    eventsOf(sim, EVENT_KIND.STATE).map((e) => e.code),
    [STATE.STARTING, STATE.SYNCHRONIZING, STATE.RUNNING],
  );
  const closing = eventsOf(sim, EVENT_KIND.STATE).at(-1);
  assert.ok(Math.abs(closing.value - 1500) < 3, `rpm at closing ${closing.value}`);
  sim.free();
});

test("steady 100 % and 50 % load reach passport x condition factor; the ledger closes", () => {
  for (const [loadPct, passportPct] of [
    [100, 40.0],
    [50, 35.0],
  ]) {
    const sim = onLoad(loadPct);
    const s = sim.snapshot();
    assert.ok(Math.abs(s[SNAP.ELECTRIC_POWER_KW] - 10 * loadPct) < 0.5, `P ${s[SNAP.ELECTRIC_POWER_KW]} kW`);
    // Fitted: passport efficiency times the condition factor 0.955, within 0.3 points.
    const expected = passportPct * 0.955;
    assert.ok(
      Math.abs(s[SNAP.ELECTRIC_EFFICIENCY_PCT] - expected) < 0.3,
      `electrical efficiency ${s[SNAP.ELECTRIC_EFFICIENCY_PCT]} % against ${expected} %`,
    );
    assert.ok(s[SNAP.BRAKE_EFFICIENCY_PCT] > s[SNAP.ELECTRIC_EFFICIENCY_PCT] + 0.5);
    // Not inputs: what leaves adds up to the fuel power (plus the gas's sensible heat,
    // 15 C gas against the 20 C datum), and the kernel's ledger closes.
    const sensibleKw = (((s[SNAP.GAS_FLOW_STD_M3_H] / 3600) * 101325 * 16.043e-3) / (8.314462618 * 293.15)) * 2.2 * -5;
    const leaving =
      s[SNAP.ELECTRIC_POWER_KW] +
      s[SNAP.GENERATOR_LOSS_KW] +
      s[SNAP.JACKET_HEAT_KW] +
      s[SNAP.EXHAUST_RECOVERED_KW] +
      s[SNAP.EXHAUST_LOSS_KW] +
      s[SNAP.RADIATION_LOSS_KW];
    assert.ok(
      Math.abs(s[SNAP.FUEL_POWER_KW] + sensibleKw - leaving) < 1e-3,
      `fuel ${s[SNAP.FUEL_POWER_KW]} kW against ${leaving} kW leaving`,
    );
    assert.ok(relativeResidual(s) < 1e-9, `ledger residual ${s[SNAP.LEDGER_RESIDUAL_J]} J`);
    assert.equal(s[SNAP.ALARM_BITS], 0);
    sim.free();
  }
});

test("full load rejection: the speed rises, the governor holds it below the trip, it settles", () => {
  const sim = onLoad(100);
  const tripRpm = sim.limits()[LIMIT.OVERSPEED_TRIP_RPM];
  sim.cmd_breaker_open();
  let peak = 0;
  for (let i = 0; i < 500; i++) {
    advance(sim, 0.01);
    peak = Math.max(peak, sim.snapshot()[SNAP.SPEED_RPM]);
  }
  assert.ok(peak > 1520, `speed must rise: peak ${peak} rpm`);
  assert.ok(peak < tripRpm, `held below ${tripRpm} rpm: peak ${peak} rpm`);
  advance(sim, 20);
  const s = sim.snapshot();
  assert.equal(s[SNAP.STATE_CODE], STATE.SYNCHRONIZING);
  assert.equal(s[SNAP.ALARM_BITS], 0);
  assert.ok(Math.abs(s[SNAP.SPEED_RPM] - 1501.5) < 0.5, `settled at ${s[SNAP.SPEED_RPM]} rpm`);
  // The no-load fuel is where torque balances friction: 357 kW / 0.955 by the passport's line.
  assert.ok(Math.abs(s[SNAP.FUEL_POWER_KW] - 374) < 8, `no-load fuel ${s[SNAP.FUEL_POWER_KW]} kW`);
  sim.free();
});

test("synchronisation is refused outside the window and accepted inside", () => {
  const sim = new KguSim();
  sim.set_auto_sync(false);
  sim.cmd_start();
  advanceUntil(sim, 200, "synchronising", (s) => s[SNAP.STATE_CODE] === STATE.SYNCHRONIZING);
  advance(sim, 6);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0);

  sim.set_excitation_pu(1.2);
  advance(sim, 0.05);
  assert.notEqual(sim.cmd_breaker_close() & CLOSE_REFUSAL.VOLTAGE, 0);
  sim.set_excitation_pu(1.0);
  advanceUntil(sim, 30, "phase away", (s) => Math.abs(s[SNAP.SYNC_PHASE_ANGLE_DEG]) > 40);
  assert.equal(sim.cmd_breaker_close(), CLOSE_REFUSAL.PHASE);
  advance(sim, 0.05);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0, "a refused request does nothing");

  let waited = 0;
  while (sim.snapshot()[SNAP.SYNC_PERMITTED] !== 1) {
    advance(sim, 0.01);
    waited += 0.01;
    assert.ok(waited < 30, "the phase never came into the window");
  }
  assert.equal(sim.cmd_breaker_close(), 0);
  advance(sim, 0.02);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 1);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.RUNNING);
  assert.equal(eventsOf(sim, EVENT_KIND.REFUSED).length, 2);
  sim.free();
});

test("excitation moves Q while P stays with the fuel", () => {
  const sim = onLoad(60);
  const points = [];
  for (const excitation of [0.9, 1.0, 1.1, 1.2]) {
    sim.set_excitation_pu(excitation);
    advance(sim, 30);
    const s = sim.snapshot();
    points.push({ p: s[SNAP.ELECTRIC_POWER_KW], q: s[SNAP.REACTIVE_POWER_KVAR], fuel: s[SNAP.FUEL_POWER_KW] });
  }
  for (let i = 1; i < points.length; i++) {
    assert.ok(points[i].q > points[i - 1].q + 200, `Q must rise with excitation: ${JSON.stringify(points)}`);
  }
  for (const point of points) {
    assert.ok(Math.abs(point.p - 600) < 0.5, `P stays: ${JSON.stringify(points)}`);
    assert.ok(Math.abs(point.fuel / points[1].fuel - 1) < 0.03, `fuel stays with P: ${JSON.stringify(points)}`);
  }
  assert.ok(points[0].q < 0 && points[3].q > 0);
  sim.free();
});

test("normal stop: cool-down burns gas, coast-down does not", () => {
  const sim = onLoad(50);
  sim.cmd_stop();
  advanceUntil(sim, 200, "breaker open", (s) => s[SNAP.BREAKER_CLOSED] === 0);
  const s0 = sim.snapshot();
  assert.equal(s0[SNAP.STATE_CODE], STATE.STOPPING);
  assert.equal(s0[SNAP.SEQUENCE_STEP], SEQUENCE.COOL_DOWN);
  advance(sim, 150);
  const mid = sim.snapshot();
  assert.ok(Math.abs(mid[SNAP.SPEED_RPM] - 1500) < 1, `idle speed ${mid[SNAP.SPEED_RPM]} rpm`);
  assert.ok(mid[SNAP.FUEL_POWER_KW] > 300, `idle fuel ${mid[SNAP.FUEL_POWER_KW]} kW`);
  advanceUntil(sim, 200, "coasting", (s) => s[SNAP.SEQUENCE_STEP] === SEQUENCE.COAST_DOWN);
  const burned = sim.snapshot()[SNAP.GAS_STD_M3] - s0[SNAP.GAS_STD_M3];
  assert.ok(burned > 3.0 && burned < 3.7, `gas burned while cooling down ${burned} m3`);

  advance(sim, 0.05);
  const shut = sim.snapshot();
  assert.equal(shut[SNAP.FUEL_POWER_KW], 0);
  assert.ok(shut[SNAP.SPEED_RPM] > 1400, "still turning when the gas is shut");
  const toRest = advanceUntil(sim, 300, "ready", (s) => s[SNAP.STATE_CODE] === STATE.READY);
  assert.ok(toRest > 5 && toRest < 60, `coast-down took ${toRest} s`);
  const rest = sim.snapshot();
  assert.equal(rest[SNAP.GAS_STD_M3], shut[SNAP.GAS_STD_M3], "no gas while coasting down");
  assert.equal(rest[SNAP.GAS_METER_M3], shut[SNAP.GAS_METER_M3]);
  assert.ok(relativeResidual(rest) < 1e-9);
  sim.free();
});

test("emergency stop latches and blocks the start until acknowledged", () => {
  const sim = onLoad(50);
  sim.cmd_emergency_stop();
  advance(sim, 0.05);
  let s = sim.snapshot();
  assert.equal(s[SNAP.STATE_CODE], STATE.TRIP);
  assert.equal(s[SNAP.BREAKER_CLOSED], 0);
  assert.equal(s[SNAP.FUEL_POWER_KW], 0);
  assert.notEqual(s[SNAP.ALARM_BITS] & ALARM.EMERGENCY_STOP, 0);
  advanceUntil(sim, 120, "standstill", (x) => x[SNAP.SPEED_RPM] < 1);
  sim.cmd_start();
  advance(sim, 2);
  s = sim.snapshot();
  assert.equal(s[SNAP.STATE_CODE], STATE.TRIP, "start is blocked while latched");
  assert.equal(s[SNAP.START_COUNT], 1);
  sim.cmd_ack();
  advance(sim, 0.05);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.READY);
  sim.cmd_start();
  advance(sim, 0.5);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.STARTING);
  sim.free();
});

const heatSide = (s) =>
  `coolant ${s[SNAP.COOLANT_SUPPLY_C].toFixed(2)} C, network ${s[SNAP.HEAT_TO_NETWORK_KW].toFixed(1)} kW, ` +
  `dumped ${s[SNAP.HEAT_DUMPED_KW].toFixed(1)} kW, stack ${s[SNAP.EXHAUST_LOSS_KW].toFixed(1)} kW, alarms ${s[SNAP.ALARM_BITS]}`;

test("full heat demand: the recovered heat reaches the network near the passport's 43 %", () => {
  const sim = onLoad(100);
  advance(sim, 900);
  const s = sim.snapshot();
  assert.equal(s[SNAP.ALARM_BITS], 0, heatSide(s));
  // The coolant is a state: it warmed from the standby 60 C into the thermostat's band.
  assert.ok(s[SNAP.COOLANT_SUPPLY_C] > 84 && s[SNAP.COOLANT_SUPPLY_C] < 91, heatSide(s));
  assert.ok(s[SNAP.COOLANT_RETURN_C] < s[SNAP.COOLANT_SUPPLY_C] - 5, heatSide(s));
  assert.equal(s[SNAP.HEAT_DUMPED_KW], 0, heatSide(s));
  // The exhaust exchanger's length is fitted to the passport at this point; the plant
  // delivers it less the block's own loss. Tolerance 1.5 points.
  assert.ok(Math.abs(s[SNAP.THERMAL_EFFICIENCY_PCT] - 43) < 1.5, `thermal ${s[SNAP.THERMAL_EFFICIENCY_PCT]} %`);
  const received = s[SNAP.JACKET_HEAT_KW] + s[SNAP.EXHAUST_RECOVERED_KW];
  assert.ok(Math.abs(received - s[SNAP.HEAT_TO_NETWORK_KW] - s[SNAP.BLOCK_LOSS_KW]) < 0.5, heatSide(s));
  assert.ok(s[SNAP.EXHAUST_TEMPERATURE_C] > 380 && s[SNAP.EXHAUST_TEMPERATURE_C] < 560);
  assert.ok(s[SNAP.ENERGY_HEAT_KWH] > 200, `heat counter ${s[SNAP.ENERGY_HEAT_KWH]} kWh`);
  assert.ok(relativeResidual(s) < 1e-9);
  sim.free();
});

test("zero heat demand, fans healthy: the dump cooler takes the heat, coolant below the alarm", () => {
  const sim = onLoad(100);
  sim.set_heat_demand_frac(0);
  advance(sim, 900);
  const s = sim.snapshot();
  const warningC = sim.limits()[LIMIT.COOLANT_WARNING_C];
  assert.equal(s[SNAP.STATE_CODE], STATE.RUNNING, heatSide(s));
  assert.equal(s[SNAP.ALARM_BITS], 0, heatSide(s));
  assert.equal(s[SNAP.HEAT_TO_NETWORK_KW], 0, heatSide(s));
  assert.ok(s[SNAP.HEAT_DUMPED_KW] > 900, heatSide(s));
  assert.ok(s[SNAP.COOLANT_SUPPLY_C] > 91 && s[SNAP.COOLANT_SUPPLY_C] < warningC, heatSide(s));
  assert.ok(relativeResidual(s) < 1e-9);
  sim.free();
});

test("zero heat demand, fans failed: the coolant climbs and the high-temperature trip opens the breaker", () => {
  const sim = onLoad(100);
  advance(sim, 600);
  const tripC = sim.limits()[LIMIT.COOLANT_TRIP_C];
  sim.set_heat_demand_frac(0);
  sim.set_fan_failed(true);
  advance(sim, 1);
  assert.notEqual(sim.snapshot()[SNAP.ALARM_BITS] & ALARM.COOLER_FAN, 0, "fan warning");
  const toTrip = advanceUntil(sim, 900, "coolant trip", (s) => s[SNAP.STATE_CODE] === STATE.TRIP) + 1;
  const s = sim.snapshot();
  // Not inputs: that it trips, and when (the heat that cannot leave against the node's capacity).
  assert.ok(toTrip > 10 && toTrip < 120, `time to the trip ${toTrip} s; ${heatSide(s)}`);
  assert.notEqual(s[SNAP.ALARM_BITS] & ALARM.COOLANT_TEMPERATURE, 0);
  assert.ok(s[SNAP.COOLANT_SUPPLY_C] >= tripC - 0.05, heatSide(s));
  assert.equal(s[SNAP.BREAKER_CLOSED], 0);
  assert.equal(s[SNAP.FUEL_POWER_KW], 0);
  assert.deepEqual(
    eventsOf(sim, EVENT_KIND.ALARM_RAISED).map((e) => e.code),
    [8, 9, 0],
    "fan warning, coolant warning, coolant trip, in that order",
  );
  sim.free();
});

test("the package returns numbers and field names only", () => {
  const sim = onLoad(30);
  sim.set_oil_pressure_low(true);
  advance(sim, 3);
  // The station controller's event codes live in comments and in the README only.
  const codes = ["A-031", "A-105", "A-150", "A-160", "A-212", "W-118", "W-210"];
  const text = [sim.snapshot_field_names(), sim.limit_field_names(), sim.last_error()].join(" ");
  assert.equal(/[AW]-\d{3}/.test(text), false, "no controller event code in any returned string");
  assert.ok(sim.events() instanceof Float64Array);
  for (const file of ["fs_kgu_cogen_bg.wasm", "fs_kgu_cogen.js", "fs_kgu_cogen.d.ts"]) {
    const bytes = readFileSync(resolve(here, "../wasm-pkg", file)).toString("latin1");
    for (const code of codes) {
      assert.equal(bytes.includes(code), false, `${code} must not appear in ${file}`);
    }
  }
  sim.free();
});

test("two runs of the same commands agree to the bit", () => {
  const run = () => {
    const sim = new KguSim();
    sim.set_load_ramp_pct_per_min(300);
    sim.set_load_setpoint_pct(70);
    sim.cmd_start();
    for (let i = 0; i < 1500; i++) advance(sim, 0.1);
    const s = Array.from(sim.snapshot());
    sim.free();
    return s;
  };
  const [a, b] = [run(), run()];
  for (let i = 0; i < a.length; i++) {
    assert.ok(Object.is(a[i], b[i]), `slot ${SNAPSHOT_FIELDS[i]} differs: ${a[i]} against ${b[i]}`);
  }

  // And through an unbalanced bus, a protection trip, the wait and the automatic return.
  const disturbed = () => {
    const sim = onLoad(60);
    sim.set_restoration_mode(RESTORATION.AUTOMATIC);
    sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.94);
    advance(sim, 20);
    sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
    advance(sim, 2);
    sim.set_grid_voltage_pu(1.0);
    advance(sim, 100);
    const out = [
      Array.from(sim.snapshot()),
      Array.from(sim.events()),
      Array.from(sim.analyser_events()),
      Array.from(sim.meter_events()),
    ];
    sim.free();
    return out;
  };
  const [c, d] = [disturbed(), disturbed()];
  assert.equal(c[0][SNAP.BREAKER_CLOSED], 1);
  assert.equal(c[0][SNAP.RESTORATION_ATTEMPTS], 1);
  assert.equal(c[3].length, METER_EVENT_STRIDE, "one record of the 2 s dip in the meter's journal");
  for (const part of [0, 1, 2, 3]) {
    assert.equal(c[part].length, d[part].length);
    for (let i = 0; i < c[part].length; i++) {
      assert.ok(Object.is(c[part][i], d[part][i]), `part ${part}, value ${i}: ${c[part][i]} against ${d[part][i]}`);
    }
  }
});

// ---------------------------------------------------------------------------
// Three-phase grid, breaker cabinet, restoration, power-quality analyser
// ---------------------------------------------------------------------------

const PHASE_VOLT = 400 / Math.sqrt(3);

const gridSide = (s) =>
  `t ${s[SNAP.TIME_S].toFixed(2)} s, state ${s[SNAP.STATE_CODE]} step ${s[SNAP.SEQUENCE_STEP]}, ` +
  `breaker ${s[SNAP.BREAKER_CLOSED]}, P ${s[SNAP.ELECTRIC_POWER_KW].toFixed(1)} kW, ` +
  `phases ${s[SNAP.PHASE_A_VOLTAGE_V].toFixed(1)}/${s[SNAP.PHASE_B_VOLTAGE_V].toFixed(1)}/${s[SNAP.PHASE_C_VOLTAGE_V].toFixed(1)} V, ` +
  `currents ${s[SNAP.PHASE_A_CURRENT_A].toFixed(0)}/${s[SNAP.PHASE_B_CURRENT_A].toFixed(0)}/${s[SNAP.PHASE_C_CURRENT_A].toFixed(0)} A, ` +
  `U2/U1 ${s[SNAP.VOLTAGE_UNBALANCE_PCT].toFixed(2)} %, I2 ${s[SNAP.NEGATIVE_SEQUENCE_CURRENT_PCT].toFixed(2)} %, ` +
  `rotor ${s[SNAP.ROTOR_THERMAL_USED_FRAC].toFixed(3)}, pickup ${s[SNAP.PROTECTION_PICKUP_BITS]}, ` +
  `tripped ${s[SNAP.PROTECTION_TRIPPED_BITS]}, alarms ${s[SNAP.ALARM_BITS]}, attempts ${s[SNAP.RESTORATION_ATTEMPTS]}`;

const phaseCurrents = (s) => [s[SNAP.PHASE_A_CURRENT_A], s[SNAP.PHASE_B_CURRENT_A], s[SNAP.PHASE_C_CURRENT_A]];
const protectionTrips = (sim) =>
  eventsOf(sim, EVENT_KIND.PROTECTION).map((e) => ({ ...e, ...decodeProtectionCode(e.code) }));

test("a balanced grid reproduces the numbers recorded before the bus had three phases", () => {
  // Recorded on this package for slice 1b (2026-11-25): default ramp, 100 % setpoint,
  // 1 200 s after the breaker closed.
  const sim = new KguSim();
  sim.set_load_setpoint_pct(100);
  sim.cmd_start();
  while (sim.snapshot()[SNAP.BREAKER_CLOSED] !== 1) {
    advance(sim, 0.1);
    assert.ok(sim.snapshot()[SNAP.TIME_S] < 200);
  }
  const atBus = sim.snapshot();
  const near = (value, recorded) => Math.abs(value - recorded) <= 1e-12 * Math.abs(recorded);
  assert.ok(Math.abs(atBus[SNAP.TIME_S] - 91.0) < 1e-6, `to the bus in ${atBus[SNAP.TIME_S]} s`);
  assert.ok(near(atBus[SNAP.GAS_STD_M3], 0.6229275942155623), `gas ${atBus[SNAP.GAS_STD_M3]} m3`);
  advance(sim, 1200);
  const s = sim.snapshot();
  for (const [slot, recorded] of [
    ["ELECTRIC_POWER_KW", 1000.0000010635003],
    ["REACTIVE_POWER_KVAR", -193.70427017551341],
    ["ELECTRIC_EFFICIENCY_PCT", 38.2000000587074],
    ["THERMAL_EFFICIENCY_PCT", 42.75154668529072],
    ["COOLANT_SUPPLY_C", 88.3156340915915],
    ["COOLANT_RETURN_C", 68.17457035344671],
    ["EXHAUST_TEMPERATURE_C", 484.76666993389915],
    ["STACK_TEMPERATURE_C", 214.8341702904337],
  ]) {
    assert.ok(near(s[SNAP[slot]], recorded), `${slot}: ${s[SNAP[slot]]} against the recorded ${recorded}`);
  }
  assert.equal(s[SNAP.VOLTAGE_UNBALANCE_PCT], 0);
  assert.equal(s[SNAP.NEGATIVE_SEQUENCE_CURRENT_PCT], 0);
  assert.equal(s[SNAP.NEGATIVE_SEQUENCE_LOSS_KW], 0);
  assert.equal(s[SNAP.PROTECTION_PICKUP_BITS], 0);
  for (const current of phaseCurrents(s)) {
    assert.ok(Math.abs(current - s[SNAP.STATOR_CURRENT_A]) < 1e-9 * s[SNAP.STATOR_CURRENT_A]);
  }
  assert.ok(Math.abs(s[SNAP.PHASE_B_VOLTAGE_V] - PHASE_VOLT) < 1e-9);
  // A healthy balanced grid leaves both records empty.
  assert.equal(sim.analyser_events().length, 0);
  assert.equal(eventsOf(sim, EVENT_KIND.PROTECTION).length, 0);
  // And the meter's journals; its active registers are the engine's own counter.
  assert.equal(sim.meter_events().length, 0);
  for (const slot of [
    "METER_LOW_VOLTAGE_RECORDS",
    "METER_HIGH_VOLTAGE_RECORDS",
    "METER_LOSS_OF_VOLTAGE_RECORDS",
    "METER_OVERFLOW_BITS",
    "SYNCHRONISER_ARMED_BY_OPERATOR",
  ]) {
    assert.equal(s[SNAP[slot]], 0, slot);
  }
  const net = s[SNAP.METER_ACTIVE_EXPORT_KWH] - s[SNAP.METER_ACTIVE_IMPORT_KWH];
  assert.ok(Math.abs(net - s[SNAP.ENERGY_ELECTRIC_KWH]) < 1e-9 * net, `meter ${net} kWh`);
  sim.free();
});

test("one phase at 0.73 pu while exporting: the phase under-voltage trips first, the engine runs on", () => {
  const sim = onLoad(90);
  const fellAt = sim.snapshot()[SNAP.TIME_S];
  sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
  advance(sim, 0.1);
  const during = sim.snapshot();
  assert.equal(during[SNAP.BREAKER_CLOSED], 1, gridSide(during));
  assert.ok(Math.abs(during[SNAP.PHASE_B_VOLTAGE_V] - 0.73 * PHASE_VOLT) < 1e-9, gridSide(during));
  // Not inputs: the unbalance, the negative-sequence current it drives, the unequal
  // phase currents, the extra heat, and which functions have picked up.
  assert.ok(Math.abs(during[SNAP.VOLTAGE_UNBALANCE_PCT] - (100 * 0.09) / 0.91) < 1e-9, gridSide(during));
  assert.ok(
    during[SNAP.NEGATIVE_SEQUENCE_CURRENT_PCT] > 30 && during[SNAP.NEGATIVE_SEQUENCE_CURRENT_PCT] < 38,
    gridSide(during),
  );
  const currents = phaseCurrents(during);
  assert.ok(Math.max(...currents) - Math.min(...currents) > 500, gridSide(during));
  assert.ok(during[SNAP.NEGATIVE_SEQUENCE_LOSS_KW] > 5, gridSide(during));
  for (const fn of [PROTECTION.UNDERVOLTAGE, PROTECTION.VOLTAGE_UNBALANCE, PROTECTION.NEGATIVE_SEQUENCE]) {
    assert.notEqual(during[SNAP.PROTECTION_PICKUP_BITS] & (1 << fn), 0, `function ${fn} picked up`);
  }

  advance(sim, 0.1);
  const after = sim.snapshot();
  assert.equal(after[SNAP.BREAKER_CLOSED], 0, gridSide(after));
  const trips = protectionTrips(sim);
  assert.equal(trips.length, 1);
  assert.equal(trips[0].protection, PROTECTION.UNDERVOLTAGE);
  assert.equal(trips[0].phase, 2, "phase b");
  assert.ok(Math.abs(trips[0].value - 0.73 * PHASE_VOLT) < 1e-6, `tripped at ${trips[0].value} V`);
  const delay = trips[0].timeS - fellAt;
  assert.ok(Math.abs(delay - sim.limits()[LIMIT.PHASE_UNDERVOLTAGE_DELAY_S]) < 0.011, `tripped after ${delay} s`);
  assert.equal(after[SNAP.PROTECTION_TRIPPED_BITS], 1 << PROTECTION.UNDERVOLTAGE);
  assert.equal(after[SNAP.ALARM_BITS], ALARM.GRID_UNDERVOLTAGE);
  // Off the bus, not stopped: at speed, burning its no-load gas.
  assert.equal(after[SNAP.STATE_CODE], STATE.SYNCHRONIZING);
  assert.equal(after[SNAP.SEQUENCE_STEP], SEQUENCE.GRID_WAIT);
  assert.equal(after[SNAP.WAITING_FOR_GRID], 1);
  advance(sim, 20);
  const idling = sim.snapshot();
  assert.ok(Math.abs(idling[SNAP.SPEED_RPM] - 1500) < 1, gridSide(idling));
  assert.ok(idling[SNAP.FUEL_POWER_KW] > 300 && idling[SNAP.FUEL_POWER_KW] < 450);
  assert.ok(relativeResidual(idling) < 1e-9);
  sim.free();

  // All three phases at 0.73 instead: the same function, and no unbalance at all.
  const all = onLoad(90);
  all.set_grid_phase_voltages_pu(0.73, 0.73, 0.73);
  advance(all, 0.1);
  assert.equal(all.snapshot()[SNAP.VOLTAGE_UNBALANCE_PCT], 0);
  assert.equal(all.snapshot()[SNAP.NEGATIVE_SEQUENCE_CURRENT_PCT], 0);
  assert.equal(all.snapshot()[SNAP.PROTECTION_PICKUP_BITS], 1 << PROTECTION.UNDERVOLTAGE);
  advance(all, 0.1);
  assert.equal(all.snapshot()[SNAP.BREAKER_CLOSED], 0);
  assert.equal(protectionTrips(all)[0].protection, PROTECTION.UNDERVOLTAGE);
  all.free();
});

test("unbalance without low voltage: the rotor's limit trips, sooner the deeper the unbalance", () => {
  const times = [];
  for (const low of [0.88, 0.86]) {
    const sim = onLoad(80);
    const start = sim.snapshot()[SNAP.TIME_S];
    sim.set_grid_phase_voltages_pu(1.0, 1.0, low);
    advance(sim, 5);
    const s = sim.snapshot();
    assert.equal(s[SNAP.BREAKER_CLOSED], 1, gridSide(s));
    assert.equal(s[SNAP.PROTECTION_PICKUP_BITS], 1 << PROTECTION.NEGATIVE_SEQUENCE, gridSide(s));
    // The time is not a setting: it comes out of the integral (I2 / I_rated)^2 t.
    const negativePu = s[SNAP.NEGATIVE_SEQUENCE_CURRENT_PCT] / 100;
    const expected = sim.limits()[LIMIT.NEGATIVE_SEQUENCE_LIMIT_S] / negativePu ** 2;
    advanceUntil(sim, 1500, "rotor limit", (x) => x[SNAP.BREAKER_CLOSED] === 0);
    const trips = protectionTrips(sim);
    assert.equal(trips.length, 1);
    assert.equal(trips[0].protection, PROTECTION.NEGATIVE_SEQUENCE);
    assert.equal(trips[0].phase, 0);
    const time = trips[0].timeS - start;
    assert.ok(Math.abs(time - expected) < 0.01 * expected, `tripped after ${time} s; the integral gives ${expected} s`);
    advance(sim, 0.05);
    assert.equal(sim.snapshot()[SNAP.ALARM_BITS], ALARM.NEGATIVE_SEQUENCE);
    times.push({ negativePu, time });
    sim.free();
  }
  assert.ok(times[1].negativePu > times[0].negativePu && times[1].time < times[0].time, JSON.stringify(times));
});

test("a small unbalance inside the settings: runs on, unequal currents, extra heat in the balance", () => {
  const sim = onLoad(80);
  const balanced = sim.snapshot();
  sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.94);
  advance(sim, 300);
  const s = sim.snapshot();
  assert.equal(s[SNAP.STATE_CODE], STATE.RUNNING, gridSide(s));
  assert.equal(s[SNAP.ALARM_BITS], 0, gridSide(s));
  assert.equal(s[SNAP.PROTECTION_PICKUP_BITS], 0, gridSide(s));
  assert.ok(Math.abs(s[SNAP.VOLTAGE_UNBALANCE_PCT] - (100 * 0.02) / 0.98) < 1e-9, gridSide(s));
  assert.ok(Math.abs(s[SNAP.ELECTRIC_POWER_KW] - 800) < 0.5, gridSide(s));
  const currents = phaseCurrents(s);
  assert.ok(Math.max(...currents) - Math.min(...currents) > 100, gridSide(s));
  const extra = s[SNAP.NEGATIVE_SEQUENCE_LOSS_KW];
  assert.ok(extra > 0.3 && extra < 1.0, `negative-sequence heat ${extra} kW`);
  assert.ok(s[SNAP.GENERATOR_LOSS_KW] > balanced[SNAP.GENERATOR_LOSS_KW] + 0.9 * extra);
  const sensibleKw = (((s[SNAP.GAS_FLOW_STD_M3_H] / 3600) * 101325 * 16.043e-3) / (8.314462618 * 293.15)) * 2.2 * -5;
  const leaving =
    s[SNAP.ELECTRIC_POWER_KW] +
    s[SNAP.GENERATOR_LOSS_KW] +
    s[SNAP.JACKET_HEAT_KW] +
    s[SNAP.EXHAUST_RECOVERED_KW] +
    s[SNAP.EXHAUST_LOSS_KW] +
    s[SNAP.RADIATION_LOSS_KW];
  assert.ok(Math.abs(s[SNAP.FUEL_POWER_KW] + sensibleKw - leaving) < 1e-3, `fuel against ${leaving} kW leaving`);
  assert.ok(relativeResidual(s) < 1e-9);
  sim.free();
});

test("automatic restoration: back on the bus and at the setpoint with no command", () => {
  const sim = onLoad(50);
  const limits = sim.limits();
  sim.set_restoration_mode(RESTORATION.AUTOMATIC);
  sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
  advance(sim, 2);
  const off = sim.snapshot();
  assert.equal(off[SNAP.BREAKER_CLOSED], 0, gridSide(off));
  assert.equal(off[SNAP.WAITING_FOR_GRID], 1);
  assert.equal(off[SNAP.GRID_HEALTHY_COUNTDOWN_S], limits[LIMIT.HEALTHY_GRID_DELAY_S]);

  sim.set_grid_voltage_pu(1.0);
  const gasAt = sim.snapshot()[SNAP.GAS_STD_M3];
  // No command from here on.
  const toBus = advanceUntil(sim, 200, "back on the bus", (s) => s[SNAP.BREAKER_CLOSED] === 1);
  const closed = sim.snapshot();
  // Not an input: the healthy-grid delay plus what the synchroniser needs for the phase.
  assert.ok(
    toBus > limits[LIMIT.HEALTHY_GRID_DELAY_S] && toBus < limits[LIMIT.HEALTHY_GRID_DELAY_S] + 22,
    `back on the bus after ${toBus} s; ${gridSide(closed)}`,
  );
  assert.equal(closed[SNAP.ALARM_BITS], 0, gridSide(closed));
  assert.equal(closed[SNAP.RESTORATION_ATTEMPTS], 1);
  // The gas burned unloaded while waiting is on the meter.
  const burned = closed[SNAP.GAS_STD_M3] - gasAt;
  const noLoad = (374e3 * toBus) / 33.5e6;
  assert.ok(Math.abs(burned - noLoad) < 0.1 * noLoad, `${burned} m3 burned waiting ${toBus} s`);
  const toLoad = advanceUntil(sim, 120, "load back", (s) => Math.abs(s[SNAP.LOAD_REFERENCE_PCT] - 50) < 1e-9);
  assert.ok(Math.abs(toLoad - 10) < 0.2, `ramp to 50 % at 300 %/min took ${toLoad} s`);
  advance(sim, 30);
  const s = sim.snapshot();
  assert.ok(Math.abs(s[SNAP.ELECTRIC_POWER_KW] - 500) < 0.5, gridSide(s));
  assert.equal(s[SNAP.STATE_CODE], STATE.RUNNING);
  assert.ok(relativeResidual(s) < 1e-9);
  sim.free();
});

test("automatic restoration: a lasting disturbance ends in a normal stop after the longest wait", () => {
  const sim = onLoad(50);
  const longest = sim.limits()[LIMIT.LONGEST_GRID_WAIT_S];
  sim.set_restoration_mode(RESTORATION.AUTOMATIC);
  sim.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
  advance(sim, 1);
  advance(sim, longest - 10);
  const waiting = sim.snapshot();
  assert.equal(waiting[SNAP.WAITING_FOR_GRID], 1, gridSide(waiting));
  assert.equal(waiting[SNAP.BREAKER_CLOSED], 0);
  assert.equal(waiting[SNAP.RESTORATION_ATTEMPTS], 0);
  assert.ok(waiting[SNAP.FUEL_POWER_KW] > 300, "still running unloaded");
  advance(sim, 15);
  const stopping = sim.snapshot();
  assert.equal(stopping[SNAP.STATE_CODE], STATE.STOPPING, gridSide(stopping));
  assert.equal(stopping[SNAP.SEQUENCE_STEP], SEQUENCE.COOL_DOWN);
  advanceUntil(sim, 400, "ready", (s) => s[SNAP.STATE_CODE] === STATE.READY);
  assert.equal(eventsOf(sim, EVENT_KIND.BREAKER).length, 2, "closed once, opened once");
  assert.equal(sim.snapshot()[SNAP.ALARM_BITS], ALARM.GRID_UNDERVOLTAGE, "still latched");
  sim.free();
});

test("automatic restoration: a flickering grid blocks the return after the allowed attempts", () => {
  const sim = onLoad(30);
  const limits = sim.limits();
  sim.set_restoration_mode(RESTORATION.AUTOMATIC);
  const flicker = () => {
    sim.set_grid_phase_voltages_pu(0.73, 1.0, 1.0);
    advance(sim, 1);
    assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0, gridSide(sim.snapshot()));
    sim.set_grid_voltage_pu(1.0);
  };
  for (let attempt = 1; attempt <= limits[LIMIT.LARGEST_AUTO_ATTEMPTS]; attempt++) {
    flicker();
    advanceUntil(sim, 200, "back on the bus", (s) => s[SNAP.BREAKER_CLOSED] === 1);
    assert.equal(sim.snapshot()[SNAP.RESTORATION_ATTEMPTS], attempt, gridSide(sim.snapshot()));
    advance(sim, 5);
  }
  flicker();
  const blocked = sim.snapshot();
  assert.equal(blocked[SNAP.ALARM_BITS], ALARM.GRID_UNDERVOLTAGE | ALARM.RESTORATION_LOCKOUT, gridSide(blocked));
  advance(sim, limits[LIMIT.HEALTHY_GRID_DELAY_S] + 60);
  const still = sim.snapshot();
  assert.equal(still[SNAP.BREAKER_CLOSED], 0, gridSide(still));
  assert.equal(still[SNAP.WAITING_FOR_GRID], 1);
  // An acknowledgement releases it.
  sim.cmd_ack();
  advanceUntil(sim, 30, "back on the bus", (s) => s[SNAP.BREAKER_CLOSED] === 1);
  assert.equal(protectionTrips(sim).length, 4);
  sim.free();
});

test("manual restoration: off the bus until the operator acknowledges and closes", () => {
  const sim = onLoad(50);
  sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.73);
  advance(sim, 1);
  sim.set_grid_voltage_pu(1.0);
  const gasAt = sim.snapshot()[SNAP.GAS_STD_M3];
  advance(sim, 300);
  const waiting = sim.snapshot();
  assert.equal(waiting[SNAP.BREAKER_CLOSED], 0, gridSide(waiting));
  assert.equal(waiting[SNAP.WAITING_FOR_GRID], 1, gridSide(waiting));
  assert.equal(waiting[SNAP.ALARM_BITS], ALARM.GRID_UNDERVOLTAGE);
  assert.ok(Number.isNaN(waiting[SNAP.GRID_HEALTHY_COUNTDOWN_S]), "no countdown in manual");
  const burned = waiting[SNAP.GAS_STD_M3] - gasAt;
  assert.ok(burned > 3.0 && burned < 3.7, `${burned} m3 burned unloaded in 300 s`);
  assert.ok(relativeResidual(waiting) < 1e-9);

  assert.notEqual(sim.cmd_breaker_close() & CLOSE_REFUSAL.TRIP_LATCHED, 0, "refused before the acknowledgement");
  sim.cmd_ack();
  advance(sim, 40);
  assert.equal(sim.snapshot()[SNAP.ALARM_BITS], 0);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0, "waits for the operator's request");
  let waited = 0;
  while (sim.snapshot()[SNAP.SYNC_PERMITTED] !== 1) {
    advance(sim, 0.01);
    waited += 0.01;
    assert.ok(waited < 30, "the phase never came into the window");
  }
  assert.equal(sim.cmd_breaker_close(), 0);
  advance(sim, 60);
  const s = sim.snapshot();
  assert.equal(s[SNAP.BREAKER_CLOSED], 1);
  assert.ok(Math.abs(s[SNAP.ELECTRIC_POWER_KW] - 500) < 0.5, gridSide(s));
  sim.free();
});

test("the analyser records a dip the protection rides through, and keeps its own clock", () => {
  // Shorter than the protection's delay: one analyser event, breaker closed.
  const sim = onLoad(50);
  sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.8);
  advance(sim, 0.1);
  assert.equal(sim.snapshot()[SNAP.ANALYSER_OPEN_EVENTS], 1);
  sim.set_grid_voltage_pu(1.0);
  advance(sim, 1);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 1, gridSide(sim.snapshot()));
  assert.equal(protectionTrips(sim).length, 0);
  const record = decodeAnalyserEvents(sim.analyser_events());
  assert.equal(record.length, 1);
  assert.equal(record[0].kind, QUALITY_KIND.DIP);
  assert.equal(record[0].phase, 3);
  assert.ok(Math.abs(record[0].durationS - 0.1) < 0.011, `duration ${record[0].durationS} s`);
  assert.ok(Math.abs(record[0].extremeV - 0.8 * PHASE_VOLT) < 1e-9, `residual ${record[0].extremeV} V`);
  assert.equal(sim.snapshot()[SNAP.ANALYSER_RECORDED_EVENTS], 1);
  sim.clear_analyser_events();
  assert.equal(sim.analyser_events().length, 0);
  sim.free();

  // The analyser's clock an hour ahead: its event moves by the hour, the protection's
  // event stays in model time.
  const run = (offsetS) => {
    const unit = onLoad(50);
    unit.set_analyser_clock_offset_s(offsetS);
    unit.set_grid_phase_voltages_pu(1.0, 0.73, 1.0);
    advance(unit, 0.5);
    unit.set_grid_voltage_pu(1.0);
    advance(unit, 1);
    const out = { dip: decodeAnalyserEvents(unit.analyser_events()), trip: protectionTrips(unit) };
    unit.free();
    return out;
  };
  const inStep = run(0);
  const ahead = run(3600);
  assert.equal(inStep.dip.length, 1);
  assert.equal(inStep.trip.length, 1);
  assert.equal(inStep.dip[0].phase, 2);
  assert.ok(Math.abs(inStep.dip[0].durationS - 0.5) < 0.011);
  assert.ok(Math.abs(inStep.trip[0].timeS - inStep.dip[0].startS - 0.15) < 0.011);
  assert.ok(Math.abs(ahead.dip[0].startS - inStep.dip[0].startS - 3600) < 1e-6);
  assert.ok(Object.is(ahead.trip[0].timeS, inStep.trip[0].timeS));
  assert.ok(Math.abs(ahead.dip[0].startS - ahead.trip[0].timeS - (3600 - 0.15)) < 0.011);
});

// ---------------------------------------------------------------------------
// The revenue meter: one look per second, journals of limited capacity
// ---------------------------------------------------------------------------

const DIP_160_V_PU = 160 / PHASE_VOLT;

test("a 130 ms dip is in the meter's journal only when one of its looks falls inside", () => {
  // The same dip twice: 130 ms to 160 V on phase b, the unit exporting. Only the place of
  // the meter's one look per second differs (`lookAfterS`, counted from the phase's fall).
  const run = (lookAfterS) => {
    const sim = onLoad(50);
    const fellAt = sim.snapshot()[SNAP.TIME_S];
    sim.set_meter_sample_phase_s(fellAt + lookAfterS);
    sim.set_grid_phase_voltages_pu(1.0, DIP_160_V_PU, 1.0);
    advance(sim, 0.13);
    sim.set_grid_voltage_pu(1.0);
    advance(sim, 3);
    const s = sim.snapshot();
    // Shorter than the protection's 0.15 s: the breaker stays in either way.
    assert.equal(s[SNAP.BREAKER_CLOSED], 1, gridSide(s));
    assert.equal(protectionTrips(sim).length, 0);
    // The analyser has it either way, with its length and depth.
    const quality = decodeAnalyserEvents(sim.analyser_events());
    assert.equal(quality.length, 1);
    assert.equal(quality[0].kind, QUALITY_KIND.DIP);
    assert.equal(quality[0].phase, 2);
    assert.ok(Math.abs(quality[0].durationS - 0.13) < 1e-9, `duration ${quality[0].durationS} s`);
    assert.ok(Math.abs(quality[0].extremeV - 160) < 1e-9, `residual ${quality[0].extremeV} V`);
    const out = { fellAt, records: decodeMeterEvents(sim.meter_events()), count: s[SNAP.METER_LOW_VOLTAGE_RECORDS] };
    sim.free();
    return out;
  };

  // The looks half a second either side of the dip: nothing in the journal.
  const between = run(0.5);
  assert.equal(between.records.length, 0);
  assert.equal(between.count, 0);

  // A look 65 ms into the dip: a record, and all it holds is the look that saw the voltage
  // low, the phase, and the next look, which saw it back.
  const inside = run(0.065);
  assert.equal(inside.records.length, 1);
  assert.equal(inside.count, 1);
  const record = inside.records[0];
  assert.equal(record.journal, METER_JOURNAL.LOW_VOLTAGE);
  assert.deepEqual(record.phases, [2]);
  assert.ok(Math.abs(record.seenS - (inside.fellAt + 0.065)) < 1e-9, `seen at ${record.seenS}`);
  assert.ok(Math.abs(record.clearedS - record.seenS - 1) < 1e-9, "seen gone at the next look, a second later");
  assert.deepEqual(Object.keys(record).sort(), ["clearedS", "journal", "phaseBits", "phases", "seenS"]);
  assert.equal(METER_EVENT_STRIDE, 4, "journal, phases and two instants: no depth, no duration");
});

test("the share of sample phases that catch a 130 ms dip is its share of the second", () => {
  let caught = 0;
  for (let place = 0; place < 100; place++) {
    const sim = new KguSim();
    sim.set_meter_sample_phase_s((place + 0.5) / 100);
    advance(sim, 2);
    sim.set_grid_phase_voltages_pu(DIP_160_V_PU, 1.0, 1.0);
    advance(sim, 0.13);
    sim.set_grid_voltage_pu(1.0);
    advance(sim, 1.5);
    caught += sim.meter_events().length / METER_EVENT_STRIDE;
    assert.equal(decodeAnalyserEvents(sim.analyser_events()).length, 1, "the analyser sees every one");
    sim.free();
  }
  assert.ok(Math.abs(caught / 100 - 0.13) < 0.011, `caught at ${caught} of 100 sample phases`);
});

test("a 3 s dip is in both records, each by its own clock; the meter's time is one of its looks", () => {
  const sim = onLoad(50);
  sim.set_analyser_clock_offset_s(100);
  sim.set_meter_clock_offset_s(250);
  sim.set_meter_sample_phase_s(0.4);
  const fellAt = sim.snapshot()[SNAP.TIME_S];
  sim.set_grid_phase_voltages_pu(1.0, 1.0, DIP_160_V_PU);
  advance(sim, 3);
  sim.set_grid_voltage_pu(1.0);
  advance(sim, 3);
  const s = sim.snapshot();
  assert.ok(Math.abs(s[SNAP.ANALYSER_CLOCK_S] - s[SNAP.TIME_S] - 100) < 1e-9);
  assert.ok(Math.abs(s[SNAP.METER_CLOCK_S] - s[SNAP.TIME_S] - 250) < 1e-9);
  const quality = decodeAnalyserEvents(sim.analyser_events());
  assert.equal(quality.length, 1);
  assert.ok(Math.abs(quality[0].startS - (fellAt + 100)) < 1e-9, "the analyser stamps the start");
  assert.ok(Math.abs(quality[0].durationS - 3) < 1e-9);
  const records = decodeMeterEvents(sim.meter_events());
  assert.equal(records.length, 1);
  assert.equal(records[0].journal, METER_JOURNAL.LOW_VOLTAGE);
  assert.deepEqual(records[0].phases, [3]);
  // A look of the meter's clock (whole seconds plus 0.4 s), the first after the phase fell.
  const whole = records[0].seenS - 0.4;
  assert.ok(Math.abs(whole - Math.round(whole)) < 1e-9, `seen at ${records[0].seenS}`);
  const late = records[0].seenS - 250 - fellAt;
  assert.ok(late > 0 && late < 1, `the look came ${late} s after the phase fell`);
  assert.ok(Math.abs(records[0].clearedS - records[0].seenS - 3) < 1e-9);
  assert.ok(Math.abs(records[0].seenS - quality[0].startS - (150 + late)) < 1e-9);
  sim.free();
});

test("the 101st low-voltage event overflows the meter's journal as its card says", () => {
  const sim = new KguSim();
  const limits = sim.limits();
  const capacity = limits[LIMIT.METER_LOW_VOLTAGE_CAPACITY];
  assert.equal(capacity, 100);
  assert.equal(limits[LIMIT.METER_LOSS_OF_VOLTAGE_CAPACITY], 20);
  assert.equal(limits[LIMIT.METER_SUPERVISION_PERIOD_S], 1);
  // Event k holds from 3k s for 1.5 s; the meter looks 0.505 s past each second.
  sim.set_meter_sample_phase_s(0.505);
  const event = () => {
    sim.set_grid_phase_voltages_pu(1.0, 0.8, 1.0);
    advance(sim, 1.5);
    sim.set_grid_voltage_pu(1.0);
    advance(sim, 1.5);
  };
  for (let i = 0; i < 100; i++) event();
  assert.equal(sim.snapshot()[SNAP.METER_LOW_VOLTAGE_RECORDS], capacity);
  assert.equal(sim.snapshot()[SNAP.METER_OVERFLOW_BITS], 0, "full, not yet overflowed");
  event();
  const s = sim.snapshot();
  assert.equal(s[SNAP.METER_LOW_VOLTAGE_RECORDS], capacity, "the journal holds no more");
  assert.equal(s[SNAP.METER_OVERFLOW_BITS], 1 << (METER_JOURNAL.LOW_VOLTAGE - 1));
  const records = decodeMeterEvents(sim.meter_events());
  assert.equal(records.length, capacity);
  if (limits[LIMIT.METER_OVERFLOW_OVERWRITES_OLDEST] === 1) {
    assert.ok(Math.abs(records[0].seenS - 3.505) < 1e-9, `oldest record seen at ${records[0].seenS}`);
    assert.ok(Math.abs(records.at(-1).seenS - 300.505) < 1e-9);
  } else {
    assert.ok(Math.abs(records[0].seenS - 0.505) < 1e-9);
    assert.ok(Math.abs(records.at(-1).seenS - 297.505) < 1e-9);
  }
  assert.equal(s[SNAP.ANALYSER_RECORDED_EVENTS], 101, "the analyser has all of them");
  sim.clear_meter_events();
  advance(sim, 0.01);
  assert.equal(sim.meter_events().length, 0);
  assert.equal(sim.snapshot()[SNAP.METER_OVERFLOW_BITS], 0);
  sim.free();
});

test("the meter's registers follow the direction of active and reactive power", () => {
  const sim = onLoad(100);
  const registers = (s) => [
    s[SNAP.METER_ACTIVE_EXPORT_KWH],
    s[SNAP.METER_ACTIVE_IMPORT_KWH],
    s[SNAP.METER_REACTIVE_EXPORT_KVARH],
    s[SNAP.METER_REACTIVE_IMPORT_KVARH],
  ];
  // A quarter of an hour of steady export, under-excited (absorbing about 194 kvar).
  let before = sim.snapshot();
  assert.ok(before[SNAP.REACTIVE_POWER_KVAR] < -100);
  advance(sim, 900);
  let after = sim.snapshot();
  let [r0, r1] = [registers(before), registers(after)];
  const engine = after[SNAP.ENERGY_ELECTRIC_KWH] - before[SNAP.ENERGY_ELECTRIC_KWH];
  const exported = r1[0] - r0[0];
  assert.ok(Math.abs(exported - engine) < 1e-9 * engine, `meter ${exported} kWh, engine ${engine} kWh`);
  assert.ok(Math.abs(exported - 250) < 0.25, `1 000 kW for a quarter of an hour: ${exported} kWh`);
  assert.ok(Object.is(r1[1], r0[1]), "no active import while exporting");
  assert.ok(Object.is(r1[2], r0[2]), "no reactive export while under-excited");
  const expected = -0.25 * 0.5 * (before[SNAP.REACTIVE_POWER_KVAR] + after[SNAP.REACTIVE_POWER_KVAR]);
  assert.ok(Math.abs(r1[3] - r0[3] - expected) < 0.01 * expected, `${r1[3] - r0[3]} kvarh against ${expected}`);

  // Over-excited, the reactive power goes out and into the other register.
  sim.set_excitation_pu(1.2);
  advance(sim, 60);
  before = sim.snapshot();
  assert.ok(before[SNAP.REACTIVE_POWER_KVAR] > 100, `${before[SNAP.REACTIVE_POWER_KVAR]} kvar`);
  advance(sim, 300);
  after = sim.snapshot();
  [r0, r1] = [registers(before), registers(after)];
  assert.ok(r1[2] - r0[2] > 5, `reactive export ${r1[2] - r0[2]} kvarh`);
  assert.ok(Object.is(r1[3], r0[3]), "no reactive import while over-excited");
  assert.ok(Object.is(r1[1], r0[1]));
  const net = r1[0] - r1[1];
  assert.ok(Math.abs(net - after[SNAP.ENERGY_ELECTRIC_KWH]) < 1e-9 * net);
  sim.free();
});

// ---------------------------------------------------------------------------
// Synchronisation on the operator's command
// ---------------------------------------------------------------------------

// Advance in single sub-steps until the breaker closes; returns the time taken and the
// phase angle of the last look before it closed.
function advanceToClosing(sim, limitS) {
  let waited = 0;
  let phaseDeg = sim.snapshot()[SNAP.SYNC_PHASE_ANGLE_DEG];
  while (sim.snapshot()[SNAP.BREAKER_CLOSED] !== 1) {
    phaseDeg = sim.snapshot()[SNAP.SYNC_PHASE_ANGLE_DEG];
    advance(sim, 0.01);
    waited += 0.01;
    assert.ok(waited < limitS, `the breaker did not close in ${limitS} s`);
  }
  return { waited, phaseDeg };
}

test("manual return: the operator's command lets the synchroniser close, once", () => {
  const sim = onLoad(50);
  sim.set_grid_phase_voltages_pu(1.0, 1.0, 0.73);
  advance(sim, 1);
  sim.set_grid_voltage_pu(1.0);
  advance(sim, 5);

  // Before the acknowledgement the command is refused, with the reasons as bits.
  const refusedBits = CLOSE_REFUSAL.NOT_SYNCHRONISING | CLOSE_REFUSAL.TRIP_LATCHED;
  assert.equal(sim.cmd_synchronise(), refusedBits);
  advance(sim, 30);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0);

  // Acknowledged, and no command: off the bus, however often the phase passes zero.
  sim.cmd_ack();
  advance(sim, 45);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0, gridSide(sim.snapshot()));
  assert.equal(sim.snapshot()[SNAP.SEQUENCE_STEP], SEQUENCE.SYNCHRONISING);
  assert.equal(sim.snapshot()[SNAP.SYNCHRONISER_ARMED_BY_OPERATOR], 0);

  // The command, given well away from zero phase: the synchroniser does the rest.
  advanceUntil(sim, 30, "phase away", (s) => Math.abs(s[SNAP.SYNC_PHASE_ANGLE_DEG]) > 40);
  const commandedAt = sim.snapshot()[SNAP.TIME_S];
  assert.equal(sim.cmd_synchronise(), 0);
  advance(sim, 0.02);
  assert.equal(sim.snapshot()[SNAP.SYNCHRONISER_ARMED_BY_OPERATOR], 1);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0, "not before the phase is there");
  const { waited, phaseDeg } = advanceToClosing(sim, 25);
  assert.ok(waited > 1, `closed ${waited} s after the command`);
  assert.ok(Math.abs(phaseDeg) < 1.5, `closed at ${phaseDeg} degrees`);
  assert.equal(sim.snapshot()[SNAP.SYNCHRONISER_ARMED_BY_OPERATOR], 0, "one attempt");
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.RUNNING);
  // The log shows who decided.
  const commands = eventsOf(sim, EVENT_KIND.SYNCHRONISE_COMMAND);
  assert.deepEqual(commands.map((e) => e.code), [refusedBits, 0]);
  assert.ok(Math.abs(commands[1].timeS - commandedAt) < 1e-9);
  assert.ok(Math.abs(commands[1].value) > 40, `phase at the command ${commands[1].value} degrees`);
  const closings = eventsOf(sim, EVENT_KIND.BREAKER).filter((e) => e.code === 1);
  assert.equal(closings.length, 2);
  assert.ok(closings[1].timeS > commands[1].timeS);
  assert.equal(eventsOf(sim, EVENT_KIND.REFUSED).length, 0, "no close request was made");
  advance(sim, 60);
  assert.ok(Math.abs(sim.snapshot()[SNAP.ELECTRIC_POWER_KW] - 500) < 0.5, gridSide(sim.snapshot()));
  sim.free();

  // A stop cancels a commanded synchronisation that is still waiting for the phase.
  const other = onLoad(50);
  other.cmd_breaker_open();
  advance(other, 20);
  advanceUntil(other, 30, "phase away", (s) => Math.abs(s[SNAP.SYNC_PHASE_ANGLE_DEG]) > 40);
  assert.equal(other.cmd_synchronise(), 0);
  advance(other, 0.02);
  assert.equal(other.snapshot()[SNAP.SYNCHRONISER_ARMED_BY_OPERATOR], 1);
  other.cmd_stop();
  advance(other, 30);
  assert.equal(other.snapshot()[SNAP.SEQUENCE_STEP], SEQUENCE.COOL_DOWN);
  assert.equal(other.snapshot()[SNAP.BREAKER_CLOSED], 0);
  assert.equal(other.snapshot()[SNAP.SYNCHRONISER_ARMED_BY_OPERATOR], 0);
  other.free();
});

test("a start without automatic synchronisation waits for the command", () => {
  const sim = new KguSim();
  assert.equal(sim.cmd_synchronise(), CLOSE_REFUSAL.NOT_SYNCHRONISING, "a unit at rest refuses it");
  sim.set_auto_sync(false);
  sim.set_load_setpoint_pct(40);
  sim.cmd_start();
  advanceUntil(sim, 200, "synchronising", (s) => s[SNAP.STATE_CODE] === STATE.SYNCHRONIZING);
  advance(sim, 45);
  assert.equal(sim.snapshot()[SNAP.BREAKER_CLOSED], 0, "waits without a command");
  advanceUntil(sim, 30, "phase away", (s) => Math.abs(s[SNAP.SYNC_PHASE_ANGLE_DEG]) > 40);
  assert.equal(sim.cmd_synchronise(), 0);
  const { waited, phaseDeg } = advanceToClosing(sim, 25);
  assert.ok(waited > 1 && Math.abs(phaseDeg) < 1.5, `closed after ${waited} s at ${phaseDeg} degrees`);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.RUNNING);
  assert.deepEqual(
    eventsOf(sim, EVENT_KIND.SYNCHRONISE_COMMAND).map((e) => e.code),
    [CLOSE_REFUSAL.NOT_SYNCHRONISING, 0],
  );
  sim.free();
});

// ---------------------------------------------------------------------------
// Standstill: the shaft comes to rest and the unit stands, for hours
// ---------------------------------------------------------------------------

// Stand for `seconds`, checking every minute that the shaft is at exactly zero speed
// and the model still steps.
function stand(sim, seconds) {
  const minutes = Math.ceil(seconds / 60);
  for (let i = 0; i < minutes; i++) {
    advance(sim, seconds / minutes);
    const s = sim.snapshot();
    assert.ok(Object.is(s[SNAP.SPEED_RPM], 0), `speed ${s[SNAP.SPEED_RPM]} rpm at t ${s[SNAP.TIME_S]} s`);
    assert.equal(s[SNAP.KINETIC_ENERGY_KJ], 0);
  }
}

// Start, wait for the breaker and for the load; returns the time to the bus.
function startAndLoad(sim, loadPct) {
  sim.set_load_ramp_pct_per_min(300);
  sim.set_load_setpoint_pct(loadPct);
  sim.cmd_start();
  const toBus = advanceUntil(sim, 200, "breaker closed", (s) => s[SNAP.BREAKER_CLOSED] === 1);
  advanceUntil(sim, 120, "load reference", (s) => Math.abs(s[SNAP.LOAD_REFERENCE_PCT] - loadPct) < 1e-9);
  advance(sim, 30);
  const s = sim.snapshot();
  assert.ok(Math.abs(s[SNAP.ELECTRIC_POWER_KW] - 10 * loadPct) < 0.5, gridSide(s));
  assert.ok(relativeResidual(s) < 1e-9, `ledger residual ${s[SNAP.LEDGER_RESIDUAL_J]} J`);
  return toBus;
}

test("normal stop, two hours at rest at exactly zero speed, start, load", () => {
  const sim = new KguSim();
  const firstToBus = startAndLoad(sim, 60);
  sim.cmd_stop();
  advanceUntil(sim, 500, "coasting", (s) => s[SNAP.SEQUENCE_STEP] === SEQUENCE.COAST_DOWN);
  // Not inputs: the time to the controller's standstill level and to rest, and that
  // rest is zero, not a small number.
  const toReady = advanceUntil(sim, 60, "ready", (s) => s[SNAP.STATE_CODE] === STATE.READY);
  const toRest = toReady + advanceUntil(sim, 5, "rest", (s) => s[SNAP.SPEED_RPM] === 0);
  assert.ok(toReady > 11 && toReady < 12.5, `ready ${toReady} s after the gas was shut`);
  assert.ok(toRest - toReady < 0.5, `at rest ${toRest} s after the gas was shut`);
  assert.ok(relativeResidual(sim.snapshot()) < 1e-9);
  stand(sim, 7200);
  const rested = sim.snapshot();
  assert.equal(rested[SNAP.STATE_CODE], STATE.READY);
  assert.ok(relativeResidual(rested) < 1e-9, `ledger residual ${rested[SNAP.LEDGER_RESIDUAL_J]} J`);
  assert.ok(rested[SNAP.COOLANT_SUPPLY_C] < 80 && rested[SNAP.COOLANT_SUPPLY_C] > 30);
  const secondToBus = startAndLoad(sim, 60);
  assert.ok(Math.abs(secondToBus - firstToBus) < 25, `to the bus in ${secondToBus} s, ${firstToBus} s the first time`);
  assert.equal(sim.snapshot()[SNAP.START_COUNT], 2);
  sim.free();
});

test("emergency stop, two hours at rest, acknowledgement, start", () => {
  const sim = new KguSim();
  startAndLoad(sim, 60);
  sim.cmd_emergency_stop();
  const toRest = advanceUntil(sim, 60, "rest", (s) => s[SNAP.SPEED_RPM] === 0);
  assert.ok(toRest > 11 && toRest < 13, `at rest ${toRest} s after the emergency stop`);
  stand(sim, 7200);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.TRIP, "still latched");
  assert.ok(relativeResidual(sim.snapshot()) < 1e-9);
  sim.cmd_ack();
  advance(sim, 0.05);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.READY);
  startAndLoad(sim, 60);
  sim.free();
});

test("interrupted and failed starts leave a unit that stands and starts", () => {
  const sim = new KguSim();
  const limits = sim.limits();
  assert.ok(limits[LIMIT.SHAFT_STATIC_FRICTION_NM] > 0);
  assert.ok(limits[LIMIT.SHAFT_REST_SPEED_RPM] > 0 && limits[LIMIT.SHAFT_REST_SPEED_RPM] < 5);
  // Interrupted while pre-lubricating: the shaft never turned.
  sim.cmd_start();
  advance(sim, 10);
  assert.equal(sim.snapshot()[SNAP.SEQUENCE_STEP], SEQUENCE.PRE_LUBRICATION);
  sim.cmd_stop();
  advance(sim, 1);
  assert.equal(sim.snapshot()[SNAP.STATE_CODE], STATE.READY);
  stand(sim, 600);
  // Interrupted while cranking: the starter had the shaft turning.
  sim.cmd_start();
  advance(sim, limits[LIMIT.PRE_LUBRICATION_TIME_S] + 3);
  assert.equal(sim.snapshot()[SNAP.SEQUENCE_STEP], SEQUENCE.CRANKING);
  assert.ok(sim.snapshot()[SNAP.SPEED_RPM] > 50);
  sim.cmd_stop();
  advanceUntil(sim, 30, "rest", (s) => s[SNAP.SPEED_RPM] === 0);
  advanceUntil(sim, 5, "ready", (s) => s[SNAP.STATE_CODE] === STATE.READY);
  stand(sim, 600);
  // A start that fails for want of gas: cranked, tripped, coasted down.
  sim.set_gas_gauge_kpa(0);
  sim.cmd_start();
  advanceUntil(sim, 120, "failed start", (s) => s[SNAP.STATE_CODE] === STATE.TRIP);
  assert.notEqual(sim.snapshot()[SNAP.ALARM_BITS] & ALARM.FAILED_START, 0);
  advanceUntil(sim, 30, "rest", (s) => s[SNAP.SPEED_RPM] === 0);
  stand(sim, 600);
  assert.equal(sim.snapshot()[SNAP.GAS_STD_M3], 0);
  // Gas back, acknowledged: it starts and takes load.
  sim.set_gas_gauge_kpa(50); // the supply's default gauge pressure
  sim.cmd_ack();
  advance(sim, 0.05);
  startAndLoad(sim, 50);
  sim.free();
});
