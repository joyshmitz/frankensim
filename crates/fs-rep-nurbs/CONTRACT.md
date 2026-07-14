# fs-rep-nurbs — CONTRACT

Rational B-spline charts (plan §7.2): EXACT spline algebra, trimmed
patches with certified classification, measured closest-point brackets,
and the HONEST Boolean position.

Ambition tags: exact algebra / trims / closest-point [F per bead label;
what is shipped is tested to [S] discipline]; direct B-rep Booleans
deliberately NOT shipped (see the Boolean position).

## Purpose and layer

Layer **L2** (MORPH). Runtime deps: `std`, fs-ivl, fs-math. Consumers:
fs-iga (geometry basis = analysis basis), fs-render NURBS tracing
(shares the clipping/Newton machinery), the NURBS↔SDF converter beads
(wqd.11/12).

## Public types and semantics

- `Rat` — exact i128 rational scalar (gcd-reduced, overflow-CHECKED:
  leaving the exactness domain is a named panic, never wraparound).
- `Scalar` — the field abstraction; the SAME generic basis/curve/surface
  code runs at `f64` (fast) and `Rat` (exact). The conformance suite
  also instantiates it at a test dual, so derivative checks flow through
  the identical code path.
- `KnotVector` (clamped, validated), Cox–de Boor basis (Piegl–Tiller
  A2.2 shape).
- `NurbsCurve<S, DIM>` — homogeneous de Boor evaluation; f64 derivatives
  to arbitrary order (homogeneous differencing + rational Leibniz);
  EXACT Boehm `insert_knot`; EXACT `remove_knot` (reconstruction checked
  with scalar EQUALITY — in `Rat` a proof); Bézier decomposition; EXACT
  `elevate_degree` (per-segment binomial elevation reassembled on a
  full-multiplicity knot vector — valid and evaluation-identical;
  minimal-multiplicity elevation is a follow-up).
- `NurbsSurface<S>` — tensor-product evaluation, EXACT directional knot
  insertion, first partials via isocurve nets, per-span control boxes.
- `TrimLoop`/`TrimmedPatch` — trim curves in EXACT RATIONAL form
  (closure validated by rational equality). `classify` is CERTIFIED:
  outside every Bézier span hull ⇒ the exactly-computed control-polygon
  winding equals the curve winding (hull-homotopy argument); ambiguous
  points subdivide EXACTLY (rational midpoints) and become `Boundary`
  after the budget — never a guessed in/out.
- `closest_point_curve`/`closest_point_surface` — best-first
  branch-and-bound over rational-Bézier hull boxes (positive weights
  survive de Casteljau, so every sub-segment hull bounds it), one local
  split per iteration, split junctions as free upper-bound samples,
  Newton polish on curves. Returns
  `DistanceBracketEstimate { lower, upper }` with hulls heuristically
  expanded by one ULP. Cartesian division, evaluation and norm
  arithmetic are ordinary f64, so dense-oracle containment is measured
  evidence rather than a rigorous enclosure.
- `boolean(op, policy)` — THE BOOLEAN POSITION: always a structured
  `BooleanRefusal` in v0. Default policy routes through SDF (convert →
  implicit CSG → re-fit); `DirectCertificateGated` refuses pending a
  coverage-complete continuum watertightness certificate. The current
  sampled sheaf interface-agreement evidence is insufficient. An attempt,
  never a promise.

- `sdf` module (plan §7.3 edge 3, bead wqd.11; [F], behind the
  `nurbs-sdf` feature until its Gauntlet tier is green): the current measured
  NURBS → distance-field approximation. `ShellSdf::distance` uses
  convex-hull branch-and-bound plus damped Gauss–Newton, but its bracket is
  not outward-rounded. When the found point is trimmed away or in the boundary
  band, the query carries an infinite upper value and the chart returns
  `NoClaim`; distance-to-kept-region search remains successor work.
  `ShellSdfChart` presents `nurbs-sdf/estimated-signed` under declared local
  orientation or `nurbs-sdf/estimated-unsigned` otherwise; it emits
  `NumericalKind::Estimate`, no Lipschitz authority, and no continuity claim:
  finite-budget best-first selection can switch sampled witnesses.
  `generate_tile` is effort-adaptive under defensive static sample/split
  ceilings, not caller-budgeted (P4 remains successor work): refinement fires
  within two cell diagonals of the surface, and achieved measured widths plus
  branch-and-bound splits are reported per tile. A trim-downgraded cell emits
  an infinite sentinel/no usable distance rather than the finite distance to a
  point that is not on the kept surface.

- `refit` module (plan §7.3 edge 4, bead wqd.12; [F], behind the
  `nurbs-refit` feature — which enables `nurbs-sdf`, since the CSG
  sampled forward-converter and interface-evidence loop): the SDF → NURBS
  RE-FIT, the edge that makes the
  honest Boolean policy work (§7.2: Booleans route through F-rep, then
  re-fit when a spline chart is required). v1 pipeline for star-shaped
  domains: radial-bisection sampling ON THE FIELD ITSELF, tensor-product
  B-spline least squares with discrete thin-plate (control-lattice
  Laplacian) regularization, exact G⁰ seam closure by control-column
  tying (G¹ measured). ERROR HONESTY: the spline→field direction separately
  reports `max sampled |f(S)|` and the geometric probe-spacing estimate
  `(L_u+L_v)·Δ/2`, where `Δ = 1/probe` is the per-axis probe spacing, with
  each hodograph coefficient evaluated using its actual clamped knot span,
  `L_u = max_i p‖ΔC_i‖/(u_{i+p+1}−u_{i+1})` (and analogously for v).
  The generic closure API supplies no units, admitted Lipschitz witness, or
  metric-error-bound witness, so it does not add these potentially
  dimensionally incompatible quantities. All arithmetic is ordinary f64 rather
  than outward-rounded. Neither quantity is a continuum field enclosure or a
  Hausdorff certificate. The other direction is the worst retained radial
  sign-bracket-target fit residual; a generic closure supplies no continuity
  or root-existence witness, so this is not named source-surface coverage.
  The report records all three estimates. Features encountered by retained
  projection rays can produce STRUCTURED WARNINGS with parameter and world
  locations; a feature missed by every ray is invisible to this API. The
  patch-density knobs are the ErrBudget trade, ledgered.

## Invariants

1. **Refinement exactness (the definitive test)**: knot insertion,
   degree elevation, and surface insertion leave evaluation EXACTLY
   equal (rational `==`) at common parameters; insert+remove recovers
   the control net exactly.
2. **G0 laws in exact arithmetic**: partition of unity sums to exactly
   1; clamped endpoints interpolate exactly.
3. **Derivatives match duals**: the generic evaluator instantiated at a
   dual scalar equals the analytic derivative pipeline.
4. **Trim classification never lies**: adversarial fixtures (nested
   square/diamond/island, near-tangent vertices, sliver holes) classify
   correctly; on-boundary points report `Boundary`.
5. **Closest-point estimates cross-check sampled oracles**: dense-sampling
   oracles (1e5 curve samples, 300² surface grid) fell inside `[lower, upper]`
   in the retained fixtures. This is measured evidence, not a universal
   containment theorem.

## Error model

`NurbsError`: `Structure`, `Domain`, `Exactness`. `Rat` overflow panics
with a named message (exactness-domain exit — a documented boundary, not
a data path).

## Determinism class

**D0**: exact arithmetic on the `Rat` path; fixed iteration orders
everywhere; best-first ties broken by a monotone logical insertion identity.

## Cancellation behavior

Trim and closest-point subdivision loops carry explicit static iteration
limits. The legacy refit path has validated static work/allocation/probe caps
but no `Cx`; it is not yet P7 cancellation-correct. The successor budgeted
interfaces are tracked explicitly and must add bounded polling plus
request→drain→finalize semantics before promotion.

## Unsafe boundary

Zero `unsafe`.

## Feature flags

All OFF by default per the Ambition-Tag rule:
- `nurbs-sdf` [F] — the NURBS→SDF converter; disabled until its
  Gauntlet tier is green. Gates the `sdf` integration target.
- `nurbs-refit` [F] — the SDF→NURBS refit (implies `nurbs-sdf`; its current
  acceptance evidence is sampled and does not close a continuum certificate).
  Gates the `refit` integration target.

## Conformance tests

`tests/conformance.rs` (JSON verdicts, suite `fs-rep-nurbs/conformance`):
nb-001 exact G0 laws + derivative-vs-dual through the generic evaluator;
nb-002 six random rational curves: insertion/elevation evaluation-exact +
insert/remove lossless round trip; nb-003 adversarial trim battery
(nesting, tangency, slivers, boundary honesty); nb-004 measured curve +
surface brackets vs dense oracles (iteration stats logged, no enclosure
claim); nb-005 the
Boolean policy refusals with teaching routes; nb-006 exact surface
refinement + partials vs central differences.

## No-claim boundaries

- **No direct B-rep Booleans** — by design, not omission. The SDF route
  is the product; the direct path requires the successor coverage-complete
  continuum watertightness certificate. The sampled interface evidence from
  wqd.13 is intentionally insufficient for that authority.
- **Degree elevation emits full-multiplicity knot vectors** (valid,
  evaluation-identical); minimal-multiplicity reassembly is follow-up.
- **Closest-point and NURBS→distance brackets are measured estimates.** The
  one-ULP hull expansion is heuristic and cannot authorize `Enclosure`, exact sign,
  a 1-Lipschitz field, or no-tunneling. The fs-ivl/Taylor path with outward
  rational projection, norm bounds and interval Newton is the certified upgrade.
- **Greville/Gauss quadrature tables for IGA** land with the fs-iga
  consumer (tfz.9), which owns the quadrature accuracy claims.
- **`Rat` is i128-bounded**: deep repeated refinement can exceed the
  exactness domain; the failure is loud and named.
- Ray intersection shares this machinery but ships with the LUMEN
  chart-backend bead (qfx.2).

## No-claim boundaries (sdf converter)

- Lower estimates come from f64 control-hull boxes; near MEDIAL AXES (many
  equidistant patches) and pole-degenerate parameterizations, bracket
  widths converge slowly with splits — the documented budget trade
  (~1e-3 at 2000 splits/cell, 2.6e-4 at 8000 on the unit-sphere tile).
  Interval-Newton (Krawczyk) contraction of the projection equations is
  the upgrade path when fs-ivl grows 2-D machinery.
- Sign is estimated from the declared local B-rep orientation; the winding-style
  fallback for imperfect shells lands with the quarantine/census beads
  (fs-io owns mesh-side honesty).
- Trim downgrades widen the estimate; distance-to-kept-region
  (excluding trimmed area from the B&B itself) is future work — the
  current f64 lower value has no enclosure authority even for the untrimmed
  surface.

## No-claim boundaries (refit)

- v1 parameterization is RADIAL (star-shaped domains around the given
  center; the bracket failure is a structured teaching error).
  General-topology segmentation over the dual-contoured mesh (wqd.10)
  is the upgrade path.
- The generic refit accepts an arbitrary scalar closure and therefore grants
  no field, zero-set, or distance authority. Even a proven 1-Lipschitz field
  satisfies `|f(x)| ≤ dist(x,Z)` (the opposite direction from the upper
  geometric bound a Hausdorff claim needs); a geometric promotion requires a
  separate metric-error-bound witness such as `dist(x,Z) ≤ κ|f(x)|`, plus
  outward-rounded evaluation and coverage evidence. Min/max CSG fields do not
  acquire that authority automatically.
- Retained radial sign-bracket-target → spline evidence is sampled at the
  projection grid: a feature no ray hits is invisible, and without admitted
  continuity/root-existence evidence the target is not authorized as a point
  on a source surface. Density is the caller's knob, and the warning channel
  reports what the samples DID see.
- The legacy closure API uses validated dimensions plus a conservative live-
  payload allocation estimate, explicit checked reservations for its largest
  side buffers, and fixed probe/algorithmic-work ceilings so malformed
  configurations refuse before evaluating the field. Allocator metadata,
  arbitrary closure cost, and every small transient are not a complete memory
  or time budget. These caps are process constants, not caller budgets;
  the dense fit has no `Cx` cancellation points. Typed admitted-field authority,
  ledgered caller budgets, bounded cancellation latency, and outward-rounded
  geometric certification remain explicit successor work.
- G¹ cross-seam continuity is measured, not enforced; pole rows are
  least-squares-collapsed, not pinned.
