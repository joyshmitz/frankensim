# CONTRACT: fs-asbuilt

As-built ingestion — reality is just another chart (plan addendum,
Proposal 11): register scan data to the design and emit an honestly colored
as-built candidate.

## Purpose and layer

Layer L2 (representation/geometry). Depends on `fs-evidence` (`Color` and
`ValidityDomain`), `fs-exec` (explicit `Cx`, execution mode, and budgets),
`fs-ivl` (outward-rounded observability enclosures), and the native `fs-blake3`
content-identity primitive. The legacy scientific calculation is deterministic
and uses a closed-form 2-D rigid fit (no SVD). The additive `uncertainty`
module refits that transform under an explicit calibrated covariance model and
keeps its stronger decision semantics separate from the residual-RMS screen.

## Public types and semantics

- `Point2::new(x, y) -> Result<Point2, RegError>` constructs finite points and
  canonicalizes signed zero; coordinates are private and available through
  `x()` / `y()`.
- `Fiducial::new(design, measured)` pairs already-valid typed points; fields
  are private with read-only accessors.
- `register(&[Fiducial], &fs_exec::Cx<'_>) -> Result<Registration, RegError>` —
  the rigid rotation+translation best mapping design → measured (2-D
  Umeyama/Procrustes closed form). Requires `>= MIN_FIDUCIALS` (3)
  non-collinear fiducials and a numerically observable rotation objective;
  retains `residual_rms` as a global fit diagnostic, not registration
  uncertainty. Registration fields
  are private; `Registration::new` is fallible, accessors are read-only, and
  `Registration::apply` refuses non-finite arithmetic overflow.
- `registration_invocation_resources(point_count)` returns the checked typed
  work, poll, cost, evaluation, memory, and retained-output grant for one
  registration. `register_budgeted(fiducials, &mut ChildBudget)` consumes that
  grant from an affine parent-issued child rather than reconstructing an
  allowance from the ambient `Cx`.
- `well_posed(&Registration, certified_deviation) -> bool` — the R8 gate: true
  iff the supplied deviation is finite and positive and the registration
  residual is BELOW the deviation being certified.
- `as_built_diff(&Registration, design, scanned, design_tolerance,
  measurement_noise, calibration_candidate, &fs_exec::Cx<'_>) ->
  Result<AsBuiltDiff, RegError>` — the
  per-point δ after registration; `within_tolerance`, `above_noise_floor`, and
  a `Color::Estimated` whose domain-separated BLAKE3 identity binds every
  scientific function input plus the documented execution subset below.
  `proposed_regime` carries residual/noise/tolerance bounds for later
  authenticated review. The calibration string is a bounded, structurally
  valid candidate identity, never authority. Result fields are private and
  exposed through read-only accessors, so callers cannot forge or mutate an
  authenticated-looking `AsBuiltDiff` value. `max_deviation_index()` retains
  the last input-order index attaining the maximum, including deterministic
  ties, so a composed workflow need not rescan the deviation payload.
- `as_built_diff_invocation_resources(...)` returns the checked typed grant and
  conservative retained-payload shape for one diff.
  `as_built_diff_budgeted(..., &Cx, &mut ChildBudget)` reserves that live-memory
  envelope before allocation, spends through the same affine child, and
  publishes retained output only on success.
- `RegError` covers cancellation with exact phase/progress,
  work-plan overflow, insufficient/collinear/unobservable/interval-unresolved/oversized points, length mismatch,
  empty data, non-finite or negative numeric inputs, malformed calibration
  identity, arithmetic overflow, bounded-allocation failure, and typed
  invocation-budget refusal.
- `uncertainty::{Covariance2, CrossFiducialModel, HuberPolicy, BiasBound,
  MetrologyModel}` declares strictly positive-definite, heteroscedastic
  per-fiducial covariance; within-pair x/y covariance; either independent or
  symmetric-principal-factor equicorrelated standardized fiducials; a finite
  radial bound on the total registered-inspection systematic vector error over
  the complete query domain (or explicit unbounded state); and a bounded
  deterministic robust policy. A raw fiducial/scanner bias is not accepted as
  that already-propagated bound. Unknown cross-fiducial dependence refuses.
- `uncertainty::estimate_calibrated_registration(fiducials, model, &Cx)`
  globally solves the fixed-weight constrained system in
  `(tx, ty, cos(theta), sin(theta))`, refitting after every Huber weight update.
  It returns `CalibratedRegistration` with the full bit-symmetric 3x3
  first-order covariance, exact `2n-3` degrees of freedom, final robust weights,
  explicit outlier dispositions/standardized residuals, full-model leverage
  diagnostics, and a domain-separated model identity. Ambiguous global
  rotations refuse. Huber covariance is a frozen-weight sandwich and is marked
  conditional; it cannot issue a finite-sample tolerance decision.
- `uncertainty::assess_calibrated_as_built(...)` propagates pose uncertainty as
  `G Cov(tx,ty,theta) G^T`, adds each independent inspection covariance exactly
  once, and applies a familywise Chebyshev-plus-union radial bound. It returns
  `DecisionState::{WithinTolerance, ExceedsTolerance, Indeterminate}` with
  lower/upper maximum-deviation bounds, confidence, family size, and a stable
  reason. Unknown registration/inspection overlap, unbounded bias, or adaptive
  weights produces an explicit bound-unavailable `Indeterminate` result.
- `uncertainty::{EvidenceReceipt, EvidenceVerifier,
  AuthenticatedAsBuiltEvidence}` separates a full content identity from
  authority. The opaque wrapper is constructible only after an injected
  verifier accepts the exact candidate/receipt under the receipt-bound policy;
  the default verifier denies everything. Authentication proves lineage, not
  physical validation or the calibration assumptions.

## Invariants

- Registration RECOVERS a ground-truth rigid transform (residual → 0 on clean
  fiducials) and retains the noisy-fit RMS as an advisory diagnostic.
- Well-posedness needs `>= 3` non-collinear fiducials (rank-2 design scatter).
  Centered design and measured coordinates are normalized by their finite
  point-set extents before the relative squared-scatter rank gate and rotation
  objective, so their product expressions do not underflow/overflow merely
  because a representable configuration is uniformly rescaled;
  collinear/too-few is refused. Registration and diff inputs are capped at
  `MAX_AS_BUILT_POINTS`.
- Registration separately requires measured spread and an outward-rounded
  proof that at least one component of the centered cross-covariance vector is
  nonzero. Centroids and both cross sums carry `fs-ivl::Interval` enclosures;
  both enclosures must have finite endpoints. A collapsed measured set returns
  `UnobservableRotation`; if spread exists but both components can contain
  zero, `RotationCertificationUnresolved` reports the distinct numerical
  ambiguity and refuses fail-closed. This rejects collapsed scans and
  reflection/cancellation configurations instead
  of publishing `atan2(0, 0)`'s arbitrary zero-angle convention, without an
  epsilon heuristic.
- Public point, registration, and as-built result fields cannot be forged; all numeric inputs
  are finite, residual/tolerance/noise are non-negative, and unrecoverable
  non-finite arithmetic or a non-finite final result is refused.
- Rotation-plus-translation components preserve the ordinary binary64
  evaluation whenever it is finite and use scaled three-term evaluation when
  a rotation sum overflows before a cancelling finite translation. Recovery is
  fail-closed unless an outward-rounded interval proves the original real
  affine sum remains inside the finite binary64 range. Residual RMS uses scaled
  sum-of-squares accumulation, so a finite RMS is not rejected merely because
  squaring an individual finite distance would overflow.
- R8: `well_posed` requires a finite positive supplied deviation and is false
  when the residual meets or exceeds it (signal below the noise floor).
- The default as-built δ is always `Estimated`. Its bounded identity uses
  length-framed canonical fields followed by a domain-separated native BLAKE3
  digest, preventing delimiter and prefix collisions. Numeric identity fields
  canonicalize `-0.0` to `+0.0`, matching their mathematical equality.
- A well-formed string such as `forged-calibration-claim` cannot promote a
  result: this crate has no validated-promotion API.
- Constant-time preflight declares exactly `6n` point visits for registration
  (extrema/running-mean and anchored-normalized passes for each centroid,
  followed by scatter and residual) and `3n` point visits for a diff
  (deviation, maximum, and identity). Work is checked in `u128` before the
  initial checkpoint. Point scans poll every 256 points, with additional phase
  and final-publication checkpoints; cancellation never publishes a partial
  registration or diff.
- Each typed planner maps logical work to a distinct `WorkUnits` field and an
  equal-valued, representable `CostUnits` field, declares one scientific
  `EvaluationUnits`, and computes its poll and payload byte shapes before a
  child can spend. Registration declares no live-memory payload and only its
  fixed retained result. A diff declares the same conservative byte envelope
  for live memory and retained output. These dimensions are never converted
  into one another by the lower-layer API.
- Budgeted entry points accept only a borrowed, non-cloneable `ChildBudget`.
  Work, cost, evaluation, every poll, memory, and output are charged to that
  child; scientific refusals are latched into the parent receipt. Unused
  capacity returns only when the child is consumed by `finish`, so sibling
  stages cannot mint fresh authority through this crate.
- The `asbuilt-diff-v4` identity binds execution mode, every field of the
  ambient `fs_exec::Budget`, work-plan v2 and exact `3n` shape, poll-policy v2
  and its 256-point/256-byte strides, plus all scientific and provenance inputs.
  `StreamKey` is intentionally not part of this identity. Registration has no
  retained execution identity in this crate.
- Spatial covariance uses the rigid Jacobian ordered as `(tx, ty, theta)` and
  retains every translation/rotation cross term. Fiducial covariance factors
  are symmetric principal square roots, so the declared standardized
  equicorrelation is not an axis-order-dependent Cholesky convention. The
  equicorrelation domain is strict `-1/(n-1) < rho < 1`; boundaries are never
  clamped. Robust weighting is supported only for independent fiducials. Each
  fixed-weight transform is the global unit-circle trust-region minimum after
  eliminating translation; hard cases with multiple minima refuse. The local
  sensitivity includes the global solver's trust multiplier, and covariance is
  symmetrized once and revalidated as positive definite before publication.
- The calibrated model never converts `Registration::residual_rms` into pose or
  pointwise uncertainty. Absolute calibrated fiducial covariance determines
  parameter covariance; scaling it again by residual scatter and then adding
  residual RMS pointwise would double count the same fit error.
- For a disjoint inspection family of size `M`, confidence `1-alpha`, total
  point covariance `S_j`, and finite radial bias `b`, the simultaneous radius
  is `b + sqrt(trace(S_j) * M / alpha)`. The maximum lower bound is
  `max_j max(0, observed_j-radius_j)` and the upper bound is
  `max_j(observed_j+radius_j)`. The union bound assumes no independence among
  inspection points, but it does require calibrated covariance upper models
  and disjointness from the registration measurements. Rotation sine/cosine,
  affine mapping, pose trace, inspection trace, observed norm, radius, and
  final lower/upper arithmetic use `fs-ivl` outward enclosures so
  round-to-nearest equality cannot false-accept.
- Registration-model identity v1 binds the factor/correlation/robust/bias
  model, calibration identity, every ordered fiducial and covariance, final
  transform/covariance, standardized residual, weight, outlier disposition,
  leverage diagnostic, global-solver semantics, and degrees-of-freedom
  semantics. Spatial-evidence identity v1 additionally binds every inspection
  pair/covariance, relation, tolerance, confidence, point bound, and tri-state
  output. Both canonicalize signed zero and are tamper-evident addresses only.

## Error model

Structured `RegError` values; hostile numeric/identity inputs return errors.
`WorkPlanOverflow` refuses an unrepresentable plan, and `Cancelled` retains the
stable phase plus exact completed/planned point visits.
`InvocationBudget` preserves the underlying typed deadline, cancellation, or
resource refusal from `fs-exec`; a scientific preflight or domain refusal is
also latched fail-closed into that invocation.
`BudgetRefused` (bead sj31i.6) retains the ambient accountant's typed refusal
verbatim: the plain `register`/`as_built_diff` entry points admit `cx.budget()`
plus the preflighted work plan through `fs_exec::AdmittedBudget` before any
work (expired deadlines - `Budget::ZERO` included - deadlines without an
ambient time source, and over-quota cost plans refuse at admission), enforce
cancellation/deadline/poll quota at every checkpoint, and charge completed
work as cost at checkpoint boundaries. Real cancellation keeps the structured
`Cancelled` shape.
Deviation allocation uses `try_reserve_exact`; no public path intentionally
panics.
`uncertainty::SpatialUncertaintyError` separately names malformed covariance,
correlation, confidence, geometry, dependence, arithmetic, allocation, and
cancellation failures. Unknown scientific dependence is never silently
converted to independence.

## Determinism class

The fit, gate, δ, and calibrated spatial model are deterministic functions of
their semantic inputs.
G5 tests lock that mode, budget, work-plan, poll-policy version, and stride move
the retained diff identity without changing the numerical result.
The calibrated module uses fixed iteration counts, ordered scans, symmetric
covariance factors, canonical binary64 identity fields, and no scheduling-
dependent reduction.

## Cancellation behavior

Synchronous and cancellation-aware. Both public long-running entry points take
an explicit `Cx`; preflight precedes the initial poll, long scans poll at the
fixed 256-point stride, and a final checkpoint gates publication. Cancellation
returns `RegError::Cancelled` with exact progress and no partial output.
The budgeted forms poll the child authority, which checks its absolute clock
and originating cancellation gate before spending each poll. Typed output is
not published after a deadline, cancellation, resource, or scientific refusal.
The calibrated registration and spatial assessment also take an explicit `Cx`,
poll at bounded 256-point scan boundaries plus finalization, and publish no
partial result after cancellation. They do not yet have affine `ChildBudget`
entry points; this absence is a no-claim, not declared resource enforcement.

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
deterministic replay; typed resource planning, affine budgeted registration and
diff execution, retained last-maximum index ties, and receipt integrity; G4
pre-cancel, exact stride-boundary, mid-phase, and publication cancellation; and
G5 execution/work/poll identity separation.

`tests/spatial_uncertainty.rs`: G0 analytic independent/equicorrelated
cardinal-geometry covariance and leverage, covariance/correlation/rank refusal,
ambiguous-rotation refusal, direct-construction Huber validation,
robust-outlier disposition/downweighting with conditional no-claim,
pose-plus-inspection propagation without residual double counting, far-point
rotational leverage, outward tolerance equality at zero and nonzero rotation,
total-bias application, family-size widening, all three decision states,
overlap/bias no-claims, G3
heteroscedastic off-diagonal unit/order metamorphisms, G5 semantic identity
movement/replay, receipt mutation/policy refusal, and pre-cancel publication
refusal.

## No-claim boundaries

- v1 is 2-D rigid registration (rotation + translation) with KNOWN
  correspondences; 3-D (Kabsch/SVD), scale, and correspondence-free ICP are
  follow-ons.
- Registration is treated as an optimization whose global fit RMS diagnostic
  is propagated into advisory screens and the proposed regime. That residual
  is not transform covariance or a pointwise spatial uncertainty bound.
  Writing it (and the as-built δ) to the design ledger is fs-ledger's
  integration, and the fiducial/datum PRIMITIVES at design time are fs-geom's
  (this crate consumes the correspondences).
- The scan is modeled as sampled points; admitting a full CT voxel grid /
  point cloud as a representation type with restriction maps to interface trace
  spaces extends fs-rep-voxel + fs-geom's chart zoo.
- The δ reuses the deviation metric directly; the full sheaf δ / watertightness
  machinery is the geometry layer's.
- `well_posed`, `within_tolerance`, and `above_noise_floor` are advisory
  residual/dispersion screens, not pointwise uncertainty bounds, statistical
  significance tests, or tolerance certificates.
- The calibrated module provides evidence-bearing tri-state bounds, but the
  legacy boolean API remains for compatibility until downstream consumers
  migrate. Those booleans are not projections of the calibrated bounds and
  must not be promoted.
- Spatial evidence remains first-order and conditional on the supplied
  calibrated covariance/correlation and a bound on total systematic error over
  the queried domain. Raw sensor/fiducial bias is not automatically a spatial
  registration-bias bound. Huber sandwich covariance does not cover
  data-dependent weight selection, so its decision is deliberately
  unavailable. No Gaussian, exact nonlinear confidence,
  unknown-dependence, or high-leverage asymptotic claim is made.
- `EvidenceVerifier` authenticates retained lineage/policy binding only. It
  does not independently prove calibration artifact contents, the declared
  noise law, physical validation, or coverage. A lying injected verifier is an
  explicit composition-root trust failure; `NoEvidenceVerifier` admits
  nothing.
- Registration/inspection sample reuse needs retained cross-covariance and
  influence terms that v1 does not accept. Unknown or overlapping input is
  `Indeterminate` with no numeric bound rather than a zero-correlation guess.
- Point-visit work is a deterministic logical accounting unit, not an
  instruction count or a guarantee about wall-clock latency, memory pressure,
  deadline enforcement, drain behavior, or a 200-microsecond cancellation
  bound. Registration also makes no retained provenance claim about the `Cx`
  under which it ran.
- Typed planner byte counts are conservative semantic payload envelopes, not
  allocator-overhead or process-RSS measurements. `CostUnits` is abstract and
  is not a wall-time, currency, or energy certificate. A planner describes a
  grant but does not itself admit an invocation; the parent `fs-exec` issuer
  owns admission, the absolute deadline, and the terminal receipt.
- The retained diff identity is a replay/integrity binding, not authenticated
  provenance. In addition to `StreamKey`, it excludes arena identity,
  cancel-gate state, scheduler state, and other internal `Cx` state.
