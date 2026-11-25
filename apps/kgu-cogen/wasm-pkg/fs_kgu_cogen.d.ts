/* tslint:disable */
/* eslint-disable */

/**
 * The cogeneration unit as the console drives it.
 */
export class KguSim {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Empty the event log.
     */
    clear_events(): void;
    /**
     * Acknowledge alarms; a latched trip whose cause is gone is cleared.
     */
    cmd_ack(): void;
    /**
     * Request to close the generator breaker. Returns zero when the request
     * is accepted (the breaker closes in the next sub-step), otherwise the
     * reasons as bits: 1 voltage difference, 2 frequency difference, 4 phase
     * angle, 8 dead bus, 16 the unit is not synchronising, 32 a trip is
     * latched. A refused request does nothing.
     */
    cmd_breaker_close(): number;
    /**
     * Open the generator breaker by hand (a load rejection when on load).
     * The synchroniser does not reclose by itself afterwards.
     */
    cmd_breaker_open(): void;
    /**
     * Emergency stop: gas shut and breaker open at once; latches.
     */
    cmd_emergency_stop(): void;
    /**
     * Start command. Refused by the controller unless the unit is ready.
     */
    cmd_start(): void;
    /**
     * Normal stop: unload, open the breaker, cool down, shut the gas.
     */
    cmd_stop(): void;
    /**
     * The event log, oldest first, four values per event: time [s], kind
     * (1 state, 2 breaker, 3 alarm raised, 4 alarm cleared, 5 command
     * refused, 6 sequence step), code, value.
     */
    events(): Float64Array;
    /**
     * The last solver refusal, empty when healthy.
     */
    last_error(): string;
    /**
     * Names of the entries of [`Self::limits`], comma separated.
     */
    limit_field_names(): string;
    /**
     * Thresholds and ratings the page draws against; names are
     * [`Self::limit_field_names`].
     */
    limits(): Float64Array;
    /**
     * Fixed facts of the machine: `[cylinders, pole pairs, shaft inertia
     * kg m2, generator kVA, generator V, bus kV, fitted friction power at
     * rated speed kW, rated fuel flow kg/s]`.
     */
    machine_facts(): Float64Array;
    /**
     * A unit at standstill, cold and ready to start, on a healthy bus.
     */
    constructor();
    /**
     * Atmospheric pressure [kPa]; default 101.325.
     */
    set_atm_pressure_kpa(kpa: number): void;
    /**
     * Whether the synchroniser closes the breaker by itself; default on.
     */
    set_auto_sync(on: boolean): void;
    /**
     * Machine-condition factor [-]; default 0.955.
     */
    set_condition_factor(factor: number): void;
    /**
     * Excitation [pu]; the open-circuit voltage before synchronising, the
     * EMF behind the reactance after it.
     */
    set_excitation_pu(pu: number): void;
    /**
     * Fault input: emergency cooler fan failed.
     */
    set_fan_failed(failed: boolean): void;
    /**
     * Gas supply gauge pressure [kPa].
     */
    set_gas_gauge_kpa(kpa: number): void;
    /**
     * Gas supply temperature [C].
     */
    set_gas_temp_c(celsius: number): void;
    /**
     * Bus frequency [Hz].
     */
    set_grid_freq_hz(hz: number): void;
    /**
     * Bus voltage [pu of 10 kV].
     */
    set_grid_voltage_pu(pu: number): void;
    /**
     * Heat demand of the network, 0 to 1: the share of its rated water flow
     * that passes the plate heat exchanger. What the network does not take
     * the emergency cooler has to reject.
     */
    set_heat_demand_frac(frac: number): void;
    /**
     * Load ramp of the controller [% of rated per minute]; default 10.
     */
    set_load_ramp_pct_per_min(pct_per_min: number): void;
    /**
     * Load setpoint [% of rated], 0 to 100.
     */
    set_load_setpoint_pct(pct: number): void;
    /**
     * Temperature of the water returning from the network [C].
     */
    set_network_return_temp_c(celsius: number): void;
    /**
     * Fault input: low oil pressure (input-driven; no oil circuit is modelled).
     */
    set_oil_pressure_low(low: boolean): void;
    /**
     * The flat snapshot; slot indices are [`snap`], names
     * [`Self::snapshot_field_names`].
     */
    snapshot(): Float64Array;
    /**
     * Names of the snapshot slots in order, comma separated.
     */
    snapshot_field_names(): string;
    /**
     * Advance by `dt_s` in the plant's fixed 10 ms sub-steps. Returns false
     * when the kernel refused a sub-step; see [`Self::last_error`].
     */
    step(dt_s: number): boolean;
    /**
     * The plant's sub-step [s].
     */
    sub_step_s(): number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_kgusim_free: (a: number, b: number) => void;
    readonly kgusim_clear_events: (a: number) => void;
    readonly kgusim_cmd_ack: (a: number) => void;
    readonly kgusim_cmd_breaker_close: (a: number) => number;
    readonly kgusim_cmd_breaker_open: (a: number) => void;
    readonly kgusim_cmd_emergency_stop: (a: number) => void;
    readonly kgusim_cmd_start: (a: number) => void;
    readonly kgusim_cmd_stop: (a: number) => void;
    readonly kgusim_events: (a: number) => [number, number];
    readonly kgusim_last_error: (a: number) => [number, number];
    readonly kgusim_limit_field_names: (a: number) => [number, number];
    readonly kgusim_limits: (a: number) => [number, number];
    readonly kgusim_machine_facts: (a: number) => [number, number];
    readonly kgusim_new: () => number;
    readonly kgusim_set_atm_pressure_kpa: (a: number, b: number) => void;
    readonly kgusim_set_auto_sync: (a: number, b: number) => void;
    readonly kgusim_set_condition_factor: (a: number, b: number) => void;
    readonly kgusim_set_excitation_pu: (a: number, b: number) => void;
    readonly kgusim_set_fan_failed: (a: number, b: number) => void;
    readonly kgusim_set_gas_gauge_kpa: (a: number, b: number) => void;
    readonly kgusim_set_gas_temp_c: (a: number, b: number) => void;
    readonly kgusim_set_grid_freq_hz: (a: number, b: number) => void;
    readonly kgusim_set_grid_voltage_pu: (a: number, b: number) => void;
    readonly kgusim_set_heat_demand_frac: (a: number, b: number) => void;
    readonly kgusim_set_load_ramp_pct_per_min: (a: number, b: number) => void;
    readonly kgusim_set_load_setpoint_pct: (a: number, b: number) => void;
    readonly kgusim_set_network_return_temp_c: (a: number, b: number) => void;
    readonly kgusim_set_oil_pressure_low: (a: number, b: number) => void;
    readonly kgusim_snapshot: (a: number) => [number, number];
    readonly kgusim_snapshot_field_names: (a: number) => [number, number];
    readonly kgusim_step: (a: number, b: number) => number;
    readonly kgusim_sub_step_s: (a: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
