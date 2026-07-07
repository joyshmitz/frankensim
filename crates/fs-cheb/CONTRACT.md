# CONTRACT: fs-cheb

> Status: PARTIAL — the 1D core, collocation, and ORR–SOMMERFELD
> sections are in force; 2D/3D low-rank, Fourier-periodic,
> colleague-matrix roots, and Qty integration remain follow-up scope.

## Purpose and layer
Chebfun-style function objects (plan §6.5): smooth 1D functions as
adaptively truncated Chebyshev expansions, plus spectral collocation
differentiation matrices. Layer: **L1**. Deps: fs-fft (DCT pair),
fs-la (LU for the collocation eigen demo), fs-math (strict cos/sin).

## Public types and semantics
- `Cheb1` — coefficients over FIRST-KIND Chebyshev points (roots grid):
  values ↔ coefficients is exactly fs-fft's DCT-II/III pair. `build`
  doubles the grid until the trailing quarter of coefficients sits at
  the machine-precision plateau (8.9e-16 relative), then truncates;
  unresolvable functions panic at `max_degree` with a structured
  message. `eval` (Clenshaw), `differentiate` (coefficient recurrence,
  domain chain rule), `integral` (even-coefficient formula), `add`,
  `mul` (resample + rebuild), `roots` (subdivision + bisection +
  Newton polish).
- `lobatto_points`, `diff_matrix` — Chebyshev–Lobatto collocation:
  Trefethen construction with the negative-sum-trick diagonal (rows sum
  to EXACT zero, tested bitwise).
- `orr_sommerfeld::{growth_rates, max_growth, critical_reynolds}` —
  plane-Poiseuille temporal stability via clamped Chebyshev collocation
  (Trefethen D4c construction from the φ = (1−y²)u substitution),
  generalized problem reduced through fs-la's complex LU, spectrum via
  the complex QR eigensolver. `growth_rates` is the "modal growth rates
  σ₁..σ_k" first-class query (descending real part, deterministic
  tie-breaks). ACCEPTANCE EVIDENCE: the neutral crossing at α = 1.02056
  reproduces the published Re_c = 5772.22 (displayed digits exact at
  N = 48); stability verdicts correct on both sides; golden hash
  `0x7b3b_e74e_d5a6_faad` cross-ISA.
- `dirichlet_laplace_eigs` — deflated inverse-power-iteration demo of
  the collocation eigen path (validates against analytic (kπ/2)²).

## Invariants
1. Machine-precision recovery on analytic fixtures with expected degree
   growth (exp ≤ 20, Runge in (exp, 300], sin(20x) on [0,3] in
   (40, 200]) — tested.
2. Calculus identities: d/dx exp = exp to 1e-11; definite integrals to
   1e-12 with domain scaling — tested.
3. Plateau detection does NOT chase noise floors (tested with a
   deterministic ~1e-18 jitter fixture).
4. `diff_matrix` rows sum to exact zero (differentiation annihilates
   constants bitwise).
5. Deterministic cross-ISA: all state built on strict fs-math cos/sin
   and fixed-order arithmetic (golden hash, trj-verified).

## Error model
Structured panics for programmer/modeling errors: inverted domains,
unresolved functions at `max_degree`, domain mismatches in algebra.

## Determinism class
Bit-deterministic cross-ISA by construction. Golden hash over
coefficients + integral + derivative sample + roots + collocation
eigenvalues, recorded on aarch64-apple and required to match on x86-64.

## Cancellation behavior
Construction is bounded (max_degree cap); no poll points needed at v1
scales.

## Unsafe boundary
None.

## Feature flags
None.

## Conformance tests
tests/cheb_battery.rs (recovery, calculus, plateau robustness, roots,
collocation accuracy, eigen demo, golden hash).

## No-claim boundaries
- Roots of even multiplicity (no sign change) are NOT found by the v1
  subdivision rootfinder; colleague-matrix roots with fs-ivl
  certification are the follow-up (needs a nonsymmetric eigensolver).
- No 2D/3D low-rank, no Fourier-periodic variant, no Qty-dimensioned
  functions, no FrankenScipy cross-checks yet.
- `mul` may overshoot the minimal degree (resample-based); fine for
  correctness, recorded for the perf lane.
