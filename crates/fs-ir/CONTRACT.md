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
  validity POLICY belongs to admission (gp3.5).
- `lower::lower` — high-level verbs (`optimize-shape`, `simulate-pour`)
  expand to explicit IR with an inspectable trace naming every injected
  default (progressive disclosure with nothing hidden); idempotent;
  malformed verb usage refuses with the verb's span.
- `IrError` — span + stable kind code + detail + fix hint (refusals
  teach). `IR_VERSION` — the language version this build reads/writes.

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

## No-claim boundaries

- No admission checks (dimensions/charts/budgets/capabilities) — gp3.5.
- No operator catalog or per-operator semantic versions — gp3.6; the
  `IR_VERSION` constant covers the language only.
- JSON `\uXXXX` escapes cover Unicode scalar values only (surrogate
  pairs are rejected with a structured error).
- The verb table is v1-small (optimize-shape, simulate-pour); verbs are
  data to extend, not a framework.
- Qty literals must be written in units fs-qty accepts; information
  units are Counts, not quantities, by design.
