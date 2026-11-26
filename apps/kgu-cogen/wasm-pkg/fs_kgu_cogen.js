/* @ts-self-types="./fs_kgu_cogen.d.ts" */

/**
 * The cogeneration unit as the console drives it.
 */
export class KguSim {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        KguSimFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_kgusim_free(ptr, 0);
    }
    /**
     * The power-quality analyser's record of closed voltage events, oldest
     * first, five values per event: kind (1 dip, 2 swell, 3 interruption),
     * phase (1, 2, 3 for a, b, c), start by the analyser's own clock [s],
     * duration [s], residual or highest voltage [V, phase to neutral].
     * @returns {Float64Array}
     */
    analyser_events() {
        const ret = wasm.kgusim_analyser_events(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * Empty the analyser's record. Events still open stay open.
     */
    clear_analyser_events() {
        wasm.kgusim_clear_analyser_events(this.__wbg_ptr);
    }
    /**
     * Empty the event log.
     */
    clear_events() {
        wasm.kgusim_clear_events(this.__wbg_ptr);
    }
    /**
     * Empty the meter's journals and their overflow flags. The energy
     * registers are not touched.
     */
    clear_meter_events() {
        wasm.kgusim_clear_meter_events(this.__wbg_ptr);
    }
    /**
     * Acknowledge alarms; a latched trip whose cause is gone is cleared.
     */
    cmd_ack() {
        wasm.kgusim_cmd_ack(this.__wbg_ptr);
    }
    /**
     * Request to close the generator breaker. Returns zero when the breaker
     * may be closed from the present state, otherwise the reasons as bits:
     * 1 voltage difference, 2 frequency difference, 4 phase angle, 8 dead
     * bus, 16 the unit is not synchronising, 32 a trip is latched, 64 the
     * bus is not fit by the cabinet's settings (a phase, the unbalance or
     * the frequency outside them). A refused request does nothing but leave
     * its entry in the log. An accepted one is handed to the controller,
     * which takes the same permission once more in the sub-step in which it
     * closes: its answer differs only if an input was changed in between,
     * and is then in the log.
     * @returns {number}
     */
    cmd_breaker_close() {
        const ret = wasm.kgusim_cmd_breaker_close(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Open the generator breaker by hand (a load rejection when on load).
     * The synchroniser does not reclose by itself afterwards. While the
     * unit is unloading for a stop, the stop goes on to its cool-down.
     */
    cmd_breaker_open() {
        wasm.kgusim_cmd_breaker_open(this.__wbg_ptr);
    }
    /**
     * Emergency stop: gas shut and breaker open at once; latches.
     */
    cmd_emergency_stop() {
        wasm.kgusim_cmd_emergency_stop(this.__wbg_ptr);
    }
    /**
     * Start command. Refused by the controller unless the unit is ready.
     */
    cmd_start() {
        wasm.kgusim_cmd_start(this.__wbg_ptr);
    }
    /**
     * Normal stop: unload, open the breaker, cool down, shut the gas.
     */
    cmd_stop() {
        wasm.kgusim_cmd_stop(this.__wbg_ptr);
    }
    /**
     * Command to synchronise: the operator decides, the automatic
     * synchroniser closes the breaker near zero phase, for this one attempt.
     * Accepted on the synchronising step (a start with the automatic
     * synchronisation off, after a manual opening, after the acknowledgement
     * in manual restoration). Returns zero when accepted, otherwise the
     * reasons with the bits of [`Self::cmd_breaker_close`]: 16 the unit is
     * not synchronising, 32 a trip is latched, 8 the bus is dead, 64 the
     * bus is not fit by the cabinet's settings. (The synchroniser waits by
     * itself for the phase and for the voltage and frequency differences;
     * it cannot wait out a bus it must not close onto.) A normal stop
     * cancels the wait. Either way the command is in the event log.
     * @returns {number}
     */
    cmd_synchronise() {
        const ret = wasm.kgusim_cmd_synchronise(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The event log, oldest first, four values per event: time [s], kind
     * (1 state, 2 breaker, 3 alarm raised, 4 alarm cleared, 5 command
     * refused, 6 sequence step, 7 cabinet protection trip, 8 the operator's
     * command to synchronise, 9 outcome of a command, 10 synchronising time
     * limit), code, value. For kind 9 the code is the command (1 start,
     * 2 stop, 3 emergency stop, 4 acknowledge, 5 breaker close, 6 breaker
     * open, 7 synchronise) and the value zero when it was carried out, else
     * the refusal bits. For kind 10 the code is what kept the breaker out
     * (refusal bits) and the value the time at the synchroniser [s].
     * For a protection trip the code is ten times the function's index plus
     * the phase (1 to 3, or 0), and the value is what it tripped at. For a
     * command to synchronise the code is zero when it was accepted, else the
     * refusal bits, and the value is the phase angle at the command [deg].
     * @returns {Float64Array}
     */
    events() {
        const ret = wasm.kgusim_events(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * The last solver refusal, empty when healthy.
     * @returns {string}
     */
    last_error() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.kgusim_last_error(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Names of the entries of [`Self::limits`], comma separated.
     * @returns {string}
     */
    limit_field_names() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.kgusim_limit_field_names(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Thresholds and ratings the page draws against; names are
     * [`Self::limit_field_names`].
     * @returns {Float64Array}
     */
    limits() {
        const ret = wasm.kgusim_limits(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * The longest time one call of [`Self::step`] takes [s].
     * @returns {number}
     */
    longest_step_s() {
        const ret = wasm.kgusim_longest_step_s(this.__wbg_ptr);
        return ret;
    }
    /**
     * Fixed facts of the machine: `[cylinders, pole pairs, shaft inertia
     * kg m2, generator kVA, generator V, bus kV, fitted friction power at
     * rated speed kW, rated fuel flow kg/s]`.
     * @returns {Float64Array}
     */
    machine_facts() {
        const ret = wasm.kgusim_machine_facts(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * The revenue meter's journals: the low-voltage journal first, then
     * high voltage, then loss of voltage, oldest first within each. Four
     * values per record: journal (1 low voltage, 2 high voltage, 3 loss of
     * voltage), phases seen (bit 0 phase a, 1 b, 2 c), the supervision
     * instant that first saw the condition and the first later one that saw
     * it gone (NaN until then), both by the meter's own clock [s]. The meter
     * knows no depth and no duration finer than its supervision period.
     * @returns {Float64Array}
     */
    meter_events() {
        const ret = wasm.kgusim_meter_events(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * A unit at standstill, cold and ready to start, on a healthy bus.
     */
    constructor() {
        const ret = wasm.kgusim_new();
        this.__wbg_ptr = ret;
        KguSimFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * How far the power-quality analyser's clock is ahead of the model's
     * time [s]. The controller's and the protection's events stay in model
     * time.
     * @param {number} offset_s
     */
    set_analyser_clock_offset_s(offset_s) {
        wasm.kgusim_set_analyser_clock_offset_s(this.__wbg_ptr, offset_s);
    }
    /**
     * Atmospheric pressure [kPa]; default 101.325.
     * @param {number} kpa
     */
    set_atm_pressure_kpa(kpa) {
        wasm.kgusim_set_atm_pressure_kpa(this.__wbg_ptr, kpa);
    }
    /**
     * Whether the synchroniser closes the breaker by itself; default on.
     * @param {boolean} on
     */
    set_auto_sync(on) {
        wasm.kgusim_set_auto_sync(this.__wbg_ptr, on);
    }
    /**
     * Machine-condition factor [-]; default 0.955.
     * @param {number} factor
     */
    set_condition_factor(factor) {
        wasm.kgusim_set_condition_factor(this.__wbg_ptr, factor);
    }
    /**
     * Excitation [pu]; the open-circuit voltage before synchronising, the
     * EMF behind the reactance after it.
     * @param {number} pu
     */
    set_excitation_pu(pu) {
        wasm.kgusim_set_excitation_pu(this.__wbg_ptr, pu);
    }
    /**
     * Fault input: emergency cooler fan failed.
     * @param {boolean} failed
     */
    set_fan_failed(failed) {
        wasm.kgusim_set_fan_failed(this.__wbg_ptr, failed);
    }
    /**
     * Gas supply gauge pressure [kPa].
     * @param {number} kpa
     */
    set_gas_gauge_kpa(kpa) {
        wasm.kgusim_set_gas_gauge_kpa(this.__wbg_ptr, kpa);
    }
    /**
     * Gas supply temperature [C].
     * @param {number} celsius
     */
    set_gas_temp_c(celsius) {
        wasm.kgusim_set_gas_temp_c(this.__wbg_ptr, celsius);
    }
    /**
     * Bus frequency [Hz].
     * @param {number} hz
     */
    set_grid_freq_hz(hz) {
        wasm.kgusim_set_grid_freq_hz(this.__wbg_ptr, hz);
    }
    /**
     * How far each phase voltage is turned from its nominal position
     * [degrees], phases a, b, c; zero for a grid with its phases a third of
     * a turn apart.
     * @param {number} a
     * @param {number} b
     * @param {number} c
     */
    set_grid_phase_angles_deg(a, b, c) {
        wasm.kgusim_set_grid_phase_angles_deg(this.__wbg_ptr, a, b, c);
    }
    /**
     * Magnitudes of the three phase voltages of the bus [pu of the rated
     * phase voltage], phases a, b, c. A grid disturbance is a circumstance
     * of the scenario: any phase, any depth.
     * @param {number} a
     * @param {number} b
     * @param {number} c
     */
    set_grid_phase_voltages_pu(a, b, c) {
        wasm.kgusim_set_grid_phase_voltages_pu(this.__wbg_ptr, a, b, c);
    }
    /**
     * Bus voltage [pu]: all three phases at this magnitude.
     * @param {number} pu
     */
    set_grid_voltage_pu(pu) {
        wasm.kgusim_set_grid_voltage_pu(this.__wbg_ptr, pu);
    }
    /**
     * Heat demand of the network, 0 to 1: the share of its rated water flow
     * that passes the plate heat exchanger. What the network does not take
     * the emergency cooler has to reject.
     * @param {number} frac
     */
    set_heat_demand_frac(frac) {
        wasm.kgusim_set_heat_demand_frac(this.__wbg_ptr, frac);
    }
    /**
     * Load ramp of the controller [% of rated per minute]; default 10.
     * @param {number} pct_per_min
     */
    set_load_ramp_pct_per_min(pct_per_min) {
        wasm.kgusim_set_load_ramp_pct_per_min(this.__wbg_ptr, pct_per_min);
    }
    /**
     * Load setpoint [% of rated], 0 to 100.
     * @param {number} pct
     */
    set_load_setpoint_pct(pct) {
        wasm.kgusim_set_load_setpoint_pct(this.__wbg_ptr, pct);
    }
    /**
     * How far the revenue meter's clock is ahead of the model's time [s].
     * The meter's journals are stamped with its own clock.
     * @param {number} offset_s
     */
    set_meter_clock_offset_s(offset_s) {
        wasm.kgusim_set_meter_clock_offset_s(this.__wbg_ptr, offset_s);
    }
    /**
     * Where within its supervision period the meter looks at the phase
     * voltages [s]: the instants are at whole periods of the meter's own
     * clock plus this. Taken modulo the period (one second by the card).
     * @param {number} phase_s
     */
    set_meter_sample_phase_s(phase_s) {
        wasm.kgusim_set_meter_sample_phase_s(this.__wbg_ptr, phase_s);
    }
    /**
     * Temperature of the water returning from the network [C].
     * @param {number} celsius
     */
    set_network_return_temp_c(celsius) {
        wasm.kgusim_set_network_return_temp_c(this.__wbg_ptr, celsius);
    }
    /**
     * Fault input: low oil pressure (input-driven; no oil circuit is modelled).
     * @param {boolean} low
     */
    set_oil_pressure_low(low) {
        wasm.kgusim_set_oil_pressure_low(this.__wbg_ptr, low);
    }
    /**
     * How the unit comes back after the cabinet's protection has opened the
     * breaker on a grid event: `0` manual (people acknowledge and close),
     * `1` automatic (the automation waits for a healthy grid, synchronises
     * and loads the unit by itself). Default manual.
     * @param {number} mode
     */
    set_restoration_mode(mode) {
        wasm.kgusim_set_restoration_mode(this.__wbg_ptr, mode);
    }
    /**
     * The flat snapshot; slot indices are [`snap`], names
     * [`Self::snapshot_field_names`].
     * @returns {Float64Array}
     */
    snapshot() {
        const ret = wasm.kgusim_snapshot(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * Names of the snapshot slots in order, comma separated.
     * @returns {string}
     */
    snapshot_field_names() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.kgusim_snapshot_field_names(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Advance by `dt_s` in the plant's fixed 10 ms sub-steps. Returns false,
     * with the reason in [`Self::last_error`], when the kernel refused a
     * sub-step, or when `dt_s` is not a positive number of at most
     * [`Self::longest_step_s`]: a longer step is refused whole, never cut
     * short.
     * @param {number} dt_s
     * @returns {boolean}
     */
    step(dt_s) {
        const ret = wasm.kgusim_step(this.__wbg_ptr, dt_s);
        return ret !== 0;
    }
    /**
     * The plant's sub-step [s].
     * @returns {number}
     */
    sub_step_s() {
        const ret = wasm.kgusim_sub_step_s(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) KguSim.prototype[Symbol.dispose] = KguSim.prototype.free;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_5d9e815e6fdf150f: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./fs_kgu_cogen_bg.js": import0,
    };
}

const KguSimFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_kgusim_free(ptr, 1));

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedFloat64ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('fs_kgu_cogen_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
