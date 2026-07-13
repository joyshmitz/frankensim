# CONTRACT: fs-surrogate

Learned accelerators with guarantees: surrogates permitted only inside certified
validity bands — ML proposes, certified numerics disposes.

## Purpose and layer

Layer L4 (surrogate / ROM). The default core is dependency-free and pure Rust
(an in-house symmetric eigensolver for the method of snapshots); the optional
ladder feature depends on `fs-evidence` for color payloads.

## Public types and semantics

- `pod(&[Vec<f64>], energy_threshold) -> Result<Pod, SurrogateError>` — a POD
  reduced-order model via the method of snapshots (correlation matrix `SᵀS`,
  symmetric eigendecomposition, modes `φₖ = Svₖ/σₖ`), retaining the fewest modes
  capturing `energy_threshold` of the mean-centered energy.
- `Pod` — `rank`, `energy_captured`, `project`, `reconstruct`,
  `reconstruction_error` (the reduced-vs-full error).
- `conformal_band(residuals, alpha) -> ConformalBand` — the distribution-free
  split-conformal band (the `⌈(1−α)(n+1)⌉`-th smallest residual); `covers`,
  `half_width`. `empirical_coverage(&ConformalBand, &[(pred, truth)])`.
- `certify_or_escalate(&ConformalBand, in_validity_domain, decision_tolerance)
  -> Decision` — `UseSurrogate` iff inside the domain AND the band is at least as
  tight as the decision tolerance, else `Escalate`.
- Root `SurrogateError` — `NoSnapshots` / `DimMismatch` / `BadThreshold`.

- `ladder` module (addendum Proposal A, bead knh1.4; [F], behind
  `abstraction-ladder`): a bounded abstraction ladder whose present
  authority is ESTIMATED, not certified. `TruthModel` defines the P1
  full-order elliptic family's DECLARED level-0 semantics; its f64 solve
  is not an enclosure. `RbLevel` uses offline snapshots, an
  energy-orthonormal basis, and online Galerkin evaluation of the
  textbook residual/coercivity a-posteriori estimator. `ConceptLevel`
  uses interpolation with total dispersion calibrated at finite probes
  as `|concept − lower RB| + lower RB QoI estimator`; admission also
  evaluates that quantity at the actual query and takes the larger value.
  `Ladder::at_level(k)?.query(μ, tol)` performs AUTOMATIC BOUNDED
  DESCENT: an RB/concept rung answers only when its estimator is within
  tolerance; otherwise the leak is recorded and the query descends.
  `ladder::SurrogateError` names all ladder refusals. `rb_coverage` is
  the bounded, fallible kill measurement.

## Invariants

- POD reproduces an exactly-representable (low-rank) snapshot set to roundoff;
  its modes are orthonormal; the retained rank captures `>= energy_threshold`.
- The conformal band achieves at least its nominal `(1−α)` empirical coverage on
  exchangeable held-out data.
- `certify_or_escalate` uses the surrogate ONLY when trustworthy (in-domain +
  band tight enough), so a fleet of queries costs strictly less than
  all-high-fidelity whenever any query is served by the surrogate.
- Every ladder-emitted color is `Estimated` and passes the shared
  `fs-evidence` payload validator. RB answers carry the f64-evaluated
  QoI estimator as dispersion; concept answers carry the larger of the probe
  maximum and query-local cross-rung discrepancy PLUS lower-rung QoI
  dispersion, so agreement with an inaccurate RB cannot erase its
  uncertainty. Level 0 carries
  infinite dispersion because an unproved floating-point solve makes no
  spread claim.
- Ladder state is sealed. Truth dimension, training range, basis,
  calibrated dispersion, rung collection, family identity, and answer
  evidence cannot be mutated or forged through public fields. Every rung
  is bound to one identity containing the truth dimension and exact
  floating-point range endpoints.
- Public ladder arithmetic and lookup operations are fallible. Queries
  reject non-finite/non-coercive/out-of-range inputs before lookup, and
  generated training/probe grids must be strictly increasing in f64.
- Ladder construction preflights nonempty, capped, strictly decreasing
  requested RB dimensions plus checked aggregate memory/work budgets
  before the first snapshot. After orthogonalization, actual retained
  dimensions must also strictly decrease before a rung is stored.
  Coverage batteries are nonempty and capped on both axes, their Cartesian
  product, and conservative aggregate work. Each parameter performs at most
  one descent (including at most one truth fallback), and the resulting RB
  estimators classify every requested tolerance without repeating solves.

## Error model

Structured `SurrogateError`. Ladder construction, energy, compliance,
training, lookup, level selection, querying, and coverage return named
errors for invalid shapes/values/ranges/grids, singular or non-finite
derived arithmetic, and resource excess. The non-ladder conformal helper
still panics on nonsensical inputs (empty residuals, `α ∉ (0,1)`).

## Determinism class

Fully deterministic: the eigensolver, POD, band, and policy are pure functions
of their inputs.

## Cancellation behavior

None (synchronous pure functions). In particular, the feature-gated coverage
battery has checked admission but no `Cx` or bounded cancellation latency; it
is not yet a production hot-kernel execution surface.

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

- `abstraction-ladder` [F] (default OFF) — the estimated, leak-alarmed
  abstraction ladder (knh1.4/y6yv, Proposal A; `dep:fs-evidence`); gates
  the `ladder` integration target.

## Conformance tests

`tests/surrogate.rs` (8 cases): POD reproduces a low-rank set exactly;
orthonormal modes; energy-based rank + reduced error; bad-input rejection; the
conformal band achieves nominal coverage; certify-or-escalate uses the surrogate
only when trustworthy; the policy reduces cost vs all-high-fidelity;
determinism.

`tests/ladder.rs` (feature-gated): f64 RB estimator containment on the
elliptic fixture, bounded descent, Estimated-only payload authority,
deterministic replay, structured hostile-input refusals, representable
grid and family binding, requested/retained fidelity descent,
lower-rung uncertainty inheritance, pre-training memory/work limits,
and bounded coverage batteries.

## No-claim boundaries

- v0 is the CLASSICAL ROM core (POD via method of snapshots) + the conformal /
  certify-or-escalate guardrail. NEURAL OPERATORS (Fourier neural operators,
  DeepONets via FrankenTorch), DEIM nonlinear-term interpolation, BALANCED
  TRUNCATION for LTI subsystems, and KOOPMAN/DMD are the fuller deliverable,
  staged.
- The eigensolver is a small dense Jacobi for the snapshot correlation matrix;
  the production path is fs-la randomized/TSQR SVD over large snapshot matrices.
- The conformal band is SPLIT-conformal (exchangeable data); the anytime-valid
  e-value formulation with online recalibration under drift is the
  conformal-hardening follow-on.
- Continuous training from the ledger, versioned/model-carded surrogate
  artifacts, and design-family-respecting splits are downstream integrations.

## No-claim boundaries (ladder)

- The beachhead covers the AFFINE-PARAMETRIC ELLIPTIC regime (1-D
  fixture family here); nonlinear/transient coarse levels are the
  research frontier and enter only as estimated-color concept rungs.
- Level 0 is the declared FE semantics, but neither floating-point solve
  error nor FE discretization error is enclosed here. Its Estimated
  color therefore has infinite dispersion; there is no zero-error claim.
- The RB residual/Riesz/solve path is evaluated in round-to-nearest f64
  without outward rounding or independent linear-solve certificates.
  Its textbook estimator is useful and tested for containment on this
  fixture, but it does not authorize `Color::Verified` and descent is not
  called certified.
- Compliance dispersion includes both the squared energy estimator and the
  floating reduced solve's computable Galerkin defect
  `|f(u_rb) - a(u_rb,u_rb)|`; exact orthogonality is never assumed.
- The concept rung's dispersion is a finite-probe MAXIMUM of
  `|concept − lower RB| + lower RB QoI estimator`, augmented by the same
  query-local quantity. Neither is an enclosure over the continuous range.
  The Estimated color is load-bearing.
- Coverage is currently a synchronous feature-gated measurement helper. Its
  checked work cap bounds admission, but it has no `Cx`, tile polling, or
  cancellation/drain contract. Production-scale batteries remain out of claim
  until that execution surface is added; the current implementation is for
  bounded Gauntlet/activation fixtures.
- The eventual certificate destination is an outward-rounded residual,
  Riesz solve, reduced solve, coercivity floor, and QoI enclosure whose
  complete arithmetic path is independently checkable. Only that path,
  once admitted by the Gauntlet, may upgrade a rung to `Verified`.
- Per-REGION (spatial) RB decomposition and the fs-ir at_level query
  integration are the named growth seams.
