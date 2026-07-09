# CONTRACT: fs-flowcert-e2e

FlowCert — a certified credibility map for a lattice-Boltzmann channel flow.
Layer L4 (ASCENT).

## Purpose and layer

Composes `fs-lbm` (LBM + analytic Poiseuille solution + scaling planner),
`fs-archive` (MAP-Elites), `fs-evidence` (Verified). Deps point downward.

## Public types and semantics

- `OperatingPoint { reynolds, ny, tau, viscosity, profile_error, accurate,
  regime_stable }`.
- `certify_point(reynolds, ny, u_lattice, steps, tol) -> OperatingPoint` — runs a
  channel to steady state, compares to the analytic Poiseuille profile, and reads
  the scaling-planner regime certificate.
- `run_campaign(&reynolds, &resolutions, steps, tol) -> FlowReport` — illuminates
  the (Reynolds × resolution) atlas.
- `default_sweep()` — the default grid.

## Invariants

- ACCURACY: `profile_error` is the relative max deviation from the analytic
  Poiseuille solution; the best operating point matches it tightly (`< tol`).
- CREDIBILITY MAP: the atlas SEPARATES fully-credible points (accurate AND in a
  `Verified` stable regime) from flagged ones — low Reynolds is credible, high
  Reynolds is honestly flagged; `stable_fraction ∈ (0, 1)`.
- The whole-map color is `Verified` only if every point is accurate and stable.
- Deterministic (fixed LBM sweep; no RNG).

## Error model

Panics only on an empty sweep.

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

None (a synchronous batch); production LBM would poll `Cx` per streaming sweep.

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/flowcert.rs` (3): the credibility map is illuminated and separates
credible from flagged points; a single low-Reynolds point is fully verified;
determinism.

## No-claim boundaries

A 1-D Poiseuille channel (no scalar transport, so no mixing metric); accuracy is
a manufactured-solution comparison, not a grid-convergence order; the regime
certificate is the `fs-lbm` low-Mach/`τ`-margin heuristic, not a full stability
spectrum.
