# CONTRACT: fs-assimilate

Data assimilation — validation as a living belief (plan addendum,
Proposal 11): sensor readings update the per-regime model-form posterior.

## Purpose and layer

Layer L4 (inference/UQ). Depends only on `fs-evidence` (the `Color` +
`ValidityDomain`). Pure, deterministic; the linear-Gaussian core of
weak-constraint assimilation.

## Public types and semantics

- `Belief { mean, cov }` — a Gaussian state belief (`scalar`, `diagonal`,
  `dim`, `variance`).
- `Observation { operator, value, noise_var, instrument }` — a scalar reading:
  `value = operator·state + noise`, `operator` is the restriction-map row (the
  sensor's trace).
- `point_sensor(component, dim, value, instrument_noise, instrument)` — the
  REGISTRATION-FREE observation (operator = `e_component`); `scan_observation(…,
  registration_var, …)` adds the registration variance (R8).
- `misfit(&Belief, &[Observation])` — `Σ (h·mean − y)² / r`.
- `assimilate(&Belief, &Observation)` / `assimilate_all(&Belief,
  &[Observation])` — the sequential Kalman fusion.
- `assimilate_colored(&Belief, &[Observation], regime_param, lo, hi) ->
  AssimilatedPosterior { belief, color, misfit_before, misfit_after }` — a
  validated, instrument-anchored, regime-tagged posterior.
- `AssimError` — `DimMismatch` / `NonPositiveNoise` / `SingularInnovation`.

## Invariants

- Fusing an observation only REDUCES uncertainty (posterior variance ≤ prior)
  and REDUCES the model-data misfit.
- The linear-Gaussian posterior is ORDER-INDEPENDENT (sequential fusion in any
  order yields the same mean + covariance).
- A scan observation's noise ≥ the corresponding point sensor's (it carries the
  registration variance); point sensors are registration-free.
- The assimilated posterior is `Color::Validated`, anchored (dataset) to the
  contributing instruments.

## Error model

Structured `AssimError`; no panics. Non-positive/non-finite noise and dimension
mismatches are refused.

## Determinism class

Fully deterministic: the fusion, misfit, and posterior are pure functions of the
inputs.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/assimilate.rs` (Proposal 11, 7 cases): a measurement shifts the mean +
shrinks the variance; misfit reduction; scan noise > point-sensor noise;
order-independence + sequential tightening; the validated + anchored posterior;
bad-input rejection; determinism.

## No-claim boundaries

- v1 is the LINEAR-GAUSSIAN assimilation with linear observation operators;
  full weak-constraint 4D-Var over a nonlinear forward model uses Proposal 1's
  CERTIFIED adjoints (adjoint-certs) and is the fuller deliverable — this crate
  is the Kalman/Gauss-Newton core.
- The observation operators are SUPPLIED restriction-map rows; deriving them
  from the sheaf's trace maps is fs-geom's; the registration variance for scan
  observations comes from fs-asbuilt.
- Ledgering the update onto the per-regime model-form posterior (Proposal 3's
  maps) is fs-ledger's integration; this crate produces the colored belief.
