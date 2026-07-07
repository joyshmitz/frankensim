//! Battery for the addendum risk register (Part V, R1–R10). Covers register
//! completeness + ordering, per-risk field non-emptiness, lookup, the audit
//! (complete + instrumented counts + gap detection on a deliberately
//! incomplete slice), JSON well-formedness, and determinism.

use fs_govern::{Risk, RiskId, audit, audit_slice, register, risk, to_json};

#[test]
fn register_has_all_ten_risks_in_order() {
    let reg = register();
    assert_eq!(reg.len(), 10);
    for (r, id) in reg.iter().zip(RiskId::ALL) {
        assert_eq!(r.id, id);
    }
    // codes are R1..R10.
    let codes: Vec<&str> = reg.iter().map(|r| r.id.code()).collect();
    assert_eq!(
        codes,
        vec!["R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10"]
    );
}

#[test]
fn every_risk_has_a_metric_owner_and_mitigation() {
    for r in register() {
        assert!(!r.name.is_empty(), "{:?} name", r.id);
        assert!(!r.description.is_empty(), "{:?} description", r.id);
        assert!(!r.mitigation.is_empty(), "{:?} mitigation", r.id);
        assert!(!r.early_warning.is_empty(), "{:?} early_warning", r.id);
        assert!(!r.threshold.is_empty(), "{:?} threshold", r.id);
        assert!(!r.owner.is_empty(), "{:?} owner", r.id);
    }
}

#[test]
fn owners_are_real_addendum_bead_ids_or_governance() {
    for r in register() {
        assert!(
            r.owner.starts_with("frankensim-"),
            "{:?} owner should be a bead id, got {}",
            r.id,
            r.owner
        );
    }
}

#[test]
fn lookup_returns_the_right_risk() {
    let r3 = risk(RiskId::R3);
    assert_eq!(r3.id, RiskId::R3);
    assert_eq!(r3.name, "Stable entity identity");
    // R1 is the Proposal-9 estimator-constants risk.
    assert_eq!(risk(RiskId::R1).owner, "frankensim-epic-flywheel-lmp4.1");
}

#[test]
fn audit_of_the_canonical_register_is_complete() {
    let a = audit();
    assert_eq!(a.total, 10);
    assert_eq!(a.complete, 10, "every risk must have a metric and an owner");
    assert!(a.ok(), "no gaps: {:?}", a.gaps);
    // honest baseline: nothing is instrumented yet.
    assert_eq!(a.instrumented, 0);
}

#[test]
fn audit_detects_a_missing_metric_or_owner() {
    // a deliberately incomplete risk must be caught (the audit is not vacuous).
    let bad = [
        Risk {
            id: RiskId::R1,
            name: "x",
            description: "x",
            mitigation: "x",
            early_warning: "", // missing
            threshold: "x",
            owner: "", // missing
            instrumented: false,
        },
        Risk {
            id: RiskId::R2,
            name: "y",
            description: "y",
            mitigation: "y",
            early_warning: "a metric",
            threshold: "y",
            owner: "frankensim-epic-x",
            instrumented: true,
        },
    ];
    let a = audit_slice(&bad);
    assert_eq!(a.total, 2);
    assert_eq!(a.complete, 1);
    assert_eq!(a.instrumented, 1);
    assert!(!a.ok());
    // both a missing-metric and a missing-owner gap on R1.
    assert!(
        a.gaps
            .iter()
            .any(|(id, why)| *id == RiskId::R1 && why.contains("metric"))
    );
    assert!(
        a.gaps
            .iter()
            .any(|(id, why)| *id == RiskId::R1 && why.contains("owner"))
    );
}

#[test]
fn json_is_well_formed_and_complete() {
    let j = to_json();
    assert!(j.starts_with('[') && j.ends_with(']'));
    // one object per risk.
    assert_eq!(j.matches("\"id\":\"R").count(), 10);
    for id in RiskId::ALL {
        assert!(
            j.contains(&format!("\"id\":\"{}\"", id.code())),
            "missing {}",
            id.code()
        );
    }
    // owner bead ids and the instrumented flag are present.
    assert!(j.contains("frankensim-epic-flywheel-lmp4.1"));
    assert!(j.contains("\"instrumented\":false"));
    // no accidental double-commas between objects.
    assert!(!j.contains(",,"));
}

#[test]
fn register_json_and_audit_are_deterministic() {
    assert_eq!(to_json(), to_json());
    assert_eq!(audit(), audit());
}
