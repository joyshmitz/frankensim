# CONTRACT: fs-truss-e2e

TrussPath — a deterministic truss iterate with an advisory, endpoint-checked
critical load path. Layer L4 (ASCENT).

## Purpose and layer

Composes `fs-truss` (ground-structure LP + PDHG diagnostics), `fs-tropical`
(critical path), and `fs-evidence` (honest evidence state). Deps point downward.

## Public types and semantics

- `run_campaign(nx, ny, w, h, gap_tol, cx) -> Result<TrussReport, TrussError>` —
  optimizes a cantilever ground structure by PDHG and extracts a checked
  tropical path, refusing invalid solver-derived latencies. Grid dimensions
  must be at least 2x2. Admission bounds cubic ground-generation work,
  candidate members, sparse PDHG scalar work, active tasks, and path edges.
  Ground rules, the support/load case, and the assembled sparse LP cross the
  immutable fallible `fs-truss` admission boundary before solver work.
- `analyze_load_path(...)` — shared native/WASM path analysis. It admits only
  unique in-range identities, finite positive weights, and a connected chain
  of at least two strictly support-ward bars from the indexed load node to an
  indexed support.

## Invariants

- OPTIMALITY: `solver_converged` records the declared gap and equilibrium-
  residual thresholds. The approximate primal iterate is not exactly feasible,
  so its finite optimum interval remains `Estimated`.
- LOAD PATH: active bars are oriented by strict distance-to-support progress.
  Reachability and co-reachability filter out disconnected components and
  interior-only chains. The max-plus witness must start at the load, end at a
  support, contain at least two bars, and be joint-continuous. A bottleneck is
  named only for a unique path with one strictly heaviest positive bar.
- The load-path color remains `Estimated`: directed tropical rounding bounds
  only arithmetic over already-rounded weights. They do not enclose PDHG
  forces, the force-threshold active set, or the member-volume products.
- The optimizer prunes the candidate set (`num_active < num_members`).
- Deterministic (fixed ground structure + deterministic PDHG; no RNG).

## Error model

`TrussError` refuses degenerate/oversized grids, unsafe geometry scales,
non-finite or non-positive tolerances, excessive construction/solver/path work,
allocation failure, cancellation, empty member sets, malformed path data, and
incomplete load-support chains.
Caller input does not panic, and a path refusal is never converted to positive
evidence.

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

Ground construction and LP assembly poll the caller's `Cx` at deterministic
bounded strides and return a structured cancellation refusal without publishing
partial state. The fixed PDHG solve remains synchronous and iteration-bounded;
solver-loop cancellation is a separate successor.

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/truss.rs`: diagnostic convergence and iteration-cap truthfulness;
determinism; invalid/exact-bound work admission; index-based supports; and a
synthetic disconnected-heavy-component path falsifier. A pre-cancelled context
is refused before a campaign report exists.

## No-claim boundaries

The LP is the plastic (lower-bound) ground-structure formulation. The critical
"load path" is an advisory material-volume longest chain through a thresholded
iterate, not a stress-flow proof. Sizing to a catalog and buckling checks
(`fs-truss::size_and_snap`, `rod_buckling_check`) are downstream and not
exercised here. A finite Verified optimum interval needs an exactly feasible
primal repair or another rigorous upper-bound construction. Verified load-path
evidence additionally needs interval member forces/products and active-set
separation; the current campaign claims neither.
