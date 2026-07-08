# CONTRACT: fs-sos

Proof-carrying optimization: sum-of-squares certificates as executable proofs of
polynomial lower bounds.

## Purpose and layer

Layer L4 (ASCENT). No dependencies — pure Rust (in-house Jacobi eigensolver for
the PSD test).

## Public types and semantics

- `Poly` — a univariate polynomial (ascending coefficients): `new`, `constant`,
  `eval` (Horner), `add`, `sub`, `mul`, `degree`, `coeffs`, `max_abs_coeff`;
  free `square(q)`.
- `SosCertificate { squares, lower_bound }` — the claim `p − lower_bound = Σ
  squaresᵢ²`. `residual(p)` (largest coefficient of the mismatch), `verify(p,
  tol)`, `certified_bound(p, tol)` (returns the bound ONLY if it verifies).
- `certify_quadratic(a, b, c)` — the exact global minimum of `a x² + b x + c`
  (`a > 0`) with its SOS certificate, else `None`.
- `is_psd(matrix, tol)` — positive-semidefiniteness (min eigenvalue `≥ −tol`),
  the SDP-feasibility core.
- `lyapunov_certifies_stability(A, P)` — does `V = xᵀPx` certify stability of `ẋ
  = Ax` (`P ≻ 0` and `−(AᵀP + PA) ≻ 0`)?

## Invariants

- SOUNDNESS: a certificate that verifies implies `p(x) ≥ lower_bound` for every
  `x` (a square is nonnegative).
- ZERO FALSE CERTIFICATES: a claimed bound above the true minimum, or a
  mismatched square set, fails to verify — `certified_bound` returns `None`.
- `certify_quadratic` returns the exact (tight) global minimum + a verifying
  certificate.
- `is_psd` decides definiteness correctly (incl. the semidefinite boundary).
- `lyapunov_certifies_stability` is sound (certifies only genuinely stable
  systems for the given `P`).

## Error model

Total functions; `certify_quadratic` returns `None` on `a <= 0`. No panics.

## Determinism class

Fully deterministic: polynomial arithmetic and the Jacobi eigensolver are pure.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/sos.rs` (8 cases): polynomial arithmetic; an exact + sound quadratic
certificate; a multi-square tight-bound certificate; ZERO false certificates
(too-high bound + bogus squares rejected); unbounded-form rejection; PSD
feasibility decided correctly; a Lyapunov stability certificate; determinism.

## No-claim boundaries

- v0 is the CERTIFICATE layer: it VERIFIES sum-of-squares proofs and constructs
  them for quadratics. The general LASSERRE/SOS moment relaxations that SEARCH
  for a certificate — the in-house first-order / Burer–Monteiro SDP SOLVER over
  the moment matrix — are the fuller deliverable, staged. Here the PSD feasibility
  core (`is_psd`) is present but not yet driven by an optimizer.
- Univariate SOS is complete (every nonnegative univariate polynomial is a sum
  of squares); the MULTIVARIATE gap (the Motzkin polynomial is nonnegative but
  not SOS) requires the moment/Positivstellensatz machinery — staged.
- `lyapunov_certifies_stability` verifies a candidate `P`; SEARCHING for `P`
  (and SOS Lyapunov functions certifying nonlinear regions of attraction) is the
  SDP, staged.
- Dual certificates are checked by coefficient matching; interval-verified dual
  certificates against an adversarial battery are a follow-on.
