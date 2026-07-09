# CONTRACT: fs-neuroshape-e2e

NeuroShapeCert — a PROVEN neural implicit shape. Layer L5 (LUMEN).

## Purpose and layer

Composes `fs-rep-neural` (Lipschitz + IBP), `fs-viz` (isocontour + Morse),
`fs-evidence` (Verified). Deps point downward.

## Public types and semantics

- `blob_sdf_net() -> MlpSdf` — the `tanh`-MLP `f = Σ tanh(3(±coord−0.7)) + 3`.
- `run_campaign(&MlpSdf, ring_r, inner) -> NeuroShapeReport` — certifies the
  Lipschitz bound, a no-tunnel sphere-trace radius, an interval topology
  certificate (inside box + outside ring), a Morse single-minimum cross-check,
  and localizes the zero set.

## Invariants

- `safe_radius = |f|/L` under-estimates the true distance to the surface (sound
  sphere tracing — no tunneling); every zero crossing is farther than a step.
- TOPOLOGY: a certified-inside central box (`hi < 0`) trapped by a ring of
  certified-outside boxes (`lo > 0`) proves a single BOUNDED component →
  `Verified`; an open ring yields `Estimated`.
- The interior has a single Morse minimum (`classify_hessian → Minimum`).
- Deterministic (fixed net + grid; no RNG).

## Error model

Total on the demo net; `eval_interval`/`classify_hessian` are total.

## Determinism class

Fully deterministic (G5).

## Cancellation behavior

None (a synchronous batch).

## Unsafe boundary

None; `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/neuroshape.rs` (3): topology certified (Lipschitz, sound safe radius,
inside+ring, Morse minimum, closed surface inside the ring); an open ring yields
no certificate; determinism.

## No-claim boundaries

2-D demo net; the Lipschitz bound is the (loose) product-of-spectral-norms; the
topology certificate proves boundedness + non-emptiness + a single Morse minimum,
not the full homeomorphism type. The Hessian is a finite-difference estimate.
