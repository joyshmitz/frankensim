# fs-truss CONTRACT

## Purpose and layer

Layer: L4 (ASCENT). Ground-structure truss layout optimization (plan
§9.5 [S/F], bead 7tv.13): candidate members under fabrication rules →
plastic-design LP solved by an in-house first-order primal-dual
iteration with explicit convergence diagnostics → Euler/code sizing
with catalog snapping → fs-solid rod re-analysis. The
steel-and-concrete flagship's engine (§15.2).

## Public types and semantics

- `GroundRules`/`GroundStructure`: node grids + all-pairs candidates
  filtered by length bounds, allowed angle sets, and
  collinear-through-node dedup, carried as a FrankenNetworkx `Graph`;
  generation reproducible (BTree orders), `stats()` the ledger row
  (counts + FNV hash).
- `LayoutLp`: the member-force LP — split tension/compression
  variables `q⁺, q⁻ ≥ 0`, volume objective `Σ l(q⁺+q⁻)/σ_y`, nodal
  equilibrium on free DOFs. `solve` = PDHG (Chambolle–Pock) with
  power-iteration step sizing, sparse matvecs, warm starts across load
  cases, deterministic iterations. `solve` is fallible, caps direct solves at
  one million iterations, and validates controls plus warm-start shape/domain
  before work. `diagnostics` returns
  (relative primal/dual objective separation, equilibrium residual, volume):
  under this saddle the nominal dual objective is `−bᵀy` with feasibility
  `c + Aᵀy ≥ 0`, approximately restored by a floating uniform shrink of y —
  the battery pinned the OPPOSITE textbook
  convention (`+bᵀy`, `Aᵀy ≤ c`) reporting gap = 2 on exactly-solved
  instances.
- `sizing::size_and_snap` → `CatalogAudit`: areas from yield, EULER
  floors for compression members (solid square `A ≥ √(12|q|l²/π²E)`),
  joint parsimony pruning with MANDATORY least-squares equilibrium
  re-verification on survivors (CG on the normal equations), catalog
  UP-snapping (feasibility preserved by construction), member-by-
  member post-snap re-checks as fs-constraint `Code` rows.
- `rodcheck::rod_buckling_check`: the critical compression member as
  an fs-solid Cosserat rod with a seeded bow, loaded to factor×design
  — stable/bow-ratio outcome (the tfz.14/tfz.15 spot check).

## Invariants

1. Ground rules hold member-by-member and generation is bitwise
   reproducible (truss-001).
2. PDHG reaches hand-provable optima (aligned tie `PL/σ`; symmetric
   two-bar `2PL/σ`) to 1e-4 with objective separation < 1e-5,
   equilibrium residual < 1e-5, complementary slackness and observed dual
   feasibility violation < 1e-4 (truss-002). These are numerical oracle checks,
   not an outward-rounded certificate for arbitrary instances.
3. Densifying the ground structure does not worsen the returned-iterate volume
   beyond the declared diagnostic tolerance
   (truss-003); the Michell closed-form catalogue comparison is a
   LEDGERED PENDING row until its vetted constants land via the
   fs-fab oracle spec — stated, never silently skipped.
4. PDHG cost per (iteration × nnz) is flat across problem sizes
   (spread < 3×) and warm starts reduce iterations on perturbed load
   cases (truss-004; the 10⁶-member wall-clock target is perf-lane
   scope, ledgered).
5. Sizing: post-prune equilibrium re-verified < 1e-6; Euler floors
   active on compression members; post-snap member-by-member audit
   all-pass (truss-005).
6. The rod spot check has teeth: catalog area stable at 1.3× design,
   an under-sized member fails or bows an order harder (truss-006).

## Error model

Structured asserts remain on ground-construction programmer contracts
(degenerate grids, empty clouds). `LayoutLp::solve` returns `PdhgError` for
zero iteration/check intervals, invalid tolerance, malformed warm-start shape,
or non-finite/out-of-domain warm state. The objective-separation and KKT
numbers are diagnostics, not a rigorous optimum interval; `NaN` catalog area
marks an un-satisfiable member in the audit rather than silently
clamping.

## Determinism class

Bit-deterministic across runs on a platform (BTree generation, fixed
iteration order, deterministic solvers). Cross-ISA goldens not yet
recorded.

## Cancellation behavior

Bounded synchronous loops (iteration caps everywhere); chunked Cx
polling belongs to the fs-exec driver.

## Unsafe boundary

`#![deny(unsafe_code)]` via workspace lints; no capsules.

## Feature flags

None.

## Conformance tests

`tests/battery.rs`: truss-001 rules + determinism; truss-002 provable
oracles with numerical diagnostics and malformed-solver-input refusal;
truss-003 refinement monotonicity within declared tolerances;
truss-004 scale trend + warm starts; truss-005 sizing/snap audit;
truss-006 rod spot check.

## No-claim boundaries

- SOCP extensions (elastic-compatible layout, stress constraints
  beyond plastic design) — the LP ships; SOCP is the recorded
  successor under the same PDHG surface.
- The vetted Michell closed-form catalogue (0.08-tolerance
  comparisons land with the fs-fab `:oracle (michell …)` spec
  constants).
- 10⁶⁺-member wall-clock budgets (perf lanes; the trend is ledgered
  here).
- 3D ground structures; frame (moment-carrying) layout; connection
  families beyond angle sets; discrete member-count MILP.
- Multi-load-case simultaneous layout (warm starts ship; the
  worst-case envelope LP is follow-up).
- A finite optimum enclosure. The returned primal is only approximately
  equilibrated, and the floating dual scaling is not outward-verified. Exact or
  interval primal repair plus independently checked dual feasibility is tracked
  separately before either objective becomes a certified bound.
