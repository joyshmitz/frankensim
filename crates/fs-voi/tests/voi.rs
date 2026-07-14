//! Battery for value-of-information (fs-voi). Covers ranking-flip probability,
//! EVPI (robust → 0, close → positive), the decision-vs-estimator distinction
//! (info on a decision-irrelevant design is worthless), STOP-when-robust, VOI
//! beating the uncertainty-proportional baseline, and model-form escalation.

use fs_voi::{
    Action, ActionKind, Component, DesignEstimate, EVPI_SEMANTICS_VERSION, Recommendation,
    Uncertainty, action_value, decision_posture, evpi, heuristic_choice, ranking_flip_probability,
    recommend,
};

fn unc(n: f64, s: f64, m: f64) -> Uncertainty {
    Uncertainty {
        numerical: n,
        statistical: s,
        model: m,
    }
}
fn design(name: &str, mean: f64, u: Uncertainty) -> DesignEstimate {
    DesignEstimate::new(name, mean, u)
}
fn act(name: &str, kind: ActionKind, target: &str, cost: f64) -> Action {
    Action {
        name: name.into(),
        kind,
        target_design: target.into(),
        reduction: 0.9,
        cost,
    }
}

#[test]
fn evpi_semantics_version_is_locked() {
    assert_eq!(EVPI_SEMANTICS_VERSION, 1);
}

#[test]
fn ranking_flip_probability_reflects_separation() {
    let a = design("a", 0.0, unc(1.0, 0.0, 0.0));
    let far = design("far", 3.0, unc(1.0, 0.0, 0.0));
    // 3σ-ish apart -> flip is unlikely.
    assert!(ranking_flip_probability(&a, &far) < 0.05);
    // a near tie -> flip probability near 0.5.
    let near = design("near", 0.1, unc(1.0, 0.0, 0.0));
    let p = ranking_flip_probability(&a, &near);
    assert!(p > 0.4 && p < 0.5);
}

#[test]
fn evpi_is_zero_for_a_robust_decision_and_positive_when_close() {
    let robust = [
        design("a", 0.0, unc(0.05, 0.0, 0.0)),
        design("b", 20.0, unc(0.05, 0.0, 0.0)),
    ];
    assert!(evpi(&robust) < 1e-6);
    let close = [
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
        design("c", 0.15, unc(0.1, 0.1, 0.1)),
    ];
    assert!(evpi(&close) > 0.0);
    // the posture names the two closest designs.
    assert_eq!(
        decision_posture(&close).map(|p| (p.best, p.runner_up)),
        Some(("a".to_string(), "c".to_string()))
    );
}

#[test]
fn non_finite_means_do_not_win_the_decision_boundary() {
    let designs = [
        design("nan", f64::NAN, unc(100.0, 0.0, 0.0)),
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
        design("c", 0.15, unc(0.1, 0.1, 0.1)),
    ];
    assert_eq!(
        decision_posture(&designs).map(|p| (p.best, p.runner_up)),
        Some(("a".to_string(), "c".to_string()))
    );
    assert!(evpi(&designs).is_finite());
    let insufficient = [
        design("nan", f64::NAN, unc(100.0, 0.0, 0.0)),
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
    ];
    assert_eq!(decision_posture(&insufficient), None);
    assert!(evpi(&insufficient).abs() <= f64::EPSILON);
}

#[test]
fn information_on_a_decision_irrelevant_design_is_worthless() {
    let designs = [
        design("a", 0.0, unc(0.1, 0.1, 0.1)),  // best, in the decision
        design("c", 0.15, unc(0.1, 0.1, 0.1)), // close runner-up, in the decision
        design("b", 20.0, unc(5.0, 0.0, 0.0)), // clearly last but VERY uncertain
    ];
    // reducing the boundary design's uncertainty has value...
    let on_boundary = action_value(&designs, &act("refine-a", ActionKind::Refine, "a", 1.0));
    assert!(on_boundary.value > 0.0);
    // ...but reducing the uncertain-yet-irrelevant design's has ~none.
    let on_irrelevant = action_value(&designs, &act("refine-b", ActionKind::Refine, "b", 1.0));
    assert!(on_irrelevant.value < 1e-9);
}

#[test]
fn a_robust_decision_recommends_stop() {
    let robust = [
        design("a", 0.0, unc(0.05, 0.0, 0.0)),
        design("b", 20.0, unc(0.05, 0.0, 0.0)),
    ];
    let actions = [act("refine-a", ActionKind::Refine, "a", 1.0)];
    assert!(matches!(
        recommend(&robust, &actions, 1e-3),
        Recommendation::Stop { .. }
    ));
}

#[test]
fn voi_beats_the_uncertainty_proportional_baseline() {
    let designs = [
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
        design("c", 0.15, unc(0.1, 0.1, 0.1)),
        design("b", 20.0, unc(5.0, 0.0, 0.0)), // most uncertain, decision-irrelevant
    ];
    let actions = [
        act("refine-a", ActionKind::Refine, "a", 1.0),
        act("refine-c", ActionKind::Refine, "c", 1.0),
        act("refine-b", ActionKind::Refine, "b", 1.0),
    ];
    // the uncertainty-proportional baseline chases the most-uncertain design (b).
    assert_eq!(
        heuristic_choice(&designs, &actions).unwrap().name,
        "refine-b"
    );
    // VOI spends on the DECISION boundary (a or c) instead.
    let rec = recommend(&designs, &actions, 1e-4);
    assert!(
        matches!(&rec, Recommendation::Act { action, .. } if action == "refine-a" || action == "refine-c"),
        "{rec:?}"
    );
}

#[test]
fn zero_cost_decision_changing_action_wins_per_cost_ranking() {
    let designs = [
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
        design("c", 0.15, unc(0.1, 0.1, 0.1)),
    ];
    let free_action = act("free-refine-c", ActionKind::Refine, "c", 0.0);
    let free = action_value(&designs, &free_action);
    let actions = [
        act("paid-refine-a", ActionKind::Refine, "a", 1.0),
        free_action,
    ];
    assert!(free.value > 0.0);
    assert!(free.value_per_cost.is_infinite());
    assert!(matches!(
        recommend(&designs, &actions, 1e-4),
        Recommendation::Act { action, .. } if action == "free-refine-c"
    ));
}

#[test]
fn invalid_cost_actions_are_not_recommended() {
    let designs = [
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
        design("c", 0.15, unc(0.1, 0.1, 0.1)),
    ];
    let negative = act("negative-cost", ActionKind::Refine, "c", -1.0);
    let nan_cost = act("nan-cost", ActionKind::Refine, "c", f64::NAN);
    assert!(action_value(&designs, &negative).value > 0.0);
    assert!(action_value(&designs, &negative).value_per_cost <= f64::EPSILON);
    assert!(action_value(&designs, &nan_cost).value > 0.0);
    assert!(action_value(&designs, &nan_cost).value_per_cost <= f64::EPSILON);
    assert!(matches!(
        recommend(&designs, &[negative, nan_cost], 1e-4),
        Recommendation::Stop { .. }
    ));
}

#[test]
fn voi_escalates_model_fidelity_when_model_uncertainty_dominates() {
    // the decision boundary is blocked by MODEL uncertainty (0.5), not statistical.
    let designs = [
        design("a", 0.0, unc(0.01, 0.01, 0.5)),
        design("c", 0.1, unc(0.01, 0.01, 0.5)),
    ];
    assert_eq!(designs[1].uncertainty.dominant(), Component::Model);
    let actions = [
        act("sample-c", ActionKind::Sample, "c", 1.0), // reduces statistical (tiny)
        act("test-c", ActionKind::Test, "c", 1.0),     // reduces model (dominant)
    ];
    // sampling barely helps; a physical test resolves the decision -> VOI escalates to Test.
    assert!(action_value(&designs, &actions[1]).value > action_value(&designs, &actions[0]).value);
    let rec = recommend(&designs, &actions, 1e-4);
    assert!(
        matches!(&rec, Recommendation::Act { action, .. } if action == "test-c"),
        "{rec:?}"
    );
}

#[test]
fn voi_is_deterministic() {
    let designs = [
        design("a", 0.0, unc(0.1, 0.1, 0.1)),
        design("c", 0.12, unc(0.1, 0.1, 0.1)),
    ];
    let actions = [act("refine-a", ActionKind::Refine, "a", 1.0)];
    assert_eq!(
        recommend(&designs, &actions, 1e-4),
        recommend(&designs, &actions, 1e-4)
    );
}
