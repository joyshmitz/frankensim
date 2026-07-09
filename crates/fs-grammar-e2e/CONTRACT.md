# CONTRACT: fs-grammar-e2e

GrammarForge — certified-fabricable geometric program discovery. Layer L4
(ASCENT).

## Purpose and layer

Composes `fs-shapeprog` (CSG programs + certified rewrites), `fs-archive`
(MAP-Elites), `fs-fab` (manufacturability), `fs-evidence` (Verified). Deps
point downward.

## Public types and semantics

- `target() -> Geom` — the peanut target (two unit spheres at `x=±0.8`).
- `build_program(r1, r2, d, o) -> Geom` — a candidate CSG program.
- `run_campaign(match_tol, simplify_tol) -> GrammarReport` — sweeps a program
  grid, illuminates (size × fab-margin), simplifies each elite, and re-verifies
  every simplification certificate.

## Invariants

- ILLUMINATION: a MAP-Elites archive of the best-matching program per
  (size × fab-margin) niche.
- CERTIFICATE-PRESERVING SIMPLIFICATION: some elites shrink; for EVERY elite the
  independently re-measured SDF discrepancy between the original and simplified
  program stays within the certified `max_error` (`simplification_sound`), which
  itself never exceeds `simplify_tol`.
- The headline is `Verified` iff the best program matches within `match_tol`, is
  fab-satisfied, and simplifies soundly.
- Deterministic (fixed program grid + sample grid; no RNG).

## Error model

Total; `run_campaign` expects a non-empty archive (the grid guarantees it).

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

None (a synchronous batch).

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/grammar.rs` (4): a fabricable family is illuminated + simplified soundly;
the certified simplification bound holds; the target matches itself exactly;
determinism.

## No-claim boundaries

The grammar is a fixed two-primitive CSG family; discrepancy is sampled on a
finite grid (not a global bound); fabricability uses a single minimum-feature
proxy. A full generative grammar + adjoint fitness is the fuller `fs-shapeprog`/
`fs-xform` deliverable.
