# CONTRACT: fs-truss-e2e

TrussPath — an optimal truss with a certified critical load path. Layer L4
(ASCENT).

## Purpose and layer

Composes `fs-truss` (ground-structure LP + PDHG certificate), `fs-tropical`
(critical path), `fs-evidence` (Verified). Deps point downward.

## Public types and semantics

- `run_campaign(nx, ny, w, h, gap_tol) -> TrussReport` — optimizes a cantilever
  ground structure by PDHG and extracts the tropical critical load path.

## Invariants

- OPTIMALITY: the returned volume carries a certified relative duality gap; the
  design is `certified_optimal` (→ `Verified`) when the gap and equilibrium
  residual are tiny.
- LOAD PATH: the active bars are oriented into a DAG by distance-to-support
  (index order = topological order), and the max-plus critical path names the
  dominant load-transmission chain (≥ 2 bars) and its bottleneck bar; the path
  volume never exceeds the total volume. Exact → `Verified`.
- The optimizer prunes the candidate set (`num_active < num_members`).
- Deterministic (fixed ground structure + deterministic PDHG; no RNG).

## Error model

Panics only on a degenerate grid (no members).

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

None (a synchronous batch); production PDHG would poll `Cx` per iteration block.

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/truss.rs` (2): the optimum has a certified duality gap and a ≥2-bar
critical load path with a bottleneck inside the total volume; determinism.

## No-claim boundaries

The LP is the plastic (lower-bound) ground-structure formulation; the critical
"load path" is a material-volume longest chain, not a stress-flow proof; sizing
to a catalog and buckling checks (`fs-truss::size_and_snap`, `rod_buckling_check`)
are downstream and not exercised here.
