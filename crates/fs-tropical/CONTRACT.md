# CONTRACT: fs-tropical

Tropical (max-plus) critical-path analytics: task-DAG timing in the max-plus
semiring as an online scheduling instrument.

## Purpose and layer

Layer L1 (fundamental graph/algebra). No dependencies — pure Rust.

## Public types and semantics

- `oplus(a,b) = max`, `otimes(a,b) = +` (absorbing at `NEG_INF`) — the max-plus
  semiring.
- `TaskDag::new(latencies).with_edge(u,v)` — a precedence DAG.
  `critical_path() -> Result<CriticalPath, TropicalError>` gives the makespan
  (CPM longest path), the critical path, and per-task slack (`0` ⇔ critical);
  errors `Cyclic` on a cycle, `BadEdge` on an out-of-range endpoint.
- `tune_next(&CriticalPath)` — the zero-slack tasks ranked by latency (the
  autotuner's tune-next list); `bottleneck(&CriticalPath)` — the top one.
- `max_cycle_mean(&[Vec<f64>]) -> Option<f64>` — the tropical eigenvalue (Karp),
  the pipelined steady-state throughput; `None` if acyclic.
- `critical_circuit` / `brute_force_max_cycle` — the maximum-mean cycle (nodes,
  mean), the ground truth `max_cycle_mean` is checked against.
- `RecommendationAudit` — `record`, `hit_rate`, `promoted(min_samples,
  min_hit_rate)`: the [M] promotion gate on realized outcomes.

## Invariants

- `critical_path` returns the longest source-to-sink path length as the
  makespan; a task with positive slack is off the critical path (tuning it
  cannot move the makespan).
- `max_cycle_mean` (Karp) equals the brute-force maximum simple-cycle mean.
- `bottleneck` is the highest-latency task on the critical path.
- The audit only reports `promoted` with enough samples AND hit rate.

## Error model

Structured `TropicalError`; no panics.

## Determinism class

Fully deterministic: every quantity is a pure function of the DAG / matrix.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/tropical.rs` (7 cases): the max-plus semiring; the critical path is the
longest path (+ slack); tune-next ranks the bottleneck first; a cyclic DAG has
no critical path; the tropical eigenvalue matches brute force + recovers the
critical circuit; a faster cycle dominates a self-loop (+ acyclic → None); the
promotion audit gates on realized outcomes.

## No-claim boundaries

- `critical_circuit` / `brute_force_max_cycle` enumerate SIMPLE cycles — for the
  small timing matrices this instrument runs on; `max_cycle_mean` (Karp) is the
  efficient `O(n·E)` path for large graphs.
- The LIVE pipeline (windowed re-computation over the ledger's event stream with
  drift detection, and contributing the tropical kernels upstream to
  FrankenNetworkx) consumes this core over `fs-ledger` events — this crate
  provides the batch analytics + the audit.
- The [M] discipline is advisory-until-validated; `RecommendationAudit` is the
  self-audit machinery, the promotion policy is the planner's.
