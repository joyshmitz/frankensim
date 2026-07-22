# CONTRACT: fs-airflow

> Status: ACTIVE for bead `frankensim-extreal-program-f85xj.5.5`.

## Purpose and layer

`fs-airflow` is the L3 low-cost enclosure-airflow rung. It retains typed fan
pressure/volume-flow data, validates monotone interpolation, applies bounded
fan-law speed scaling, composes quadratic loss elements in series and parallel,
requires an explicit leakage branch, and solves the fan/system operating point.

Runtime dependencies are `fs-convection`, `fs-evidence`, `fs-ivl`, `fs-math`,
`fs-qty`, and `fs-regime`. The result flows downward only as evidence-bearing
typed quantities and as a Re/Pr handoff to the existing convection rung.

## Public types and semantics

- `FanCurve` owns typed `(VolumetricFlowRate, Pressure)` points, source identity,
  tolerance authority, a stall boundary, and the validity range for RPM affinity
  scaling.
- `FanBank` represents identical fans in series or parallel. At speed ratio
  `s`, flow scales as `s` and pressure as `s^2`; series pressure adds and
  parallel flow adds.
- `LossElement` uses a typed `LossResistance` in Pa/(m^3/s)^2. `LossNetwork`
  composes quadratic elements recursively in series and parallel.
- `EnclosureNetwork` cannot be constructed without a distinct
  `LeakageElement`; leakage is not an implicit constant.
- `solve_operating_point` returns an `OperatingPoint` with an interval-Newton
  unique-root bracket for the nominal model, weaker physical uncertainty
  estimates, per-terminal flows, and nominal leakage fraction.
- `OperatingPoint::correlation_handoff` converts one branch flow to typed mean
  velocity, computes Reynolds through role-tagged `fs-regime` dimensions, and
  produces `fs-convection::CorrelationInputs` without discarding evidence.

## Invariants

1. Fan flow points increase strictly and pressure never increases.
2. Fan-law scaling refuses outside its caller-declared speed-ratio validity
   range.
3. The interval below the explicit stall boundary is non-admissible; the solver
   refuses an intersection there.
4. Every loss resistance is finite, positive, typed, and carries source and
   uncertainty authority.
5. A complete operating-point result has exactly one `Certified` interval root
   and no `Possible` root boxes for the nominal declared model.
6. Manufacturer/loss/leakage uncertainty is attached as model-form `Estimate`
   evidence. It is never relabeled as a rigorous physical enclosure.
7. Terminal branch order follows deterministic network traversal, and all
   provenance hashes are order-stable and bind the complete fan curve, source,
   tolerance, fan-bank configuration, recursive network topology, loss data,
   and explicit leakage identity.

## Error model

`AirflowError` refuses malformed curves, invalid tolerance or speed domains,
empty network groups, zero/invalid resistances, stall operation, absent curve
intersections, incomplete/ambiguous root searches, unknown branches, and bad
convection-handoff inputs. Caller input does not panic.

## Determinism class

Interpolation and network traversal have fixed order. Square roots use
`fs-math::det`; the numerical root uses deterministic `fs-ivl` subdivision.
Results are intended to be bit-stable on the same ISA. Cross-ISA G5 evidence is
not yet retained.

## Cancellation behavior

Curve evaluation and each finite network reduction are bounded scalar work.
The interval search has an explicit 65,536-box ceiling and returns a structured
refusal rather than running without bound. No asupersync cancellation poll is
required for this rung.

## Unsafe boundary

None. Workspace unsafe-code denial applies.

## Feature flags

None.

## Conformance tests

- G0 fan interpolation, monotonicity refusal, and series/parallel resistance
  composition;
- G0 identical-fan series/parallel affinity identities;
- explicit stall refusal and a sign-changing, unique interval root bracket;
- three declared fan speeds and leakage-resistance sensitivity;
- typed branch velocity/Reynolds handoff into `fs-convection`;
- semantic-identity separation when uncertainty authority changes without
  changing the nominal operating point.

## No-claim boundaries

- Fan points and tolerances are caller-supplied. Synthetic fixtures prove API
  and algebra behavior, not a manufacturer product's performance.
- AMCA/ASHRAE fan laws justify similarity scaling, not arbitrary operation:
  the curve's declared speed-ratio domain remains binding.
- Piecewise-linear interpolation and quadratic loss coefficients do not model
  swirl, recirculation, acoustic interaction, thermal buoyancy, compressibility,
  fouling, or installation system effects.
- The nominal root bracket certifies only the declared mathematical model.
  Tolerance-propagated flow, pressure, branch splits, and Reynolds values remain
  `Estimate`, not validated hardware envelopes.
- Parallel fans are identical and equally loaded. Unequal curves, unstable
  parallel operation, active control, fan-fan interference, and transient
  startup remain outside this slice.
- No retained manufacturer table, wind-tunnel corpus, CFD comparison, or
  experimental enclosure validation exists; there is no L4 or product claim.
