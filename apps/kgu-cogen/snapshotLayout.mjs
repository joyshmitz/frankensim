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
  // Slice 1c: the bus's three phases, the breaker cabinet, restoration, the analyser.
  "PHASE_A_VOLTAGE_V",
  "PHASE_B_VOLTAGE_V",
  "PHASE_C_VOLTAGE_V",
  "PHASE_A_CURRENT_A",
  "PHASE_B_CURRENT_A",
  "PHASE_C_CURRENT_A",
  "VOLTAGE_UNBALANCE_PCT",
  "NEGATIVE_SEQUENCE_CURRENT_PCT",
  "ROTOR_THERMAL_USED_FRAC",
  "PROTECTION_PICKUP_BITS",
  "PROTECTION_TRIPPED_BITS",
  "NEGATIVE_SEQUENCE_LOSS_KW",
  "RESTORATION_MODE",
  "GRID_HEALTHY_COUNTDOWN_S",
  "RESTORATION_ATTEMPTS",
  "WAITING_FOR_GRID",
  "ANALYSER_CLOCK_S",
  "ANALYSER_OPEN_EVENTS",
  "ANALYSER_RECORDED_EVENTS",
  // Slice 1d: the revenue meter, and synchronisation on the operator's command.
  "METER_ACTIVE_EXPORT_KWH",
  "METER_ACTIVE_IMPORT_KWH",
  "METER_REACTIVE_EXPORT_KVARH",
  "METER_REACTIVE_IMPORT_KVARH",
  "METER_CLOCK_S",
  "METER_LOW_VOLTAGE_RECORDS",
  "METER_HIGH_VOLTAGE_RECORDS",
  "METER_LOSS_OF_VOLTAGE_RECORDS",
  "METER_OVERFLOW_BITS",
  "SYNCHRONISER_ARMED_BY_OPERATOR",
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
  GRID_WAIT: 11, // off the bus after a grid event, engine running unloaded; STATE is SYNCHRONIZING
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
  // Raised by the breaker cabinet's protection. Grid events open the breaker and leave
  // the engine running unloaded; unit faults stop it.
  VOLTAGE_UNBALANCE: 1 << 11, // grid event
  NEGATIVE_SEQUENCE: 1 << 12, // grid event: rotor thermal limit, (I2/In)^2 t
  OVERCURRENT: 1 << 13, // unit fault, trip
  OVERVOLTAGE: 1 << 14, // grid event
  FREQUENCY: 1 << 15, // grid event
  REVERSE_POWER: 1 << 16, // unit fault, trip
  RESTORATION_LOCKOUT: 1 << 17, // too many automatic returns; latched until cmd_ack()
});

/**
 * Protection functions of the breaker cabinet: the index is the function, and
 * `1 << index` its bit in `PROTECTION_PICKUP_BITS` and `PROTECTION_TRIPPED_BITS`.
 */
export const PROTECTION = Object.freeze({
  UNDERVOLTAGE: 0, // per phase
  OVERVOLTAGE: 1, // per phase
  VOLTAGE_UNBALANCE: 2, // U2/U1
  NEGATIVE_SEQUENCE: 3, // rotor thermal limit
  OVERCURRENT: 4, // per phase, time-delayed
  OVERCURRENT_HIGH: 5, // per phase, high-set
  UNDERFREQUENCY: 6,
  OVERFREQUENCY: 7,
  REVERSE_POWER: 8,
});

/** `RESTORATION_MODE` and the argument of `set_restoration_mode()`. */
export const RESTORATION = Object.freeze({ MANUAL: 0, AUTOMATIC: 1 });

/**
 * Decode the `code` of a `PROTECTION` event: which function tripped and on which
 * phase (1, 2, 3 for a, b, c; 0 when the function is not about one phase). The
 * event's `value` is what it tripped at: phase voltage in V, unbalance in %,
 * negative-sequence current in % of rated, phase current in A, frequency in Hz,
 * reverse power in kW.
 */
export function decodeProtectionCode(code) {
  return { protection: Math.floor(code / 10), phase: code % 10 };
}

/**
 * Bits returned by `cmd_breaker_close()`; zero means accepted. `cmd_synchronise()`
 * returns the same bits, of which only NOT_SYNCHRONISING and TRIP_LATCHED can refuse it.
 */
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
  // The breaker cabinet's protection settings.
  "RATED_CURRENT_A",
  "RATED_PHASE_VOLTAGE_V",
  "PHASE_UNDERVOLTAGE_PU",
  "PHASE_UNDERVOLTAGE_DELAY_S",
  "PHASE_OVERVOLTAGE_PU",
  "PHASE_OVERVOLTAGE_DELAY_S",
  "VOLTAGE_UNBALANCE_TRIP_PCT",
  "VOLTAGE_UNBALANCE_DELAY_S",
  "NEGATIVE_SEQUENCE_PICKUP_PCT",
  "NEGATIVE_SEQUENCE_LIMIT_S",
  "NEGATIVE_SEQUENCE_COOLING_S",
  "OVERCURRENT_A",
  "OVERCURRENT_DELAY_S",
  "OVERCURRENT_HIGH_A",
  "OVERCURRENT_HIGH_DELAY_S",
  "UNDERFREQUENCY_HZ",
  "UNDERFREQUENCY_DELAY_S",
  "OVERFREQUENCY_HZ",
  "OVERFREQUENCY_DELAY_S",
  "REVERSE_POWER_KW",
  "REVERSE_POWER_DELAY_S",
  // Restoration after a grid event.
  "HEALTHY_VOLTAGE_LOW_PU",
  "HEALTHY_VOLTAGE_HIGH_PU",
  "HEALTHY_UNBALANCE_PCT",
  "HEALTHY_FREQUENCY_LOW_HZ",
  "HEALTHY_FREQUENCY_HIGH_HZ",
  "HEALTHY_GRID_DELAY_S",
  "LONGEST_GRID_WAIT_S",
  "LARGEST_AUTO_ATTEMPTS",
  "ATTEMPT_WINDOW_S",
  // The power-quality analyser's thresholds.
  "ANALYSER_DIP_PU",
  "ANALYSER_SWELL_PU",
  "ANALYSER_INTERRUPTION_PU",
  "ANALYSER_HYSTERESIS_PU",
  // The revenue meter's card.
  "METER_SUPERVISION_PERIOD_S",
  "METER_LOW_VOLTAGE_PU",
  "METER_HIGH_VOLTAGE_PU",
  "METER_LOSS_OF_VOLTAGE_PU",
  "METER_LOW_VOLTAGE_CAPACITY",
  "METER_HIGH_VOLTAGE_CAPACITY",
  "METER_LOSS_OF_VOLTAGE_CAPACITY",
  "METER_OVERFLOW_OVERWRITES_OLDEST", // 1: a full journal displaces its oldest record; 0: it keeps it
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
  PROTECTION: 7, // cabinet protection trip; code: see decodeProtectionCode; value: what it tripped at
  // The operator's command to synchronise. code: 0 accepted, else CLOSE_REFUSAL bits;
  // value: phase angle at the command, degrees. A closing with no such event before it
  // was the automation's own, or a manual close request.
  SYNCHRONISE_COMMAND: 8,
});

/**
 * `meter_events()` is a flat array of `[journal, phase_bits, seen_s, cleared_s]`
 * per record: the low-voltage journal first, then high voltage, then loss of
 * voltage, oldest first within each. Both times are supervision instants of the
 * meter's OWN clock (`set_meter_clock_offset_s`, `set_meter_sample_phase_s`):
 * the look that first saw the condition and the first later look that saw it
 * gone (NaN until then). The meter looks once per supervision period, so a
 * record has no depth and no duration, and a disturbance between two looks has
 * no record at all.
 */
export const METER_EVENT_STRIDE = 4;

/** Journals of the meter; `1 << (journal - 1)` is the journal's bit in `METER_OVERFLOW_BITS`. */
export const METER_JOURNAL = Object.freeze({
  LOW_VOLTAGE: 1,
  HIGH_VOLTAGE: 2,
  LOSS_OF_VOLTAGE: 3,
});

/** Decode `meter_events()` into objects; `phases` lists the phases seen (1, 2, 3 for a, b, c). */
export function decodeMeterEvents(flat) {
  const records = [];
  for (let i = 0; i + METER_EVENT_STRIDE <= flat.length; i += METER_EVENT_STRIDE) {
    const phaseBits = flat[i + 1];
    records.push({
      journal: flat[i],
      phaseBits,
      phases: [1, 2, 3].filter((phase) => phaseBits & (1 << (phase - 1))),
      seenS: flat[i + 2],
      clearedS: flat[i + 3],
    });
  }
  return records;
}

/**
 * `analyser_events()` is a flat array of `[kind, phase, start_s, duration_s,
 * extreme_v]` per closed event. `start_s` is by the analyser's OWN clock
 * (`set_analyser_clock_offset_s`), not model time; `events()` is in model time.
 */
export const QUALITY_EVENT_STRIDE = 5;

export const QUALITY_KIND = Object.freeze({
  DIP: 1, // extreme_v: residual (lowest) voltage
  SWELL: 2, // extreme_v: highest voltage
  INTERRUPTION: 3, // extreme_v: residual voltage
});

/** Decode `analyser_events()` into objects. */
export function decodeAnalyserEvents(flat) {
  const events = [];
  for (let i = 0; i + QUALITY_EVENT_STRIDE <= flat.length; i += QUALITY_EVENT_STRIDE) {
    events.push({
      kind: flat[i],
      phase: flat[i + 1],
      startS: flat[i + 2],
      durationS: flat[i + 3],
      extremeV: flat[i + 4],
    });
  }
  return events;
}

/** Decode `events()` into objects. */
export function decodeEvents(flat) {
  const events = [];
  for (let i = 0; i + EVENT_STRIDE <= flat.length; i += EVENT_STRIDE) {
    events.push({ timeS: flat[i], kind: flat[i + 1], code: flat[i + 2], value: flat[i + 3] });
  }
  return events;
}
