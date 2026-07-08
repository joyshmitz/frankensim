# fs-fmm CONTRACT

## Purpose and layer

Layer: L2. Kernel-independent black-box FMM (plan §8.3 [F], bead
tfz.20): the Fong–Darve Chebyshev scheme — every translation operator
is polynomial interpolation, so any kernel smooth away from the
diagonal works unchanged, and accuracy is controlled by one number
(the interpolation order p).

## Public types and semantics

- `Kernel` (trait): `eval(x, y)` — smooth off the diagonal; the
  diagonal convention is the caller's (Laplace3d returns 0 there).
- `Laplace3d`: `1/(4π|x−y|)`.
- `Fmm::new(kernel, points, order, leaf_cap)`: UNIFORM-depth octree
  (depth from N/leaf_cap, empty cells omitted, ancestors registered) —
  on a uniform tree, "adjacent leaves run P2P, first-separated
  ancestors run M2L" partitions every source–target pair EXACTLY ONCE
  (no gap, no double count). `potentials(charges)` runs P2M/M2M
  (anterpolation to p³ Chebyshev grids), M2L (kernel evaluated between
  well-separated same-level grids whose parents are adjacent), L2L/L2P
  and the direct near field. `direct(charges)` is the O(N²) oracle;
  `stats()` the octree ledger row.

## Invariants

1. Accuracy is controlled by p: the order sweep against the direct
   oracle falls monotonically, reaching < 1e-5 relative L2 at p = 7 on
   uniform clouds (fmm-001; curves ledgered).
2. G3 translation invariance: a rigidly shifted cloud reproduces
   potentials to < 1e-9 relative (fmm-002).
3. Scaling: time-vs-N fitted exponent < 1.6 over a 4096→32768
   doubling ladder (fmm-003; O(N log N)-class trend — the 10⁷-point
   wall-clock target is the perf lanes' scope, stated, not silently
   skipped).
4. Determinism: BTree-keyed tree, fixed traversal orders.

## Error model

Structured asserts on programmer contracts (empty cloud, order < 2,
charge-length mismatch). No silent accuracy degradation: the order is
explicit at construction and the battery curves are the evidence.

## Determinism class

Bit-deterministic across runs on a platform. Cross-ISA goldens not
yet recorded.

## Cancellation behavior

Bounded synchronous passes; chunked Cx polling belongs to the fs-exec
driver (L2 discipline).

## Unsafe boundary

`#![deny(unsafe_code)]` via workspace lints; no capsules.

## Feature flags

None ([F] realized as a standalone crate per the crate-granular
gating rule).

## Conformance tests

`tests/battery.rs`: fmm-001 order sweep vs oracle; fmm-002
translation invariance; fmm-003 scaling trend.

## No-claim boundaries

- Adaptive trees (U/V/W/X interaction lists) — the uniform-depth
  partition ships; adaptivity is the recorded successor.
- SIMD-batched near field and precomputed M2L tables (perf lanes).
- Gradient outputs (consumers run per-component kernels today;
  a fused gradient pass is follow-up).
- Periodic/boundary-image variants; oscillatory kernels.
