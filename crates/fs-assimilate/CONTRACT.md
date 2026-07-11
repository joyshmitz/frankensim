# CONTRACT: fs-assimilate

Data assimilation as a living belief (plan addendum, Proposal 11): checked
sensor readings produce a content-addressed posterior candidate for a proposed
regime. Assimilation itself does not claim experimental validation.

## Purpose and layer

Layer L4 (inference/UQ). Depends on `fs-evidence` (`Color` and
`ValidityDomain`) and the in-tree `fs-blake3` identity primitive. Pure and
deterministic; this is the checked linear-Gaussian core of weak-constraint
assimilation.

## Public types and semantics

- `Belief` — a Gaussian state belief with private state. `new`, `scalar`, and
  `diagonal` return `Result`; admission requires a non-empty finite mean and a
  finite, square, symmetric, positive-semidefinite covariance within the
  `MAX_DENSE_STATE_DIM` v0 envelope. `mean` and
  `covariance` expose read-only slices; `component_mean` and `variance` are
  bounds-checked.
- `Observation` — a private, checked scalar reading
  `value = operator·state + noise`. `Observation::new` rejects empty, all-zero,
  and non-finite operators; non-finite readings; non-positive noise; and
  malformed instrument leaf identities.
- `point_sensor(component, dim, value, instrument_noise, instrument) -> Result`
  — the REGISTRATION-FREE observation (`operator = e_component`), rejecting
  zero dimensions and out-of-range components.
- `scan_observation(..., registration_var, ...) -> Result` — adds a finite,
  non-negative registration variance to strictly positive instrument noise and
  rejects overflow of the total (R8).
- `misfit(&Belief, &[Observation]) -> Result<f64, AssimError>` —
  `Σ (h·mean − y)² / r`; empty sets and non-finite arithmetic are refused.
- `assimilate(&Belief, &Observation)` / `assimilate_all(&Belief,
  &[Observation])` — scalar Kalman fusion. Aggregate fusion uses canonical
  observation-content order, not caller order. Aggregate admission enforces
  `MAX_DENSE_OBSERVATIONS` and the multiplicative
  `MAX_DENSE_UPDATE_CUBIC_WORK` envelope before sorting or updating.
- `assimilate_colored(&Belief, &[Observation], regime_param, lo, hi) ->
  AssimilatedPosterior { belief, color, misfit_before, misfit_after }` — a
  read-only, regime-tagged **estimated candidate**. Access is through getters.
- `AssimError` — structured invalid-state, dimension, bounds, identity,
  empty-input, noise, covariance, innovation, and non-finite-computation
  refusals. It implements `Display` and `Error`.

## Invariants

- Checked `Belief` and `Observation` values cannot be mutated into malformed
  states through the public API.
- At the external belief-admission boundary, covariance validation is an
  `O(n^3)` diagonal-pivoted Schur-complement test after dimensionless
  correlation scaling, with exact zero-variance-row handling. Negative
  curvature is never tolerance-clamped to zero: a negative pivot or diagonal
  is refused, and a zero pivot is accepted only with an exactly zero remaining
  row. Before floating scaling, every 2-by-2 principal minor is checked by an
  exact comparison of the input `f64` values as binary-rational products, so a
  sub-ULP negative determinant cannot be rounded onto the correlation boundary.
  Numerically ambiguous higher-dimensional boundary matrices may be refused.
- The scalar Kalman covariance update uses Joseph form
  `(I-KH)P(I-KH)^T + KRK^T`, computes one triangle and mirrors it for bit-exact
  symmetry, then passes the result through the same full validator as an
  externally supplied `Belief`. It does not silently clip eigenvalues or
  correlations. The checked dense implementation is `O(n^3)`.
- In exact arithmetic, fusing valid observations cannot increase component
  variances and the batch posterior cannot increase the weighted measurement
  misfit. Floating results are checked for finiteness, not interval-certified.
- `assimilate_all` and `misfit` canonicalize the complete observation records,
  making results bit-stable across input permutations while retaining duplicate
  readings and therefore their multiplicity.
- Scan noise is instrument noise plus non-negative registration variance; point
  sensors carry no registration term.
- Aggregate APIs reject empty or oversized observation sets. Dense update APIs
  additionally reject `observation_count * state_dimension^3` above the public
  work envelope before canonical sorting, identity materialization, or Joseph
  updates. Instrument and regime-axis identities use `fs-evidence`'s bounded
  leaf grammar; regime bounds must be finite and ordered.
- State and observation dimensions are capped at `MAX_DENSE_STATE_DIM` before a
  linear-size input can trigger quadratic dense allocation. The cap is 256 for
  this synchronous `O(n^3)` v0 core.
- The candidate's `Color::Estimated.estimator` is
  `assimilation-candidate:v1:<64 lowercase hex>`. Its BLAKE3 input uses typed
  length framing and binds the full prior, canonical observation multiset
  (including operator, reading, noise, instrument, and multiplicity), and
  proposed regime. The identity is deterministic, collision-resistant, bounded,
  delimiter-unambiguous, and changes with every bound semantic field.
- Candidate dispersion is `+infinity`, the shared explicit no-spread-claim
  sentinel. No API in this crate directly constructs `Color::Validated`.

## Error model

Structured `AssimError`; valid public calls return refusals rather than indexing
panics. Empty/ragged/indefinite/non-finite beliefs, malformed observations,
invalid identities and regimes, non-positive or non-finite noise, dimension
mismatches, oversized aggregate count/work requests, degenerate innovations,
and finite-input arithmetic overflow are refused. Allocation failure remains
Rust's process-level behavior inside the admitted public resource envelope.

## Determinism class

Fully deterministic: fusion, misfit, candidate identity, and posterior are pure
functions of semantic inputs. Observation permutation is canonicalized, so it
does not change result bits or identity. Duplicate observations are retained.
Changing the canonical schema or numerical algorithm requires a candidate
identity version/domain bump.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/assimilate.rs` (Proposal 11, G0/G3): mean shift and variance reduction;
misfit reduction; point-versus-scan noise; exact permutation stability; honest
estimated color and bounded identity; empty/ragged/non-finite/indefinite belief
refusals; checked indexing and point-sensor bounds; malformed observation and
instrument refusals; scan-noise failures; empty aggregates; regime failures;
delimiter-collision and multiplicity identity probes; finite-input overflow;
strict rejection of near-boundary negative curvature; Joseph-form replay of a
cancellation-induced invalid-posterior counterexample; seeded randomized
post-update validation; dense-state, observation-count, and multiplicative-work
resource admission; and deterministic replay.

## No-claim boundaries

- v1 is the LINEAR-GAUSSIAN assimilation with linear observation operators;
  full weak-constraint 4D-Var over a nonlinear forward model uses Proposal 1's
  CERTIFIED adjoints (adjoint-certs) and is the fuller deliverable — this crate
  is the Kalman/Gauss-Newton core.
- `Color::Estimated` is deliberate. Consuming calibrated measurements does not
  by itself prove model-form validity in a regime. Promotion to
  `Color::Validated` requires an external admission boundary that verifies
  calibrated dataset provenance, retained validation evidence, and an
  authenticated authority. No generic authenticated validation capability is
  currently owned by this crate, so it exposes no promotion API.
- The semidefinite admission check and Kalman arithmetic are floating-point
  fail-closed guards, not interval or theorem certificates. Correlation scaling
  and diagonal pivoting reduce numerical sensitivity, but this crate does not
  claim rigorous eigenvalue enclosures under roundoff. In particular, it may
  reject a mathematically semidefinite matrix whose computed Schur complement
  crosses below zero; it never converts such a negative result into an admitted
  zero pivot.
- The dense v0 state cap is a resource contract, not a scientific limit. Larger
  states require a future sparse or matrix-free implementation with explicit
  memory budgets and tile-boundary cancellation rather than raising this cap.
- The observation operators are SUPPLIED restriction-map rows; deriving them
  from the sheaf's trace maps is fs-geom's; the registration variance for scan
  observations comes from fs-asbuilt.
- Ledgering the update onto the per-regime model-form posterior (Proposal 3's
  maps), authenticating calibration receipts, and admitting a validated claim
  are fs-ledger/fs-package integration work; this crate produces the estimated
  candidate and proposed validity domain.
