# CONTRACT: fs-asbuilt

As-built ingestion — reality is just another chart (plan addendum,
Proposal 11): register scan data to the design and emit an honestly colored
as-built candidate.

## Purpose and layer

Layer L2 (representation/geometry). Depends on `fs-evidence` (`Color` and
`ValidityDomain`) and the native `fs-blake3` content-identity primitive. Pure,
deterministic; a closed-form 2-D rigid fit (no SVD).

## Public types and semantics

- `Point2::new(x, y) -> Result<Point2, RegError>` constructs finite points and
  canonicalizes signed zero; coordinates are private and available through
  `x()` / `y()`.
- `Fiducial::new(design, measured)` pairs already-valid typed points; fields
  are private with read-only accessors.
- `register(&[Fiducial]) -> Result<Registration, RegError>` — the rigid
  rotation+translation best mapping design → measured (2-D Umeyama/Procrustes
  closed form). Requires `>= MIN_FIDUCIALS` (3) non-collinear fiducials; carries
  `residual_rms` forward (the registration uncertainty). Registration fields
  are private; `Registration::new` is fallible, accessors are read-only, and
  `Registration::apply` refuses non-finite arithmetic overflow.
- `well_posed(&Registration, certified_deviation) -> bool` — the R8 gate: true
  iff the registration residual is BELOW the deviation being certified.
- `as_built_diff(&Registration, design, scanned, design_tolerance,
  measurement_noise, calibration_candidate) -> Result<AsBuiltDiff, RegError>` — the
  per-point δ after registration; `within_tolerance`, `above_noise_floor`, and
  a `Color::Estimated` whose domain-separated BLAKE3 identity binds every
  input. `proposed_regime` carries residual/noise/tolerance bounds for later
  authenticated review. The calibration string is a bounded, structurally
  valid candidate identity, never authority. Result fields are private and
  exposed through read-only accessors, so callers cannot forge or mutate an
  authenticated-looking `AsBuiltDiff` value.
- `RegError` covers insufficient/collinear/oversized points, length mismatch,
  empty data, non-finite or negative numeric inputs, malformed calibration
  identity, arithmetic overflow, and bounded-allocation failure.

## Invariants

- Registration RECOVERS a ground-truth rigid transform (residual → 0 on clean
  fiducials) and CARRIES its error forward on noisy ones.
- Well-posedness needs `>= 3` non-collinear fiducials (rank-2 design scatter).
  The rank gate is relative to squared scatter trace, so uniformly rescaling a
  representable non-collinear point set does not change admission;
  collinear/too-few is refused. Registration and diff inputs are capped at
  `MAX_AS_BUILT_POINTS`.
- Public point, registration, and as-built result fields cannot be forged; all numeric inputs
  are finite, residual/tolerance/noise are non-negative, and non-finite
  intermediate arithmetic is refused.
- R8: `well_posed` is false when the residual meets/exceeds the certified
  deviation (signal below the noise floor) or the deviation is non-positive.
- The default as-built δ is always `Estimated`. Its bounded identity uses
  length-framed canonical fields followed by a domain-separated native BLAKE3
  digest, preventing delimiter and prefix collisions. Numeric identity fields
  canonicalize `-0.0` to `+0.0`, matching their mathematical equality.
- A well-formed string such as `forged-calibration-claim` cannot promote a
  result: this crate has no validated-promotion API.

## Error model

Structured `RegError` values; hostile numeric/identity inputs return errors.
Deviation allocation uses `try_reserve_exact`; no public path intentionally
panics.

## Determinism class

Fully deterministic: the fit, gate, and δ are pure functions of the inputs.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/asbuilt.rs`: exact/noisy registration, fiducial well-posedness, R8,
estimated diff semantics, proposed regime, empty/length errors, NaN/infinity/
negative rejection, invalid registration, arithmetic overflow, malformed and
forged calibration identities, delimiter-collision resistance, bounded
  identity, signed-zero canonicalization, scale-invariant rank admission, and
  deterministic replay.

## No-claim boundaries

- v1 is 2-D rigid registration (rotation + translation) with KNOWN
  correspondences; 3-D (Kabsch/SVD), scale, and correspondence-free ICP are
  follow-ons.
- Registration is treated as an optimization whose error is carried forward;
  writing it (and the as-built δ) to the design ledger is fs-ledger's
  integration, and the fiducial/datum PRIMITIVES at design time are fs-geom's
  (this crate consumes the correspondences).
- The scan is modeled as sampled points; admitting a full CT voxel grid /
  point cloud as a representation type with restriction maps to interface trace
  spaces extends fs-rep-voxel + fs-geom's chart zoo.
- The δ reuses the deviation metric directly; the full sheaf δ / watertightness
  machinery is the geometry layer's.
- Calibration authenticity is an explicit no-claim. A future Validated
  promotion must inject a typed verifier, verify retained calibration artifact
  bytes/content hash under a declared policy, and bind that verification
  receipt. No such API exists in v2.
