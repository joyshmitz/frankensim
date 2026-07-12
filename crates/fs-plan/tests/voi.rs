//! VoI-query conformance (the knh1.6 bead; runs under `voi-queries`).
//! Acceptance: decision sensitivity from cached sweeps (near-free —
//! call counts prove it); rankings by sampled flip-fraction reduction per
//! dollar; the
//! probe menu unifies computational and physical experiments; the
//! ranking surfaces as the query hint and the probe scheduler; myopic
//! one-step only; the prospective-audit kill criterion demotes VoI
//! when recommendations stop outperforming.
#![cfg(feature = "voi-queries")]

use std::cell::Cell;
use std::cell::RefCell;

use fs_plan::voi::{
    AuditReport, AuditVerdict, LiveDecision, MAX_VOI_AUDIT_RECORDS, MAX_VOI_EVALUATIONS,
    MAX_VOI_GRID, MAX_VOI_NAME_BYTES, MAX_VOI_NODES, MAX_VOI_PROBES, MatchedAuditRecord, Probe,
    ProbeKind, RankedMenu, UncertaintyNode, VoiError, audit_scheduling, hint_for_query,
    rank_purchases, schedule_probes,
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

fn probe(name: &str, target: &str, cost: f64, shrink: f64) -> Probe {
    Probe {
        name: name.to_string(),
        target: target.to_string(),
        cost,
        shrink,
        kind: ProbeKind::Computational,
    }
}

fn ranked_menu(probes: &[Probe]) -> RankedMenu {
    let decision = LiveDecision {
        margin: &margin,
        arity: 3,
    };
    rank_purchases(&decision, &nodes(), probes, 64).expect("valid ranked-menu fixture")
}

fn audit_record(index: usize, recommended_wins: bool) -> MatchedAuditRecord {
    MatchedAuditRecord::new(
        format!("audit-{index:04}"),
        "voi-choice",
        "agent-choice",
        format!("fixture/run-{index:04}"),
        10.0,
        10.0,
        recommended_wins,
        !recommended_wins,
    )
    .expect("valid matched-cost audit fixture")
}

fn winning_audit() -> AuditReport {
    let records: Vec<_> = (0..128).map(|index| audit_record(index, true)).collect();
    let report = audit_scheduling(&records).expect("valid anytime audit");
    assert_eq!(report.verdict(), AuditVerdict::KeepScheduling);
    report
}

fn scheduler_menu() -> RankedMenu {
    ranked_menu(&[
        probe("a", "drag-gap", 10.0, 0.01),
        probe("b", "drag-gap", 5.0, 0.01),
    ])
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
    let p_pivotal = decision
        .flip_probability(&ns, 0, grid)
        .expect("valid pivotal sweep");
    let p_irrelevant = decision
        .flip_probability(&ns, 1, grid)
        .expect("valid irrelevant sweep");
    let p_mild = decision
        .flip_probability(&ns, 2, grid)
        .expect("valid mild sweep");
    println!(
        "{{\"metric\":\"sensitivity\",\"drag_gap\":{p_pivotal:.3},\
         \"mass_penalty\":{p_irrelevant:.3},\"hazard\":{p_mild:.3},\"surrogate_calls\":{}}}",
        calls.get()
    );
    assert!(
        p_pivotal > 0.3,
        "the straddling node is pivotal: {p_pivotal}"
    );
    assert_eq!(
        p_irrelevant.to_bits(),
        0.0f64.to_bits(),
        "the one-sided node cannot flip"
    );
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
        "sampled flip fractions from cached interval sweeps: pivotal 0.4-class, one-sided \
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
    let ranked = rank_purchases(&decision, &ns, &menu, 64).expect("valid probe menu");
    let names: Vec<&str> = ranked.iter().map(|r| r.probe().name.as_str()).collect();
    println!(
        "{{\"metric\":\"ranking\",\"order\":{names:?},\"scores\":{:?}}}",
        ranked
            .iter()
            .map(|r| (r.score() * 1e4).round() / 1e4)
            .collect::<Vec<_>>()
    );
    assert_eq!(names[0], "climb-rung-drag", "cheap+decisive wins");
    assert_eq!(names[1], "wind-tunnel-drag", "decisive-but-pricey second");
    assert_eq!(names[2], "refine-mass-model", "irrelevant last");
    let first = ranked.get(0).expect("first ranked purchase");
    let second = ranked.get(1).expect("second ranked purchase");
    let third = ranked.get(2).expect("third ranked purchase");
    assert!(first.score() > second.score() && second.score() > 0.0);
    assert_eq!(
        third.score().to_bits(),
        0.0f64.to_bits(),
        "an irrelevant probe buys nothing"
    );
    verdict(
        "voi-002",
        "the ranking is sampled flip-fraction reduction per dollar: cheap+decisive > decisive+pricey > \
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
    let ranked = rank_purchases(&decision, &ns, &menu, 64).expect("valid mixed menu");
    let top = ranked.top().expect("nonempty sealed menu");
    let second = ranked.get(1).expect("second ranked purchase");
    assert_eq!(top.probe().kind, ProbeKind::Physical);
    assert!(
        top.score() > second.score(),
        "the physical anchor wins on flip-prob-per-dollar: {:.4} vs {:.4}",
        top.score(),
        second.score()
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
    let ranked = rank_purchases(&decision, &ns, &menu, 64).expect("valid scheduling menu");
    // (i) The query-result hint (the Proposal-8 anytime shape, now
    // decision-priced).
    let hint = hint_for_query(&ranked);
    println!("{{\"metric\":\"hint\",\"hint\":{}}}", hint.to_json());
    let text = hint.render_text();
    assert!(text.contains("climb-rung-drag") && text.contains("$10"));
    assert!(text.contains("sampled flip fraction") && text.contains("64-point"));
    // (ii) Scheduling requires independently won audit authority and executes
    // one myopic purchase before the caller must update and rerank.
    // Source menu order carries no authority: sealing canonicalizes both rows
    // and the context identity.
    let mut reordered_menu = menu.clone();
    reordered_menu.reverse();
    let reordered =
        rank_purchases(&decision, &ns, &reordered_menu, 64).expect("reordered source menu");
    assert_eq!(reordered.context_id(), ranked.context_id());
    assert_eq!(reordered, ranked);
    let audit = winning_audit();
    let scheduled = schedule_probes(ranked, 40.0, audit.authority())
        .expect("valid finite schedule")
        .expect("one affordable purchase");
    assert_eq!(scheduled.purchase().probe().name, "climb-rung-drag");
    assert_eq!(scheduled.ranked_context_id(), reordered.context_id());
    assert_eq!(scheduled.ranked_grid(), 64);
    assert_eq!(scheduled.audit_context_id(), audit.audit_context_id());
    assert_eq!(scheduled.audit_observations(), audit.observations());
    assert_eq!(
        scheduled.audit_log_e_value().to_bits(),
        audit.log_e_value().to_bits()
    );
    assert_eq!(scheduled.budget_dollars().to_bits(), 40.0f64.to_bits());
    assert_eq!(
        scheduled.remaining_budget_dollars().to_bits(),
        30.0f64.to_bits()
    );
    verdict(
        "voi-004",
        "the structured hint is grid-qualified; a sealed context ignores source-menu order; \
         authority schedules exactly one purchase before mandatory reranking",
    );
}

#[test]
fn voi_005_prospective_audit_kill_criterion() {
    let empty = audit_scheduling(&[]).expect("empty audit reports safely");
    assert_eq!(empty.verdict(), AuditVerdict::DemoteToReporting);
    assert!(empty.authority().is_none());

    let insufficient = vec![audit_record(0, true)];
    let insufficient = audit_scheduling(&insufficient).expect("bounded prefix");
    assert_eq!(insufficient.verdict(), AuditVerdict::DemoteToReporting);
    assert!(matches!(
        schedule_probes(scheduler_menu(), 15.0, insufficient.authority()),
        Err(VoiError::MissingSchedulingAuthority)
    ));

    let losing: Vec<_> = (0..128).map(|index| audit_record(index, false)).collect();
    let losing = audit_scheduling(&losing).expect("valid losing audit");
    assert_eq!(losing.verdict(), AuditVerdict::DemoteToReporting);
    assert!(matches!(
        schedule_probes(scheduler_menu(), 15.0, losing.authority()),
        Err(VoiError::MissingSchedulingAuthority)
    ));

    let mut winning: Vec<_> = (0..128).map(|index| audit_record(index, true)).collect();
    let won = audit_scheduling(&winning).expect("valid winning audit");
    assert_eq!(won.verdict(), AuditVerdict::KeepScheduling);
    assert!(won.authority().is_some());
    winning.reverse();
    let replay = audit_scheduling(&winning).expect("input order is non-authoritative");
    assert_eq!(replay.audit_context_id(), won.audit_context_id());
    assert_eq!(replay.log_e_value().to_bits(), won.log_e_value().to_bits());
    verdict(
        "voi-005",
        "the fixed-alpha pairwise e-process mints authority only after a sufficient winning \
         matched-cost prefix; empty, short, and losing audits remain reporting-only",
    );
}

#[test]
fn voi_006_node_name_and_grid_boundaries() {
    let boundary_nodes: Vec<UncertaintyNode> = (0..MAX_VOI_NODES)
        .map(|index| UncertaintyNode {
            name: format!("node-{index}"),
            lo: 0.0,
            hi: 1.0,
            nominal: 0.5,
        })
        .collect();
    let wide_margin = |values: &[f64]| values[0] - 0.5;
    let boundary_decision = LiveDecision {
        margin: &wide_margin,
        arity: MAX_VOI_NODES,
    };
    assert!(
        boundary_decision.nominal_verdict(&boundary_nodes).is_ok(),
        "the exact node-count boundary is admitted"
    );
    let mut too_many = boundary_nodes.clone();
    too_many.push(UncertaintyNode {
        name: "one-too-many".to_string(),
        lo: 0.0,
        hi: 1.0,
        nominal: 0.5,
    });
    let oversized_decision = LiveDecision {
        margin: &wide_margin,
        arity: too_many.len(),
    };
    assert!(matches!(
        oversized_decision.nominal_verdict(&too_many),
        Err(VoiError::SizeLimit { .. })
    ));

    let exact_name = vec![UncertaintyNode {
        name: "n".repeat(MAX_VOI_NAME_BYTES),
        lo: 0.0,
        hi: 1.0,
        nominal: 0.5,
    }];
    let one = LiveDecision {
        margin: &wide_margin,
        arity: 1,
    };
    assert!(one.nominal_verdict(&exact_name).is_ok());
    let mut long_name = exact_name;
    long_name[0].name.push('n');
    assert!(matches!(
        one.nominal_verdict(&long_name),
        Err(VoiError::InvalidName { .. })
    ));

    let single = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: -1.0,
        hi: 1.0,
        nominal: 0.0,
    }];
    assert!(
        one.flip_probability(&single, 0, MAX_VOI_GRID).is_ok(),
        "the exact grid boundary is admitted"
    );
    for grid in [0, MAX_VOI_GRID + 1] {
        assert!(matches!(
            one.flip_probability(&single, 0, grid),
            Err(VoiError::InvalidGrid { .. })
        ));
    }
    verdict(
        "voi-006",
        "node count, visible-ASCII name bytes, and sweep grid admit exact boundaries and refuse limit+1",
    );
}

#[test]
fn voi_007_menu_and_aggregate_evaluation_boundaries() {
    let calls = Cell::new(0usize);
    let counting = |values: &[f64]| {
        calls.set(calls.get() + 1);
        values[0]
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 1,
    };
    let ns = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: -1.0,
        hi: 1.0,
        nominal: 0.0,
    }];
    let menu: Vec<Probe> = (0..MAX_VOI_PROBES)
        .map(|index| probe(&format!("probe-{index}"), "x", index as f64 + 1.0, 0.5))
        .collect();
    assert_eq!(MAX_VOI_PROBES * 2 * (1 + 1), MAX_VOI_EVALUATIONS);
    let ranked = rank_purchases(&decision, &ns, &menu, 1)
        .expect("exact menu/evaluation boundary is admitted");
    assert_eq!(ranked.len(), MAX_VOI_PROBES);
    assert_eq!(calls.get(), MAX_VOI_EVALUATIONS);

    calls.set(0);
    assert!(matches!(
        rank_purchases(&decision, &ns, &menu, 2),
        Err(VoiError::EvaluationLimitExceeded { .. })
    ));
    assert_eq!(calls.get(), 0, "aggregate refusal precedes callbacks");

    let mut oversized = menu;
    oversized.push(probe("one-too-many", "x", 1.0, 0.5));
    assert!(matches!(
        rank_purchases(&decision, &ns, &oversized, 1),
        Err(VoiError::SizeLimit { .. })
    ));
    assert_eq!(calls.get(), 0, "menu-size refusal precedes callbacks");
    verdict(
        "voi-007",
        "menu and aggregate evaluation caps admit exact limits and refuse before callback at limit+1",
    );
}

#[test]
fn voi_008_decision_interval_and_callback_refusals() {
    let calls = Cell::new(0usize);
    let counting = |values: &[f64]| {
        calls.set(calls.get() + 1);
        values.first().copied().unwrap_or(0.0)
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 1,
    };
    assert!(matches!(
        decision.nominal_verdict(&[]),
        Err(VoiError::SizeLimit { .. })
    ));
    assert_eq!(calls.get(), 0);
    let ns = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: 0.0,
        hi: 1.0,
        nominal: 0.5,
    }];
    let wrong_arity = LiveDecision {
        margin: &counting,
        arity: 2,
    };
    assert!(matches!(
        wrong_arity.nominal_verdict(&ns),
        Err(VoiError::ArityMismatch { .. })
    ));
    assert!(matches!(
        decision.flip_probability(&ns, 1, 4),
        Err(VoiError::NodeIndexOutOfRange { .. })
    ));

    let mut duplicate = ns.clone();
    duplicate.push(ns[0].clone());
    let duplicate_decision = LiveDecision {
        margin: &counting,
        arity: 2,
    };
    assert!(matches!(
        duplicate_decision.nominal_verdict(&duplicate),
        Err(VoiError::DuplicateName { .. })
    ));
    for (lo, nominal, hi) in [
        (f64::NAN, 0.5, 1.0),
        (1.0, 0.5, 0.0),
        (0.0, 2.0, 1.0),
        (-f64::MAX, 0.0, f64::MAX),
    ] {
        let invalid = vec![UncertaintyNode {
            name: "x".to_string(),
            lo,
            hi,
            nominal,
        }];
        assert!(matches!(
            decision.nominal_verdict(&invalid),
            Err(VoiError::InvalidInterval { .. })
        ));
    }

    let nan_margin = |_: &[f64]| f64::NAN;
    let malformed = LiveDecision {
        margin: &nan_margin,
        arity: 1,
    };
    assert!(matches!(
        malformed.nominal_verdict(&ns),
        Err(VoiError::NonFiniteMargin { .. })
    ));
    let interior_nan = |values: &[f64]| {
        if values[0] > 0.5 { f64::INFINITY } else { 0.0 }
    };
    let malformed = LiveDecision {
        margin: &interior_nan,
        arity: 1,
    };
    assert!(matches!(
        malformed.flip_probability(&ns, 0, 2),
        Err(VoiError::NonFiniteMargin { .. })
    ));
    verdict(
        "voi-008",
        "arity, identity, intervals, indices, duplicate nodes, and nonfinite callbacks refuse structurally",
    );
}

#[test]
fn voi_009_menu_targets_and_probe_economics_refuse_before_sweep() {
    let calls = Cell::new(0usize);
    let counting = |values: &[f64]| {
        calls.set(calls.get() + 1);
        values[0]
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 1,
    };
    let ns = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: -1.0,
        hi: 1.0,
        nominal: 0.0,
    }];
    assert!(matches!(
        rank_purchases(&decision, &ns, &[], 4),
        Err(VoiError::SizeLimit { .. })
    ));
    let duplicate_nodes = vec![ns[0].clone(), ns[0].clone()];
    let duplicate_target_decision = LiveDecision {
        margin: &counting,
        arity: 2,
    };
    assert!(matches!(
        rank_purchases(
            &duplicate_target_decision,
            &duplicate_nodes,
            &[probe("ambiguous", "x", 1.0, 0.5)],
            4,
        ),
        Err(VoiError::DuplicateName { .. })
    ));
    let unknown = vec![probe("unknown", "missing", 1.0, 0.5)];
    let unknown_before = unknown.clone();
    assert!(matches!(
        rank_purchases(&decision, &ns, &unknown, 4),
        Err(VoiError::TargetResolution { matches: 0, .. })
    ));
    assert_eq!(unknown, unknown_before, "refusal does not mutate the menu");
    assert_eq!(calls.get(), 0, "unknown target refuses before sweep");

    let duplicated = vec![probe("same", "x", 1.0, 0.5), probe("same", "x", 2.0, 0.5)];
    assert!(matches!(
        rank_purchases(&decision, &ns, &duplicated, 4),
        Err(VoiError::DuplicateName { .. })
    ));
    for cost in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            rank_purchases(&decision, &ns, &[probe("bad-cost", "x", cost, 0.5)], 4),
            Err(VoiError::InvalidProbeValue { field: "cost", .. })
        ));
    }
    for shrink in [0.0, 1.0, -0.1, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            rank_purchases(&decision, &ns, &[probe("bad-shrink", "x", 1.0, shrink)], 4),
            Err(VoiError::InvalidProbeValue {
                field: "shrink",
                ..
            })
        ));
    }
    for bad_name in ["", " padded", "trailing "] {
        assert!(matches!(
            rank_purchases(&decision, &ns, &[probe(bad_name, "x", 1.0, 0.5)], 4),
            Err(VoiError::InvalidName { .. })
        ));
    }
    assert_eq!(calls.get(), 0, "all malformed menus refuse before sweep");
    verdict(
        "voi-009",
        "unknown/ambiguous targets, duplicate identities, and invalid probe economics refuse before evaluation",
    );
}

#[test]
fn voi_010_probe_name_and_score_arithmetic_boundaries() {
    let calls = Cell::new(0usize);
    let counting = |values: &[f64]| {
        calls.set(calls.get() + 1);
        values[0]
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 1,
    };
    let ns = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: -1.0,
        hi: 1.0,
        nominal: 0.0,
    }];
    let exact = "p".repeat(MAX_VOI_NAME_BYTES);
    assert!(
        rank_purchases(&decision, &ns, &[probe(&exact, "x", 1.0, 0.5)], 4).is_ok(),
        "the exact probe-name byte boundary is admitted"
    );
    let before_long_name = calls.get();
    let too_long = "p".repeat(MAX_VOI_NAME_BYTES + 1);
    assert!(matches!(
        rank_purchases(&decision, &ns, &[probe(&too_long, "x", 1.0, 0.5)], 4),
        Err(VoiError::InvalidName { .. })
    ));
    assert_eq!(calls.get(), before_long_name);
    let too_long_target = "t".repeat(MAX_VOI_NAME_BYTES + 1);
    assert!(matches!(
        rank_purchases(
            &decision,
            &ns,
            &[probe("long-target", &too_long_target, 1.0, 0.5)],
            4,
        ),
        Err(VoiError::InvalidName { .. })
    ));

    let decision = LiveDecision {
        margin: &margin,
        arity: 3,
    };
    assert!(matches!(
        rank_purchases(
            &decision,
            &nodes(),
            &[probe("underflow-cost", "drag-gap", f64::from_bits(1), 0.01,)],
            64,
        ),
        Err(VoiError::ArithmeticRefusal { .. })
    ));
    verdict(
        "voi-010",
        "probe names admit the exact byte bound, limit+1 refuses before callback, and derived score overflow refuses",
    );
}

#[test]
fn voi_011_scheduler_is_transactional_and_budget_monotone() {
    let audit = winning_audit();
    assert!(
        schedule_probes(scheduler_menu(), 0.0, audit.authority())
            .expect("zero budget is valid")
            .is_none()
    );
    for budget in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            schedule_probes(scheduler_menu(), budget, audit.authority()),
            Err(VoiError::InvalidBudget { .. })
        ));
    }
    assert!(matches!(
        schedule_probes(scheduler_menu(), 15.0, None),
        Err(VoiError::MissingSchedulingAuthority)
    ));
    let no_progress = ranked_menu(&[probe("tiny", "drag-gap", 1.0, 0.01)]);
    assert!(matches!(
        schedule_probes(no_progress, f64::MAX, audit.authority()),
        Err(VoiError::ArithmeticRefusal { .. })
    ));

    let scheduled = schedule_probes(scheduler_menu(), 15.0, audit.authority())
        .expect("exact finite budget")
        .expect("one purchase");
    assert_eq!(scheduled.purchase().probe().name, "b");
    verdict(
        "voi-011",
        "invalid budgets and absent authority refuse; one sealed ranking epoch is consumed to schedule at most one purchase",
    );
}

#[test]
fn voi_012_asymmetric_contraction_is_a_subset_and_preflights() {
    let samples = RefCell::new(Vec::new());
    let recording = |values: &[f64]| {
        samples.borrow_mut().push(values[0]);
        values[0] - 0.5
    };
    let decision = LiveDecision {
        margin: &recording,
        arity: 1,
    };
    let asymmetric = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: 0.0,
        hi: 1.0,
        nominal: 0.9,
    }];
    rank_purchases(
        &decision,
        &asymmetric,
        &[probe("contract", "x", 1.0, 0.8)],
        2,
    )
    .expect("asymmetric interval contracts inside its support");
    let samples = samples.borrow();
    assert_eq!(samples.len(), 6, "two nominal-plus-grid sweeps");
    let after = &samples[4..6];
    assert!(after.iter().all(|sample| (0.0..=1.0).contains(sample)));
    assert!((after[0] - 0.38).abs() < 1e-12, "left midpoint");
    assert!((after[1] - 0.78).abs() < 1e-12, "right midpoint");
    assert!((after[1] - after[0] - 0.4).abs() < 1e-12);

    let calls = Cell::new(0usize);
    let counting = |values: &[f64]| {
        calls.set(calls.get() + 1);
        values[0]
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 1,
    };
    let subnormal = vec![UncertaintyNode {
        name: "tiny".to_string(),
        lo: 0.0,
        hi: f64::from_bits(1),
        nominal: 0.0,
    }];
    assert!(matches!(
        rank_purchases(
            &decision,
            &subnormal,
            &[probe("underflow", "tiny", 1.0, 0.5)],
            2,
        ),
        Err(VoiError::ArithmeticRefusal { .. })
    ));
    assert_eq!(
        calls.get(),
        0,
        "derived intervals preflight before callbacks"
    );
}

#[test]
fn voi_013_context_and_sampled_zero_are_explicit() {
    let threshold = |values: &[f64]| values[0];
    let decision = LiveDecision {
        margin: &threshold,
        arity: 1,
    };
    let ns = vec![UncertaintyNode {
        name: "x".to_string(),
        lo: -1.0,
        hi: 1.0,
        nominal: 0.0,
    }];
    let menu = vec![probe("midpoint-alias", "x", 2.0, 0.5)];
    let grid_one = rank_purchases(&decision, &ns, &menu, 1).expect("grid one is estimated");
    let hint = hint_for_query(&grid_one);
    assert!(hint.purchase().is_none());
    assert!(hint.render_text().contains("no sampled purchase"));
    assert!(hint.render_text().contains("does not prove"));
    assert!(hint.to_json().contains("\"authoritative_zero\":false"));

    let grid_two = rank_purchases(&decision, &ns, &menu, 2).expect("second grid");
    assert_ne!(grid_one.context_id(), grid_two.context_id());
    let mut expanded_menu = menu.clone();
    expanded_menu.push(probe("second-probe", "x", 3.0, 0.25));
    let expanded =
        rank_purchases(&decision, &ns, &expanded_menu, 1).expect("expanded supplied menu");
    assert_ne!(grid_one.context_id(), expanded.context_id());
    let mut changed = ns;
    changed[0].hi = 1.5;
    let changed = rank_purchases(&decision, &changed, &menu, 1).expect("changed snapshot");
    assert_ne!(grid_one.context_id(), changed.context_id());
}

#[test]
fn voi_014_audit_validation_duplicates_and_work_bound() {
    assert!(matches!(
        MatchedAuditRecord::new("obs", "same", "same", "fixture/run", 1.0, 1.0, true, false,),
        Err(VoiError::InvalidAuditPair { .. })
    ));
    for (recommended, alternative) in [(1.0, 2.0), (f64::NAN, f64::NAN), (0.0, 0.0)] {
        assert!(matches!(
            MatchedAuditRecord::new(
                "obs",
                "recommended",
                "alternative",
                "fixture/run",
                recommended,
                alternative,
                true,
                false,
            ),
            Err(VoiError::InvalidAuditCost { .. })
        ));
    }
    assert!(matches!(
        MatchedAuditRecord::new(
            "bad\nobs",
            "recommended",
            "alternative",
            "fixture/run",
            1.0,
            1.0,
            true,
            false,
        ),
        Err(VoiError::InvalidName { .. })
    ));

    let duplicated = vec![audit_record(0, true), audit_record(0, true)];
    assert!(matches!(
        audit_scheduling(&duplicated),
        Err(VoiError::DuplicateAuditObservation { .. })
    ));
    let oversized: Vec<_> = (0..=MAX_VOI_AUDIT_RECORDS)
        .map(|index| audit_record(index, true))
        .collect();
    assert!(matches!(
        audit_scheduling(&oversized),
        Err(VoiError::SizeLimit { .. })
    ));
}

#[test]
fn voi_015_structured_hint_escapes_and_preserves_price() {
    let decision = LiveDecision {
        margin: &margin,
        arity: 3,
    };
    let quoted = probe("quoted\"probe", "drag-gap", 0.49, 0.01);
    let ranked = rank_purchases(&decision, &nodes(), &[quoted], 64).expect("visible ASCII quote");
    let hint = hint_for_query(&ranked);
    let text = hint.render_text();
    assert!(
        text.contains("$0.49"),
        "price is not rounded to zero: {text}"
    );
    assert!(
        text.contains("quoted\\\"probe"),
        "text escapes identity: {text}"
    );
    let json = hint.to_json();
    assert!(json.contains("quoted\\\"probe"));
    assert!(json.contains("\"cost_dollars\":0.49"));

    let calls = Cell::new(0usize);
    let counting = |values: &[f64]| {
        calls.set(calls.get() + 1);
        margin(values)
    };
    let decision = LiveDecision {
        margin: &counting,
        arity: 3,
    };
    assert!(matches!(
        rank_purchases(
            &decision,
            &nodes(),
            &[probe("line\nbreak", "drag-gap", 1.0, 0.5)],
            64,
        ),
        Err(VoiError::InvalidName { .. })
    ));
    assert_eq!(calls.get(), 0, "control characters refuse before callback");
}
