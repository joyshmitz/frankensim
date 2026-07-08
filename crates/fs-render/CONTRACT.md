# CONTRACT: fs-render

Unbiased spectral path-tracing core: the verifiable Monte-Carlo foundations.

## Purpose and layer

Layer L5 (LUMEN). No dependencies — pure Rust.

## Public types and semantics

- `radical_inverse(base, i)` / `halton(dim, i)` — deterministic low-discrepancy
  coordinates (an image is as replayable as a solve).
- `cosine_sample_hemisphere(u1, u2) -> (dir, pdf)` — cosine-weighted hemisphere
  sample (`pdf = cosθ/π`).
- `Lambertian { albedo }` — `brdf` (`ρ/π`); `furnace_radiance(incident,
  samples)` — the FURNACE Monte-Carlo estimate (exactly `albedo·incident`).
- `balance_heuristic` / `power_heuristic` — MIS weights; `mis_weight_sum(pf,
  pg)` — the weight-sum audit (nominally `1`).
- `mis_integrate_unit(f, n)` — an unbiased MIS estimate of `∫₀¹ f` combining
  uniform + linear-importance strategies.
- `hero_wavelengths(hero, count, min, max)` / `spectral_integral(spectrum, min,
  max, samples)` — hero-wavelength spectral integration.

## Invariants

- FURNACE: `furnace_radiance` returns exactly `albedo·incident` (energy
  conservation; cosine importance sampling gives zero variance).
- MIS WEIGHT-SUM: the two balance weights at a sample sum to `1` (no energy lost
  or gained at strategy boundaries).
- MIS integration is unbiased (converges to `∫f`).
- Hero-wavelength integration is exact on a constant spectrum and accurate on a
  ramp; `cosine_sample_hemisphere` returns unit vectors in the upper hemisphere.
- Everything is deterministic (low-discrepancy sequences, no RNG here).

## Error model

Total functions; `halton` panics only on `dim >= 8` (out of the prime table).

## Determinism class

Fully deterministic: the sampling is low-discrepancy, keyed by sample index.

## Cancellation behavior

None here; the production tracer polls `Cx` at tile boundaries (a render is a
budgeted, cancellable study).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None (the `frontier-polarization` Mueller-calculus path is staged).

## Conformance tests

`tests/render.rs` (7 cases): radical inverse known values; cosine samples are
unit vectors with the right pdf; the furnace test conserves energy exactly; MIS
weights sum to one (+ heuristic ordering); MIS integration is unbiased;
hero-wavelength integration exact on a constant / accurate on a ramp;
determinism.

## No-claim boundaries

- v0 is the verifiable Monte-Carlo core (sampling, furnace, MIS, spectral
  integration). The full unidirectional PATH TRACER — wide-BVH SIMD traversal,
  watertight ray-triangle tests, next-event estimation with a LIGHT-BVH,
  Beer–Lambert media, ray-stream sorting, progressive tile streaming to HELM,
  per-tile Philox keyed by (seed, frame, tile), and `Cx` cancellation — is the
  fuller deliverable, staged.
- The spectral pipeline here integrates a spectrum; the radiometrically correct
  spectra→XYZ→display transforms and layered measured-spectrum materials are
  staged.
- `mis_integrate_unit` is a 1-D demonstrator of the balance heuristic; the
  production MIS lives in the path integrator across BSDF/light strategies.
