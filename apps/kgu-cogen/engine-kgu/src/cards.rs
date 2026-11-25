//! Cards: every number of the plant that is not computed by a conservation or
//! interface law lives here, with its origin.
//!
//! Four origins are used and named on every card:
//!
//! - **passport**: the training station's dataset passport and simulator
//!   specification (`felectra-eip/sim/virtual-kgu/SPEC.md` sections 2 and 3,
//!   `vkgu/station.py`). Those numbers are themselves conventional ("invented
//!   and plausible", SPEC section 1); they belong to no real machine.
//! - **fitted**: derived from a passport curve so that the plant reproduces
//!   it. A fitted value is an answer put in, not a result.
//! - **assumed**: an engineering assumption of this model with no source
//!   behind it. To be replaced by the machine's own documentation.
//! - **setpoint**: a setting of the genset controller (a device parameter:
//!   timers, ramps, thresholds, gains), not a result of the physics.

use core::f64::consts::PI;

// ---------------------------------------------------------------------------
// Ratings (passport)
// ---------------------------------------------------------------------------

/// Rated electrical power at the generator terminals [W]. Passport (SPEC 2).
pub const RATED_ELECTRIC_W: f64 = 1_000_000.0;
/// Rated crankshaft speed [rpm]. Passport (SPEC 2).
pub const RATED_SPEED_RPM: f64 = 1_500.0;
/// Rated crankshaft speed [rad/s].
pub const RATED_OMEGA_RAD_S: f64 = RATED_SPEED_RPM * PI / 30.0;
/// Number of cylinders. Passport (SPEC 2). Display only in stage 1: the
/// in-cylinder cycle is not modelled.
pub const CYLINDERS: u32 = 12;
/// Generator rated line-to-line voltage [V]. Passport (SPEC 2).
pub const GENERATOR_VOLTAGE_V: f64 = 400.0;
/// High-voltage side of the unit step-up transformer [V]. Passport (SPEC 2).
pub const BUS_VOLTAGE_V: f64 = 10_000.0;
/// Nominal grid frequency [Hz]. Passport (SPEC 3.1).
pub const NOMINAL_FREQUENCY_HZ: f64 = 50.0;

// ---------------------------------------------------------------------------
// Passport efficiency curves and the fuel
// ---------------------------------------------------------------------------

/// The plant passport: electrical efficiency and thermal recovery against
/// electrical load.
///
/// Origin: **passport**, SPEC 3.2 and `vkgu/station.py` (`ETA_EL`, `ETA_TH`).
/// Between points the efficiency is interpolated linearly in load, as the
/// station's simulator does. Below the first point the passport has no data;
/// see [`PassportCard::fuel_for_electric_w`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassportCard {
    /// `(load [% of rated], electrical efficiency [%])`, at the terminals.
    pub electric_efficiency_pct: [(f64, f64); 5],
    /// `(load [% of rated], recoverable heat [% of fuel power])`.
    pub thermal_recovery_pct: [(f64, f64); 5],
}

impl PassportCard {
    /// The training station's passport.
    pub const STATION: Self = Self {
        electric_efficiency_pct: [
            (30.0, 30.0),
            (50.0, 35.0),
            (75.0, 38.0),
            (90.0, 39.3),
            (100.0, 40.0),
        ],
        thermal_recovery_pct: [
            (30.0, 49.0),
            (50.0, 47.0),
            (75.0, 45.0),
            (90.0, 43.5),
            (100.0, 43.0),
        ],
    };

    fn interpolate(curve: &[(f64, f64); 5], load_pct: f64) -> f64 {
        if load_pct <= curve[0].0 {
            return curve[0].1;
        }
        for pair in curve.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            if load_pct <= x1 {
                return y0 + (y1 - y0) * (load_pct - x0) / (x1 - x0);
            }
        }
        curve[4].1
    }

    /// Passport electrical efficiency [-] at `load_frac` of rated power,
    /// held at the end values outside the tabulated range.
    #[must_use]
    pub fn electric_efficiency(&self, load_frac: f64) -> f64 {
        Self::interpolate(&self.electric_efficiency_pct, 100.0 * load_frac) / 100.0
    }

    /// Passport thermal recovery [-] (recoverable heat over fuel power) at
    /// `load_frac`, held at the end values outside the tabulated range.
    #[must_use]
    pub fn thermal_recovery(&self, load_frac: f64) -> f64 {
        Self::interpolate(&self.thermal_recovery_pct, 100.0 * load_frac) / 100.0
    }

    /// Slope [W fuel per W electric] and intercept [W fuel] of the Willans
    /// line through the two lowest passport points.
    ///
    /// **Assumption**, not passport data: below 30 % load the passport is
    /// silent, and the fuel line of the 30-50 % segment is continued to zero
    /// load. Its intercept is the fuel the engine burns at rated speed with
    /// no electrical output (357 kW for the station's passport, 14 % of the
    /// rated fuel power).
    #[must_use]
    pub fn willans_line(&self) -> (f64, f64) {
        let (l0, e0) = self.electric_efficiency_pct[0];
        let (l1, e1) = self.electric_efficiency_pct[1];
        let p0 = RATED_ELECTRIC_W * l0 / 100.0;
        let p1 = RATED_ELECTRIC_W * l1 / 100.0;
        let f0 = p0 / (e0 / 100.0);
        let f1 = p1 / (e1 / 100.0);
        let slope = (f1 - f0) / (p1 - p0);
        (slope, f0 - slope * p0)
    }

    /// Fuel power [W, lower heating value] the passport machine in perfect
    /// condition takes to deliver `electric_w` at its terminals.
    ///
    /// At and above the first passport point: `P / eta(P)`. Below it: the
    /// Willans line of [`Self::willans_line`], also for negative arguments
    /// (less fuel than the no-load line needs).
    #[must_use]
    pub fn fuel_for_electric_w(&self, electric_w: f64) -> f64 {
        let first = RATED_ELECTRIC_W * self.electric_efficiency_pct[0].0 / 100.0;
        if electric_w >= first {
            electric_w / self.electric_efficiency(electric_w / RATED_ELECTRIC_W)
        } else {
            let (slope, intercept) = self.willans_line();
            intercept + slope * electric_w
        }
    }

    /// Inverse of [`Self::fuel_for_electric_w`]: the electrical power [W] the
    /// passport machine delivers from `fuel_w`. Closed form on every segment
    /// (efficiency linear in power gives `P = a F / (1 - b F)`).
    #[must_use]
    pub fn electric_for_fuel_w(&self, fuel_w: f64) -> f64 {
        let points = &self.electric_efficiency_pct;
        let power = |index: usize| RATED_ELECTRIC_W * points[index].0 / 100.0;
        let fuel = |index: usize| power(index) / (points[index].1 / 100.0);
        if fuel_w <= fuel(0) {
            let (slope, intercept) = self.willans_line();
            return (fuel_w - intercept) / slope;
        }
        for index in 0..4 {
            if fuel_w <= fuel(index + 1) {
                // eta = a + b P on this segment.
                let (p0, p1) = (power(index), power(index + 1));
                let (e0, e1) = (points[index].1 / 100.0, points[index + 1].1 / 100.0);
                let b = (e1 - e0) / (p1 - p0);
                let a = e0 - b * p0;
                return a * fuel_w / (1.0 - b * fuel_w);
            }
        }
        // Above the last point the efficiency is held.
        fuel_w * points[4].1 / 100.0
    }
}

/// Machine-condition factor: the share of the fuel's heat that the passport
/// curve still applies to. `1.0` is the passport machine.
///
/// Origin: **passport**, SPEC 3.2 (`k_st`; 0.955 is the weekly scenario's
/// value for a healthy unit, SPEC 4.4). The engine applies the passport curve
/// to `k * fuel power`; the rest leaves with the exhaust.
pub const DEFAULT_CONDITION_FACTOR: f64 = 0.955;

/// The fuel gas.
///
/// Origin: lower heating value and standard conditions are **passport**
/// (SPEC 3.2, gas quality passport of the scenario, compressibility one).
/// Molar mass, heat capacity and isentropic exponent are **assumed**: pure
/// methane as an ideal gas, which gives 33.4 MJ/m3 at these conditions against
/// the passport's 33.5. `fs-thermochem` is not called: the passport's heating
/// value is an input here, not a property computed from a composition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuelGasCard {
    /// Lower heating value [J/m3] at the standard conditions below.
    pub lower_heating_value_j_m3: f64,
    /// Standard temperature [K].
    pub standard_temperature_k: f64,
    /// Standard pressure [Pa].
    pub standard_pressure_pa: f64,
    /// Molar mass [kg/mol] (assumed: methane).
    pub molar_mass_kg_mol: f64,
    /// Isobaric heat capacity [J/(kg K)] (assumed: methane near 290 K).
    pub cp_j_kg_k: f64,
    /// Isentropic exponent [-] (assumed: methane near 290 K).
    pub isentropic_exponent: f64,
}

/// Molar gas constant [J/(mol K)] (exact SI value).
pub const MOLAR_GAS_CONSTANT: f64 = 8.314_462_618;

impl FuelGasCard {
    /// The station's gas.
    pub const STATION: Self = Self {
        lower_heating_value_j_m3: 33.5e6,
        standard_temperature_k: 293.15,
        standard_pressure_pa: 101_325.0,
        molar_mass_kg_mol: 16.043e-3,
        cp_j_kg_k: 2_200.0,
        isentropic_exponent: 1.31,
    };

    /// Specific gas constant [J/(kg K)].
    #[must_use]
    pub fn specific_gas_constant(&self) -> f64 {
        MOLAR_GAS_CONSTANT / self.molar_mass_kg_mol
    }

    /// Density at standard conditions [kg/m3].
    #[must_use]
    pub fn standard_density_kg_m3(&self) -> f64 {
        self.standard_pressure_pa / (self.specific_gas_constant() * self.standard_temperature_k)
    }

    /// Lower heating value per unit mass [J/kg].
    #[must_use]
    pub fn lower_heating_value_j_kg(&self) -> f64 {
        self.lower_heating_value_j_m3 / self.standard_density_kg_m3()
    }

    /// Specific enthalpy [J/kg] on the stream's datum: products of complete
    /// combustion at the standard temperature with water as vapour. The
    /// chemical part is the lower heating value; the sensible part is the
    /// gas's own heat above the standard temperature.
    #[must_use]
    pub fn specific_enthalpy_j_kg(&self, temperature_k: f64) -> f64 {
        self.lower_heating_value_j_kg() + self.cp_j_kg_k * (temperature_k - self.standard_temperature_k)
    }

    /// Temperature [K] of gas with that specific enthalpy.
    #[must_use]
    pub fn temperature_k(&self, specific_enthalpy_j_kg: f64) -> f64 {
        self.standard_temperature_k
            + (specific_enthalpy_j_kg - self.lower_heating_value_j_kg()) / self.cp_j_kg_k
    }
}

// ---------------------------------------------------------------------------
// Gas control valve (assumed)
// ---------------------------------------------------------------------------

/// The gas control valve and its actuator. Origin: **assumed**.
///
/// The valve is a compressible orifice of effective area `lift * full area`
/// between the supply and the mixer, which sits at atmospheric pressure. The
/// full area is sized so that rated fuel flow at the nominal supply (50 kPa
/// gauge, 15 C: the station's daily means, SPEC 4.2) needs a lift of 0.70;
/// the supply pressure at which the open valve no longer passes rated flow
/// follows from the orifice law (about 24 kPa gauge).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GasValveCard {
    /// Effective flow area at full lift [m2] (discharge coefficient included).
    pub full_area_m2: f64,
    /// Fastest opening [1/s of lift].
    pub opening_rate_per_s: f64,
    /// Fastest closing [1/s of lift].
    pub closing_rate_per_s: f64,
}

impl GasValveCard {
    /// Supply gauge pressure the valve is sized at [Pa].
    pub const SIZING_GAUGE_PA: f64 = 50_000.0;
    /// Supply temperature the valve is sized at [K].
    pub const SIZING_TEMPERATURE_K: f64 = 288.15;
    /// Lift that passes the rated fuel flow at the sizing supply [-].
    pub const SIZING_LIFT: f64 = 0.70;

    /// Size the valve for `rated_mass_flow_kg_s` of `gas`.
    #[must_use]
    pub fn sized_for(gas: &FuelGasCard, rated_mass_flow_kg_s: f64) -> Self {
        let upstream = gas.standard_pressure_pa + Self::SIZING_GAUGE_PA;
        let per_area = orifice_mass_flux_kg_m2_s(
            gas,
            upstream,
            Self::SIZING_TEMPERATURE_K,
            gas.standard_pressure_pa,
        );
        Self {
            full_area_m2: rated_mass_flow_kg_s / (Self::SIZING_LIFT * per_area),
            opening_rate_per_s: 2.0,
            closing_rate_per_s: 5.0,
        }
    }
}

/// Mass flux [kg/(m2 s)] of an ideal gas through an orifice from `upstream_pa`
/// at `temperature_k` to `downstream_pa` (isentropic nozzle relation of
/// Saint-Venant and Wantzel, choked at the critical pressure ratio). Zero
/// when the upstream pressure is not above the downstream one.
#[must_use]
pub fn orifice_mass_flux_kg_m2_s(
    gas: &FuelGasCard,
    upstream_pa: f64,
    temperature_k: f64,
    downstream_pa: f64,
) -> f64 {
    if !(upstream_pa > downstream_pa) || !(downstream_pa > 0.0) || !(temperature_k > 0.0) {
        return 0.0;
    }
    let kappa = gas.isentropic_exponent;
    let critical = (2.0 / (kappa + 1.0)).powf(kappa / (kappa - 1.0));
    let ratio = (downstream_pa / upstream_pa).max(critical);
    let psi = (ratio.powf(2.0 / kappa) - ratio.powf((kappa + 1.0) / kappa)).max(0.0);
    upstream_pa
        * (2.0 * kappa / ((kappa - 1.0) * gas.specific_gas_constant() * temperature_k) * psi).sqrt()
}

// ---------------------------------------------------------------------------
// Engine: indicated work (fitted), friction (fitted total, assumed split),
// heat split (assumed)
// ---------------------------------------------------------------------------

/// Friction and pumping torque of the engine against speed.
///
/// Origin: the power at rated speed is **fitted** (it is the passport's
/// no-load fuel line turned into work, see [`IndicatedWorkCard`]); the split
/// between a speed-independent part and a part proportional to speed is
/// **assumed**. The sign change at standstill is regularised over
/// `regularisation_rad_s` (a numerical choice, not a measured breakaway law).
///
/// The fitted power is 153 kW at rated speed, 15 % of the rated output. It is
/// large because the Willans extrapolation puts the whole no-load fuel into
/// friction and pumping: on a displacement of some 48 l (assumed, for this
/// comparison only) it is a mean effective pressure of about 2.5 bar, the
/// upper end of what such engines show. The coast-down time is its
/// consequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrictionCard {
    /// Friction torque at rated speed [N m].
    pub rated_torque_nm: f64,
    /// Share of it that does not depend on speed [-] (assumed).
    pub constant_share: f64,
    /// Speed scale of the `tanh` that takes the torque through zero [rad/s].
    pub regularisation_rad_s: f64,
}

impl FrictionCard {
    /// Friction torque opposing rotation at `omega_rad_s` [N m]; it has the
    /// sign of the speed.
    #[must_use]
    pub fn torque_nm(&self, omega_rad_s: f64) -> f64 {
        let magnitude = self.rated_torque_nm
            * (self.constant_share
                + (1.0 - self.constant_share) * omega_rad_s.abs() / RATED_OMEGA_RAD_S);
        magnitude * (omega_rad_s / self.regularisation_rad_s).tanh()
    }
}

/// Indicated power of the engine against the fuel power it burns.
///
/// Origin: **fitted** to the passport's electrical efficiency. For an
/// effective fuel power `F` (condition factor times the fuel's lower heating
/// power) the passport machine delivers `P*(F)` at its terminals
/// ([`PassportCard::electric_for_fuel_w`]). The indicated power is what the
/// crankshaft must then receive at rated speed:
///
/// `P_ind(F) = P*(F) + generator loss at P* + friction power at rated speed`,
///
/// with the generator loss taken from this plant's own generator model at the
/// reference excitation ([`GeneratorCard::REFERENCE_EXCITATION_PU`]) and
/// nominal bus voltage, tabulated once. The friction power at rated speed is
/// chosen so that `P_ind(0) = 0`: it is the Willans intercept turned into
/// work, less the generator's no-load loss. The plant therefore reproduces
/// the passport times the condition factor at the reference excitation; at
/// another excitation the generator's losses differ and so does the
/// efficiency, which is computed.
///
/// The indicated power is taken to depend on the fuel power alone, not on
/// speed (**assumed**); the engine turns it into torque at the actual speed.
#[derive(Debug, Clone, PartialEq)]
pub struct IndicatedWorkCard {
    /// The passport the card is fitted to.
    pub passport: PassportCard,
    /// Generator loss [W] at the reference excitation against electrical
    /// output [W], equally spaced from zero to `loss_table_span_w`.
    pub generator_loss_w: Vec<f64>,
    /// Largest output of the loss table [W].
    pub loss_table_span_w: f64,
    /// Friction power at rated speed [W] (fitted, see above).
    pub rated_friction_w: f64,
}

impl IndicatedWorkCard {
    /// Fit the card to `passport` through the generator loss `loss_w(P)`.
    pub fn fitted(passport: PassportCard, loss_w: impl Fn(f64) -> f64) -> Self {
        const POINTS: usize = 65;
        let span = 1.6 * RATED_ELECTRIC_W;
        let generator_loss_w: Vec<f64> = (0..POINTS)
            .map(|index| loss_w(span * index as f64 / (POINTS - 1) as f64))
            .collect();
        let (slope, intercept) = passport.willans_line();
        let rated_friction_w = intercept / slope - generator_loss_w[0];
        Self {
            passport,
            generator_loss_w,
            loss_table_span_w: span,
            rated_friction_w,
        }
    }

    /// Generator loss [W] at the reference excitation for output `electric_w`.
    #[must_use]
    pub fn reference_generator_loss_w(&self, electric_w: f64) -> f64 {
        let last = self.generator_loss_w.len() - 1;
        let position =
            (electric_w / self.loss_table_span_w).clamp(0.0, 1.0) * last as f64;
        let index = (position.floor() as usize).min(last - 1);
        let weight = position - index as f64;
        self.generator_loss_w[index] * (1.0 - weight) + self.generator_loss_w[index + 1] * weight
    }

    /// Electrical power [W] the passport machine delivers from the effective
    /// fuel power `effective_fuel_w`; negative below the no-load fuel line.
    #[must_use]
    pub fn passport_electric_w(&self, effective_fuel_w: f64) -> f64 {
        self.passport.electric_for_fuel_w(effective_fuel_w)
    }

    /// Indicated power [W] from the effective fuel power [W]. Never negative.
    #[must_use]
    pub fn indicated_power_w(&self, effective_fuel_w: f64) -> f64 {
        if !(effective_fuel_w > 0.0) {
            return 0.0;
        }
        let electric = self.passport_electric_w(effective_fuel_w);
        (electric + self.reference_generator_loss_w(electric) + self.rated_friction_w).max(0.0)
    }

    /// The friction card that goes with this fit.
    #[must_use]
    pub fn friction(&self) -> FrictionCard {
        FrictionCard {
            rated_torque_nm: self.rated_friction_w / RATED_OMEGA_RAD_S,
            constant_share: 0.60,
            regularisation_rad_s: 1.0,
        }
    }
}

/// Where the heat that is not indicated work goes. Origin: **assumed**.
///
/// Of the fuel power that does not become indicated work, `wall_share` passes
/// through the cylinder walls into the jacket coolant; `radiation_share` of
/// the whole fuel power leaves the engine's surfaces to the room; the rest is
/// exhaust enthalpy above the fuel stream's datum. Friction work is heat in
/// the oil and reaches the jacket coolant through the oil cooler. With the
/// station's passport the jacket then carries 22 % of the fuel power at rated
/// load and the exhaust 36 %.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeatSplitCard {
    /// Share of the non-work heat that enters the jacket coolant [-].
    pub wall_share: f64,
    /// Share of the fuel power radiated and convected to the room [-].
    pub radiation_share: f64,
}

impl HeatSplitCard {
    /// The assumed split.
    pub const ASSUMED: Self = Self {
        wall_share: 0.30,
        radiation_share: 0.025,
    };
}

/// Limits of the combustion model. Origin: **assumed**.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombustionLimitsCard {
    /// Largest indicated torque [N m]: the cylinder charge is limited by
    /// boost, so fuel beyond it is not burned usefully and leaves as exhaust.
    pub largest_indicated_torque_nm: f64,
    /// Speed below which the indicated power is turned into torque as if the
    /// engine ran at this speed [rad/s] (keeps the torque finite at the first
    /// firing strokes).
    pub torque_speed_floor_rad_s: f64,
}

// ---------------------------------------------------------------------------
// Shaft and starter (assumed)
// ---------------------------------------------------------------------------

/// Polar moment of inertia of crankshaft, flywheel, coupling and generator
/// rotor together [kg m2]. Origin: **assumed**, not taken from a data sheet.
///
/// Reasoning: about 20 kg m2 for a 12-cylinder engine of this output with its
/// flywheel and about 40 kg m2 for the rotor of a 4-pole 1 250 kVA
/// alternator. That is an inertia constant `H = J w^2 / (2 S)` of 0.59 s,
/// inside the 0.4 to 1 s usual for reciprocating gensets. What follows from
/// it and is computed: the speed rise on a load rejection (about 4 % with the
/// governor as set), the 4.6 Hz swing against the bus, and the coast-down
/// (about 11 s from rated speed against the fitted friction). The run-up
/// time does not follow from it: the controller's speed-reference ramp is
/// slower than the engine could accelerate. A real machine's values replace
/// this number and move all three.
pub const SHAFT_INERTIA_KG_M2: f64 = 60.0;

/// The electric starter as an ideal series of a voltage source, a resistance
/// and a torque constant, referred to the crankshaft. Origin: **assumed**.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StarterCard {
    /// Battery voltage [V].
    pub battery_voltage_v: f64,
    /// Torque at standstill and full battery voltage [N m].
    pub stall_torque_nm: f64,
    /// Crankshaft speed at which the torque falls to zero [rad/s].
    pub free_speed_rad_s: f64,
}

impl StarterCard {
    /// The assumed starter: 2.5 kN m at standstill, free at 250 rpm.
    pub const ASSUMED: Self = Self {
        battery_voltage_v: 24.0,
        stall_torque_nm: 2_500.0,
        free_speed_rad_s: 250.0 * PI / 30.0,
    };

    /// Back-EMF and torque constant [V s/rad = N m/A], referred to the crank.
    #[must_use]
    pub fn constant(&self) -> f64 {
        self.battery_voltage_v / self.free_speed_rad_s
    }

    /// Armature circuit resistance [ohm].
    #[must_use]
    pub fn resistance_ohm(&self) -> f64 {
        self.constant() * self.battery_voltage_v / self.stall_torque_nm
    }
}

// ---------------------------------------------------------------------------
// Generator and transformer (assumed), synchronisation window (assumed)
// ---------------------------------------------------------------------------

/// The synchronous generator and the unit transformer, in the classical
/// model: a constant EMF behind the transient reactance, plus the
/// transformer's leakage reactance, against an infinite bus.
///
/// Origin: the voltage is **passport**; everything else is **assumed**
/// (typical per-unit values for a 4-pole low-voltage genset alternator and a
/// 0.4/10 kV distribution transformer). The transformer is a reactance only:
/// its copper and iron losses are not modelled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneratorCard {
    /// Apparent power base [VA].
    pub rated_va: f64,
    /// Line-to-line voltage base [V].
    pub rated_voltage_v: f64,
    /// Pole pairs (4-pole machine at 1 500 rpm for 50 Hz).
    pub pole_pairs: u32,
    /// Transient d-axis reactance [pu].
    pub transient_reactance_pu: f64,
    /// Transformer leakage reactance [pu on the generator base].
    pub transformer_reactance_pu: f64,
    /// Lumped stator loss resistance [pu]: 16 kW of copper and stray loss at
    /// rated current.
    pub loss_resistance_pu: f64,
    /// Core loss at rated voltage and frequency [W].
    pub rated_core_loss_w: f64,
    /// Damper-winding torque coefficient [pu power per pu speed deviation].
    pub damping_pu: f64,
    /// Smallest and largest excitation the field system accepts [pu].
    pub excitation_range_pu: (f64, f64),
    /// Negative-sequence reactance of the generator alone [pu] (**assumed**:
    /// the order of a sub-transient reactance of a salient-pole machine with
    /// a damper cage). The transformer's reactance adds to it towards the bus.
    pub negative_sequence_reactance_pu: f64,
    /// Negative-sequence resistance of the generator [pu] (**assumed**). It is
    /// the stator's resistance plus half of what the rotor's circuits show at
    /// twice the line frequency: of the heat a negative-sequence current
    /// leaves in the rotor, half arrives through the stator from the bus and
    /// half through the shaft as a braking torque.
    pub negative_sequence_resistance_pu: f64,
}

impl GeneratorCard {
    /// Excitation the indicated-work card is fitted at [pu].
    pub const REFERENCE_EXCITATION_PU: f64 = 1.0;

    /// The assumed machine.
    pub const ASSUMED: Self = Self {
        rated_va: 1.25e6,
        rated_voltage_v: GENERATOR_VOLTAGE_V,
        pole_pairs: 2,
        transient_reactance_pu: 0.25,
        transformer_reactance_pu: 0.06,
        loss_resistance_pu: 0.02,
        rated_core_loss_w: 14_000.0,
        damping_pu: 15.0,
        excitation_range_pu: (0.5, 1.5),
        negative_sequence_reactance_pu: 0.20,
        negative_sequence_resistance_pu: 0.05,
    };

    /// Base impedance [ohm].
    #[must_use]
    pub fn base_impedance_ohm(&self) -> f64 {
        self.rated_voltage_v * self.rated_voltage_v / self.rated_va
    }

    /// Rated line current [A, RMS].
    #[must_use]
    pub fn rated_current_a(&self) -> f64 {
        self.rated_va / (3.0_f64.sqrt() * self.rated_voltage_v)
    }

    /// Rated phase-to-neutral voltage [V, RMS].
    #[must_use]
    pub fn rated_phase_voltage_v(&self) -> f64 {
        self.rated_voltage_v / 3.0_f64.sqrt()
    }

    /// Resistance the rotor's circuits show a negative-sequence current [pu]:
    /// twice the excess of the negative-sequence resistance over the stator's.
    #[must_use]
    pub fn rotor_negative_sequence_resistance_pu(&self) -> f64 {
        2.0 * (self.negative_sequence_resistance_pu - self.loss_resistance_pu)
    }

    /// Synchronous electrical angular frequency at nominal frequency [rad/s].
    #[must_use]
    pub fn nominal_electrical_omega(&self) -> f64 {
        2.0 * PI * NOMINAL_FREQUENCY_HZ
    }
}

/// The window inside which the generator breaker may close.
///
/// Origin: **assumed**. These are plausible synchro-check settings, not a
/// quotation of any standard: **to be verified against the applicable grid
/// code** and the protection settings of the real connection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynchronisationWindowCard {
    /// Largest voltage difference, generator open-circuit EMF minus bus [pu].
    pub voltage_difference_pu: f64,
    /// Largest frequency difference, generator minus bus [Hz].
    pub frequency_difference_hz: f64,
    /// Largest phase angle between the two [rad].
    pub phase_angle_rad: f64,
    /// Lowest bus voltage the synchroniser accepts as a live bus [pu].
    pub live_bus_pu: f64,
}

impl SynchronisationWindowCard {
    /// The assumed window: 5 %, 0.10 Hz, 10 degrees.
    pub const ASSUMED: Self = Self {
        voltage_difference_pu: 0.05,
        frequency_difference_hz: 0.10,
        phase_angle_rad: 10.0 * PI / 180.0,
        live_bus_pu: 0.85,
    };
}

// ---------------------------------------------------------------------------
// Genset controller setpoints
// ---------------------------------------------------------------------------

/// Settings of the genset controller: governor, sequencer and protection.
///
/// Origin: **setpoint**. These are parameters of a device, as a commissioning
/// engineer would enter them. The load ramp, the unloading time and the
/// cool-down time are the station simulator's values (`vkgu/station.py`:
/// `RAMP_KW_PER_S`, `UNLOAD_S`, `COOLDOWN_S`); the coolant trip is SPEC 3.3;
/// the rest are this model's choices and **to be verified** against the real
/// controller's settings sheet. Timers here are settings of the controller,
/// not results of the physics; what the plant does while they run is computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerSetpoints {
    /// Governor sample period [s]; the plant's sub-step.
    pub sample_period_s: f64,
    /// Proportional gain of the speed loop [N m s/rad].
    pub proportional_gain_nm_s_per_rad: f64,
    /// Integral gain of the speed loop [N m/rad].
    pub integral_gain_nm_per_rad: f64,
    /// Anti-windup tracking time [s].
    pub anti_windup_time_s: f64,
    /// Indicated torque the governor assumes per unit of valve demand [N m].
    pub torque_per_valve_demand_nm: f64,
    /// Speed droop on the bus [-]: speed change for a load change of rated.
    pub droop: f64,
    /// Time the oil pump runs before the starter is engaged [s]. A timer of
    /// the controller; the oil circuit is not modelled. **To be verified
    /// against the controller documentation of a real machine.**
    pub pre_lubrication_time_s: f64,
    /// Time the starter turns the engine without gas and ignition, to purge
    /// the exhaust tract [s]. **To be verified** likewise.
    pub purge_time_s: f64,
    /// Time the engine runs unloaded at rated speed before the synchroniser
    /// is released [s]. **To be verified** likewise. With the two timers
    /// above it puts the breaker closing 90 to 110 s after the start command,
    /// the order of the station simulator's 90 s of starting and 30 s of
    /// synchronising (`vkgu/station.py`: `START_S`, `SYNC_S`).
    pub warm_up_time_s: f64,
    /// Largest valve demand while running up [-].
    pub start_fuel_limit: f64,
    /// Crankshaft speed at which gas and ignition are switched on [rad/s].
    pub firing_speed_rad_s: f64,
    /// Speed above which the starter is released [rad/s].
    pub starter_release_rad_s: f64,
    /// Rate of the speed reference while running up [rad/s2].
    pub run_up_rate_rad_s2: f64,
    /// Longest time from the start command to rated speed [s] before the
    /// start is declared failed (W-210).
    pub start_time_limit_s: f64,
    /// Share of rated speed at which the unit counts as run up [-].
    pub at_speed_share: f64,
    /// Frequency the synchroniser holds above the bus while waiting for the
    /// phase to come round [Hz].
    pub synchronising_slip_hz: f64,
    /// Phase angle inside which the automatic synchroniser gives its close
    /// command [rad]. It aims at zero phase; the window of
    /// [`SynchronisationWindowCard`] is the wider permission a manual close
    /// request is checked against.
    pub auto_close_phase_rad: f64,
    /// Default load ramp [fraction of rated per second].
    pub load_ramp_per_s: f64,
    /// Unloading ramp before a normal stop [fraction of rated per second].
    pub unload_ramp_per_s: f64,
    /// Electrical power below which the breaker is opened on a stop [W].
    pub breaker_open_power_w: f64,
    /// Time the engine runs unloaded at rated speed before the gas is shut [s].
    pub cool_down_time_s: f64,
    /// Speed below which the shaft counts as stopped [rad/s].
    pub standstill_rad_s: f64,
    /// Over-speed trip [rad/s].
    pub overspeed_trip_rad_s: f64,
    /// Gas supply gauge pressure below which the gas pressure switch warns
    /// and a power shortfall is attributed to the gas supply [Pa].
    pub gas_low_pressure_pa: f64,
    /// Valve demand above which the valve counts as fully open [-].
    pub valve_saturated_demand: f64,
    /// Shortfall of power against the load reference that counts [W].
    pub power_shortfall_w: f64,
    /// Time the shortfall must last with the valve fully open [s] (A-150).
    pub gas_shortfall_delay_s: f64,
    /// Time the low-oil-pressure input must last with the engine firing [s]
    /// (A-160).
    pub oil_pressure_delay_s: f64,
    /// Coolant temperature that raises the warning [K].
    pub coolant_warning_k: f64,
    /// Coolant temperature that trips the unit [K] (A-105; SPEC 3.3: 98 C).
    pub coolant_trip_k: f64,
}

impl ControllerSetpoints {
    /// The model's settings.
    pub const DEFAULT: Self = Self {
        sample_period_s: 0.01,
        proportional_gain_nm_s_per_rad: 500.0,
        integral_gain_nm_per_rad: 1_200.0,
        anti_windup_time_s: 0.25,
        torque_per_valve_demand_nm: 10_000.0,
        droop: 0.04,
        pre_lubrication_time_s: 30.0,
        purge_time_s: 5.0,
        warm_up_time_s: 45.0,
        start_fuel_limit: 0.50,
        firing_speed_rad_s: 120.0 * PI / 30.0,
        starter_release_rad_s: 300.0 * PI / 30.0,
        run_up_rate_rad_s2: 150.0 * PI / 30.0,
        start_time_limit_s: 30.0,
        at_speed_share: 0.98,
        synchronising_slip_hz: 0.05,
        auto_close_phase_rad: 1.0 * PI / 180.0,
        load_ramp_per_s: 0.10 / 60.0,
        unload_ramp_per_s: 1.0 / 120.0,
        breaker_open_power_w: 0.02 * RATED_ELECTRIC_W,
        cool_down_time_s: 300.0,
        standstill_rad_s: 0.5,
        overspeed_trip_rad_s: 1.15 * RATED_OMEGA_RAD_S,
        gas_low_pressure_pa: 30_000.0,
        valve_saturated_demand: 0.98,
        power_shortfall_w: 0.03 * RATED_ELECTRIC_W,
        gas_shortfall_delay_s: 5.0,
        oil_pressure_delay_s: 2.0,
        coolant_warning_k: 273.15 + 95.0,
        coolant_trip_k: 273.15 + 98.0,
    };
}

// ---------------------------------------------------------------------------
// Generator breaker cabinet: protection, restoration, power-quality analyser
// ---------------------------------------------------------------------------

/// A protection function that trips after its condition has held for a set
/// time: a pick-up level and a delay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefiniteTimeCard {
    /// Level at which the function picks up, in the function's own unit.
    pub pickup: f64,
    /// Time the condition must hold before the trip [s].
    pub delay_s: f64,
}

/// Settings of the protection device in the generator breaker cabinet.
///
/// Origin: **setpoint**. Every number here is a setting of a relay, chosen as
/// a plausible commissioning value and **to be verified against the cabinet
/// documentation and the grid code**. The under-voltage level and delay are
/// the ones the genset controller carried before this protection moved into
/// the cabinet. No standard is quoted.
///
/// Voltages are per unit of the rated phase voltage, currents per unit of the
/// generator's rated current.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreakerCabinetCard {
    /// Under-voltage, per phase: any phase below the level [pu].
    pub undervoltage: DefiniteTimeCard,
    /// Over-voltage, per phase: any phase above the level [pu].
    pub overvoltage: DefiniteTimeCard,
    /// Voltage unbalance: negative- over positive-sequence voltage above the
    /// level [-].
    pub voltage_unbalance: DefiniteTimeCard,
    /// Negative-sequence current above which the rotor's thermal replica
    /// integrates [pu]: the current the machine is taken to carry
    /// continuously.
    pub negative_sequence_pickup_pu: f64,
    /// Thermal limit of the rotor, `(I2 / I_rated)^2 * t` [s].
    pub negative_sequence_limit_s: f64,
    /// Time constant with which the replica cools while the current is below
    /// the pick-up [s].
    pub negative_sequence_cooling_s: f64,
    /// Over-current, per phase, time-delayed stage [pu].
    pub overcurrent: DefiniteTimeCard,
    /// Over-current, per phase, high-set stage [pu].
    pub overcurrent_high: DefiniteTimeCard,
    /// Under-frequency [Hz].
    pub underfrequency: DefiniteTimeCard,
    /// Over-frequency [Hz].
    pub overfrequency: DefiniteTimeCard,
    /// Reverse power: power drawn from the bus above the level [W].
    pub reverse_power: DefiniteTimeCard,
}

impl BreakerCabinetCard {
    /// The model's settings.
    pub const DEFAULT: Self = Self {
        undervoltage: DefiniteTimeCard {
            pickup: 0.85,
            delay_s: 0.15,
        },
        overvoltage: DefiniteTimeCard {
            pickup: 1.15,
            delay_s: 0.5,
        },
        voltage_unbalance: DefiniteTimeCard {
            pickup: 0.05,
            delay_s: 5.0,
        },
        negative_sequence_pickup_pu: 0.08,
        negative_sequence_limit_s: 20.0,
        negative_sequence_cooling_s: 300.0,
        overcurrent: DefiniteTimeCard {
            pickup: 1.20,
            delay_s: 10.0,
        },
        overcurrent_high: DefiniteTimeCard {
            pickup: 2.5,
            delay_s: 0.10,
        },
        underfrequency: DefiniteTimeCard {
            pickup: 47.5,
            delay_s: 0.5,
        },
        overfrequency: DefiniteTimeCard {
            pickup: 51.5,
            delay_s: 0.5,
        },
        reverse_power: DefiniteTimeCard {
            pickup: 0.05 * RATED_ELECTRIC_W,
            delay_s: 5.0,
        },
    };
}

/// How the unit comes back after the cabinet's protection has opened the
/// breaker on a grid event, and what counts as a healthy grid.
///
/// Origin: **setpoint**, **to be verified against the grid code and the
/// plant's automation design**. In the manual mode none of the timers act:
/// people bring the unit back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestorationCard {
    /// Lowest phase voltage of a healthy grid [pu].
    pub healthy_voltage_low_pu: f64,
    /// Highest phase voltage of a healthy grid [pu].
    pub healthy_voltage_high_pu: f64,
    /// Largest voltage unbalance of a healthy grid [-].
    pub healthy_unbalance: f64,
    /// Lowest frequency of a healthy grid [Hz].
    pub healthy_frequency_low_hz: f64,
    /// Highest frequency of a healthy grid [Hz].
    pub healthy_frequency_high_hz: f64,
    /// Time the grid must stay healthy without a break before the automation
    /// synchronises again [s].
    pub healthy_delay_s: f64,
    /// Longest wait at no load for the grid [s]; after it the unit makes a
    /// normal stop with its cool-down.
    pub longest_wait_s: f64,
    /// Largest number of automatic returns within `attempt_window_s`; the
    /// next grid trip inside the window blocks the automation until it is
    /// acknowledged.
    pub largest_attempts: u32,
    /// Window over which the automatic returns are counted [s].
    pub attempt_window_s: f64,
}

impl RestorationCard {
    /// The model's settings.
    pub const DEFAULT: Self = Self {
        healthy_voltage_low_pu: 0.90,
        healthy_voltage_high_pu: 1.10,
        healthy_unbalance: 0.03,
        healthy_frequency_low_hz: 49.0,
        healthy_frequency_high_hz: 51.0,
        healthy_delay_s: 60.0,
        longest_wait_s: 600.0,
        largest_attempts: 3,
        attempt_window_s: 1_800.0,
    };
}

/// Thresholds of the power-quality analyser in the cabinet.
///
/// Origin: **setpoint**, typical defaults of such an instrument, **to be
/// verified against the current edition of the power-quality standard** the
/// plant is measured under. No clause is quoted. Levels are per unit of the
/// rated phase voltage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerQualityCard {
    /// A dip starts when a phase falls below this [pu].
    pub dip_pu: f64,
    /// A swell starts when a phase rises above this [pu].
    pub swell_pu: f64,
    /// A dip whose lowest voltage is below this is an interruption [pu].
    pub interruption_pu: f64,
    /// Margin by which the voltage must come back past a threshold before
    /// the event is closed [pu].
    pub hysteresis_pu: f64,
}

impl PowerQualityCard {
    /// The model's settings.
    pub const DEFAULT: Self = Self {
        dip_pu: 0.90,
        swell_pu: 1.10,
        interruption_pu: 0.05,
        hysteresis_pu: 0.02,
    };
}

/// What a full journal of the revenue meter does with the next record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOverflow {
    /// The oldest record is displaced by the new one.
    OverwriteOldest,
    /// The new record is not written; the journal keeps what it has.
    KeepOldest,
}

/// The revenue meter in the cabinet: a meter with one-second threshold
/// supervision of the phase voltages and capacity-limited journals.
///
/// Origin: **assumed; to be verified against the meter's documentation.**
/// Every value here is a property of an instrument, typical of the kind and
/// taken from no particular one: the supervision period, the three levels
/// (per unit of the rated phase voltage), the capacities of the journals and
/// what a full journal does. The meter's accuracy class is not modelled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterCard {
    /// Period of the voltage supervision by the meter's own clock [s]. The
    /// meter looks at the phase voltages at these instants and at no others.
    pub supervision_period_s: f64,
    /// A phase below this at a supervision instant is low [pu].
    pub low_voltage_pu: f64,
    /// A phase above this at a supervision instant is high [pu].
    pub high_voltage_pu: f64,
    /// A phase below this at a supervision instant is lost [pu].
    pub loss_of_voltage_pu: f64,
    /// Records the low-voltage journal holds.
    pub low_voltage_capacity: u32,
    /// Records the high-voltage journal holds.
    pub high_voltage_capacity: u32,
    /// Records the loss-of-voltage journal holds.
    pub loss_of_voltage_capacity: u32,
    /// What a full journal does with the next record.
    pub overflow: JournalOverflow,
}

impl MeterCard {
    /// The model's meter.
    pub const ASSUMED: Self = Self {
        supervision_period_s: 1.0,
        low_voltage_pu: 0.90,
        high_voltage_pu: 1.10,
        loss_of_voltage_pu: 0.05,
        low_voltage_capacity: 100,
        high_voltage_capacity: 100,
        loss_of_voltage_capacity: 20,
        overflow: JournalOverflow::OverwriteOldest,
    };
}

// ---------------------------------------------------------------------------
// Heat recovery (assumed, with one fitted length)
// ---------------------------------------------------------------------------

/// The exhaust gas as the heat exchanger sees it. Origin: **assumed**.
///
/// A lean-burn engine at a constant excess-air ratio; the gas is treated as
/// one ideal gas of constant properties (air-like combustion products near
/// 300 C). The exhaust enthalpy of the engine is all sensible heat above the
/// fuel stream's datum, so the exhaust temperature follows from it and the
/// mass flow; with these numbers it is about 485 C at rated load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExhaustGasCard {
    /// Excess-air ratio [-].
    pub excess_air_ratio: f64,
    /// Stoichiometric air demand [kg air per kg fuel] (methane).
    pub stoichiometric_air_kg_kg: f64,
    /// Isobaric heat capacity [J/(kg K)].
    pub cp_j_kg_k: f64,
    /// Dynamic viscosity [Pa s].
    pub viscosity_pa_s: f64,
    /// Thermal conductivity [W/(m K)].
    pub conductivity_w_m_k: f64,
    /// Prandtl number [-].
    pub prandtl: f64,
}

impl ExhaustGasCard {
    /// The assumed exhaust gas.
    pub const ASSUMED: Self = Self {
        excess_air_ratio: 1.9,
        stoichiometric_air_kg_kg: 17.2,
        cp_j_kg_k: 1_150.0,
        viscosity_pa_s: 3.0e-5,
        conductivity_w_m_k: 0.045,
        prandtl: 0.72,
    };

    /// Exhaust heat-capacity rate [W/K] for a fuel mass flow [kg/s].
    #[must_use]
    pub fn capacity_rate_w_k(&self, fuel_kg_s: f64) -> f64 {
        fuel_kg_s.max(0.0)
            * (1.0 + self.excess_air_ratio * self.stoichiometric_air_kg_kg)
            * self.cp_j_kg_k
    }
}

/// The exhaust-gas heat exchanger: a tube bundle with the gas inside the
/// tubes and the jacket coolant around them.
///
/// Origin: tube count and bore are **assumed**; the tube length is **fitted**
/// so that, at the passport's rated point and 90 C coolant, the heat it
/// recovers plus the jacket heat equals the passport's thermal recovery (43 %
/// of the fuel power). The gas-side film is the controlling resistance
/// (Gnielinski through `fs-convection`); wall and coolant-side resistances
/// are neglected. Away from the rated point the recovered heat is computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExhaustExchangerCard {
    /// Number of tubes.
    pub tubes: f64,
    /// Tube bore [m].
    pub bore_m: f64,
    /// Tube length [m] (fitted).
    pub length_m: f64,
}

impl ExhaustExchangerCard {
    /// Assumed tube count.
    pub const TUBES: f64 = 160.0;
    /// Assumed tube bore [m].
    pub const BORE_M: f64 = 0.020;
    /// Coolant temperature the length is fitted at [K] (SPEC 3.3: 90 C).
    pub const FIT_COOLANT_K: f64 = 273.15 + 90.0;

    /// Gas-side surface [m2].
    #[must_use]
    pub fn surface_m2(&self) -> f64 {
        self.tubes * core::f64::consts::PI * self.bore_m * self.length_m
    }

    /// Reynolds number in the tubes for an exhaust capacity rate [W/K].
    #[must_use]
    pub fn reynolds(&self, gas: &ExhaustGasCard, capacity_rate_w_k: f64) -> f64 {
        let mass_flow = capacity_rate_w_k / gas.cp_j_kg_k;
        4.0 * mass_flow / (core::f64::consts::PI * self.bore_m * gas.viscosity_pa_s * self.tubes)
    }
}

/// The jacket coolant circuit as one thermal node. Origin: **assumed**.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoolantCard {
    /// Heat capacity of engine block and coolant inventory together [J/K]:
    /// about 4 t of iron and 300 l of water-glycol.
    pub capacity_j_k: f64,
    /// Heat-capacity rate of the coolant pump's flow [W/K]: 20 K across the
    /// engine and the exhaust exchanger at rated heat.
    pub pump_capacity_rate_w_k: f64,
    /// Temperature the standby heater keeps the block at [K]; the model's
    /// starting temperature. The heater itself is not modelled.
    pub standby_k: f64,
    /// Conductance from the block's surface to the engine room [W/K].
    pub room_conductance_w_k: f64,
}

impl CoolantCard {
    /// The assumed circuit.
    pub const ASSUMED: Self = Self {
        capacity_j_k: 3.1e6,
        pump_capacity_rate_w_k: 56_000.0,
        standby_k: 273.15 + 60.0,
        room_conductance_w_k: 150.0,
    };
}

/// The plate heat exchanger to the district-heating network and the
/// thermostatic valve that feeds it. Origin: **assumed**.
///
/// The network's water flow is the demand input times its rated flow; the
/// thermostatic valve passes coolant to the exchanger as the coolant warms
/// through its band. The exchanger is counterflow with a constant `UA`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetworkExchangerCard {
    /// Overall conductance [W/K].
    pub ua_w_k: f64,
    /// Heat-capacity rate of the network water at full demand [W/K].
    pub network_capacity_rate_w_k: f64,
    /// Coolant temperature at which the thermostatic valve starts to open [K].
    pub thermostat_opens_k: f64,
    /// Coolant temperature at which it is fully open [K].
    pub thermostat_open_k: f64,
}

impl NetworkExchangerCard {
    /// The assumed exchanger: six transfer units at full demand.
    pub const ASSUMED: Self = Self {
        ua_w_k: 6.0 * 56_000.0,
        network_capacity_rate_w_k: 56_000.0,
        thermostat_opens_k: 273.15 + 84.0,
        thermostat_open_k: 273.15 + 88.0,
    };
}

/// The emergency (dump) cooler: a finned-tube radiator with fans.
///
/// Origin: **assumed**. Its three-way valve opens through a band above the
/// thermostat's (a setting of the cooler's own controller). With the fans
/// running the air crosses the tubes at `fan_air_speed_m_s`; with the fans
/// failed only a natural draught of `draught_air_speed_m_s` remains. The film
/// coefficient at either speed is Churchill-Bernstein for a cylinder in
/// crossflow through `fs-convection` (a single-tube relation used for a
/// bundle: an approximation), on the finned surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DumpCoolerCard {
    /// Face area the air passes [m2].
    pub face_area_m2: f64,
    /// Air-side surface, fins included [m2].
    pub surface_m2: f64,
    /// Tube outer diameter [m].
    pub tube_diameter_m: f64,
    /// Air speed with the fans running [m/s].
    pub fan_air_speed_m_s: f64,
    /// Air speed by natural draught with the fans failed [m/s].
    pub draught_air_speed_m_s: f64,
    /// Coolant temperature at which the three-way valve starts to open [K].
    pub valve_opens_k: f64,
    /// Coolant temperature at which it is fully open [K].
    pub valve_open_k: f64,
    /// Ambient air temperature [K].
    pub ambient_k: f64,
}

impl DumpCoolerCard {
    /// Air density [kg/m3], heat capacity [J/(kg K)], kinematic viscosity
    /// [m2/s], conductivity [W/(m K)] and Prandtl number near 40 C.
    pub const AIR: (f64, f64, f64, f64, f64) = (1.13, 1_007.0, 1.7e-5, 0.027, 0.71);

    /// The assumed cooler.
    pub const ASSUMED: Self = Self {
        face_area_m2: 2.8,
        surface_m2: 800.0,
        tube_diameter_m: 0.016,
        fan_air_speed_m_s: 8.0,
        draught_air_speed_m_s: 0.3,
        valve_opens_k: 273.15 + 91.0,
        valve_open_k: 273.15 + 94.0,
        ambient_k: 273.15 + 30.0,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passport_fuel_map_inverts_on_every_segment_and_below_the_first_point() {
        let passport = PassportCard::STATION;
        for step in -40..=320 {
            let electric = 5_000.0 * f64::from(step);
            let fuel = passport.fuel_for_electric_w(electric);
            let back = passport.electric_for_fuel_w(fuel);
            assert!(
                (back - electric).abs() < 1.0e-6 * RATED_ELECTRIC_W,
                "P = {electric} W -> F = {fuel} W -> P = {back} W"
            );
        }
        // The passport points themselves.
        for (load, eta) in passport.electric_efficiency_pct {
            let electric = RATED_ELECTRIC_W * load / 100.0;
            let fuel = passport.fuel_for_electric_w(electric);
            assert!((electric / fuel - eta / 100.0).abs() < 1.0e-12);
        }
    }

    #[test]
    fn willans_intercept_is_the_no_load_fuel_of_the_passport_line() {
        let (slope, intercept) = PassportCard::STATION.willans_line();
        // (500/0.35 - 300/0.30) / 200 and 300/0.30 - slope * 300, in kW.
        assert!((slope - 2.142_857_142_857).abs() < 1.0e-9, "slope {slope}");
        assert!((intercept - 357_142.857_142_857).abs() < 1.0e-3, "intercept {intercept}");
    }

    #[test]
    fn gas_card_reproduces_the_volumetric_heating_value_and_the_valve_is_sized_for_rated_flow() {
        let gas = FuelGasCard::STATION;
        let per_m3 = gas.lower_heating_value_j_kg() * gas.standard_density_kg_m3();
        assert!((per_m3 - 33.5e6).abs() < 1.0e-3);
        assert!((gas.temperature_k(gas.specific_enthalpy_j_kg(288.15)) - 288.15).abs() < 1.0e-9);
        let rated = 0.05;
        let valve = GasValveCard::sized_for(&gas, rated);
        let flux = orifice_mass_flux_kg_m2_s(&gas, 151_325.0, 288.15, 101_325.0);
        assert!((valve.full_area_m2 * 0.70 * flux - rated).abs() < 1.0e-12);
        // No flow against a higher pressure, and the choked flux is linear in pressure.
        assert_eq!(orifice_mass_flux_kg_m2_s(&gas, 100_000.0, 288.15, 101_325.0), 0.0);
        let choked_a = orifice_mass_flux_kg_m2_s(&gas, 400_000.0, 288.15, 101_325.0);
        let choked_b = orifice_mass_flux_kg_m2_s(&gas, 800_000.0, 288.15, 101_325.0);
        assert!((choked_b / choked_a - 2.0).abs() < 1.0e-12);
    }

    #[test]
    fn indicated_card_is_zero_without_fuel_and_returns_the_passport_at_the_reference_loss() {
        let card = IndicatedWorkCard::fitted(PassportCard::STATION, |p| 14_000.0 + 16.0e-9 * p * p);
        assert_eq!(card.indicated_power_w(0.0), 0.0);
        // At the no-load fuel the indicated power is the friction plus the no-load loss.
        let (_, intercept) = PassportCard::STATION.willans_line();
        let idle = card.indicated_power_w(intercept);
        assert!((idle - (card.rated_friction_w + 14_000.0)).abs() < 1.0e-6);
        // At the rated passport fuel it is rated output plus loss plus friction.
        let rated = card.indicated_power_w(2.5e6);
        assert!((rated - (1.0e6 + 30_000.0 + card.rated_friction_w)).abs() < 1.0);
        let friction = card.friction();
        assert!(
            (friction.torque_nm(RATED_OMEGA_RAD_S) * RATED_OMEGA_RAD_S - card.rated_friction_w).abs()
                < 1.0e-6
        );
        assert_eq!(friction.torque_nm(0.0), 0.0);
        assert!(friction.torque_nm(-10.0) < 0.0);
    }
}
