# CONTRACT: fs-bisect

Physics-VCS bisect (plan addendum, Proposal 10): git-bisect for a wrong number.

## Purpose and layer

Layer L6 (version control / orchestration). No numerical dependencies — pure
control flow over a caller-supplied `CommitOracle`.

## Public types and semantics

- `Verdict { Good, Bad }`; `CommitOracle::evaluate(commit) -> Verdict` —
  commit 0 is the oldest (assumed-good baseline), `len-1` the newest. The
  oracle IS "replay commit k, then evaluate the predicate".
- `bisect(len, &oracle) -> BisectRun` — `O(log n)` binary search assuming a
  monotone predicate; returns `Culprit { index, confirmed: false }`,
  `AllGood`, `AllBad`, or `Empty`. `BisectRun { result, probes }` logs the
  search path.
- `verify_monotone(len, &oracle) -> Option<(usize, usize)>` — `O(n)` scan for a
  non-monotonicity witness (a Bad followed by a later Good).
- `bisect_checked(len, &oracle) -> BisectRun` — verifies monotonicity first;
  a non-monotone predicate yields `NonMonotone { bad, later_good }` instead of
  a mis-localization.
- `bisect_two_tier(len, &low, &full) -> BisectRun` — narrows with a cheap
  `low`-fidelity oracle, then CONFIRMS the culprit at `full` fidelity; if full
  rejects the low candidate it re-searches entirely at full fidelity. The
  culprit is `confirmed = true` (a *verified* localization vs the *estimated*
  single-fidelity one).

## Invariants

- On a monotone sequence with a Good prefix and a Bad suffix, `bisect` returns
  the first Bad index.
- `bisect_two_tier` never returns a full-fidelity-rejected candidate: it
  re-searches. Its culprit is always `confirmed`.
- All functions are pure and deterministic; the probe log records every
  evaluation in order.

## Error model

No errors/panics on valid indices; degenerate inputs map to explicit result
variants (`Empty`, `AllGood`, `AllBad`, `NonMonotone`).

## Determinism class

Fully deterministic: a bisect is a pure function of `(len, oracle)`; the same
oracle reproduces the same culprit + probe path (sound only if the oracle's own
commit replay is deterministic — the ledger's `at(t)`/ExecMode contract).

## Cancellation behavior

None here; the oracle's own (possibly expensive, low- or full-fidelity)
evaluation runs under the caller's cancellation scope.

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/bisect.rs` (Proposal 10, 10 cases): first-bad localization (short +
long, `O(log n)` probe count); empty / all-good / all-bad / singleton
boundaries; `verify_monotone` witness; `bisect_checked` flags non-monotone;
two-tier agreement, re-search on full-fidelity rejection, and endpoint
confirmation; confirmed-flag semantics; determinism.

## No-claim boundaries

- `bisect` ASSUMES monotonicity (documented); use `bisect_checked` when the
  predicate may be non-monotone. Detection is `O(n)`; plain `bisect` stays
  `O(log n)`.
- The colors (estimated for the low-fidelity search, verified for the
  full-fidelity confirmation) are represented here by the `confirmed` flag; the
  caller attaches the `fs-evidence` `Color` when it records the result.
- Commit replay determinism is the ledger's contract; this crate assumes the
  oracle is a faithful replay-plus-predicate.
