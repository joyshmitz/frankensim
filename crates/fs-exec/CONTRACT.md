# CONTRACT: fs-exec

## Purpose and layer
Two-lane executor (plan §5.2): the latency lane is asupersync's async
scheduling for orchestration; the throughput lane is a work-stealing
fork-join tile pool with weighted quanta, CCD-local-first stealing, and
fixed-shape reductions. Owns the `Cx`/`TileKernel` contract every hot
kernel programs against. Layer: L0. Depends on asupersync, fs-alloc,
fs-substrate, fs-obs.

## Public types and semantics
- `Cx<'s>` — the per-tile context (plan Appendix B): `checkpoint()` /
  `is_cancel_requested()` poll the run's `CancelGate` (the MANDATORY
  tile-boundary poll), `arena()` is the tile-scoped fs-alloc arena
  (lifetime-bound; escapes are compile errors), `stream_key()` is the
  logical RNG identity, `budget()` carries asupersync's `Budget`
  vocabulary, `mode()` the `ExecMode` provenance.
- `StreamKey { seed, kernel_id, tile, iteration }` + `key128()` — RNG
  stream identity derived from LOGICAL work identity only, never from the
  worker (Decalogue P2). fs-rand's Philox consumes the 128-bit key.
- `CancelGate` — request → drain → finalize: `request()` is idempotent and
  stamps the first-request time; workers finish their current tile, stop
  claiming, and the run returns a structured outcome. Timestamps feed
  reports only, never results.
- `TileKernel` (`type Out: Reduce; tiles() -> TilePlan; run(tile, &Cx) ->
  ControlFlow<Cancelled, Out>`) and `TilePlan { tiles, kernel }` with the
  FNV-stable `kernel_id()`.
- `Reduce` — fold identity + `merge`, applied over per-tile slots on the
  FIXED-SHAPE pairwise tree: split at the largest power of two below `n`,
  recurse — shape a pure function of the tile count, items visited in
  ascending index order, so `merge` need not be commutative. Implemented
  for `()`, `u64`, `f64`, `Vec<T>`, `reduce::Compensated`.
- `reduce` module (plan §5.4, the P2 machinery): `pairwise_fold`,
  Neumaier `Compensated` partials (Reduce-composable), `det_sum`/`det_dot`
  /`det_norm2` (256-element blocked, unfused products), `det_min`/`det_max`
  (IEEE total order), `det_argmin`/`det_argmax` (ties -> LOWEST index),
  `det_prefix_sum` (compensated sequential scan), and `audit_accumulator`
  — the G5 order-sensitivity audit that catches arrival-order bugs and
  localizes them to the smallest exposing prefix.
- `TilePool` / `PoolConfig { workers, topo, quantum_weights, seed, mode,
  arena }` — `run(&kernel)` / `run_with_gate(&kernel, &gate) -> (Result<Out,
  RunError>, RunReport)`. Workers are scoped per run; per-worker deques are
  seeded with contiguous, weight-proportional tile runs (`weighted_ranges`)
  and steal HALF a victim's deque in `victim_order` (same-CCD ring first).
- `RunError { Cancelled, TilePanicked, Incomplete }` — structured, teaching
  outcomes with tile provenance. `RunReport` — steal counts, cross-CCD
  steal counts, cancel-latency samples, `cancel_latency_p99_ns()`,
  canonical `to_json()`.
- `LatencyLane` — thin configured handle on the asupersync runtime
  (`block_on`, `runtime()`); no fs-exec scheduling policy of its own.
- `victim_order(worker, workers, topo)` / `weighted_ranges(tiles, weights)`
  — pure, deterministic; these functions ARE what workers use, so fixture
  verification verifies runtime behavior.

## Invariants
1. Completeness: a non-cancelled, non-panicked run executes every tile in
   `0..plan.tiles` exactly once (exec-001).
2. Fixed-shape reduction: the pairwise tree's shape depends only on the
   tile count, so results are bit-identical across worker counts, steal
   schedules, and repeats — proven with non-associative floats, a
   non-commutative concatenation, and compensated artifact hashes across
   {1,2,P,2P} workers (exec-002/008/009, G5).
3. Stream keys are pure functions of `(seed, kernel_id, tile, iteration)`;
   shuffling worker counts changes nothing (exec-003).
4. Cancellation is request → drain → finalize: after `CancelGate::request`,
   workers claim no new tiles, in-flight tiles finish (or observe the gate
   at their own poll points), arenas reclaim to quiescence, and the run
   returns `RunError::Cancelled` with completed/total counts (exec-004/005).
5. Panic containment: a panicking tile is caught with tile provenance,
   siblings drain via the gate, the pool remains usable, and the process
   NEVER aborts (exec-005 and unit battery).
6. Steal order is CCD-local-first under the fixture topologies; initial
   quanta are weight-proportional within one tile (exec-006).
7. Per-tile arenas come from one `ArenaPool` (chunk-recycled); the pool's
   quiescence oracle is the leak check after every run.

## Error model
All fallible APIs return structured values (`RunError`, `LaneError`) with
teaching `Display` text. Kernel panics become `RunError::TilePanicked`;
executor-internal invariant violations become `RunError::Incomplete`
(reported, not panicked). The only intentional panics are lock-poisoning
`expect`s (reachable only after a panic already contained elsewhere) and
kernel-authored asserts, which are contained per invariant 5.

## Determinism class
Deterministic (P2): results and stream keys are bit-stable across runs,
worker counts, and steal schedules on the same ISA, by construction
(slot-per-tile + length-keyed pairwise tree + logical keys). Tie-breaking
law: argmin/argmax ties resolve to the lowest logical index; float
comparisons use IEEE total order. Cross-ISA: identical shapes reduce
divergence to scalar-arithmetic classes (FMA contraction, libm ULP) owned
by fs-math and reported by the G5 cross-ISA report once the second-ISA
runner lands. `ExecMode::Fast` currently shares the same reduction shape
and exists as recorded provenance for the future relaxation. Timing values (steal counts, latencies) are measurements
quarantined in `RunReport`/events, never in results.

## Cancellation behavior
The throughput lane polls the gate at every tile boundary and requires
kernels to poll `cx.checkpoint()` at bounded strides inside long tiles;
drain semantics per invariant 4. The latency lane inherits asupersync's
region state machine (request → drain → finalize) unmodified. Cancel
latency is MEASURED per run (histogram in `RunReport`, ledgered via events);
see no-claims for the 200 µs target's status.

## Unsafe boundary
None. The pool is safe Rust (scoped threads, mutex deques, atomics);
`catch_unwind` is safe containment. Lock-free deques, if they ever land,
arrive as a registered capsule with a SAFETY.md.

## Feature flags
None. Everything here is `[S]` solid-tier.

## Conformance tests
tests/conformance.rs, cases exec-001..exec-008 (JSON-line verdicts; seeded
cases carry seeds): completeness/arena hygiene, G5 bit-identity across
worker counts, stream-key worker-independence, external-cancel drain with
ledgered latency histogram, the 300-run G4 storm with panic injection,
steal-order/quanta fixtures, latency-lane responsiveness under saturation,
reduction-shape invariance, and the exec-009 G5 audit (compensated
artifact hashes bit-stable across {1,2,P,2P} workers; seeded arrival-order
bug caught with prefix localization). tests/constellation_smoke.rs pins the
asupersync Budget vocabulary. In-module unit suites cover the gate, keys,
Reduce laws, partitioning, victim orders, self-cancellation, and pool
survival after panics.

## No-claim boundaries
- NO 200 µs cancel-latency CLAIM yet: the reference-hardware p99 gate
  belongs to the roofline/perf harness with release builds and machine
  fingerprints; today the histogram is measured and ledgered per run, with
  a generous sanity envelope in CI (exec-004).
- NO lock-free deque claim: the v1 deques are mutex-based with the correct
  stealing PROTOCOL; Chase–Lev arrives only with roofline evidence that
  justifies its unsafe capsule.
- Workers are scoped per run (spawn cost ~tens of µs amortized over a
  kernel run); the persistent parked-worker pool is deferred with the same
  evidence bar.
- NO thread-pinning/NUMA-binding claim: `victim_order` steers locality;
  actual affinity syscalls are outside safe std (fs-substrate no-claim
  applies). P/E quantum WEIGHTS are plumbed but their values await the
  autotuner.
- Budget enforcement beyond cancellation (poll quotas, deadlines) is
  carried in the `Cx` but enforced by the session governor when HELM
  lands; `Budget` here is vocabulary and provenance.
- The latency lane's ≤100 ms conversational guarantee is HELM's gate;
  exec-007 measures and ledgers turnaround without claiming it.
- Speculative races and resumable-solver checkpointing (plan §5.2 items
  1–2) are the NEXT fs-exec beads (wf9.8), not this one.
- `ExecMode::Fast`'s 5–15% relaxed-reduction throughput claim is NOT made:
  Fast currently shares the deterministic tree; the relaxation (and its
  measured delta) waits for the roofline harness.
- Deterministic hash-map wrappers are not shipped: the contract's rule is
  "no HashMap iteration order in results" (BTreeMap or index-keyed slots
  in hot paths); an enforcement lint belongs to CI tooling.
