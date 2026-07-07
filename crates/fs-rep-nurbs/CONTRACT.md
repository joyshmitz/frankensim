# fs-rep-nurbs — CONTRACT

Rational B-spline charts (plan §7.2): EXACT spline algebra, trimmed
patches with certified classification, certified closest-point brackets,
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
  Newton polish on curves. Returns `CertifiedDistance { lower, upper }`
  with hulls epsilon-inflated (1e−9·scale) against f64 rounding.
- `boolean(op, policy)` — THE BOOLEAN POSITION: always a structured
  `BooleanRefusal` in v0. Default policy routes through SDF (convert →
  implicit CSG → re-fit); `DirectCertificateGated` refuses pending the
  sheaf watertightness certificate (wqd.13). An attempt, never a promise.

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
5. **Closest-point brackets contain the truth**: dense-sampling oracles
   (1e5 curve samples, 300² surface grid) always fall inside
   `[lower, upper]`.

## Error model

`NurbsError`: `Structure`, `Domain`, `Exactness`. `Rat` overflow panics
with a named message (exactness-domain exit — a documented boundary, not
a data path).

## Determinism class

**D0**: exact arithmetic on the `Rat` path; fixed iteration orders
everywhere; best-first ties broken by queue order.

## Cancellation behavior

All loops carry explicit budgets (`max_subdivision`, `max_splits`);
P7 by boundedness.

## Unsafe boundary

Zero `unsafe`.

## Feature flags

None.

## Conformance tests

`tests/conformance.rs` (JSON verdicts, suite `fs-rep-nurbs/conformance`):
nb-001 exact G0 laws + derivative-vs-dual through the generic evaluator;
nb-002 six random rational curves: insertion/elevation evaluation-exact +
insert/remove lossless round trip; nb-003 adversarial trim battery
(nesting, tangency, slivers, boundary honesty); nb-004 certified curve +
surface brackets vs dense oracles (iteration stats logged); nb-005 the
Boolean policy refusals with teaching routes; nb-006 exact surface
refinement + partials vs central differences.

## No-claim boundaries

- **No direct B-rep Booleans** — by design, not omission. The SDF route
  is the product; the certificate-gated path unlocks with wqd.13.
- **Degree elevation emits full-multiplicity knot vectors** (valid,
  evaluation-identical); minimal-multiplicity reassembly is follow-up.
- **Closest-point rigor is hull-based with an epsilon-inflated f64
  margin**, not interval-arithmetic end-to-end; the fs-ivl interval
  polish tier (Bézier-clipping-seeded interval Newton) is the certified
  upgrade path this bead names for LUMEN's tracing.
- **Greville/Gauss quadrature tables for IGA** land with the fs-iga
  consumer (tfz.9), which owns the quadrature accuracy claims.
- **`Rat` is i128-bounded**: deep repeated refinement can exceed the
  exactness domain; the failure is loud and named.
- Ray intersection shares this machinery but ships with the LUMEN
  chart-backend bead (qfx.2).
