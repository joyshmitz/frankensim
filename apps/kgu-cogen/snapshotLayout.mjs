// Layout of the flat arrays exported by the fs-kgu-cogen engine (engine-kgu/src/lib.rs).
//
// The engine is the source of truth: `KguSim.snapshot_field_names()` and
// `KguSim.limit_field_names()` return the same names in the same order, and
// test/kguEngine.test.mjs fails when this file and the engine disagree.
// Slot [0] of the snapshot is the layout version; within one version slots are
// only appended. Units are in the names. A slot with nothing to show (an
// efficiency without fuel, a gas temperature without gas) holds NaN: the page
// shows a dash, never a made-up number.
//
// The engine returns numbers and field names only. The controller's event
// codes named in the comments below appear nowhere in what it returns.

export const SNAPSHOT_LAYOUT_VERSION = 1;

export const SNAPSHOT_FIELDS = Object.freeze([
  "LAYOUT_VERSION",
  "TIME_S",
  "STATE_CODE",
  "SEQUENCE_STEP",
  "BREAKER_CLOSED",
  "ALARM_BITS",
  "SPEED_RPM",
  "TORQUE_INDICATED_NM",
  "TORQUE_FRICTION_NM",
  "TORQUE_STARTER_NM",
  "TORQUE_GENERATOR_NM",
  "TORQUE_NET_NM",
  "ELECTRIC_POWER_KW",
  "REACTIVE_POWER_KVAR",
  "STATOR_CURRENT_A",
  "POWER_FACTOR",
  "LOAD_ANGLE_DEG",
  "GENERATOR_VOLTAGE_V",
  "BUS_VOLTAGE_KV",
  "BUS_FREQUENCY_HZ",
  "GENERATOR_FREQUENCY_HZ",
  "SYNC_VOLTAGE_DIFFERENCE_PU",
  "SYNC_FREQUENCY_DIFFERENCE_HZ",
  "SYNC_PHASE_ANGLE_DEG",
  "SYNC_PERMITTED",
  "FUEL_POWER_KW",
  "GAS_FLOW_STD_M3_H",
  "GAS_FLOW_METER_M3_H",
  "GAS_VALVE_LIFT",
  "VALVE_DEMAND",
  "BRAKE_EFFICIENCY_PCT",
  "ELECTRIC_EFFICIENCY_PCT",
  "HEAT_TO_NETWORK_KW",
  "HEAT_DUMPED_KW",
  "EXHAUST_LOSS_KW",
  "RADIATION_LOSS_KW",
  "FRICTION_POWER_KW",
  "GENERATOR_LOSS_KW",
  "JACKET_HEAT_KW",
  "COOLANT_SUPPLY_C",
  "COOLANT_RETURN_C",
  "EXHAUST_TEMPERATURE_C",
  "LOAD_REFERENCE_PCT",
  "SPEED_REFERENCE_RPM",
  "PASSPORT_LOAD_PCT",
  "LEDGER_RESIDUAL_J",
  "LEDGER_BOUNDARY_ENERGY_MJ",
  "ENERGY_ELECTRIC_KWH",
  "ENERGY_HEAT_KWH",
  "ENERGY_FUEL_KWH",
  "GAS_STD_M3",
  "GAS_METER_M3",
  "ENGINE_HOURS",
  "START_COUNT",
  "KINETIC_ENERGY_KJ",
  "NEWTON_ITERATIONS",
  "LAST_CLOSE_REFUSAL_BITS",
  "EXHAUST_RECOVERED_KW",
  "STACK_TEMPERATURE_C",
  "NETWORK_SUPPLY_C",
  "THERMOSTAT_OPENING",
  "DUMP_VALVE_OPENING",
  "BLOCK_LOSS_KW",
  "THERMAL_EFFICIENCY_PCT",
  "ENERGY_DUMPED_KWH",
]);

/** Index of every snapshot slot by name: `snapshot[SNAP.SPEED_RPM]`. */
export const SNAP = Object.freeze(
  Object.fromEntries(SNAPSHOT_FIELDS.map((name, index) => [name, index])),
);

export const SNAPSHOT_LEN = SNAPSHOT_FIELDS.length;

/** `STATE_CODE`: the unit states of the station's dictionary. */
export const STATE = Object.freeze({
  READY: 0,
  STARTING: 1,
  SYNCHRONIZING: 2,
  RUNNING: 3,
  STOPPING: 4,
  TRIP: 5,
});

/** `SEQUENCE_STEP`: the controller's own steps, finer than the unit state. */
export const SEQUENCE = Object.freeze({
  READY: 0,
  PRE_LUBRICATION: 1,
  CRANKING: 2,
  RUN_UP: 3,
  WARM_UP: 4,
  SYNCHRONISING: 5,
  ON_LOAD: 6,
  UNLOADING: 7,
  COOL_DOWN: 8,
  COAST_DOWN: 9,
  TRIPPED: 10,
});

/**
 * `ALARM_BITS`, one bit per cause. Trips latch until `cmd_ack()`; warnings
 * follow their condition. A-/W- codes are the station controller's event
 * codes; the others are this model's own.
 */
export const ALARM = Object.freeze({
  COOLANT_TEMPERATURE: 1 << 0, // A-105, trip
  GAS_PRESSURE: 1 << 1, // A-150, trip
  OIL_PRESSURE: 1 << 2, // A-160, trip (input-driven)
  GRID_UNDERVOLTAGE: 1 << 3, // A-031, trip
  OVERSPEED: 1 << 4, // trip
  EMERGENCY_STOP: 1 << 5, // trip
  FAILED_START: 1 << 6, // W-210, latched
  OUT_OF_STEP: 1 << 7, // generator pole slip, trip
  COOLER_FAN: 1 << 8, // W-118, warning
  COOLANT_WARNING: 1 << 9, // warning
  GAS_PRESSURE_WARNING: 1 << 10, // warning
});

/** Bits returned by `cmd_breaker_close()`; zero means accepted. */
export const CLOSE_REFUSAL = Object.freeze({
  VOLTAGE: 1 << 0,
  FREQUENCY: 1 << 1,
  PHASE: 1 << 2,
  DEAD_BUS: 1 << 3,
  NOT_SYNCHRONISING: 1 << 4,
  TRIP_LATCHED: 1 << 5,
});

export const LIMIT_FIELDS = Object.freeze([
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
]);

/** Index of every entry of `limits()` by name. */
export const LIMIT = Object.freeze(
  Object.fromEntries(LIMIT_FIELDS.map((name, index) => [name, index])),
);

/** `events()` is a flat array of `[time_s, kind, code, value]` per event. */
export const EVENT_STRIDE = 4;

export const EVENT_KIND = Object.freeze({
  STATE: 1, // code: new STATE; value: shaft speed, rpm
  BREAKER: 2, // code: 1 closed, 0 open; value: power before the change, kW
  ALARM_RAISED: 3, // code: bit index of ALARM
  ALARM_CLEARED: 4, // code: bit index of ALARM
  REFUSED: 5, // code: CLOSE_REFUSAL bits, or 0 for a refused start
  SEQUENCE: 6, // code: new SEQUENCE; value: shaft speed, rpm
});

/** Decode `events()` into objects. */
export function decodeEvents(flat) {
  const events = [];
  for (let i = 0; i + EVENT_STRIDE <= flat.length; i += EVENT_STRIDE) {
    events.push({ timeS: flat[i], kind: flat[i + 1], code: flat[i + 2], value: flat[i + 3] });
  }
  return events;
}
