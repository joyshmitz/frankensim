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
  SEQUENCE,
  SNAP,
  SNAPSHOT_FIELDS,
  SNAPSHOT_LAYOUT_VERSION,
  STATE,
  decodeEvents,
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
});
