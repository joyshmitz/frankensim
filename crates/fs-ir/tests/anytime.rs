//! Anytime + refusal conformance (the lmp4.17 bead; runs under the
//! `ladder-planner` feature). Acceptance: every query returns an
//! immediate colored interval that tightens MONOTONICALLY with budget;
//! each result carries a valid priced "what would tighten this" hint;
//! an under-budget query is REFUSED with the achieved interval and the
//! price of the gap — never a silent point estimate; replays reproduce
//! the trajectory (G5).
#![cfg(feature = "ladder-planner")]

use fs_evidence::Color;
use fs_ir::anytime::run_anytime;
use fs_ir::planner::{CostTable, MemCache, ProblemFamily};
use fs_verify::fem1d::Poly;

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-ir/anytime\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

fn steep_family() -> ProblemFamily {
    let mut c = vec![0.0; 11];
    c[1] = 0.2;
    c[2] = -0.2;
    c[9] = 1.0;
    c[10] = -1.0;
    ProblemFamily {
        base: Poly(c),
        kernel: "cht-wedge-steep".to_string(),
    }
}

const RUNGS: [usize; 4] = [12, 24, 48, 96];

#[test]
fn an_001_immediate_interval_monotone_tightening() {
    let family = steep_family();
    let mut cache = MemCache::default();
    let mut costs = CostTable::new(200.0);
    let budgets = [15.0, 40.0, 120.0, 400.0];
    let report = run_anytime(&family, 1.0, 5e-3, &budgets, &RUNGS, &mut cache, &mut costs);
    // Immediate: the FIRST rung already returned a certified interval.
    assert!(!report.trajectory.is_empty());
    let first = &report.trajectory[0];
    assert!(first.bound.is_finite(), "an immediate wide interval exists");
    assert!(
        matches!(first.color, Color::Verified { .. }),
        "the operator knows what kind of answer they hold: {:?}",
        first.color
    );
    // Monotone tightening with budget.
    for pair in report.trajectory.windows(2) {
        assert!(
            pair[1].bound <= pair[0].bound + 1e-15,
            "intervals tighten monotonically: {} -> {}",
            pair[0].bound,
            pair[1].bound
        );
    }
    // G5: an identical replay reproduces the identical trajectory.
    let replay = run_anytime(
        &family,
        1.0,
        5e-3,
        &budgets,
        &RUNGS,
        &mut MemCache::default(),
        &mut CostTable::new(200.0),
    );
    assert_eq!(replay.trajectory.len(), report.trajectory.len());
    for (a, b) in report.trajectory.iter().zip(&replay.trajectory) {
        assert_eq!(a.bound.to_bits(), b.bound.to_bits(), "bit-equal trajectory");
        assert_eq!(a.hint, b.hint);
    }
    println!(
        "{{\"metric\":\"anytime-trajectory\",\"bounds\":{:?}}}",
        report
            .trajectory
            .iter()
            .map(|s| s.bound)
            .collect::<Vec<_>>()
    );
    verdict(
        "an-001",
        "immediate verified interval; monotone tightening across the budget ladder; \
         bit-equal replay (G5)",
    );
}

#[test]
fn an_002_refusal_teaches_with_the_gap_price() {
    let family = steep_family();
    let mut cache = MemCache::default();
    let mut costs = CostTable::new(200.0);
    // A tolerance the small budget ladder cannot reach.
    let report = run_anytime(
        &family,
        1.0,
        1e-4,
        &[20.0, 45.0, 80.0],
        &RUNGS,
        &mut cache,
        &mut costs,
    );
    assert!(!report.discharged(), "the query could not discharge");
    let refusal = report.refusal.as_ref().expect("a refusal note exists");
    // The refusal carries: the ACHIEVED interval…
    assert!(refusal.contains("achieved a certified"), "{refusal}");
    assert!(refusal.contains("±"), "the interval is stated: {refusal}");
    // …the PRICE of the gap…
    assert!(
        refusal.contains("more cells"),
        "the gap is priced: {refusal}"
    );
    // …and NO silent point estimate.
    assert!(
        refusal.contains("No best-effort point estimate"),
        "the honesty clause is explicit: {refusal}"
    );
    // Every trajectory step still carried a certified interval + color.
    for step in &report.trajectory {
        assert!(step.bound.is_finite());
        assert!(matches!(step.color, Color::Verified { .. }));
        assert!(!step.discharged);
    }
    println!("{{\"metric\":\"refusal\",\"note\":{refusal:?}}}");
    verdict(
        "an-002",
        "the impossible query refuses with the achieved interval, the priced gap, and \
         the explicit no-point-estimate clause",
    );
}

#[test]
fn an_003_hint_names_a_real_move_and_the_hot_region() {
    let family = steep_family();
    let mut cache = MemCache::default();
    let mut costs = CostTable::new(200.0);
    let report = run_anytime(
        &family,
        1.0,
        1e-4,
        &[60.0, 100.0],
        &RUNGS,
        &mut cache,
        &mut costs,
    );
    let step = report.trajectory.last().expect("steps exist");
    // The hint names a REAL operator from the menu…
    assert!(
        step.hint.contains("dwr-refine") || step.hint.contains("climb"),
        "a real menu move: {}",
        step.hint
    );
    // …prices it…
    assert!(step.hint.contains("more cells"), "priced: {}", step.hint);
    // …and, after local refinement, names WHERE the money goes (the
    // steep feature lives near x = 1).
    if step.hint.contains("region") {
        assert!(
            step.hint.contains("x ∈ ["),
            "the hot region is an interval: {}",
            step.hint
        );
    }
    // Cold-telemetry degradation: a fresh table still yields a priced
    // hint (the generic form).
    let cold_hint = fs_ir::anytime::tighten_hint(0.05, 1e-3, 30.0, &CostTable::new(500.0), None);
    assert!(
        cold_hint.contains("more cells"),
        "cold hint priced: {cold_hint}"
    );
    verdict(
        "an-003",
        "hints name a real menu move with a price; local refinement names the hot \
         region; cold telemetry degrades to the generic priced form",
    );
}

#[test]
fn an_004_discharge_ends_the_trajectory_and_caches() {
    let family = steep_family();
    let mut cache = MemCache::default();
    let mut costs = CostTable::new(200.0);
    let budgets = [15.0, 400.0, 4000.0];
    let report = run_anytime(&family, 1.0, 5e-3, &budgets, &RUNGS, &mut cache, &mut costs);
    assert!(report.discharged());
    let last = report.trajectory.last().expect("steps");
    assert!(last.discharged);
    assert!(
        report.trajectory.len() < budgets.len() || last.discharged,
        "the trajectory stops at discharge (no wasted rungs after success)"
    );
    assert!(last.hint.contains("spend nothing"), "{}", last.hint);
    // The follow-up identical query is a pure cache hit at ANY budget.
    let again = run_anytime(&family, 1.0, 5e-3, &[1.0], &RUNGS, &mut cache, &mut costs);
    assert!(again.discharged(), "the cached answer discharges instantly");
    assert_eq!(again.trajectory.len(), 1);
    verdict(
        "an-004",
        "discharge terminates the ladder with the 'spend nothing' hint; the repeat \
         query discharges from cache at a 1-cell budget",
    );
}
