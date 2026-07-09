# Changelog Research

## Scope

- Repo: `Dicklesworthstone/frankensim`
- Requested task: update the changelog using `changelog-md-workmanship`.
- Scope window: project inception on 2026-07-05 through
  `main@6725739f42b878b310e7e7e318fb8c980cef71f3` on 2026-07-09.
- Public remote state when researched: local `HEAD` resolved to
  `6725739f42b878b310e7e7e318fb8c980cef71f3`; `origin/main` resolved to
  `6725739f42b878b310e7e7e318fb8c980cef71f3`.
- Working-tree policy: the changelog covers committed history only. Local
  uncommitted edits and scratch files were not used as changelog evidence,
  including `.beads/issues.jsonl`, `crates/fs-wasm/src/flagships.rs`, Wrangler
  cache files, and the untracked FlowCert/Rand probe files.

## Evidence Sources

- Git history:
  - `git rev-list --count HEAD` -> 738 commits.
  - `git rev-list --count origin/main` -> 738 commits.
  - `git rev-list --count 43d52f2..HEAD` -> 405 commits since the prior
    changelog baseline.
  - `git rev-list --count fb08842..HEAD` -> 116 commits since the previous
    changelog pass endpoint.
  - `git rev-list --count 319cb64..HEAD` -> 105 commits since the previous
    changelog endpoint.
  - `git rev-list --count d5873bf..HEAD` -> 57 commits since the 2026-07-09
    flagship expansion checkpoint.
  - `git rev-list --count e08e302..HEAD` -> 42 commits since the previous
    changelog endpoint.
  - `git log --reverse --no-merges --pretty=format:'%h %ad %s' --date=short`.
  - `git log --all --no-merges --pretty=format:'%H %h %ad %s' --date=short`.
  - `git diff --stat --compact-summary 43d52f2..HEAD` -> 597 files changed,
    107,641 insertions, 621 deletions.
  - `git diff --stat --compact-summary fb08842..HEAD` -> 210 files changed,
    27,629 insertions, 525 deletions.
  - `git diff --stat --compact-summary 319cb64..HEAD` -> 177 files changed,
    22,313 insertions, 543 deletions.
  - `git diff --stat --compact-summary d5873bf..HEAD` -> 91 files changed,
    7,988 insertions, 333 deletions.
  - `git diff --stat --compact-summary e08e302..HEAD` -> 77 files changed,
    6,195 insertions, 219 deletions.
- Version metadata:
  - `git for-each-ref refs/tags ...` -> no tags.
  - `gh release list --limit 100` -> no GitHub Releases.
- Tracker:
  - `git show HEAD:.beads/issues.jsonl | jq ...` for closed workstreams and
    close reasons at committed `HEAD`.
- Project docs:
  - `README.md`.
  - `AGENTS.md`.
  - crate-level `CONTRACT.md` files referenced through README/workstream scope.
  - Workspace count checks on 2026-07-09:
    - `find crates -mindepth 1 -maxdepth 1 -type d -name 'fs-*' | wc -l`
      -> 125 crates.
    - `find crates -mindepth 2 -maxdepth 2 -name CONTRACT.md | wc -l`
      -> 125 contracts.
    - `git ls-files 'crates/fs-*/tests/*.rs' | wc -l` -> 222 tracked test files.

## Version Spine

| Node | Kind | Date | Notes |
|------|------|------|-------|
| `8e4c0a5` | Inception commit | 2026-07-05 | Initial FrankenSim plan. |
| `43d52f2` | Prior changelog baseline | 2026-07-07 | Latest state covered by the first changelog reconstruction. |
| `941a67e` | Earlier public checkpoint | 2026-07-08 | Value-of-information query planning; 621 commits. |
| `fb08842` | Prior changelog checkpoint | 2026-07-08 | Proof-robust, schedule, and flutter e2e campaigns; 622 commits. |
| `319cb64` | Prior public mainline snapshot | 2026-07-08 | Previous changelog endpoint; 633 commits. |
| `d5873bf` | Prior public mainline snapshot | 2026-07-09 | Ornithoid flagship contract, CutFEM-octree topopt lane, sparse-GP/MLMC/MOO decision loops, browser campaign tiers, proof-hardening fixes; 681 commits. |
| `9dc7417` | Public mainline snapshot | 2026-07-09 | Exact e2e certificate payloads, topopt marquee evidence hardening, and in-progress flagship replay-suite scaffold; 684 commits. |
| `1fe4ef5` | Public mainline snapshot | 2026-07-09 | Flagship replay-suite lint cleanup plus Beads tracker state for the fs-mesh v2 follow-up; 686 commits. |
| `20e9825` | Public mainline snapshot | 2026-07-09 | Frozen-golden flagship e2e suite; 687 commits. |
| `438128d` | Public mainline snapshot | 2026-07-09 | Mesh hull-facet encroachment protection and faster topology-optimization marquee witnesses; 688 commits. |
| `70e1d24` | Public mainline snapshot | 2026-07-09 | Boundary-splitting semantics and topopt move-application cleanup; 692 commits. |
| `d93ca59` | Public mainline snapshot | 2026-07-09 | Gated SME2 exploratory capsule and unsafe registration; 695 commits. |
| `e08e302` | Public mainline snapshot | 2026-07-09 | SME2 exploratory battery; 696 commits. |
| `6725739` | Public mainline snapshot | 2026-07-09 | Live browser flagship pipelines, mesh v2/v3 closure, sparse roofline/NUMA lanes, rand/FFT perf work, fail-closed IO/risk/probe hardening; 738 commits. |

No tags or GitHub Releases existed when researched.

## Coverage Ledger

| Chunk | Range | Status | Major themes |
|-------|-------|--------|--------------|
| 01 | 2026-07-05 | distilled | Plan, README, Beads, license, agent workflow. |
| 02 | 2026-07-06 foundations | distilled | Workspace scaffold, constellation, DSR, substrate, execution, bedrock numerics, evidence, ledger, geometry core. |
| 03 | 2026-07-06 representations | distilled | SDF, mesh, F-rep, meshing, transforms, Chebyshev, planning, optimization/image scaffolds. |
| 04 | 2026-07-07 core expansion | distilled | Constraint, GA, scenario/regime/material, query/time, FEEC, TileLang, topology, NURBS, operator DSL. |
| 05 | 2026-07-07 addendum flywheel | distilled | Three-color schema, falsifiers, recompute, physics VCS, semantic diff, speculation verifier/proposer/economics, governance, Phase 0 spine, assume-guarantee contracts. |
| 06 | latest solver slice | distilled | `fs-solver` mixed-precision Krylov refinement. |
| 07 | 43d52f2..15eb757 | distilled | NURBS/SDF conversion, CutFEM, matrix-free adjoint scaffold, physics-VCS bisect, `fs-solid` elasticity. |
| 08 | 15eb757..3f4e343 | distilled | Solid stability/structural elements, FEEC cohomology, ledger-DAG transposition, sheaf merge, topopt/topols, UQ, BO, anytime planner, whole-loop flywheel gate. |
| 09 | 3f4e343..6df2c03 | distilled | FMM/BEM, LBM, IGA, surrogate ladders, truss, neural reps, rendering, domain decomposition, Navier-Stokes, marquee runner. |
| 10 | 6df2c03..7049ca3 | distilled | LBM extensions, seismic frame, mesh coloring/recovery, contact, topology persistence, conformal hardening, lattice, e-racing, time slabs, Cheb variants, Payne-Hanek. |
| 11 | 7049ca3..fb08842 | distilled | Vortex-thruster QD campaign, DRO oracle, value-of-information queries, and three certified e2e capstones. |
| 12 | fb08842..319cb64 | distilled | Neural-shape and grammar campaigns, SensorForge, vessel flagship, metamaterial/truss/AnytimeBO/FlowCert e2e crates, inverse-trig AD, `fs-ad` bridge/Revolve/IFT integrations, vertex-patch Schwarz p-MG smoothing. |
| 13 | 319cb64..e08e302 | distilled | Self-knowledge and flywheel gates, Stokes block preconditioners, NSGA-III/MOEA/D/steering, sparse GP, adaptive MLMC/chance constraints, constrained polish engines, proposal-only generation, browser campaign tiers, differentiable rendering, ornithoid and CutFEM-octree flagships, exact e2e certificate payloads, topopt evidence hardening, frozen-golden flagship replay suite, mesh refinement protection, SME2 exploratory capsule, tracker state, proof-hygiene fixes. |
| 14 | e08e302..6725739 | distilled | Live `fs-wasm` flagship browser pipelines, `fs-mesh` v2/v3 closure, hex-dominant meshing scaffold, compact sparse roofline/NUMA work, deterministic rand/FFT perf lanes, PLY integer validation, non-finite risk/probe hardening, and packaging panic-surface cleanup. |

## Representative Commit Clusters

- Foundation:
  - `8e4c0a5` initial plan.
  - `a7e4d54` Rust workspace scaffold.
  - `4b31ce8` DSR-first policy.
- Substrate/runtime:
  - `47c7719` substrate probes.
  - `741979d` SIMD tiers.
  - `59b85cb` arenas and pools.
  - `39bd2f8` execution context and tile pools.
- Bedrock:
  - `089cc72` Philox streams.
  - `7456302` FFT core.
  - `44a883b` deterministic CSR core.
  - `e7dc872` interval arithmetic.
  - `a433365` GEMM/factorization core.
- Morph:
  - `397e325` region/chart crate.
  - `ebf97fb` SDF crate.
  - `124ed81` mesh crate.
  - `a295b6b` F-rep DAGs.
  - `4a17114` shape parameterizations.
  - `3948a82` topology certificates.
  - `f8a3cf7` NURBS algebra and trims.
- Flux:
  - `a968527` FEEC exterior calculus.
  - `89c1f82` operator DSL compiler.
  - `7aec6a5` solver battery.
  - `43d52f2` mixed-precision Krylov refinement.
- Geometry/solids/topology:
  - `279d611` certified NURBS-to-SDF conversion.
  - `c9e1a4b` SDF-to-NURBS refit.
  - `f781085` CutFEM on SDFs.
  - `15eb757` solid elasticity core.
  - `e396160` solid stability and Koiter scaffolding.
  - `bcc71d4` structural elements.
  - `4b4c24e` level-set topology optimization.
- Differentiation/planning/flywheel:
  - `41cadcc` ledger-DAG transposition.
  - `3fab970` gradient certificates.
  - `9ee7227` fidelity-ladder planner.
  - `1ade44a` anytime/refusal semantics.
  - `9ab427e` whole-loop flywheel harness.
  - `3f9a714` Phase 2 leverage gate.
  - `941a67e` value-of-information query ranking.
- Physics/numerics:
  - `7d95552` FEEC cohomology.
  - `57d775e` BDDC domain decomposition.
  - `8f031ad` pressure-robust Navier-Stokes.
  - `8c5b0e5` FMM and BEM.
  - `ded5b78` LBM extensions.
  - `7049ca3` Payne-Hanek trig reduction.
- Optimization/e2e:
  - `a76cbea` value-of-information crate.
  - `d947001` Wasserstein DRO oracle.
  - `c0f1a5c` SOS proof-carrying optimization.
  - `3c56418` quality-diversity archives.
  - `6df2c03` marquee study runner.
  - `8d27622` seismic frame flagship.
  - `dc1bf7f` certified vortex-thruster QD campaign.
  - `fb08842` three certified e2e campaigns.
  - `5cbdd90` neural-shape and grammar e2e campaigns.
  - `94404c4` SensorForge OED campaign.
  - `b95e00f` laminar-pour vessel flagship.
  - `8028bbc` metamaterial stiffness-density frontier.
  - `4c573d6` truss critical-load-path campaign.
  - `76e9c89` AnytimeBO.
  - `3eb480c` FlowCert CFD credibility map.
- Latest numerics/solver:
  - `922c835` deterministic inverse trig and AD `Real` operations.
  - `7575cdd` `fs-ad` bridge, Revolve, spill, and IFT integrations.
  - `319cb64` vertex-patch Schwarz p-MG smoothing.
- 2026-07-09 mainline expansion:
  - `7624964` self-knowledge e2e battery.
  - `3dcbfaa` Stokes block preconditioners and PMINRES.
  - `8c20bab` NSGA-III reference directions.
  - `61e9348` edge-aware differentiable rendering.
  - `fe8a2c2` `fs-plan` budget allocator.
  - `6d074b8` seismic UQ, anytime stopping, and adaptive MLMC.
  - `e2b3dff` sparse Gaussian-process lane.
  - `73c1c6b` proposal-only `fs-gen`.
  - `51adc98` steering and chance constraints.
  - `2f29772` constrained polish engines and Problem-IR runner.
  - `2199248` certified `fs-wasm` campaign tier.
  - `ffcb4a9` `fs-ornith` flagship crate.
  - `8c06c93` CutFEM-octree topology marquee lane.
  - `d5873bf` `fs-ornith` contract and flagship bead close.
  - `fbba704` flutter and neuroshape exact certificate payloads.
  - `1092e94` CutFEM-octree marquee evidence hardening.
  - `9dc7417` `fs-flagship-e2e` staged replay-suite scaffold.
  - `1337058` `fs-flagship-e2e` replay-suite lint posture.
  - `1fe4ef5` Beads tracker state for `frankensim-uee3`.
  - `20e9825` frozen-golden flagship e2e suite.
  - `438128d` hull-facet encroachment protection and topopt marquee witness tuning.
  - `70e1d24` boundary splitting and topopt move-application cleanup.
  - `a8e98b2` hull encroachment regression bound.
  - `d93ca59` gated SME2 exploratory capsule.
  - `e08e302` SME2 exploratory battery.
- Post-SME2 follow-up:
  - `e8c496e` CutFEM-octree topology marquee zero-remeshing contract.
  - `29fb1ea` SME2 streaming-mode GEMM prototype contract.
  - `f759916` deterministic ziggurat normal fast path.
  - `cb6dbde` Philox statistical stream battery.
  - `8254164` bitwise bulk Philox fills.
  - `648a891` c2r inverse FFT and 2D/3D pencil transforms.
  - `9a6d303` conforming interior facet recovery.
  - `6510b02` non-convex recovery facet triangulation.
  - `fa3d93d` 10-million-point `fs-mesh` perf lane closeout.
  - `151a643` hex-dominant meshing module.
  - `0cd9de2` compact/sharded SpMV and sparse roofline lane.
  - `dccc078` NUMA first-touch and real shared STREAM slices.
  - `b4f849f` invalid topology-optimization density-shape rejection.
  - `220d28c` fractional PLY face-list rejection.
  - `3842369` non-convex recovery and boundary-layer gates.
  - `4b77149` measured `fs-mesh` v3 boundary-layer decision.
  - `ff9ef32` CVaR/objective non-finite rejection.
  - `38f871f` non-finite probe budget rejection.
  - `fdffa73` provenance in evidence package roots.
  - `91478e2` package string-write panic cleanup.
  - `462f0f6` panic-arm removal from `fs-io` conformance checks.
  - `6725739` live browser flagship pipelines in `fs-wasm`.
- Addendum:
  - `e43e3b1` three-color schema.
  - `39fd1a5` falsifier pairing.
  - `ea102b5` recompute store.
  - `772d975` physics VCS.
  - `2f2fe56` certified-speculation verifier.
  - `92636d0` speculation economics and ledger v3 telemetry.
  - `544eaee` Phase 0 spine gate.
  - `b33496e` assume-guarantee contracts.

## Open Questions

- The repo has no formal versioning scheme yet. Future changelog updates should
  split entries by tag/release once the first release is created.
- Links to Beads currently target the committed `.beads/issues.jsonl` file as a
  durable tracker source; a future issue viewer could provide more precise
  per-record URLs.
- Uncommitted local work should only be added after it is committed and proven.
