# CONTRACT: fs-govern

The addendum's machine-readable risk register (Part V, R1–R10) with a
CI-gateable completeness audit.

## Purpose and layer

Layer UTIL. Pure data + audit — no dependencies. Encodes the ten named risks,
each with its mitigation, early-warning metric, action threshold, and owning
bead, and audits that none survives unmeasured (design principle P8 /
Governance Rule 2).

## Public types and semantics

- `RiskId` (`R1`..`R10`) with `RiskId::ALL` and `code()`.
- `Risk { id, name, description, mitigation, early_warning, threshold, owner,
  instrumented }` — `early_warning` is the metric that makes the risk visible
  before it is fatal; `owner` is the bead that owns the mitigation;
  `instrumented` is whether that metric is live on a dashboard (default
  `false` — the honest baseline).
- `register() -> &'static [Risk]` — the canonical R1–R10 in order;
  `risk(id) -> &'static Risk` for lookup.
- `audit() -> RiskAudit` / `audit_slice(&[Risk]) -> RiskAudit` — checks every
  risk has a non-empty early-warning metric AND an owner, counts how many are
  instrumented, and lists `(RiskId, reason)` gaps. `RiskAudit::ok()` is true
  iff there are no gaps.
- `to_json() -> String` — a deterministic machine-readable JSON array (one
  object per risk: id, name, early_warning, threshold, owner, instrumented,
  mitigation) with JSON-escaped strings, for dashboards / CI gates.

## Invariants

- The register is complete: `audit().ok()` is true and `audit().complete == 10`.
- `register()` and `RiskId::ALL` share the same order.
- `to_json()` and `audit()` are deterministic.

## Error model

None (pure data + total functions); the audit reports gaps as data, it does
not error.

## Determinism class

Fully deterministic — pure functions over `const` data, no RNG or I/O.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/register.rs` (Part V, 8 cases): all ten risks present + ordered;
every risk has a metric/owner/mitigation; owners are real bead ids; lookup;
the canonical audit is complete with an honest zero-instrumented baseline;
`audit_slice` detects missing metric AND owner on an incomplete entry (the
audit is not vacuous); JSON is well-formed + complete; determinism.

## No-claim boundaries

- This crate encodes the risk register as governance DATA; it does not itself
  measure the early-warning metrics — a dashboard/CI wires them and flips
  `instrumented`. The audit enforces that each risk DECLARES a metric + owner,
  not that the metric is currently green.
- The addendum's design principles (P1–P8), the four governance rules, and the
  per-proposal kill criteria are the sibling doctrine bead's scope; this crate
  is the risk register (R1–R10) only.
- Bead-id owners are string references; this crate does not read the beads
  database (that coupling is deliberately avoided).
