# SAFETY: fs-simd/src/x86/mod.rs

## Invariants
`unsafe` is confined to (a) AVX2/AVX-512 load/store/arithmetic intrinsics on
pointers derived from `as_chunks::<4/8/16>()` fixed-size arrays over live slices
(exact lane extents, unaligned-safe `loadu`/`storeu` only), and
(b) calls to `#[target_feature]` functions. Tails are handled by the scalar
twin; no partial-lane access exists.

## Aliasing assumptions
`&[f64]` in, `&mut [f64]` out; borrow rules preclude mutable aliasing. The
only read-modify-write is `axpy`'s exclusively-borrowed chunk.

## Alignment assumptions
None: only unaligned load/store intrinsics are used. Upstream 128-byte
alignment is performance, not soundness.

## Lifetime assumptions
No pointer outlives the loop iteration deriving it.

## Panic behavior
Length asserts fire before any unsafe block. No unwinding between a load and
its paired store.

## Cancellation behavior
Bounded, allocation-free loops; no poll points (callers chunk work at tile
granularity per the fs-exec discipline).

## Concurrency behavior
No shared state, no atomics; Send/Sync are the slices' properties.

## Miri coverage
Miri cannot interpret x86 vector intrinsics; under `cfg(miri)` dispatch
routes to the scalar twin. Compensating checks: the tier-equivalence battery
runs natively on x86-64 hardware (trj machine + CI runner).

## Model-checking coverage
N/A (no concurrency).

## Fuzz/property coverage
`tier_equivalence_battery` (shared with NEON): seeded inputs, special
values, every tail length 0..67; elementwise bitwise vs twin, reductions
within the documented envelope.

## Proof obligations discharged by callers
None. Façades re-verify CPU features via `is_x86_feature_detected!` before
every `#[target_feature]` call and fall back to the scalar twin otherwise —
the dispatch table's tier choice is optimization, not precondition. The
inner `#[target_feature]` functions are reachable ONLY through these
façades (module privacy enforces it).

## mk8x4_f64 (bead xlvx)

The 8×4 GEMM microkernel façade asserts panel bounds BEFORE the unsafe
body (`a_panel ≥ kc·8`, `b_panel ≥ kc·4`); the AVX2+FMA body reads
exactly 4 f64 per `loadu` at offsets `kk·4 ≤ kc·4 − 4` (B) and
broadcasts single elements at `kk·8 + r ≤ kc·8 − 1` (A); every
`storeu` writes 4 f64 into a row of the caller's `[[f64; 4]; 8]`.
Feature availability (avx2+fma) is runtime-verified in the façade
immediately before the call. Compensating check: the tier-equivalence
battery gates bitwise equality with the scalar twin over kc ∈ 0..17 ∪
{256} including special values and nonzero starting accumulators.
