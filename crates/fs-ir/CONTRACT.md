# CONTRACT: fs-ir

> Status: ACTIVE (FrankenScript core, IR language v1). Owns the typed AST,
> both concrete syntaxes, study recognition, and verb lowering. Admission
> (dimensional/chart/budget/capability checks) is the gp3.5 bead's;
> the operator catalog is gp3.6's.

## Purpose and layer

FrankenScript — the system's one true interface (plan §11.1, Decalogue
P10): a typed, versioned IR with two isomorphic concrete syntaxes
(canonical s-expressions; lossless JSON mapping), both parsing to the same
typed AST. Layer: L6 (HELM). Runtime deps: `std` + fs-qty.

## Public types and semantics

- `Node`/`NodeKind`/`Span` — every node carries a byte span. Atoms are the
  real nouns: `Int`, `Float` (finite only), `Qty` (fs-qty SI value + dims +
  the ORIGINAL literal text — fs-qty normalizes 65deg → rad, so the
  literal is preserved verbatim for lossless printing; equality is
  value+dims), `Count` (information/core grants: B/KiB/MiB/GiB/cores —
  deliberately outside fs-qty's SI domain), `Seed` (0x… u64), `Str`,
  `Symbol`, `Keyword`, `List`.
- `Node::same_shape` — semantic equality ignoring spans and Qty
  presentation; the isomorphism property is stated in terms of it.
- `sexpr::parse/print` — total reader with spans, comments (`;`),
  string escapes, deterministic atom classification (numeric-leading
  tokens MUST fully parse — a number with a garbage suffix is a structured
  error, never silently a symbol), depth cap (adversarial nesting refuses
  structurally). Printer output reparses to the same shape.
- `json::parse/print` — the lossless mapping (single-key tagged objects:
  i/f/q/c/seed/s/sym/kw; arrays = lists). Qty/Count/Seed reuse the s-expr
  literal grammar inside strings so ONE classifier owns numeric semantics
  for both syntaxes. Unknown tags and tag/literal mismatches refuse with
  spans.
- `Study::from_node` — recognizes Appendix C study forms: name, seed,
  versions/budget/capability clauses, `(let …)` bindings, body;
  `constellation_lock()` extracts the versions pin. Extraction only —
  validity POLICY lives in `admission` (below).
- `lower::lower` — high-level verbs (`optimize-shape`, `simulate-pour`)
  expand to explicit IR with an inspectable trace naming every injected
  default (progressive disclosure with nothing hidden); idempotent;
  malformed verb usage refuses with the verb's span.
- `IrError` — span + stable kind code + detail + fix hint (refusals
  teach). `IR_VERSION` — the language version this build reads/writes.

- `admission` (the gp3.5 bead): `admit(node, &AdmissionContext) ->
  AdmissionReport` runs six timed dimensions — Five Explicits structure,
  dimensional analysis (fs-qty dims inferred bottom-up through `+ - * /
  min max` and comparisons; unknown verbs never false-reject), budget
  feasibility (fs-plan cost models over `:dof`/`:size` features, p90
  totals vs the `(budget (wall …))` bound, with RANKED cost-model-derived
  fixes: coarsen / surrogate-screen / relax), capability sufficiency
  (session token globs vs namespaced verbs + declared asks), chart
  routability (fs-geom Router as an admission predicate with the
  RouteRefusal's own fixes attached), and regime gating (explicit
  `(assert (regime.allows …))` plus `flux.*` verbs checked against an
  fs-regime report; policy-graded Reject/Warn). Findings carry spans,
  diagnoses, and `RankedFix { action, predicted_wall_s, qoi_impact }`.

## Invariants

1. Isomorphism: `parse(print(x))` has the same shape as `x`, per syntax
   and across syntaxes (property-tested on generated programs and the
   Appendix C fixtures).
2. Both parsers are total: any input yields a value or a structured error
   with an in-bounds span; recursion is depth-capped (no stack overflow).
3. No silent reinterpretation: numeric-leading tokens either fully parse
   as int/float/quantity/count or refuse; non-finite literals refuse.
4. Lowering is explicit, inspectable, and idempotent; the trace names
   every injected default.

- Admission determinism: same study + context → byte-identical
  `diagnosis()`; findings sorted (check, span).
- Admission latency is milliseconds-class on Appendix C studies (six
  checks timed individually; conformance logs and bounds the total).
- Zero false admits on the violation zoo; missing verifiers (no Router,
  no RegimeReport) degrade to WARN verification-gap findings, never to
  silent admits of violations they could not check.

## Error model

All fallible APIs return `IrError` (span, stable `IrErrorKind::code()`,
detail, hint). Never panics across the crate boundary (fuzz-tested).

## Determinism class

Parsing, printing, and lowering are pure functions of their input text —
bit-deterministic across runs, thread counts, and ISAs.

## Cancellation behavior

No compute loops beyond input length; parsing is bounded by source size
and the depth cap. No `Cx` integration needed at this layer.

## Unsafe boundary

None. Safe Rust only.

## Feature flags

None. All v1 behavior is `[S]` default-path.

## Conformance tests

`tests/conformance.rs`: Appendix C spout + frame studies as verbatim
fixtures (names, seeds, locks, lets, and typed-noun counts asserted);
isomorphism property over 200 generated programs plus the fixtures
(s-expr, JSON, and cross-syntax cycles); 8000-parse garbage battery with
in-bounds spans and non-empty hints plus 100k-deep nesting rejections;
span-accuracy cases (bad seed, bad quantity); verb lowering explicitness,
trace content, idempotence, and structured refusal; version-pin
round-trip through both syntaxes.

`tests/admission.rs` (suite `fs-ir/admission`): ad-001 Appendix C admits
cleanly + ms-class latency + determinism; ad-002 five-study violation zoo
(all rejected on the right dimension, fixes attached); ad-003 dimensional
spans pinpoint the offending operand, products stay legal; ad-004
BudgetInfeasible with ranked cost-derived fixes + fix-quality harness
(applying fixes admits); ad-005 Router-backed feasibility; ad-006 regime
gating with alternatives + policy grading; ad-007 2000 mutants + all
truncation prefixes never panic (a fuzz-found scanner panic became a
structured refusal).

## No-claim boundaries

- No operator catalog or per-operator semantic versions — gp3.6; the
  `IR_VERSION` constant covers the language only.
- JSON `\uXXXX` escapes cover Unicode scalar values only (surrogate
  pairs are rejected with a structured error).
- The verb table is v1-small (optimize-shape, simulate-pour); verbs are
  data to extend, not a framework.
- Qty literals must be written in units fs-qty accepts; information
  units are Counts, not quantities, by design.
- Admission's dimensional pass covers arithmetic/comparison heads;
  verb-signature dimension contracts (per-operator expected dims) land
  with the operator registry.
- Chart requirements are supplied by lowering/callers; admission does not
  yet derive them from raw study text.
- `SessionCapability` is admission's view of a token; issuance,
  revocation, and idempotency keys are fs-session's bead (gp3.7).
