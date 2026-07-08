# CONTRACT: fs-voi

Value-of-information and active validation: the strategic layer deciding what
information to acquire next — spend where it can CHANGE A DECISION.

## Purpose and layer

Layer L4 (decision/optimization). No dependencies — Gaussian decision algebra
with an in-house normal CDF (`erf`). Pure, deterministic.

## Public types and semantics

- `Uncertainty { numerical, statistical, model }` — `total_std` (quadrature),
  `dominant` component.
- `DesignEstimate { name, mean, uncertainty }` — an Evidence-carrying objective
  estimate (minimizing).
- `ranking_flip_probability(chosen, other)` — `P(other actually better)` (Φ).
- `decision_posture(&[DesignEstimate])` — best, runner-up, flip probability.
- `evpi(&[DesignEstimate])` — expected opportunity loss of the current top-two
  decision (0 when robust; positive when close).
- `ActionKind` (Surrogate / Simulate / Refine → numerical; Sample → statistical;
  Test → model) + `Action { name, kind, target_design, reduction, cost }`.
- `action_value(&[DesignEstimate], &Action) -> ActionValue` — the EVPI reduction
  per cost; ~0 for a decision-irrelevant target.
- `recommend(&[…], &[Action], stop_threshold) -> Recommendation` — the best
  decision-value-per-cost action, or STOP when EVPI ≤ threshold.
- `heuristic_choice(&[…], &[Action])` — the uncertainty-proportional baseline
  ([M]) VOI must beat.

## Invariants

- ESTIMATOR-vs-DECISION: an action on a design outside the decision boundary
  buys ~0 EVPI reduction, however uncertain that design is.
- STOP is returned exactly when the current EVPI ≤ the stop threshold (a robust
  decision), never prematurely below it.
- When the decision-boundary uncertainty is MODEL-dominated, a model-reducing
  action (Test) beats a statistical one (Sample) — decision-aware escalation.
- Ranking-flip probability is the Gaussian `Φ` of the standardized mean gap.

## Error model

Total functions; no panics (degenerate zero-σ cases handled explicitly).

## Determinism class

Fully deterministic: every quantity is a pure function of the estimates +
actions.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/voi.rs` (7 cases): ranking-flip probability vs separation; EVPI zero
when robust / positive when close + posture; information on a decision-
irrelevant design is worthless; STOP for a robust decision; VOI beats the
uncertainty-proportional baseline (spends on the boundary, not the most
uncertain); VOI escalates model fidelity when model uncertainty dominates;
determinism.

## No-claim boundaries

- v1 measures decision-relevance via the TOP-TWO pairwise EVPI (the ranking is
  dominated by the two lowest-mean designs); a full multi-design `E[min]` EVPI
  is a refinement.
- Objectives are treated as Gaussian; heavy-tailed / correlated estimates are a
  refinement.
- Action cost models are SUPPLIED (fs-plan-models); this crate arbitrates value
  per cost. Sequential-decision validity (fs-eproc) and the HELM planner
  consuming the rankings are downstream integrations.
- `reduction` is the fractional uncertainty cut an action achieves; calibrating
  it from real action outcomes is later work.
