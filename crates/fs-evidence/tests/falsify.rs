//! Falsifier-pairing conformance (addendum Proposal 6, the qmao.4 bead).
//! Acceptance: no certificate class registers without ≥1 falsifier; the
//! consequence×doubt allocator concentrates budget correctly (monotone in
//! both factors, honest cold-start/floor boundaries); a falsifier hit
//! auto-creates a tombstone + estimator bug report; the
//! no-falsifier-no-ship Gauntlet gate blocks unpaired classes; yield-less
//! falsifiers pay rent via automatic budget-share decay.

use fs_evidence::{
    ClaimContext, FalsifierHistory, FalsifierHit, FalsifierRegistry, FalsifierSpec, FalsifyError,
    allocate_budget,
    falsify::{CONSEQUENCE_FLOOR, DOUBT_COLD_START, DOUBT_FLOOR, RENT_SHARE_FLOOR, RENT_VOLUME},
};

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-evidence/falsify\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

fn claim(class: &str, regime: &str, consequence: f64) -> ClaimContext {
    ClaimContext {
        class: class.to_string(),
        regime: regime.to_string(),
        consequence,
    }
}

#[test]
fn fp_001_registration_requires_a_falsifier() {
    let mut r = FalsifierRegistry::new();
    // Empty falsifier list REFUSES — the rule at its source.
    let refusal = r.register("watertightness", Vec::new());
    assert_eq!(
        refusal,
        Err(FalsifyError::NoFalsifier {
            class: "watertightness".to_string()
        })
    );
    // With a falsifier it registers; duplicates refuse.
    r.register(
        "watertightness",
        vec![FalsifierSpec {
            name: "ray-parity-sampling".to_string(),
            method: "independent ray crossings".to_string(),
        }],
    )
    .expect("registers");
    assert!(matches!(
        r.register(
            "watertightness",
            vec![FalsifierSpec {
                name: "x".to_string(),
                method: "y".to_string()
            }]
        ),
        Err(FalsifyError::Duplicate { .. })
    ));
    // The STARTING REGISTRY covers the six proposal pairings.
    let std_reg = FalsifierRegistry::standard();
    for class in [
        "watertightness",
        "conservation",
        "adjoint-gradient",
        "surrogate-accept",
        "symmetry-block-solve",
        "validated-color",
    ] {
        let fs = std_reg.falsifiers(class).expect("registered");
        assert!(!fs.is_empty(), "{class} must carry a falsifier");
        assert!(!fs[0].method.is_empty(), "{class}: the method is stated");
    }
    assert!(std_reg.falsifiers("warp-drive").is_err());
    verdict(
        "fp-001",
        "empty registration refused; duplicates refused; standard registry covers all six",
    );
}

#[test]
fn fp_002_budget_allocator_monotone_with_honest_boundaries() {
    let mut history = FalsifierHistory::new();
    // Build asymmetric history: class A has a strong record in regime r1,
    // class B has embarrassed itself.
    for _ in 0..99 {
        history.record_pass("A", "r1", 1.0);
    }
    history.record_pass("B", "r1", 1.0);
    let (t, b) = history.record_hit(
        &FalsifierHit {
            class: "B".to_string(),
            regime: "r1".to_string(),
            falsifier: "global-flux-audit".to_string(),
            detail: "flux imbalance 3e-2".to_string(),
        },
        1.0,
    );
    let _ = (t, b);
    // Doubt: cold start is MAX; strong record floors, never zero.
    assert!(
        (history.doubt("C", "r1") - DOUBT_COLD_START).abs() < 1e-12,
        "cold start"
    );
    let doubt_a = history.doubt("A", "r1");
    assert!(
        (DOUBT_FLOOR..0.06).contains(&doubt_a),
        "perfect-ish record floors: {doubt_a}"
    );
    let doubt_b = history.doubt("B", "r1");
    assert!(doubt_b > 0.4, "an embarrassed class is doubted: {doubt_b}");
    // Monotone in doubt (same consequence).
    let claims = vec![
        claim("A", "r1", 5.0),
        claim("B", "r1", 5.0),
        claim("C", "r1", 5.0),
    ];
    let alloc = allocate_budget(100.0, &claims, &history);
    assert!(
        (alloc.iter().sum::<f64>() - 100.0).abs() < 1e-9,
        "budget fully spent"
    );
    assert!(
        alloc[0] < alloc[1] && alloc[1] < alloc[2],
        "doubt-monotone: {alloc:?} (A trusted < B embarrassed < C unknown)"
    );
    // Monotone in consequence (same class/doubt).
    let claims2 = vec![claim("B", "r1", 1.0), claim("B", "r1", 10.0)];
    let alloc2 = allocate_budget(50.0, &claims2, &history);
    assert!(alloc2[0] < alloc2[1], "consequence-monotone: {alloc2:?}");
    assert!(
        (alloc2[1] / alloc2[0] - 10.0).abs() < 1e-9,
        "proportional to consequence"
    );
    // Boundaries: zero-dependents claim gets a nonzero floor share;
    // zero claims spend zero.
    let claims3 = vec![claim("B", "r1", 0.0), claim("B", "r1", 1.0)];
    let alloc3 = allocate_budget(10.0, &claims3, &history);
    assert!(alloc3[0] > 0.0, "no-dependents claim keeps a floor share");
    assert!(
        (alloc3[0] / alloc3[1] - CONSEQUENCE_FLOOR).abs() < 1e-9,
        "floor ratio is the declared constant"
    );
    assert!(
        allocate_budget(10.0, &[], &history).is_empty(),
        "no claims, no spend"
    );
    verdict(
        "fp-002",
        "monotone in consequence and doubt; cold-start max, perfect-record floor, \
         dependent-free floor, empty-job zero",
    );
}

#[test]
fn fp_003_hit_wiring_creates_tombstone_and_bug_report() {
    let mut history = FalsifierHistory::new();
    let hit = FalsifierHit {
        class: "adjoint-gradient".to_string(),
        regime: "Re~1e3".to_string(),
        falsifier: "finite-difference-spot-check".to_string(),
        detail: "FD says 0.031, tape says 0.29 along direction 7".to_string(),
    };
    let (tombstone, bug) = history.record_hit(&hit, 2.5);
    for (name, json) in [("tombstone", &tombstone.json), ("bug", &bug.json)] {
        assert!(json.contains("adjoint-gradient"), "{name} names the class");
        assert!(
            json.contains("finite-difference-spot-check"),
            "{name} names the catcher"
        );
        assert!(json.contains("0.031"), "{name} carries the evidence");
    }
    assert!(tombstone.json.contains("\"kind\":\"tombstone\""));
    assert!(bug.json.contains("\"kind\":\"estimator-bug\""));
    // The hit moved the class's doubt and yield.
    assert!(history.doubt("adjoint-gradient", "Re~1e3") > 0.9);
    let (hits, compute, runs) = history.yield_of("adjoint-gradient");
    assert_eq!((hits, runs), (1, 1));
    assert!((compute - 2.5).abs() < 1e-12);
    verdict(
        "fp-003",
        "hit -> tombstone + estimator bug, both canonical; doubt/yield updated",
    );
}

#[test]
fn fp_004_no_falsifier_no_ship_gate() {
    let registry = FalsifierRegistry::standard();
    // All-registered ships.
    assert!(
        registry
            .ship_gate(&["watertightness", "conservation"])
            .is_empty(),
        "paired classes pass the gate"
    );
    // An unpaired class BLOCKS, named.
    let violations = registry.ship_gate(&["watertightness", "novel-certificate-v0"]);
    assert_eq!(violations, vec!["novel-certificate-v0".to_string()]);
    verdict(
        "fp-004",
        "gate passes paired classes, blocks and names unpaired ones",
    );
}

#[test]
fn fp_005_rent_review_decays_yieldless_falsifiers() {
    let mut history = FalsifierHistory::new();
    // "quiet" runs at meaningful volume with zero hits; "worker" catches.
    for _ in 0..RENT_VOLUME {
        history.record_pass("quiet", "r", 0.5);
    }
    for _ in 0..RENT_VOLUME {
        history.record_pass("worker", "r", 0.5);
    }
    let _ = history.record_hit(
        &FalsifierHit {
            class: "worker".to_string(),
            regime: "r".to_string(),
            falsifier: "f".to_string(),
            detail: "caught one".to_string(),
        },
        0.5,
    );
    // Below-volume class is exempt.
    for _ in 0..10 {
        history.record_pass("young", "r", 0.5);
    }
    let decayed = history.rent_review();
    assert_eq!(
        decayed.len(),
        1,
        "only the yield-less at-volume class decays: {decayed:?}"
    );
    assert_eq!(decayed[0].0, "quiet");
    assert!((history.share("quiet") - 0.5).abs() < 1e-12);
    assert!(
        (history.share("worker") - 1.0).abs() < 1e-12,
        "rent-payers keep their share"
    );
    assert!(
        (history.share("young") - 1.0).abs() < 1e-12,
        "low-volume classes are exempt"
    );
    // Repeated reviews floor, never kill (the pairing rule survives).
    for _ in 0..10 {
        let _ = history.rent_review();
    }
    assert!(
        history.share("quiet") >= RENT_SHARE_FLOOR,
        "share floors at {RENT_SHARE_FLOOR}, never zero"
    );
    // The decayed share flows into allocation.
    let claims = vec![claim("quiet", "r", 1.0), claim("worker", "r", 1.0)];
    let alloc = allocate_budget(10.0, &claims, &history);
    assert!(
        alloc[0] < alloc[1],
        "decayed share reduces allocation: {alloc:?}"
    );
    verdict(
        "fp-005",
        "zero-yield at-volume class decays to a floor; payers and young classes keep share; \
         decay flows into the allocator",
    );
}
