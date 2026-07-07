# fs-verify — CONTRACT

## Purpose and layer

L3 (FLUX-adjacent). The certified-speculation VERIFIER (Proposal 9,
the addendum's SINGLE research bet): a certified accept test that lets
untrusted proposers be maximally aggressive, because correctness never
depends on the learned component. For the elliptic class,
equilibrated-flux a-posteriori estimators (Prager–Synge) give
GUARANTEED, CONSTANT-FREE upper bounds on the energy-norm error; the
remaining risk — floating-point rounding — is retired by
OUTWARD-ROUNDED interval evaluation over mathematically exact
quadrature.

## Public types and semantics

- `interval::Iv` — outward-rounded interval arithmetic: every op
  widens one ulp each direction (bit-nudge directed rounding), so
  enclosures are RIGOROUS, not to-nearest-plus-slack. Small on
  purpose (`add/sub/mul/sq/sqrt/scale_pos`).
- `fem1d` — the v0 class testbed: P1 elements for `−u″ = f` on (0,1),
  polynomial manufactured solutions of degree ≤ 5 (so 5-point Gauss
  quadrature is MATHEMATICALLY exact for every integrand the verifier
  meets — rigor rests on rounding alone), Thomas solves, a
  high-resolution oracle, and the toy nonlinear class
  (`−u″ + u³ = f`, Newton).
- `estimator::verify(problem, candidate, tolerance)` — THE VERIFIER:
  the equilibrated flux is `σ = c − F` (any `c` is sound because
  `σ′ = −f` exactly; the FREE CONSTANT is optimized in plain f64 for
  TIGHTNESS — rigor from structure, tightness from optimization), and
  the bound `‖σ − u_h′‖` is interval-evaluated. Accept ⟺
  `bound.hi ≤ tolerance`; an accept carries `Color::Verified`; a
  reject — or ANY unbounded/NaN enclosure — carries NOTHING (fail
  closed, never a badge without a bound). Reports carry the
  review-round-3 ledger fields (family id, flux hash, bound
  endpoints, oracle error, effectivity, verdict, tolerance).
- `estimator::hierarchical_estimate` — the INDEPENDENT second family
  (refined-mesh comparison; not guaranteed; the falsifier's
  cross-check, never a color source).
- `estimator::warm_start` — the honest nonlinear fallback: candidates
  are WARM STARTS with measured iteration savings and an ESTIMATED
  color, never certificates (the R1 boundary).

- `zoo` (bead lmp4.2) — the PROPOSER ZOO behind one `propose()`
  trait: hot-swap `Registry` (register/deregister without touching
  consumers), `speculate()` ordering candidates by ADVISORY confidence
  (descending, NaN last, deterministic name tie-break — confidence
  never enters any accept decision or certificate), and the
  TYPE-LEVEL safety invariant: `CertifiedAnswer` has no public
  constructor — it exists only when the verifier says yes. v0
  proposers: NEIGHBOR EXTRAPOLATION (nearest certified run, Taylor
  correction with a cached sensitivity, zeroth-order degradation,
  smaller-θ equidistant tie-break) and COARSE-RUNG PROLONGATION
  (halved-mesh solve, linear prolongation, honest decline on tiny
  meshes). `quantize_f16` demonstrates the precision discipline:
  speculate LOW, verify HIGH — the certificate inherits the
  VERIFIER's precision. `ZooTelemetry` tracks per-proposer per-regime
  accept rates with the AUTO-DEMOTION hook (collapse ⇒ disabled in
  that regime) and ledger rows.

- `economics` (bead lmp4.3; COMPLEMENTARY to the standalone
  fs-spececo policy crate, which owns the abstract decide/telemetry/
  drift logic — this module is the INTEGRATED loop driving real
  solves through the zoo, and fs-ledger v3 supplies the persistence
  fs-spececo's no-claim defers; drift-logic consolidation between the
  two is a follow-up) — the accept/reject CONTROL LOOP:
  `run_speculative` accepts OUTRIGHT on a certified pass (no solve at
  all); otherwise the best rejected candidate (smallest certified
  bound) WARM-STARTS the true solve with savings MEASURED and recorded
  CLAMPED at ≥ 0 (a worse-than-cold start is never a win; the raw
  negative delta stays in the ledger). `DriftGuard` is the drift
  detector that falls out of the telemetry: an accept-rate collapse in
  a regime demotes the proposer THERE (localized), only after a
  minimum sample count, with probation hysteresis whose evidence bar
  DOUBLES per failed probation (no flapping). Zero-telemetry regimes
  report the conservative 0.0 prior. `solve_node_record` emits the
  four-field schema amendment `(proposer_id, accepted, bound,
  iterations_saved)` stored in fs-ledger's v3 `speculation` extension
  table. Dashboards are kernel × regime × proposer with median
  savings.

## Invariants

1. THE UPPER-BOUND PROPERTY (G1 MMS): the bound dominates the oracle
   truth on every battery case INCLUDING adversarially perturbed
   candidates — Prager–Synge holds for ANY conforming candidate,
   which is exactly what makes untrusted proposers safe (ver-001,
   120/120).
2. EFFECTIVITY: median bound/truth = 1.000 on the Galerkin battery
   (band ≤ 3; the ~30% accept-rate kill criterion is unreachable with
   loose-but-sound bounds — soundness alone does not close the
   economy), zero tightness failures (ver-002).
3. Interval soundness: near-ulp enclosure widths; NaN/∞ candidates
   FAIL CLOSED; wild-but-finite candidates stay finite and rejected
   (ver-003).
4. G5: verdicts, bound endpoints, and flux hashes are BITWISE
   reproducible; equality accepts are sound by domination; single-
   and zero-interior-DOF meshes bound truthfully (ver-004).
5. Certify-the-certifiers: an injected UNSOUND estimator (bound/10)
   undershoots truth and is CAUGHT by the harness (a fooled bound is
   a Sev-0 wrong answer wearing a badge); the hierarchical family
   stays within its stated band (ver-005).
6. The warm start saves ≥ 1.5× Newton iterations with an ESTIMATED
   color and complete ledger rows (ver-006).
7. Zoo: answers exist only through the verifier; confidence is
   advisory in BOTH directions (a NaN-confidence good proposer still
   wins; a confidence-1.0 adversary never does) (zoo-001).
8. Warm adjoints beat zeroth-order >2×; both degrade into verified
   accepts; equidistant ties are deterministic (zoo-002).
9. Coarse-rung candidates accept at honest tolerances, reject at
   tight ones, and fp16-quantized candidates still verify (zoo-003).
10. THE FALSIFIER: an adversarial surrogate lands ZERO incorrect
    accepts, its rate collapse auto-demotes it, and demoted proposers
    stop being consulted (zoo-004).
11. The economics loop stays sound end-to-end with per-proposer
    per-regime rows shipped to the ledger (zoo-005).
12. Outright accepts ship without a solve; warm starts measure real
    savings on the strongly nonlinear class (econ-001); antithetical
    warm starts clamp to zero recorded savings with the raw delta
    preserved (econ-002).
13. Drift demotion is LOCALIZED to the collapsed regime (econ-003);
    hysteresis prevents flapping and genuine recovery re-promotes
    (econ-004); priors are conservative, decisions deterministic, and
    the dashboard ships via fs-obs (econ-005).

## Error model

Fail closed is the error model: no exceptions cross the boundary; an
unevaluable bound is a REJECT with no color, never a panic and never
an accept.

## Determinism class

Fully deterministic and sequential in v0: fixed quadrature, fixed
Thomas elimination order, bit-nudge rounding. Bit-identical reports
across runs (ver-004). Thread-count independence rides the
deterministic-reduction contract when the tile-kernel form lands
(no-claim below).

## Cancellation behavior

v0 solves are milliseconds-scale direct solves (no polling loops);
the tiled/parallel form inherits fs-exec's checkpoint discipline.

## Unsafe boundary

None. `#![forbid(unsafe_code)]` via workspace lints; no capsules.

## Feature flags

`certified-speculation` — the whole verifier, OFF by default per the
Ambition-Tag gating rule until its Gauntlet tier and the six-month
kill-metric (accept rates > ~30% at customer tolerances, warm-start
savings ≥ 1.5×) stay green.

## Conformance tests

`tests/conformance.rs`, cases ver-001..ver-006 — JSON-line verdicts,
seeded LCG randomness, fs-obs events for the effectivity table and
ledger rows. Any reimplementation must pass the suite unchanged.

## No-claim boundaries

- v0 is the 1D elliptic class with polynomial data (quadrature
  exactness is the rigor backbone). The 2D/3D FEEC H(div) patchwise
  equilibration (Braess–Schöberl / Ern–Vohralík) rides fs-feec's
  Whitney machinery as the successor — the architecture (accept test,
  colors, falsifier, fail-closed) is class-independent and lands here
  unchanged.
- Variable diffusion coefficients, non-polynomial data (with data-
  oscillation terms and explicit Poincaré constants), and quadrature
  ERROR bounds for transcendental integrands are the same successor.
- The roofline tile-kernel form of patchwise equilibration (with the
  stated ISA acceptance bands) belongs to the perf lane.
- Interval arithmetic here is local; unification with fs-ivl is the
  workspace-wide interval consolidation.
- Accept-rate telemetry at customer-realistic tolerances (the kill
  measurement) accumulates once the first physics vertical is live.
- The NEURAL surrogate proposer (FrankenTorch fp16/fp8 over
  FrankenNetworkx graphs) is fs-surrogate's (bead 7tv.8); the zoo's
  trait and adversarial gating are ready for it.
- The asupersync speculative RACE (proposer vs target, loser drained
  request→drain→finalize at a tile boundary, zero leaks) is the
  concurrent form — v0 speculates sequentially before the solve.
