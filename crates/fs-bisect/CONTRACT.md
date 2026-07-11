# CONTRACT: fs-bisect

Physics-VCS bisect (plan addendum, Proposal 10): git-bisect for a wrong number.
Plus the Gauntlet failure-compounding workflow (`compound`, bead 6nb.9):
minimize every counterexample into a permanent regression family.

## Purpose and layer

Layer L6 (version control / orchestration). No numerical dependencies — pure
control flow over a caller-supplied `CommitOracle`, and pure data plumbing for
`compound`'s capture → minimize → probe → family → replay pipeline.

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
- `compound` module (bead 6nb.9): `FailureCase<I>` (id, seed, typed input,
  `InvariantClass`, contract surface, detail); `Shrink` (deterministic
  candidate order) + `minimize` (greedy first-failing descent to a fixpoint or
  step budget, `converged` flag, typed `NotFailing` refusal on a passing input,
  and hard per-step/aggregate evaluation ceilings); `probe_neighborhood` over
  a bounded, uniquely labeled caller set; sealed `RegressionFamily<I>`
  construction (minimum + failing neighbors, nonempty unique tracking refs,
  `recommended_admission`) with construction-time bounded `Canon` snapshots,
  `content_hash` (domain-separated in-tree BLAKE3 over versioned, tagged,
  length-prefixed bytes — floats via `to_bits`), escaped JSON-lines `manifest`
  (hash trailer), and `replay` (members that stop failing are REPORTED as
  `now_passing`, never silently dropped); `compound(...)` drives the whole
  workflow. `COMPOUND_CANON_VERSION` is the golden-couplings surface const: any
  canon/tag/hash-assembly change bumps it and deliberately re-freezes dependents.

## Invariants

- On a monotone sequence with a Good prefix and a Bad suffix, `bisect` returns
  the first Bad index.
- `bisect_two_tier` never returns a full-fidelity-rejected candidate: it
  re-searches. Its culprit is always `confirmed`.
- All functions are pure and deterministic; the probe log records every
  evaluation in order.
- Failure compounding validates identifiers and descriptions before descent,
  rejects empty tracking, duplicate labels/references, reserved custom
  invariant names, and limit+1 work. Content identity uses the canonical bytes
  sealed at family construction, so stateful caller implementations cannot
  change a manifest after admission.

## Error model

No errors/panics on valid indices; degenerate inputs map to explicit result
variants (`Empty`, `AllGood`, `AllBad`, `NonMonotone`). Failure compounding
returns `CompoundError::{NotFailing, InvalidField, LimitExceeded,
DuplicateIdentity}`; it never silently truncates or repairs an inadmissible
family.

## Determinism class

Fully deterministic: a bisect is a pure function of `(len, oracle)`; the same
oracle reproduces the same culprit + probe path (sound only if the oracle's own
commit replay is deterministic — the ledger's `at(t)`/ExecMode contract).
Compounding identity is BLAKE3 over the versioned construction-time canonical
snapshot and is independent of later interior mutation in a caller's input.

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

`tests/compound_battery.rs` (6nb.9 + j3q2 acceptance, 12 cases): a deliberately
broken cross-crate golden modeled on the real powi incident (sequential vs
square-multiply chains; minimizes to the exact k=7 divergence boundary for
base 0.7, neighborhood shows the sharp region edge, family replays live and
goes stale under the fixed implementation); a falsifier hit on a wrong tail
constant (systematic error minimizes to n=1, whole neighborhood fails);
frozen manifest content hash
`ff9c945e8f3ecbaee37e82b5d57e7da7f710644ce9d8d0095c4974815aa132b7`
(registered in `golden-couplings.json` against
`fs-bisect:compound-canon=2`); bitwise
minimization determinism + `NotFailing` refusal; canon concatenation-collision
resistance; per-field content-hash sensitivity; exact work boundaries and
limit+1 refusals; nonempty tracking/reserved-invariant/canonical-byte gates;
JSON escaping; and construction-time canonical snapshot stability.

## No-claim boundaries

- `bisect` ASSUMES monotonicity (documented); use `bisect_checked` when the
  predicate may be non-monotone. Detection is `O(n)`; plain `bisect` stays
  `O(log n)`.
- The colors (estimated for the low-fidelity search, verified for the
  full-fidelity confirmation) are represented here by the `confirmed` flag; the
  caller attaches the `fs-evidence` `Color` when it records the result.
- Commit replay determinism is the ledger's contract; this crate assumes the
  oracle is a faithful replay-plus-predicate.
- `compound` does not write to the ledger and emits no fs-obs events —
  recorded follow-up once the huq.16 observability schema lands; manifests are
  returned to the caller, not persisted here.
- `compound` does not enact admission rules: `recommended_admission` is
  carried prose (as check-powi was born from the powi incident); enacting it
  is the responding agent's task.
- `minimize` finds A minimal failing input under the caller's `Shrink` order,
  not THE global minimum (greedy, not exhaustive); `converged = false` marks a
  budget-limited descent honestly.
- Caller callbacks (`fails`, `shrink_candidates`, and `neighbors_of`) execute
  outside FrankenSim's control and must enforce their own cancellation and
  allocation budgets. This crate bounds the candidate counts it accepts and
  the predicate calls it makes after those callbacks return; it cannot prevent
  a callback from doing excessive work internally.
- The manifest carries typed canonical bytes but this generic crate does not
  supply a decoder for arbitrary caller-defined `I`; replay uses the sealed
  in-memory typed family. Durable type-specific decoding belongs with each
  regression-family owner.
