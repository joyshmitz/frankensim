# Changelog Research

## Scope

- Repo: `Dicklesworthstone/frankensim`
- Requested task: update the changelog using `changelog-md-workmanship`.
- Scope window: project inception on 2026-07-05 through `main` at
  `43d52f2ed960ede2067eec353359063c86d9dbb3` on 2026-07-07.
- Working-tree policy: ignore uncommitted in-progress `.beads`, `fs-cutfem`,
  `fs-rep-nurbs`, and `.wrangler/cache` changes. This changelog covers committed
  history only.

## Evidence Sources

- Git history:
  - `git rev-list --count HEAD` -> 333 commits.
  - `git log --reverse --no-merges --pretty=format:'%h %ad %s' --date=short`.
  - `git log --all --no-merges --pretty=format:'%H %h %ad %s' --date=short`.
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

## Version Spine

| Node | Kind | Date | Notes |
|------|------|------|-------|
| `8e4c0a5` | Inception commit | 2026-07-05 | Initial FrankenSim plan. |
| `43d52f2` | Mainline snapshot | 2026-07-07 | Latest committed state researched for this changelog. |

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
- Uncommitted local `fs-cutfem`/`fs-rep-nurbs` work should only be added after it
  is committed and proven.
