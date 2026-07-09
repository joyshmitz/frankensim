//! VoI-query conformance (the knh1.6 bead; runs under `voi-queries`).
//! Acceptance: decision sensitivity from cached sweeps (near-free —
//! call counts prove it); rankings by flip-probability-per-dollar; the
//! probe menu unifies computational and physical experiments; the
//! ranking surfaces as the query hint and the probe scheduler; myopic
//! one-step only; the prospective-audit kill criterion demotes VoI
//! when recommendations stop outperforming.
#![cfg(feature = "voi-queries")]

use std::cell::Cell;

use fs_plan::voi::{
    AuditRecord, AuditVerdict, LiveDecision, Probe, ProbeKind, UncertaintyNode, audit_verdict,
    hint_for_query, rank_purchases, schedule_probes,
};

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-plan/voi\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

/// The live decision: ship when margin = 2·drag_gap − mass_penalty +
/// 0.3·hazard > 0 (a cheap cached surrogate).
fn margin(v: &[f64]) -> f64 {
    2.0 * v[0] - v[1] + 0.3 * v[2]
}

fn nodes() -> Vec<UncertaintyNode> {
    vec![
        // Straddles the boundary: pivotal.
        UncertaintyNode {
            name: "drag-gap".to_string(),
            lo: -0.4,
            hi: 0.6,
            nominal: 0.1,
        },
        // Entirely on one side: irrelevant to the verdict.
        UncertaintyNode {
            name: "mass-penalty".to_string(),
            lo: 0.05,
            hi: 0.15,
            nominal: 0.1,
        },
        // Mildly pivotal.
        UncertaintyNode {
            name: "hazard".to_string(),
            lo: -1.0,
            hi: 1.0,
            nominal: 0.2,
        },
    ]
}

#[test]
fn voi_001_sensitivity_from_cached_sweeps() {
    let calls = Cell::new(0usize);
    let counting = |v: &[f64]| -> f64 {
        calls.set(calls.get() + 1);
        margin(v)
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 3,
    };
    let ns = nodes();
    let grid = 32;
    let p_pivotal = decision.flip_probability(&ns, 0, grid);
    let p_irrelevant = decision.flip_probability(&ns, 1, grid);
    let p_mild = decision.flip_probability(&ns, 2, grid);
    println!(
        "{{\"metric\":\"sensitivity\",\"drag_gap\":{p_pivotal:.3},\
         \"mass_penalty\":{p_irrelevant:.3},\"hazard\":{p_mild:.3},\"surrogate_calls\":{}}}",
        calls.get()
    );
    assert!(
        p_pivotal > 0.3,
        "the straddling node is pivotal: {p_pivotal}"
    );
    assert!(p_irrelevant == 0.0, "the one-sided node cannot flip");
    assert!(
        p_mild > 0.0 && p_mild < p_pivotal,
        "mildly pivotal: {p_mild}"
    );
    // NEAR-FREE: three sweeps cost 3·(grid + 1) surrogate calls — no
    // solver in the loop, no combinatorial sweep.
    assert!(
        calls.get() <= 3 * (grid + 1),
        "cached-sweep budget held: {} calls",
        calls.get()
    );
    verdict(
        "voi-001",
        "flip probabilities from cached interval sweeps: pivotal 0.4-class, one-sided \
         exactly 0, mild in between — at <= 3(grid+1) surrogate calls (near-free)",
    );
}

#[test]
fn voi_002_ranking_is_flip_prob_per_dollar() {
    let decision = LiveDecision {
        margin: &margin,
        arity: 3,
    };
    let ns = nodes();
    let menu = vec![
        // Cheap AND decisive: must rank first.
        Probe {
            name: "climb-rung-drag".to_string(),
            target: "drag-gap".to_string(),
            cost: 10.0,
            shrink: 0.2,
            kind: ProbeKind::Computational,
        },
        // Decisive but expensive: positive score, ranks below.
        Probe {
            name: "wind-tunnel-drag".to_string(),
            target: "drag-gap".to_string(),
            cost: 500.0,
            shrink: 0.05,
            kind: ProbeKind::Physical,
        },
        // Cheap but irrelevant: zero score, ranks last.
        Probe {
            name: "refine-mass-model".to_string(),
            target: "mass-penalty".to_string(),
            cost: 5.0,
            shrink: 0.1,
            kind: ProbeKind::Computational,
        },
    ];
    let ranked = rank_purchases(&decision, &ns, &menu, 64);
    let names: Vec<&str> = ranked.iter().map(|r| r.probe.name.as_str()).collect();
    println!(
        "{{\"metric\":\"ranking\",\"order\":{names:?},\"scores\":{:?}}}",
        ranked
            .iter()
            .map(|r| (r.score * 1e4).round() / 1e4)
            .collect::<Vec<_>>()
    );
    assert_eq!(names[0], "climb-rung-drag", "cheap+decisive wins");
    assert_eq!(names[1], "wind-tunnel-drag", "decisive-but-pricey second");
    assert_eq!(names[2], "refine-mass-model", "irrelevant last");
    assert!(ranked[0].score > ranked[1].score && ranked[1].score > 0.0);
    assert!(ranked[2].score == 0.0, "an irrelevant probe buys nothing");
    verdict(
        "voi-002",
        "the ranking is flip-probability-per-dollar: cheap+decisive > decisive+pricey > \
         irrelevant (score exactly 0)",
    );
}

#[test]
fn voi_003_menu_unifies_compute_and_physical() {
    // A physical anchor that shrinks the pivotal node far harder can
    // BEAT a cheap but weak computational probe — the menu prices
    // evidence, not its substrate.
    let decision = LiveDecision {
        margin: &margin,
        arity: 3,
    };
    let ns = nodes();
    let menu = vec![
        Probe {
            name: "weak-rung-climb".to_string(),
            target: "drag-gap".to_string(),
            cost: 8.0,
            shrink: 0.95, // barely tightens
            kind: ProbeKind::Computational,
        },
        Probe {
            name: "wind-tunnel-anchor".to_string(),
            target: "drag-gap".to_string(),
            cost: 60.0,
            shrink: 0.05, // near-eliminates the interval
            kind: ProbeKind::Physical,
        },
    ];
    let ranked = rank_purchases(&decision, &ns, &menu, 64);
    assert_eq!(ranked[0].probe.kind, ProbeKind::Physical);
    assert!(
        ranked[0].score > ranked[1].score,
        "the physical anchor wins on flip-prob-per-dollar: {:.4} vs {:.4}",
        ranked[0].score,
        ranked[1].score
    );
    verdict(
        "voi-003",
        "a decisive wind-tunnel anchor outranks a weak rung climb despite 7.5x the \
         price — computational and physical evidence priced on one menu",
    );
}

#[test]
fn voi_004_surfacing_hint_and_scheduler() {
    let decision = LiveDecision {
        margin: &margin,
        arity: 3,
    };
    let ns = nodes();
    let menu = vec![
        Probe {
            name: "climb-rung-drag".to_string(),
            target: "drag-gap".to_string(),
            cost: 10.0,
            shrink: 0.2,
            kind: ProbeKind::Computational,
        },
        Probe {
            name: "hazard-samples".to_string(),
            target: "hazard".to_string(),
            cost: 25.0,
            shrink: 0.3,
            kind: ProbeKind::Computational,
        },
        Probe {
            name: "wind-tunnel-drag".to_string(),
            target: "drag-gap".to_string(),
            cost: 500.0,
            shrink: 0.05,
            kind: ProbeKind::Physical,
        },
    ];
    let ranked = rank_purchases(&decision, &ns, &menu, 64);
    // (i) The query-result hint (the Proposal-8 anytime shape, now
    // decision-priced).
    let hint = hint_for_query(&ranked);
    println!("{{\"metric\":\"hint\",\"text\":\"{hint}\"}}");
    assert!(hint.contains("climb-rung-drag") && hint.contains("$10"));
    assert!(hint.contains("flip-probability"));
    // (ii) The probe scheduler under a budget: greedy top-k affordable.
    let scheduled = schedule_probes(&ranked, 40.0);
    let names: Vec<&str> = scheduled.iter().map(|r| r.probe.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["climb-rung-drag", "hazard-samples"],
        "greedy affordable top-k under $40"
    );
    // Myopic-only is structural: rank_purchases takes ONE state and
    // returns ONE ranked step — there is no sequential-tree API to
    // misuse.
    verdict(
        "voi-004",
        "the top purchase surfaces as the priced query hint; the scheduler buys the \
         greedy affordable top-k under budget; the API is one-step myopic by shape",
    );
}

#[test]
fn voi_005_prospective_audit_kill_criterion() {
    // Recommended purchases realize more decision changes: keep.
    let winning: Vec<AuditRecord> = (0..10)
        .map(|k| AuditRecord {
            recommended_changed_decision: k < 6,
            alternative_changed_decision: k < 3,
        })
        .collect();
    assert_eq!(audit_verdict(&winning), AuditVerdict::KeepScheduling);
    // They stop outperforming at matched cost: DEMOTE to reporting.
    let losing: Vec<AuditRecord> = (0..10)
        .map(|k| AuditRecord {
            recommended_changed_decision: k < 3,
            alternative_changed_decision: k < 4,
        })
        .collect();
    assert_eq!(audit_verdict(&losing), AuditVerdict::DemoteToReporting);
    // No evidence, no authority.
    assert_eq!(audit_verdict(&[]), AuditVerdict::DemoteToReporting);
    verdict(
        "voi-005",
        "the prospective audit keeps VoI's scheduling authority only while recommended \
         purchases measurably outperform agent-chosen alternatives — and with no audit \
         evidence there is no authority",
    );
}
