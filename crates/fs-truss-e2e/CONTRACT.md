# CONTRACT: fs-truss-e2e

TrussPath — a deterministic truss iterate with an advisory, endpoint-checked
critical load path. Layer L4 (ASCENT).

## Purpose and layer

Composes `fs-truss` (ground-structure LP + PDHG diagnostics), `fs-tropical`
(critical path), and `fs-evidence` (honest evidence state). Deps point downward.

## Public types and semantics

- `run_campaign(nx, ny, w, h, gap_tol, cx) -> Result<TrussReport, TrussError>` —
  optimizes a cantilever ground structure by PDHG and extracts a checked
  tropical path. The final iterates also pass through `fs-truss`'s outward
  optimum-certificate kernel before the report can carry `Color::Verified`.
  Grid dimensions must be at least 2x2. Admission bounds cubic ground-generation
  work, candidate members, sparse PDHG scalar work, certificate work and retained
  state, active tasks, and path edges. Ground rules, the support/load case, and
  the assembled sparse LP cross the immutable fallible `fs-truss` admission
  boundary before solver work.
- `analyze_load_path(...)` — shared native/WASM path analysis. It admits only
  unique in-range identities, finite positive weights, and a connected chain
  of at least two strictly support-ward bars from the indexed load node to an
  indexed support.
- `optimality_color_from_certificate(problem, x, y, settings, status, gap,
  eq_residual)` — the sole native/browser promotion gate. Only a structurally
  valid private certificate bound to those canonical arrays, iterates, and
  settings yields `Verified { lo, hi }`. The gate checks retained shapes and the
  certificate's operation cap before hashing caller arrays; every mismatch,
  work excess, or unavailable proof remains `Estimated`.
- `rescale_optimality_color(color, positive_divisor)` — preserves an existing
  Verified interval through outward division (used for normalized-to-physical
  yield-stress scaling), preserves weaker colors, and demotes invalid scaling.

## Invariants

- OPTIMALITY: `solver_converged` records only the declared gap and equilibrium-
  residual diagnostics. Independently, `fs-truss` proves an exactly feasible
  primal repair with a Neumann enclosure and a feasible scaled dual with outward
  slack checks. Only finite ordered endpoints bound to the exact LP, settings,
  iterates, methods, limits, and retained witness become `Verified`.
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
allocation failure, cancellation, malformed certificate state, empty member
sets, malformed path data, and incomplete load-support chains. Certificate work
or conditioning limits are a sound numerical unavailability represented by an
`Estimated` color, not an error and never a partial `Verified` result.
Caller input does not panic, and a path refusal is never converted to positive
evidence.

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

Ground construction and LP assembly poll the caller's `Cx` at deterministic
bounded strides and return a structured cancellation refusal without publishing
partial state. The certificate proof also polls the same `Cx` through admission,
repair, verification, identity binding, and atomic publication. The fixed PDHG
solve remains synchronous and iteration-bounded; solver-loop cancellation is a
separate successor.

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/truss.rs`: independent diagnostic convergence; finite positive outward
optimality bounds; deterministic bound replay; iteration-cap truthfulness;
invalid/exact-bound work admission; index-based supports; and a synthetic
disconnected-heavy-component path falsifier. A pre-cancelled context is refused
before a campaign report exists.

## No-claim boundaries

The certified interval bounds the plastic ground-structure LP optimum; it does
not certify catalog sizing, buckling, geometric nonlinearity, or the advisory
critical path. The critical "load path" is a material-volume longest chain
through a thresholded iterate, not a stress-flow proof. Sizing to a catalog and
buckling checks (`fs-truss::size_and_snap`, `rod_buckling_check`) are downstream
and not exercised here. Verified load-path evidence additionally needs interval
member forces/products and active-set separation; the current campaign claims
neither.
