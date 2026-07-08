# CONTRACT: fs-dimine

Dimensional knowledge mining (plan addendum, Proposal 9's knowledge apex): fit
closed-form power-law scaling laws over a certified corpus in dimensionless-
group (π) space.

## Purpose and layer

Layer L4. Depends only on `fs-evidence` (UTIL, the `Color` lattice). Pure-Rust
log-linear least squares — NO external symbolic-regression library, Python, or
FFI (Franken-only).

## Public types and semantics

- `Sample { pi: Vec<f64>, qoi }` — one corpus point (all π and qoi strictly
  positive; logs are taken).
- `fit_power_law(&[Sample]) -> Result<MinedLaw, MineError>` — fits
  `y = C · Π πⱼ^{aⱼ}` by solving the normal equations of the log-linear model
  `ln y = ln C + Σ aⱼ ln πⱼ` (Gaussian elimination with partial pivoting).
- `MinedLaw { coefficient, exponents, r_squared, envelope, samples, color }` —
  `r_squared` is the log-space fit significance; `envelope` is the per-group
  trained `(min, max)` support (in 1D exactly the convex hull); `color` is
  always `Color::Estimated` (a mined law is a conjecture, never a certified
  bound). `is_significant(threshold)` gates on `r²`; `predict(pi)` evaluates the
  law, REFUSING to extrapolate beyond the envelope.
- `MineError` — `TooFewSamples` / `DimMismatch` / `NonPositive` / `Singular` /
  `Extrapolation`. `Color` is re-exported.

## Invariants

- A mined law is ALWAYS estimated-color.
- `fit_power_law` needs `>= groups + 2` samples and rejects a rank-deficient
  (collinear) design as `Singular`.
- `predict` refuses (does not silently serve) any point outside the trained
  π-space envelope; boundary is inclusive.
- Non-positive π or qoi fail closed (`NonPositive`) — logs are undefined there.

## Error model

Structured `MineError` values (refusals that teach), never panics.

## Determinism class

Fully deterministic: fitting is a pure function of the corpus (no RNG); the
same corpus reproduces bit-identical coefficients/exponents.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/dimine.rs` (Proposal 9, 9 cases): 1D and multi-π power-law recovery
(exact data → r² ≈ 1, correct C/exponents), estimated-color, noise → not
significant, extrapolation refusal (envelope = convex hull in 1D, boundary
inclusive), too-few-samples / non-positive / singular / dim-mismatch errors,
determinism.

## No-claim boundaries

- Buckingham-π extraction (forming dimensionless groups from units-typed
  quantities) is fs-regime/fs-qty's job; this crate fits laws over PRE-FORMED
  π-coordinates.
- v1 fits POWER LAWS only (log-linear). Other functional forms (additive,
  saturating, piecewise) and general symbolic regression are later work.
- The validity envelope is the per-coordinate trained range — exactly the
  convex hull in 1D, and a conservative axis-aligned box in higher dimensions
  (a tighter hull is a refinement).
- A mined law is a CONJECTURE (estimated color). Promotion toward validated is
  the falsification budget's job (Proposal 6), not this crate's.
