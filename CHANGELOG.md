# Changelog

This is a synthesized, agent-facing changelog for FrankenSim.

Scope window: project inception on 2026-07-05 through `main` at
[`43d52f2`](https://github.com/Dicklesworthstone/frankensim/commit/43d52f2ed960ede2067eec353359063c86d9dbb3)
on 2026-07-07.

This document was rebuilt from git history, tag/release metadata, the checked-in
Beads tracker, and the current README/contract surface. It is organized by
landed capabilities rather than raw diff order.

## Version Timeline

There are no git tags and no GitHub Releases as of
[`43d52f2`](https://github.com/Dicklesworthstone/frankensim/commit/43d52f2ed960ede2067eec353359063c86d9dbb3).

| Version | Kind | Date | Summary |
|---------|------|------|---------|
| [`main@43d52f2`](https://github.com/Dicklesworthstone/frankensim/commit/43d52f2ed960ede2067eec353359063c86d9dbb3) | Mainline snapshot | 2026-07-07 | Working Rust workspace with 333 committed changes, no formal release tag, and the Phase 0 spine closed. |
| [`8e4c0a5`](https://github.com/Dicklesworthstone/frankensim/commit/8e4c0a5c4f18aa7d0bd0add47e407b835c7a3b86) | Inception commit | 2026-07-05 | Initial FrankenSim plan. |

## 1. Project Foundation And Policy Spine

FrankenSim started as a plan-first repository and quickly became a real Rust
workspace with project-specific agent policy, Beads issue tracking, licensing,
README/illustration assets, DSR-first validation policy, and workspace policy
checks.

### Delivered capability

- Initial architecture plan and README explaining the deterministic,
  evidence-oriented simulation substrate.
- Beads tracker and `bv` workflow documentation for project workstreams.
- MIT license with the Anthropic/OpenAI rider.
- Rust workspace scaffold with flat `fs-*` crates.
- `xtask` policy gates for layer direction, dependency policy, contracts,
  unsafe capsules, and constellation locks.
- DSR documented as the preferred validation lane over GitHub Actions.

### Closed workstreams

- [`frankensim-epic-foundations-huq.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - workspace scaffold, layer enforcement, lint/format rails.
- [`frankensim-epic-foundations-huq.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - pinned Franken constellation integration.
- [`frankensim-epic-foundations-huq.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - CI/Gauntlet gate policy.

### Representative commits

- [`8e4c0a5`](https://github.com/Dicklesworthstone/frankensim/commit/8e4c0a5c4f18aa7d0bd0add47e407b835c7a3b86) - initial FrankenSim plan.
- [`48e45c7`](https://github.com/Dicklesworthstone/frankensim/commit/48e45c7e2965db98c29e85b7bf06cf2f021eb22e) - initialize Beads project tracker.
- [`7554599`](https://github.com/Dicklesworthstone/frankensim/commit/755459986a056368685250c932407da019af7a8b) - add the MIT license with AI rider.
- [`a7e4d54`](https://github.com/Dicklesworthstone/frankensim/commit/a7e4d54d904d131850ef238f4b8e32b366c60736) - add the initial Rust workspace scaffold.
- [`4b31ce8`](https://github.com/Dicklesworthstone/frankensim/commit/4b31ce842848f40c9548a21a6513bbc050f38aa6) - document DSR as the primary CI runner.

## 2. Substrate, Execution, Determinism, And Performance

The first implementation wave established the lower layers that later crates
depend on: hardware capability discovery, explicit execution contexts,
deterministic reductions, aligned allocation, tile identity, SIMD capsules,
autotuning, and roofline measurement.

### Delivered capability

- `fs-substrate` capability probes, machine fingerprints, tiled fields, Morton
  identities, bandwidth probes, and CCD-aware topology helpers.
- `fs-simd` scalar and capsule-backed SIMD facades.
- `fs-alloc` scoped arenas, aligned allocation, hugepage policy, arena pools,
  and site tracking.
- `fs-exec` execution contexts, cancellation gates, tile pools, deterministic
  reductions, speculative races, resumable solver state, and autotuner hooks.
- `fs-roofline` machine-axis probing, kernel registry, ledgered measurements,
  and staleness checks.

### Closed workstreams

- [`frankensim-epic-substrate-wf9.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - substrate capability probes and machine fingerprints.
- [`frankensim-epic-substrate-wf9.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - SIMD facades and capsules.
- [`frankensim-epic-substrate-wf9.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - scoped arenas and allocation pools.
- [`frankensim-epic-substrate-wf9.7`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - two-lane executor and `Cx` contract.
- [`frankensim-epic-perf-fz2.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - roofline harness.

### Representative commits

- [`47c7719`](https://github.com/Dicklesworthstone/frankensim/commit/47c771994b3294d1dda1949640e4c46fc920203a) - implement `fs-substrate` capability probes.
- [`741979d`](https://github.com/Dicklesworthstone/frankensim/commit/741979d7cc8f11b9ca1e0f556d28fc7095476ca0) - implement `fs-simd` scalar and capsule tiers.
- [`59b85cb`](https://github.com/Dicklesworthstone/frankensim/commit/59b85cb47f69f2fffbf2258ff119288de05c58a8) - implement `fs-alloc` scope arenas and pools.
- [`39bd2f8`](https://github.com/Dicklesworthstone/frankensim/commit/39bd2f8e686cb97623d322fd0bd8ab96e2e8db80) - add `fs-exec` tile context and pool modules.
- [`05e3c3d`](https://github.com/Dicklesworthstone/frankensim/commit/05e3c3d6554ca094f7a8d526834f0142107ff4fb) - add deterministic reduction support.

## 3. Bedrock Numerics And Certified Arithmetic

The BEDROCK layer filled in deterministic math, random streams, dense and sparse
linear algebra, FFTs, interval/affine/Taylor models, Chebyshev tools,
automatic differentiation, e-process inference, and mixed-precision ladders.

### Delivered capability

- `fs-math` deterministic elementary functions, strict complex arithmetic, EFT,
  and double-double helpers.
- `fs-rand` logical Philox streams, deterministic distributions, QMC/Sobol
  components, and replayable sampling.
- `fs-la` GEMM/factorization kernels, eigensolver scaffolding, mixed-precision
  refinement ladders, randomized NLA, and batched small-dense kernels.
- `fs-sparse` deterministic COO/CSR/BSR/SELL assembly and sparse operations.
- `fs-fft` FFT/DCT transform kernels.
- `fs-ivl` intervals, affine arithmetic, exact predicates, Taylor models, and
  Newton/Krawczyk certification helpers.
- `fs-cheb` Chebyshev function objects and Orr-Sommerfeld spectral probes.
- `fs-ad` forward duals and adjoint infrastructure.
- `fs-eproc` anytime-valid e-process machinery.

### Closed workstreams

- [`frankensim-epic-bedrock-6ys.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - logical Philox streams.
- [`frankensim-epic-bedrock-6ys.3`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - GEMM and dense kernels.
- [`frankensim-epic-bedrock-6ys.9`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - sparse formats and deterministic assembly.
- [`frankensim-epic-bedrock-6ys.12`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - interval and affine arithmetic.
- [`frankensim-epic-bedrock-6ys.13`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - Taylor models and certified roots.
- [`frankensim-epic-ascent-7tv.6`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - e-process inference core.

### Representative commits

- [`089cc72`](https://github.com/Dicklesworthstone/frankensim/commit/089cc72cd6d00db69e272fcfae13af42b1629f19) - implement logical Philox streams.
- [`7456302`](https://github.com/Dicklesworthstone/frankensim/commit/7456302d9ae1b6aaa96be2f7f04a8aeb8a12209f) - add FFT correctness-first transform core.
- [`44a883b`](https://github.com/Dicklesworthstone/frankensim/commit/44a883b17167dc0f848b13710208f9a606c35d67) - add deterministic CSR core.
- [`e7dc872`](https://github.com/Dicklesworthstone/frankensim/commit/e7dc872f211fa66bc3d41969ebe21c4c886dc97b) - add interval arithmetic core.
- [`a433365`](https://github.com/Dicklesworthstone/frankensim/commit/a433365ab46ce18726c20cb80ff25f527507e432) - add GEMM and factorization core.
- [`a8f88f8`](https://github.com/Dicklesworthstone/frankensim/commit/a8f88f82c3aaebc0864926e9b6125d7d27f1c14c) - add evidence and model-form certification core.

## 4. Geometry, Representation, Meshing, And Shape Control

The MORPH layer became a working representation stack rather than a plan:
regions/charts, SDFs, meshes, F-reps, NURBS, voxel fields, exact topology,
representation routing, geometric constraints, IO quarantine, and
parameterization levers all landed as crate-level surfaces with contracts.

### Delivered capability

- `fs-geom` chart/region contracts, conversion records, representation router,
  semantic diff, sheaf watertightness certificates, and sheaf repair.
- `fs-rep-sdf`, `fs-rep-mesh`, `fs-rep-frep`, `fs-rep-nurbs`, and
  `fs-rep-voxel` representation crates.
- `fs-mesh` Delaunay/refinement/remeshing scaffolding.
- `fs-xform` FFD, RBF, level-set, density, and composition parameterizations
  with Jacobian actions and foldover checks.
- `fs-topo` validity and topology certificates.
- `fs-query` geometry interrogation layer and `fs-geocon` geometric constraint
  primitives.
- `fs-io` import/export with quarantine receipts.

### Closed workstreams

- [`frankensim-epic-morph-wqd.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - region/chart core.
- [`frankensim-epic-morph-wqd.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - SDF representation crate.
- [`frankensim-epic-morph-wqd.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - mesh representation crate.
- [`frankensim-epic-morph-wqd.5`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - NURBS spline algebra and trims.
- [`frankensim-epic-morph-wqd.19`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - design parameterizations.
- [`frankensim-epic-morph-wqd.23`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - topology certificates.
- [`frankensim-epic-morph-wqd.25`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - import/export quarantine.

### Representative commits

- [`397e325`](https://github.com/Dicklesworthstone/frankensim/commit/397e3255e28599bbd1ebc99dee9fd12923b26837) - add `fs-geom` region and chart crate.
- [`ebf97fb`](https://github.com/Dicklesworthstone/frankensim/commit/ebf97fb63dc4b3c921b6b08e2d7db9f8ad0c1834) - add SDF representation crate.
- [`124ed81`](https://github.com/Dicklesworthstone/frankensim/commit/124ed81ceb7a8b89283d504177b9c5739e997208) - add mesh representation crate.
- [`a295b6b`](https://github.com/Dicklesworthstone/frankensim/commit/a295b6b3f6a380e79dd4cf4ec4f9fb6e4a47b4df) - add F-rep CSG DAGs and evaluators.
- [`4a17114`](https://github.com/Dicklesworthstone/frankensim/commit/4a17114c15002faf5d400706ef8971187da26614) - add design parameterizations.
- [`3948a82`](https://github.com/Dicklesworthstone/frankensim/commit/3948a82fb0793064c3428052335c59a46d70f8fe) - add validity and topology certificates.
- [`f8a3cf7`](https://github.com/Dicklesworthstone/frankensim/commit/f8a3cf7d60c56afdc121f53f85a9cbbb9c10d9eb) - implement NURBS algebra, trims, and honest Boolean policy.
- [`3432e6e`](https://github.com/Dicklesworthstone/frankensim/commit/3432e6e6d5f6a847cab52d00eaa47afff8eba2b0) - implement quarantined import/export.

## 5. Physics, Operators, Solvers, And Time Integration

The FLUX layer gained typed scenario/regime/material foundations, exterior
calculus infrastructure, operator generation, time integration, solver
state/reporting, p-multigrid, and mixed-precision Krylov refinement.

### Delivered capability

- `fs-scenario` typed boundary/load-case algebra.
- `fs-regime` Buckingham-Pi scaling, nondimensionalization, and admission
  predicates.
- `fs-material` constitutive kernels with consistent tangents.
- `fs-feec` exterior calculus complexes and high-order tensor/simplex families.
- `fs-opdsl` typed operator IR that generates primal/JVP/VJP/adjoint/DWR hooks.
- `fs-time` structure-preserving and resumable integrator batteries.
- `fs-solver` Krylov solvers, p-multigrid preconditioning, structured stall
  diagnosis, and mixed-precision refinement.
- `fs-iface` coupling-graph static checker.

### Closed workstreams

- [`frankensim-epic-flux-tfz.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - scenario algebra.
- [`frankensim-epic-flux-tfz.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - material kernels.
- [`frankensim-epic-flux-tfz.3`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - regime/admission gates.
- [`frankensim-epic-flux-tfz.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - operator DSL/compiler.
- [`frankensim-epic-flux-tfz.5`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - FEEC complexes.
- [`frankensim-epic-flux-tfz.6`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - high-order FEEC.
- [`frankensim-epic-flux-tfz.12`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - time integrators.

### Representative commits

- [`88c1b46`](https://github.com/Dicklesworthstone/frankensim/commit/88c1b46c85f0895f25cc84f8f85a60e1e28c95b5) - implement regime/admission gates.
- [`052d38b`](https://github.com/Dicklesworthstone/frankensim/commit/052d38bc5524e47f8ec931fcb74f9b852bd937e8) - implement material kernels.
- [`a968527`](https://github.com/Dicklesworthstone/frankensim/commit/a96852737b948c61233e6f8c6541233d3c9d10e2) - complete FEEC exterior calculus contract.
- [`89c1f82`](https://github.com/Dicklesworthstone/frankensim/commit/89c1f827e72a6e22a2a19b3d9a9b3737c5bf224c) - add typed operator IR compiler.
- [`de11ad1`](https://github.com/Dicklesworthstone/frankensim/commit/de11ad1caf20be601d165916685903ba45d0dfe8) - lock time-integrator evidence.
- [`7aec6a5`](https://github.com/Dicklesworthstone/frankensim/commit/7aec6a5b1d603d8fe96040dabdb752fb0baac976) - add Krylov and p-multigrid battery.
- [`43d52f2`](https://github.com/Dicklesworthstone/frankensim/commit/43d52f2ed960ede2067eec353359063c86d9dbb3) - add mixed-precision Krylov refinement.

## 6. ASCENT Optimization And Constraint Machinery

The optimization layer moved from problem sketches to typed optimization data,
constraints, geometric constraints, derivative-free engines, Goodhart checks,
and problem serialization.

### Delivered capability

- `fs-opt` typed objective/constraint graphs, manifold variables, PDE and
  stochastic nodes, differentiability routing, canonical serialization, and
  the Goodhart guard.
- `fs-constraint` typed hard/soft/chance/robust/cert/fabrication/code
  constraints, interval certificates, unsat cores, and ranked repairs.
- `fs-dfo` CMA-ES/IGO core with deterministic restart behavior and DFO
  no-claim boundaries.
- `fs-geocon` geometric constraint primitives for min-thickness, draft,
  symmetry, envelopes, and volume/mass.

### Closed workstreams

- [`frankensim-epic-ascent-7tv.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - optimization problem IR.
- [`frankensim-epic-ascent-7tv.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - constraint calculus.
- [`frankensim-epic-ascent-7tv.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - CMA-ES/DFO engines.
- [`frankensim-epic-epistype-qmao.5`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - Goodhart guard.
- [`frankensim-epic-morph-wqd.21`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - geometric constraint primitives.

### Representative commits

- [`5b86049`](https://github.com/Dicklesworthstone/frankensim/commit/5b860492632414045326fa7a260701eae515cc16) - scaffold `fs-opt`.
- [`64dbee1`](https://github.com/Dicklesworthstone/frankensim/commit/64dbee1cf45d28669e36e9eb658e08dab5827f78) - add the typed constraint calculus.
- [`f536e98`](https://github.com/Dicklesworthstone/frankensim/commit/f536e98f8254a443988830b2365e6a29d865dd3a) - add the Goodhart guard.
- [`b5f1210`](https://github.com/Dicklesworthstone/frankensim/commit/b5f12106da8c28f0a97e12ba2df968790537c8ac) - harden DFO/CMA behavior.
- [`19a4193`](https://github.com/Dicklesworthstone/frankensim/commit/19a4193b5356f8c910f3b3166ae52cdc8a068584) - add geometric constraint primitives.

## 7. HELM, Ledger, IR, Sessions, And Evidence

The orchestration layer gained evidence-carrying values, ledger schema,
time-travel queries, FrankenScript IR, admission checks, sessions/governor
support, planning models, image artifacts, and a narrow PV vertical skeleton.

### Delivered capability

- `fs-evidence` `Evidence<T>`/`Certified<T>` wrappers, model cards,
  discrepancy/bracketing records, falsifier pairing, and evidence gates.
- `fs-ledger` content-addressed artifacts, operations, event streams, tune rows,
  lineage, time travel, physics-VCS primitives, tombstone memory, and telemetry.
- `fs-ir` typed FrankenScript AST, JSON/s-expression syntax, admission checks,
  declarative query IR, and structured errors.
- `fs-plan` cost/error ledgers and planning models.
- `fs-session` capability tokens, governor checks, idempotency, and dry-run
  estimates.
- `fs-vskeleton` a photovoltaic vertical skeleton tying geometry, PDE,
  objective, adjoint, optimization, and ledger concepts together.
- `fs-img` deterministic PNG/EXR-style artifact plumbing and bias-labeled
  denoising.

### Closed workstreams

- [`frankensim-epic-helm-gp3.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - ledger core.
- [`frankensim-epic-helm-gp3.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - evidence wrappers.
- [`frankensim-epic-helm-gp3.3`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - ledger time travel.
- [`frankensim-epic-helm-gp3.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - IR core.
- [`frankensim-epic-helm-gp3.5`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - IR admission.
- [`frankensim-epic-helm-gp3.7`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - session/governor.
- [`frankensim-epic-foundations-huq.8`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - PV vertical skeleton milestone.

### Representative commits

- [`1e20bda`](https://github.com/Dicklesworthstone/frankensim/commit/1e20bdac6b18431dae1507c874721709cefc1fee) - add Design Ledger v0.
- [`a8f88f8`](https://github.com/Dicklesworthstone/frankensim/commit/a8f88f82c3aaebc0864926e9b6125d7d27f1c14c) - add evidence and certification core.
- [`33c1ed4`](https://github.com/Dicklesworthstone/frankensim/commit/33c1ed4c6ba49af09740e4b3c1cc9ec57449a30d) - complete ledger time travel.
- [`21369ba`](https://github.com/Dicklesworthstone/frankensim/commit/21369ba96892ba136bf8630a221005107f698be9) - add FrankenScript typed AST.
- [`df6e8dc`](https://github.com/Dicklesworthstone/frankensim/commit/df6e8dc93478dcf151de77b773ef1b3fd3429763) - implement IR static admission.
- [`c8e5ca9`](https://github.com/Dicklesworthstone/frankensim/commit/c8e5ca9523f7586d219799cbefbaf2d4e1b9dbfe) - implement session governor.
- [`8f71102`](https://github.com/Dicklesworthstone/frankensim/commit/8f71102ca9f82c68d69848dc5d9d493e164c435d) - complete `fs-img`.

## 8. Addendum Flywheel, Certified Speculation, And Governance

The addendum wave added a machine-readable governance layer and a speculative
execution/evidence flywheel: three-color epistemic schema, falsifier gates,
incremental recompute, physics VCS, semantic diff, proposer zoo, speculation
economics, risk registers, contract registries, and an executable Phase 0 spine
gate.

### Delivered capability

- Three-color ledger schema for verified/validated/estimated evidence.
- Falsifier pairing, consequence-by-doubt budgets, and no-falsifier-no-ship
  checks.
- Slack-bearing recompute store and tolerance-aware invalidation.
- Physics-VCS commit/checkout primitives and semantic diff.
- Certified speculation verifier, proposer zoo, and accept/reject economics.
- `fs-spececo`, `fs-verify`, `fs-probe`, `fs-ladder`, `fs-recompute`,
  `fs-govern`, and `fs-contract` addendum crates.
- Phase 0 spine gate: laundering refusal, falsifier pairing, tombstone protocol,
  interface-type checks, Goodhart guard, and budget-pie reporting.

### Closed workstreams

- [`frankensim-epic-epistype-qmao.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - three-color schema.
- [`frankensim-epic-epistype-qmao.4`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - falsifier pairing.
- [`frankensim-epic-flywheel-lmp4.1`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - certified speculation verifier.
- [`frankensim-epic-flywheel-lmp4.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - proposer zoo.
- [`frankensim-epic-flywheel-lmp4.3`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - accept/reject economics.
- [`frankensim-epic-flywheel-lmp4.6`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - recompute store.
- [`frankensim-epic-flywheel-lmp4.10`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - semantic diff.
- [`frankensim-epic-addendum-xpck.2`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - Phase 0 spine complete.
- [`frankensim-epic-flywheel-lmp4.14`](https://github.com/Dicklesworthstone/frankensim/blob/43d52f2ed960ede2067eec353359063c86d9dbb3/.beads/issues.jsonl) - assume-guarantee contracts.

### Representative commits

- [`e43e3b1`](https://github.com/Dicklesworthstone/frankensim/commit/e43e3b14c8db88bbc5263a98cfc9cfde19fcc2e3) - add the three-color epistemic schema and ledger gate.
- [`39fd1a5`](https://github.com/Dicklesworthstone/frankensim/commit/39fd1a5144ae211073fbf8100774f1b4366304b3) - add falsifier pairing.
- [`ea102b5`](https://github.com/Dicklesworthstone/frankensim/commit/ea102b596b725fe77808ee6a6c16473a8d2e5f11) - add the slack-bearing recompute store.
- [`772d975`](https://github.com/Dicklesworthstone/frankensim/commit/772d97508f1e19bf5d551c1147bd83b65f4e470f) - add physics-VCS base verbs.
- [`2f2fe56`](https://github.com/Dicklesworthstone/frankensim/commit/2f2fe56339aa811649d1ebd0702d0f1cf43e09b3) - add certified-speculation verifier v0.
- [`6c6f633`](https://github.com/Dicklesworthstone/frankensim/commit/6c6f6338e04fd412ba29c630f4d4bfd86a56d1d3) - add proposer zoo.
- [`92636d0`](https://github.com/Dicklesworthstone/frankensim/commit/92636d0bb5071a503ed872bb8337cd90b2676350) - land integrated speculation economics and ledger v3 telemetry.
- [`544eaee`](https://github.com/Dicklesworthstone/frankensim/commit/544eaee3c0fd27ab672c9f6cf36fbcbd7a99204a) - close the Phase 0 spine milestone.
- [`b33496e`](https://github.com/Dicklesworthstone/frankensim/commit/b33496e291b928f5113bd65c651b2d5b3b97b044) - add assume-guarantee component contracts.

## Current Non-Release Status

- No package or crate release has been tagged yet.
- The canonical project state is `main`, not a versioned artifact.
- The repository is actively changing; use crate `CONTRACT.md` files and Beads
  close reasons for detailed no-claim boundaries.
- The README describes the implemented workspace; the long-form plan remains
  `COMPREHENSIVE_PLAN_FOR_FRANKENSIM.md`.
- Uncommitted work in the local working tree is not covered by this changelog.

## Notes For Agents

- Start with the version timeline for chronology.
- Use thematic sections for architectural orientation.
- Use commit links for implementation evidence and Beads links for intent and
  acceptance criteria.
- Do not cite GitHub Releases for this repo until a real release exists.
- Treat DSR as the project validation source of truth unless `AGENTS.md`
  changes.
