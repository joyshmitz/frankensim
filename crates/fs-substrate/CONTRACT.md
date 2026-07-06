# CONTRACT: fs-substrate

## Purpose and layer
Hardware capability probes, stable machine fingerprints, topology facts, and
one-shot SIMD dispatch resolution (plan §5.1, patch Rev Q: measured facts,
never static claims). Layer: L0.

## Public types and semantics
- `CACHE_LINE: usize` — compile-time cache-line const (128 Apple aarch64 /
  64 elsewhere), runtime-VERIFIED against the OS report by the test suite.
- `CapabilityProbe` — `run()` (topology + ~100–300 ms bandwidth measurement)
  / `topology_only()` (fast); fields: ISA, brand, sorted feature list,
  cache line, page size, memory, logical CPUs, Apple perf/eff core counts,
  per-cluster L2, Linux NUMA node count, OPTIONAL gpu fact (separate class,
  unused by the CPU build), `Measured { single_thread_gbs, all_core_gbs }`.
- `fingerprint() -> u64` — FNV-1a over STABLE facts only; measured bandwidth
  excluded by design (jitter must not mint new machine identities).
- `to_json()` — ledger `capability_probes` row shape; caller supplies
  probe_time (this crate is clock-free outside the bandwidth measurement).
- `dispatch_tier() -> SimdTier` (Scalar/Neon/Avx2/Avx512) — resolved EXACTLY
  once (OnceLock; `dispatch_resolution_count()` test hook proves it).
- `bandwidth::measure` — STREAM-triad sweep (24 B/elem accounting, 64 MiB
  arrays, best-of-3), single-thread + all-core aggregate.

## Invariants
- Fingerprint stable across probes on the same machine; changes iff stable
  topology changes; never depends on measured numbers.
- Dispatch resolution happens at most once per process; no hot-path branching.
- Probe never panics; missing OS facts degrade to None/defaults, loudly
  visible in the JSON row.

## Error model
Absent OS facts → `None` fields (no fabrication). Subprocess/file read
failures degrade gracefully; nothing here throws across the boundary.

## Determinism class
Topology facts: deterministic per machine/boot. Bandwidth: measurement
(jittery by nature, quarantined from identity). No compute paths.

## Cancellation behavior
Startup-time, latency-lane work; longest operation ~300 ms. No Cx.

## Unsafe boundary
None. Feature detection via std::arch macros; OS facts via subprocess/file
I/O (no FFI, per P1).

## Feature flags
None. SME/AMX-class matrix units: deliberately NOT claimed; future probes
gate the fs-simd `frontier-sme2` tier.

## Conformance tests
Plausibility battery (cache line ∈ {64,128}, page ∈ {4Ki,16Ki}, ≥1 GiB RAM),
CACHE_LINE-vs-OS agreement, fingerprint stability + measured-exclusion +
topology-sensitivity, single dispatch resolution, JSON shape/balance,
bandwidth plausibility bounds.

## No-claim boundaries
- Per-core-CLASS bandwidth (P vs E pinning) — needs QoS/affinity outside safe
  std; per-class COUNTS are reported, aggregate bandwidth measured; real
  pinning lands with fs-exec or an audited capsule.
- x86-64/Linux backend is written but verified only when the CI runner lands
  (aarch64-apple verified locally).
- GPU/Neural facts are descriptive strings only; no capability claims.
- CCD topology mapping for EPYC/Threadripper (needs the x86 machine).
