//! CutFEM-octree marquee conformance (the b7d0 bead; runs under
//! `cutfem-marquee`). Acceptance: the objective improves at fixed
//! volume with ZERO mesh rebuilds (the marquee property — asserted
//! from the run log); topology EVOLVES (void components change or
//! boundary-side nodes flip) without any remesh; the DWR cut-band gate
//! concentrates refinement near the design boundary; the benchmark envelope is
//! recorded as evidence with per-iteration timing (the
//! interactive-cadence measurement, debug-build labeled); the
//! medial-axis thickness oracle audits the optimized geometry's
//! minimum length scale.
#![cfg(feature = "cutfem-marquee")]

use fs_cutfem::{CellKey, Circle, CutElasticity, CutFemError, CutSdf, Quadtree};
use fs_dwr::estimate_elasticity_compliance;
use fs_ivl::Interval;
use fs_material::IsotropicElastic;
use fs_topopt::marquee::{DensityDesign, refine_dwr_cut_band, run_marquee};
use std::collections::BTreeMap;

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-topopt/marquee\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

/// The volume-to-point start: a solid plate with two seeded cooling
/// voids (the classic heat-sink layout germ).
fn seeded_design() -> DensityDesign {
    let n = 17;
    let mut d = DensityDesign::uniform(n, 0.75);
    #[allow(clippy::cast_precision_loss)]
    let scale = (n - 1) as f64;
    for j in 0..n {
        for i in 0..n {
            let (x, y) = (i as f64 / scale, j as f64 / scale);
            for &(cx, cy) in &[(0.3, 0.5), (0.7, 0.5)] {
                if (x - cx).powi(2) + (y - cy).powi(2) < 0.012 {
                    d.rho[j * n + i] = 0.1;
                }
            }
        }
    }
    d
}

struct InfiniteHalo<'a>(&'a DensityDesign);

impl CutSdf for InfiniteHalo<'_> {
    fn value(&self, point: [f64; 2]) -> f64 {
        self.0.value(point)
    }

    fn gradient(&self, point: [f64; 2]) -> [f64; 2] {
        self.0.gradient(point)
    }

    fn enclose(&self, lo: [f64; 2], hi: [f64; 2]) -> Interval {
        if hi[0] - lo[0] > 0.25 || hi[1] - lo[1] > 0.25 {
            if lo[0] > 0.0 {
                Interval::WHOLE
            } else {
                Interval::new(-1.0, 1.0)
            }
        } else {
            self.0.enclose(lo, hi)
        }
    }
}

fn assert_grid_and_level_unchanged(
    grid: &Quadtree,
    leaves_before: &[CellKey],
    level: u32,
    expected_level: u32,
) {
    assert_eq!(grid.leaves().collect::<Vec<_>>(), leaves_before);
    assert_eq!(level, expected_level);
}

fn vector_dwr_problem<'a>(
    grid: &'a Quadtree,
    disk: &'a Circle,
    material: &'a IsotropicElastic,
) -> CutElasticity<'a> {
    CutElasticity {
        grid,
        sdf: disk,
        material,
        nitsche_beta: 100.0,
        ghost_gamma: 0.5,
        quad_depth: 3,
        clamp: None,
        boundary_traction: None,
        traction_free_interface: false,
        solver_tol: 1e-12,
        solver_max_iters: 60_000,
    }
}

#[test]
#[should_panic(expected = "density lattice needs at least 2 nodes per side")]
fn tm_000_rejects_degenerate_lattice() {
    let _ = DensityDesign::uniform(1, 0.5);
}

#[test]
#[should_panic(expected = "density lattice length must equal n*n")]
fn tm_000_rejects_manual_shape_mismatch() {
    let design = DensityDesign {
        n: 3,
        rho: vec![0.5; 8],
    };
    let _ = design.volume();
}

#[test]
fn tm_001_improves_with_zero_rebuilds() {
    // The marquee property is zero mesh rebuilds: the background
    // quadtree is built once, then the configured adaptation path may
    // refine it by octree splits. Split concentration is tm-003's
    // executable evidence.
    let report = run_marquee(seeded_design(), 4, 6, 6, true).expect("marquee runs");
    let first = report.iterations.first().expect("first").compliance;
    let last = report.iterations.last().expect("last").compliance;
    println!(
        "{{\"metric\":\"marquee-objective\",\"first\":{first:.5},\"last\":{last:.5},\
         \"total_splits\":{},\"total_rebuilds\":{}}}",
        report.total_splits, report.total_rebuilds
    );
    assert!(last < first, "compliance improves: {first:.5} -> {last:.5}");
    // THE MARQUEE PROPERTY: zero mesh rebuilds across the whole run;
    // every refinement event is an octree split.
    assert_eq!(report.total_rebuilds, 0, "no mesh rebuild events, ever");
    assert!(
        report.iterations.iter().all(|it| it.rebuilds == 0),
        "no per-iteration rebuilds either"
    );
    assert!(report.total_splits > 0, "refinement happened as splits");
    // Volume held.
    for it in &report.iterations {
        assert!(
            (it.volume - 0.75).abs() < 0.06,
            "solid fraction held at iter {}: {}",
            it.iter,
            it.volume
        );
    }
    verdict(
        "tm-001",
        "compliance improves at fixed volume with ZERO mesh rebuilds; adaptation \
         enters only as octree splits, straight from the run log",
    );
}

#[test]
fn tm_002_topology_evolves_without_remeshing() {
    // Start from a SOLID plate (one void region: the exterior ring of
    // clamped-low boundary values doesn't exist here — fully solid,
    // zero interior voids) and let the optimizer act: redistribution
    // plus the volume constraint must change the void-component count
    // at SOME point in the run — new topology, same background grid.
    let start = seeded_design();
    let report = run_marquee(start.clone(), 4, 6, 8, false).expect("runs");
    let void_counts: Vec<usize> = report.iterations.iter().map(|it| it.voids).collect();
    // The evolution witness: lattice nodes whose SIDE of the design
    // boundary flipped between start and finish (the void-count is a
    // coarser signal that only moves on merge/split events).
    let flips = start
        .rho
        .iter()
        .zip(&report.design.rho)
        .filter(|(a, b)| (**a > 0.5) != (**b > 0.5))
        .count();
    #[allow(clippy::cast_precision_loss)]
    let flip_frac = flips as f64 / start.rho.len() as f64;
    println!(
        "{{\"metric\":\"topology\",\"void_counts\":{void_counts:?},\
         \"boundary_flips\":{flips},\"flip_frac\":{flip_frac:.3}}}"
    );
    assert!(
        flips >= 3 || void_counts.windows(2).any(|w| w[0] != w[1]),
        "the design genuinely evolves: {flips} flips, voids {void_counts:?}"
    );
    assert_eq!(report.total_rebuilds, 0, "and still zero rebuilds");
    verdict(
        "tm-002",
        "the boundary-side witness or void-component count changes mid-run — topology \
         genuinely evolves on the same never-rebuilt background grid",
    );
}

#[test]
fn tm_003_dwr_splits_concentrate_at_the_boundary() {
    // The DWR cut-band mass gate must put its refinement where the
    // design boundary lives, not uniformly.
    let design = seeded_design();
    let report = run_marquee(design, 4, 6, 3, true).expect("runs");
    // Re-derive the split footprint from the final grid: leaves finer
    // than the base level must mostly sit in the design-boundary halo.
    assert!(report.total_splits >= 6, "enough splits to measure");
    let refined = report.refined_boundary_leaves + report.refined_off_boundary_leaves;
    assert!(refined > 0, "refined leaf footprint is recorded");
    #[allow(clippy::cast_precision_loss)]
    let boundary_frac = report.refined_boundary_leaves as f64 / refined as f64;
    println!(
        "{{\"metric\":\"dwr-boundary-concentration\",\"near\":{},\"far\":{},\
         \"near_frac\":{boundary_frac:.3},\"total_splits\":{}}}",
        report.refined_boundary_leaves, report.refined_off_boundary_leaves, report.total_splits
    );
    // The ghost-penalty contract (CutBandNotUniform) forces the
    // refined band to include the one-cell HALO around every cut cell
    // — the equal-level face-neighbor requirement — so the honest
    // concentration ceiling is band/(band+halo) ~ 2/3, not the 0.80 a
    // halo-free marker could reach.
    assert!(
        boundary_frac >= 0.60,
        "refined leaves concentrate near the design boundary: near {} far {}",
        report.refined_boundary_leaves,
        report.refined_off_boundary_leaves
    );
    verdict(
        "tm-003",
        "the DWR cut-band mass gate advances refinement and the final refined leaf \
         footprint is concentrated in the design-boundary halo",
    );
}

#[test]
fn tm_004_benchmark_envelope_and_cadence() {
    // THE GOLDEN ENVELOPE, recorded as evidence (heat-conduction
    // topopt class — the volume-to-point fixture; elasticity envelopes
    // ride CutFEM elasticity when it lands, per the CONTRACT):
    // at 75% solid, 6 iterations, the final compliance must sit inside
    // the recorded band. The band was measured from this fixture and
    // is intentionally loose enough to survive floating-point drift
    // but tight enough to catch regressions.
    let report = run_marquee(seeded_design(), 4, 5, 4, true).expect("runs");
    let last = report.iterations.last().expect("last").compliance;
    let envelope = (0.005, 0.035);
    println!(
        "{{\"metric\":\"envelope\",\"final\":{last:.5},\"band\":[{},{}],\
         \"timings_ms\":{:?}}}",
        envelope.0,
        envelope.1,
        report
            .iterations
            .iter()
            .map(|it| (it.wall_ms * 10.0).round() / 10.0)
            .collect::<Vec<_>>()
    );
    assert!(
        last > envelope.0 && last < envelope.1,
        "the converged compliance sits in the recorded envelope: {last:.5}"
    );
    // Interactive cadence: iterations don't blow up over the run
    // (splits add cost, but no remeshing spikes — the max/min ratio
    // stays bounded; debug-build measurement, labeled).
    let times: Vec<f64> = report.iterations.iter().map(|it| it.wall_ms).collect();
    let (lo, hi) = times
        .iter()
        .fold((f64::INFINITY, 0.0f64), |(l, h), &t| (l.min(t), h.max(t)));
    assert!(
        hi / lo.max(0.1) < 25.0,
        "per-iteration cost stays bounded (no remeshing cliffs): {times:?}"
    );
    verdict(
        "tm-004",
        "the converged compliance sits inside the ledgered golden envelope; \
         per-iteration wall times (debug label) show no remeshing cliffs",
    );
}

#[test]
fn tm_005_thickness_oracle_audits_the_result() {
    let report = run_marquee(seeded_design(), 4, 6, 6, false).expect("runs");
    let min_feature = report.design.min_feature_cells();
    println!(
        "{{\"metric\":\"thickness-oracle\",\"min_feature_cells\":{min_feature},\
         \"floor\":2}}"
    );
    // The optimized geometry keeps a resolvable minimum length scale:
    // the medial-axis-class audit (geometry layer auditing optimizer
    // output) must report at least the 2-cell floor.
    assert!(
        min_feature >= 2,
        "the thickness oracle certifies the length-scale floor: {min_feature}"
    );
    verdict(
        "tm-005",
        "the medial-axis thickness oracle reports the optimized design's minimum \
         feature at or above the 2-cell floor — geometry auditing optimization output",
    );
}

#[test]
fn tm_006_synthetic_indicators_drive_shared_band_policy_once() {
    // This is deliberately estimator-agnostic helper coverage. Vector
    // compliance DWR can supply the same CellKey map, but this test does not
    // claim a graded vector re-solve: it exercises one planning decision only.
    let design = seeded_design();
    let make_grid = || Quadtree::with_room(2, 4);
    let seed_grid = make_grid();
    let indicators: BTreeMap<_, _> = seed_grid
        .leaves()
        .map(|cell| {
            let (lo, hi) = seed_grid.rect(cell);
            let eta = if design.enclose(lo, hi).contains_zero() {
                1.0
            } else {
                0.01
            };
            (cell, eta)
        })
        .collect();

    let mut grid_a = make_grid();
    let mut grid_b = make_grid();
    let mut level_a = 2;
    let mut level_b = 2;
    let decision_a = refine_dwr_cut_band(&mut grid_a, &design, &indicators, &mut level_a, true)
        .expect("valid shared policy input");
    let decision_b = refine_dwr_cut_band(&mut grid_b, &design, &indicators, &mut level_b, true)
        .expect("valid shared policy input");
    let leaves_a: Vec<_> = grid_a.leaves().collect();
    let leaves_b: Vec<_> = grid_b.leaves().collect();

    assert_eq!(decision_a, decision_b, "policy evidence is deterministic");
    assert_eq!(
        leaves_a, leaves_b,
        "the structural split set is deterministic"
    );
    assert!(
        decision_a.advanced,
        "cut-band indicator mass passes the gate"
    );
    assert!(
        decision_a.splits > 0,
        "one planning step performs actual splits"
    );
    assert_eq!(decision_a.previous_level, 2);
    assert_eq!(decision_a.band_level, 3);

    let mut disabled_grid = make_grid();
    let disabled_leaf_count = disabled_grid.leaf_count();
    let mut disabled_level = 2;
    let disabled = refine_dwr_cut_band(
        &mut disabled_grid,
        &design,
        &indicators,
        &mut disabled_level,
        false,
    )
    .expect("valid disabled policy input");
    assert!(!disabled.advanced, "disabled policy is a no-op");
    assert_eq!(disabled.splits, 0);
    assert_eq!(disabled_grid.leaf_count(), disabled_leaf_count);
    assert_eq!(disabled_level, 2);

    println!(
        "{{\"metric\":\"dwr-shared-band-policy\",\"cut_mass\":{:.3},\
         \"total_mass\":{:.3},\"advanced\":{},\"splits\":{},\"band_level\":{}}}",
        decision_a.cut_mass,
        decision_a.total_mass,
        decision_a.advanced,
        decision_a.splits,
        decision_a.band_level
    );
    verdict(
        "tm-006",
        "synthetic CellKey indicators deterministically drive one shared cut-band \
         planning step; no graded vector re-solve is claimed",
    );
}

#[test]
fn tm_007_invalid_band_policy_inputs_refuse_without_mutation() {
    let design = seeded_design();
    let make_grid = || Quadtree::with_room(2, 4);

    let mut nonfinite_grid = make_grid();
    let nonfinite_before: Vec<_> = nonfinite_grid.leaves().collect();
    let nonfinite_cell = nonfinite_before[0];
    let mut nonfinite_level = 2;
    let nonfinite = BTreeMap::from([(nonfinite_cell, f64::NAN)]);
    let err = refine_dwr_cut_band(
        &mut nonfinite_grid,
        &design,
        &nonfinite,
        &mut nonfinite_level,
        true,
    )
    .expect_err("non-finite indicator must refuse");
    assert!(matches!(err, CutFemError::InvalidFemInput { .. }));
    assert_grid_and_level_unchanged(&nonfinite_grid, &nonfinite_before, nonfinite_level, 2);

    let mut overflow_grid = make_grid();
    let overflow_before: Vec<_> = overflow_grid.leaves().collect();
    let mut overflow_level = 2;
    let overflow = BTreeMap::from([
        (overflow_before[0], f64::MAX),
        (overflow_before[1], f64::MAX),
    ]);
    let err = refine_dwr_cut_band(
        &mut overflow_grid,
        &design,
        &overflow,
        &mut overflow_level,
        true,
    )
    .expect_err("non-finite accumulated mass must refuse");
    assert!(matches!(err, CutFemError::InvalidFemInput { .. }));
    assert_grid_and_level_unchanged(&overflow_grid, &overflow_before, overflow_level, 2);

    let mut nonleaf_grid = make_grid();
    let nonleaf_before: Vec<_> = nonleaf_grid.leaves().collect();
    let mut nonleaf_level = 2;
    let nonleaf = BTreeMap::from([((1, 0, 0), 1.0)]);
    let err = refine_dwr_cut_band(
        &mut nonleaf_grid,
        &design,
        &nonleaf,
        &mut nonleaf_level,
        true,
    )
    .expect_err("non-leaf indicator key must refuse");
    assert!(matches!(err, CutFemError::InvalidFemInput { .. }));
    assert_grid_and_level_unchanged(&nonleaf_grid, &nonleaf_before, nonleaf_level, 2);

    let mut enclosure_grid = make_grid();
    let enclosure_before: Vec<_> = enclosure_grid.leaves().collect();
    let mut enclosure_level = 2;
    let indicators: BTreeMap<_, _> = enclosure_before
        .iter()
        .copied()
        .map(|cell| {
            let (lo, hi) = enclosure_grid.rect(cell);
            let eta = if design.enclose(lo, hi).contains_zero() {
                1.0
            } else {
                0.01
            };
            (cell, eta)
        })
        .collect();
    let err = refine_dwr_cut_band(
        &mut enclosure_grid,
        &InfiniteHalo(&design),
        &indicators,
        &mut enclosure_level,
        true,
    )
    .expect_err("non-finite recursive halo enclosure must refuse");
    assert!(matches!(err, CutFemError::InvalidFemInput { .. }));
    assert_grid_and_level_unchanged(&enclosure_grid, &enclosure_before, enclosure_level, 2);

    let mut level_grid = make_grid();
    let level_before: Vec<_> = level_grid.leaves().collect();
    let level_cell = level_before[0];
    let mut invalid_level = 5;
    let indicators = BTreeMap::from([(level_cell, 1.0)]);
    let err = refine_dwr_cut_band(
        &mut level_grid,
        &design,
        &indicators,
        &mut invalid_level,
        true,
    )
    .expect_err("out-of-range band level must refuse");
    assert!(matches!(err, CutFemError::InvalidFemInput { .. }));
    assert_grid_and_level_unchanged(&level_grid, &level_before, invalid_level, 5);

    verdict(
        "tm-007",
        "non-finite indicators, accumulated masses, and recursive enclosures, \
         non-leaf keys, and invalid levels return structured refusals without \
         mutating grid or band level",
    );
}

#[test]
fn tm_008_real_vector_dwr_drives_shared_band_policy_once() {
    // This is the fs-dwr effectivity battery's manufactured all-embedded disk
    // family, with radius 0.10 < h=0.125 on the level-three grid. Its off-grid
    // center makes every active coarse cell a genuine cut cell, so the real
    // estimator's cut-band mass equals its total marking mass by construction;
    // no indicators are fabricated or renormalized to pass the shared gate.
    // The resulting graded tree is a planning artifact only: vector CutFEM is
    // NOT re-solved on it until componentwise hanging constraints exist.
    let grid = Quadtree::with_room(3, 4);
    let disk = Circle {
        center: [0.47, 0.53],
        radius: 0.10,
    };
    let material = IsotropicElastic::new(1.0, 0.3, 10.0).expect("compressible material");
    let (lambda, mu) = material.lame();
    let displacement_scale = 0.01;
    let body = |_: f64, _: f64| [-2.0 * displacement_scale * (lambda + 3.0 * mu), 0.0];
    let zero = |_: f64, _: f64| [0.0, 0.0];
    let problem = vector_dwr_problem(&grid, &disk, &material);
    let estimate =
        estimate_elasticity_compliance(&problem, &body, &zero).expect("real vector DWR estimate");
    assert!(
        estimate.indicators.keys().all(|&cell| {
            let (lo, hi) = grid.rect(cell);
            disk.enclose(lo, hi).contains_zero()
        }),
        "the radius-below-h fixture must put every authentic indicator on a cut cell"
    );

    let mut planning_grid = grid.clone();
    let mut band_level = 3;
    let decision = refine_dwr_cut_band(
        &mut planning_grid,
        &disk,
        &estimate.indicators,
        &mut band_level,
        true,
    )
    .expect("real vector indicators satisfy shared policy input contract");
    assert_eq!(
        decision.total_mass.to_bits(),
        estimate.eta_abs.to_bits(),
        "the helper consumes the estimator's exact marking mass"
    );
    assert_eq!(
        decision.cut_mass.to_bits(),
        decision.total_mass.to_bits(),
        "all authentic indicator mass is cut-band mass by fixture construction"
    );
    assert!(
        decision.advanced,
        "real vector DWR mass advances the cut band"
    );
    assert!(
        decision.splits > 0,
        "real vector DWR causes actual halo splits"
    );
    assert_eq!(decision.previous_level, 3);
    assert_eq!(decision.band_level, 4);
    assert_eq!(band_level, 4);

    let cut_mass_fraction = decision.cut_mass / decision.total_mass;
    let enriched_delta = estimate.j_enriched - estimate.j_primal;
    let eta_over_enriched_delta = estimate.eta_signed / enriched_delta;
    assert!(
        cut_mass_fraction.is_finite() && cut_mass_fraction > 0.15,
        "authentic boundary-dominated vector indicators pass the shared gate: \
         {cut_mass_fraction:.6}"
    );
    assert!(
        enriched_delta.is_finite()
            && enriched_delta.abs() > 0.0
            && eta_over_enriched_delta.is_finite(),
        "enriched-compliance proxy metadata must be finite and non-degenerate"
    );
    println!(
        "{{\"metric\":\"real-vector-dwr-band-policy\",\"j_h\":{:.10e},\
         \"j_h2\":{:.10e},\"enriched_delta\":{enriched_delta:.10e},\
         \"eta_signed\":{:.10e},\"eta_abs\":{:.10e},\
         \"eta_over_enriched_delta\":{eta_over_enriched_delta:.8},\
         \"cut_mass\":{:.10e},\"cut_mass_fraction\":{cut_mass_fraction:.8},\
         \"dofs\":{},\"enriched_dofs\":{},\"ghost_method\":\"{}\",\
         \"advanced\":{},\"splits\":{},\"band_level\":{}}}",
        estimate.j_primal,
        estimate.j_enriched,
        estimate.eta_signed,
        estimate.eta_abs,
        decision.cut_mass,
        estimate.dofs,
        estimate.enriched_dofs,
        estimate.ghost_method.as_str(),
        decision.advanced,
        decision.splits,
        decision.band_level,
    );
    verdict(
        "tm-008",
        "an authentic vector-compliance DWR indicator map advances the shared heat-parity \
         cut-band policy once; the resulting graded planning grid is not re-solved",
    );
}
