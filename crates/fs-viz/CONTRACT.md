# CONTRACT: fs-viz

Scientific visualization: the verifiable topological-summary primitives.

## Purpose and layer

Layer L5 (LUMEN). No dependencies — pure Rust.

## Public types and semantics

- `Vec2` — a 2-D point/vector.
- `streamline(field, seed, dt, steps) -> Vec<Vec2>` — RK4 integration of a flow
  field into an ordered polyline (seed included).
- `CriticalKind` (`Minimum`/`Saddle`/`Maximum`/`Degenerate`), `CriticalPoint {
  kind, morse_index }`, `classify_hessian(hessian, tol)` — the Morse type + index
  (number of negative Hessian eigenvalues) of a critical point.
- `Grid2::from_fn(nx, ny, lo, hi, f)` — a scalar field on a regular grid; `at`,
  `point`, `isocontour_crossings(iso)` (linearly-interpolated edge crossings).

## Invariants

- STREAMLINES honor the flow: a rotation field `(-y, x)` conserves the radius (a
  circle); a saddle field `(x, -y)` conserves `xy` (a hyperbola).
- `classify_hessian` recovers the known Morse type: `x²+y²`→min (index 0),
  `x²−y²`/`xy`→saddle (index 1), `−(x²+y²)`→max (index 2); a zero eigenvalue is
  degenerate.
- `isocontour_crossings` of a circle SDF all lie on the circle (to grid
  resolution); a level set outside the field's range yields no crossings.
- All primitives are deterministic.

## Error model

Total functions; `Grid2::from_fn` panics only on a degenerate grid (`< 2` points
per axis).

## Determinism class

Fully deterministic: RK4, classification, and contouring are pure functions.

## Cancellation behavior

None here; the production viz shares LUMEN tiling + progressive streaming with
`Cx` cancellation.

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/viz.rs` (6 cases): a rotation field streams along a circle; a saddle
field conserves the hyperbola invariant; Hessian classification recovers the
Morse type; a circle-SDF isocontour lies on the circle (+ empty out-of-range);
grid sampling/addressing; determinism.

## No-claim boundaries

- v0 is the ANALYTICALLY-VERIFIABLE core: RK4 streamlines, Morse critical-point
  classification, and the isocontour edge-crossing pass. The fuller deliverable
  — DUAL contouring with sharp-feature (QEF) vertex placement into a mesh,
  DIRECT VOLUME RENDERING with preintegrated transfer functions, LINE-INTEGRAL
  CONVOLUTION, tensor/stress-ellipsoid glyphs, and full MORSE–SMALE complexes /
  Reeb graphs with persistence thresholding — is staged.
- The contouring shares its edge-crossing pass with the SDF→mesh converter
  (one contouring implementation); the 3-D dual-contouring surface + Hermite
  normals are downstream.
- Adaptive/embedded-pair integration (fs-time steppers) and Qty-labeled
  perceptually-uniform colormaps are staged with the rendering integration.
