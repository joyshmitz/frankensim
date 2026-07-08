# CONTRACT: fs-archive

Quality-diversity archives: MAP-Elites / CVT illumination + novelty.

## Purpose and layer

Layer L4 (ASCENT). No dependencies — pure Rust (`BTreeMap` for deterministic
niche storage).

## Public types and semantics

- `Elite { solution, descriptor, fitness }` — the best solution in a niche.
- `MapElites::new(lo, hi, bins)` — a gridded behavior space; `cell_of`
  (discretize, clamped), `add` (insert iff empty niche or strict improvement,
  returns whether it became an elite), `capacity`, `num_elites`, `coverage`,
  `qd_score`, `elite_at`, `best`, `elites`.
- `CvtArchive::new(centroids)` — the same illumination over a Voronoi
  tessellation; `nearest_centroid`, `add`, `capacity`, `num_elites`, `coverage`,
  `qd_score`, `best`.
- `novelty(descriptor, others, k)` — mean distance to the `k` nearest
  neighbours (`+∞` for an empty set).

## Invariants

- MAP-Elites keeps exactly one elite per niche: a worse candidate in a filled
  niche is rejected, a strictly better one replaces it.
- ILLUMINATION preserves DIVERSITY: a low-fitness solution in an otherwise-empty
  niche is retained (unlike single-objective optimization).
- `coverage` and `qd_score` are monotone non-decreasing under `add`.
- `cell_of` / `nearest_centroid` assign descriptors correctly (grid clamps
  out-of-range descriptors).
- `novelty` grows with distance from the archive.

## Error model

Total functions; constructors panic only on malformed configuration
(dimension mismatch, zero bins, empty centroids, non-increasing bounds).

## Determinism class

Fully deterministic: `BTreeMap` gives a fixed niche order; identical add
sequences yield identical archives.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/archive.rs` (7 cases): descriptors map to the right cells; MAP-Elites
keeps the best per niche; illumination preserves diversity; coverage + QD-score
are monotone; the CVT archive assigns to the nearest centroid; novelty rewards
distance; determinism.

## No-claim boundaries

- v0 is the ARCHIVE data structure + its metrics + novelty scoring. The full QD
  ALGORITHM loop (variation operators, batch selection/emitters, CMA-ME-class
  emitters) and the descriptor computations from layer-native definitions
  (behavior descriptors sourced from geometry/physics/optimization layers) are
  the fuller deliverable, staged.
- The CVT centroids are supplied by the caller; generating them by k-means /
  Lloyd relaxation over a sampled descriptor space is a follow-on.
- Novelty is brute-force k-NN; a spatial index for large archives is a
  performance follow-on.
