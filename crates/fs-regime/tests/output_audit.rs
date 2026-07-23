//! Product-boundary regime-demotion conformance for `f85xj.8.3`.

use fs_evidence::{Ambition, Color, ModelCard, ValidityDomain};
use fs_regime::{
    AxisViolationKind, EnvelopeCoverage, OperatingPoint, OverrideAcknowledgement, QoiClaim,
    audit_product_output,
};
use std::collections::{BTreeMap, BTreeSet};

fn point(id: &str, axes: &[(&str, f64)]) -> OperatingPoint {
    OperatingPoint {
        id: id.to_string(),
        groups: axes
            .iter()
            .map(|(axis, value)| ((*axis).to_string(), *value))
            .collect(),
    }
}

fn card(name: &str, lo: f64, hi: f64) -> ModelCard {
    ModelCard::new(
        name,
        "1.2.3",
        Ambition::Solid,
        vec![],
        ValidityDomain::unconstrained().with("Re", lo, hi),
        vec![],
        0.05,
    )
}

fn claim(cards: &[&str], acknowledgement: Option<OverrideAcknowledgement>) -> QoiClaim {
    QoiClaim {
        qoi: "drag-coefficient".to_string(),
        color: Color::Validated {
            regime: ValidityDomain::unconstrained().with("Re", 10.0, 100.0),
            dataset: "cylinder-corpus-v1".to_string(),
        },
        model_cards: cards.iter().map(|name| (*name).to_string()).collect(),
        override_acknowledgement: acknowledgement,
    }
}

#[test]
fn audits_every_card_and_partitions_partial_sweeps_without_averaging() {
    let registry = vec![card("closure", 10.0, 100.0), card("wall-law", 20.0, 80.0)];
    let points = vec![
        point("inside", &[("Re", 50.0)]),
        point("low", &[("Re", 5.0)]),
        point("high", &[("Re", 90.0)]),
    ];
    let audit = audit_product_output(&registry, &points, &[claim(&["wall-law", "closure"], None)])
        .expect("valid audit");
    let receipt = &audit.receipts[0];

    assert_eq!(receipt.coverage, EnvelopeCoverage::Partial);
    assert_eq!(receipt.in_domain_points, ["inside"]);
    assert_eq!(receipt.out_of_domain_points, ["high", "low"]);
    assert_eq!(
        receipt
            .model_cards
            .iter()
            .map(|card| card.name.as_str())
            .collect::<Vec<_>>(),
        ["closure", "wall-law"]
    );
    assert!(
        receipt
            .model_cards
            .iter()
            .all(|card| card.version == "1.2.3")
    );
    assert_eq!(receipt.violations.len(), 3);
    assert!(receipt.violations.iter().any(|violation| {
        violation.point == "low"
            && violation.card == "closure"
            && violation.kind == AxisViolationKind::Below
    }));
    assert!(receipt.violations.iter().any(|violation| {
        violation.point == "low"
            && violation.card == "wall-law"
            && violation.kind == AxisViolationKind::Below
    }));
    assert!(receipt.violations.iter().any(|violation| {
        violation.point == "high"
            && violation.card == "wall-law"
            && violation.kind == AxisViolationKind::Above
    }));
    assert!(matches!(
        receipt.effective_color,
        Color::Estimated { dispersion, .. } if dispersion.is_infinite()
    ));
    assert_eq!(
        receipt.in_domain_color,
        Some(receipt.original_color.clone())
    );
    assert_eq!(
        receipt.out_of_domain_color,
        Some(receipt.effective_color.clone())
    );
    let no_claim = receipt.no_claim_markdown().expect("demotion renders");
    assert!(no_claim.contains("2 of 3 operating points"));
    assert!(no_claim.contains("`wall-law` / `Re`"));
    assert!(!no_claim.contains("coverage probability"));
}

#[test]
fn shrinking_a_domain_only_preserves_or_expands_the_out_partition() {
    let points = vec![
        point("p20", &[("Re", 20.0)]),
        point("p50", &[("Re", 50.0)]),
        point("p80", &[("Re", 80.0)]),
    ];
    let wide = audit_product_output(
        &[card("closure", 10.0, 90.0)],
        &points,
        &[claim(&["closure"], None)],
    )
    .expect("wide audit");
    let narrow = audit_product_output(
        &[card("closure", 30.0, 70.0)],
        &points,
        &[claim(&["closure"], None)],
    )
    .expect("narrow audit");
    let wide_out = wide.receipts[0]
        .out_of_domain_points
        .iter()
        .collect::<BTreeSet<_>>();
    let narrow_out = narrow.receipts[0]
        .out_of_domain_points
        .iter()
        .collect::<BTreeSet<_>>();

    assert!(wide_out.is_subset(&narrow_out));
    assert_eq!(narrow.receipts[0].coverage, EnvelopeCoverage::Partial);
}

#[test]
fn fully_in_domain_output_preserves_color_and_emits_no_no_claim_entry() {
    let original = claim(&["closure"], None);
    let audit = audit_product_output(
        &[card("closure", 10.0, 100.0)],
        &[point("inside", &[("Re", 50.0)])],
        &[original.clone()],
    )
    .expect("in-domain audit");
    let receipt = &audit.receipts[0];

    assert_eq!(receipt.coverage, EnvelopeCoverage::FullyInDomain);
    assert_eq!(receipt.effective_color, original.color);
    assert_eq!(
        receipt.in_domain_color,
        Some(receipt.original_color.clone())
    );
    assert_eq!(receipt.out_of_domain_color, None);
    assert_eq!(receipt.no_claim_markdown(), None);
}

#[test]
fn override_is_recorded_but_cannot_restore_color() {
    let registry = vec![card("closure", 10.0, 100.0)];
    let points = vec![point("outside", &[("Re", 1_000.0)])];
    let without = audit_product_output(&registry, &points, &[claim(&["closure"], None)])
        .expect("audit without override");
    let acknowledgement = OverrideAcknowledgement {
        actor: "reviewer-7".to_string(),
        reason: "exploratory-only".to_string(),
    };
    let with = audit_product_output(
        &registry,
        &points,
        &[claim(&["closure"], Some(acknowledgement.clone()))],
    )
    .expect("audit with override");

    assert_eq!(
        without.receipts[0].effective_color,
        with.receipts[0].effective_color
    );
    assert_eq!(
        with.receipts[0].override_acknowledgement,
        Some(acknowledgement)
    );
    let no_claim = with.receipts[0]
        .no_claim_markdown()
        .expect("demotion renders");
    assert!(no_claim.contains("acknowledgement does not restore color"));
}

#[test]
fn every_input_color_class_demotes_to_an_unbounded_estimate() {
    let registry = vec![card("closure", 10.0, 100.0)];
    let points = vec![point("outside", &[("Re", 1_000.0)])];
    let colors = [
        Color::Verified { lo: 0.9, hi: 1.1 },
        Color::Validated {
            regime: ValidityDomain::unconstrained().with("Re", 10.0, 100.0),
            dataset: "cylinder-corpus-v1".to_string(),
        },
        Color::Estimated {
            estimator: "cross-model-v2".to_string(),
            dispersion: 0.2,
        },
    ];

    for (index, color) in colors.into_iter().enumerate() {
        let mut input = claim(&["closure"], None);
        input.qoi = format!("qoi-{index}");
        input.color = color;
        let audit = audit_product_output(&registry, &points, &[input]).expect("valid audit");
        assert!(matches!(
            audit.receipts[0].effective_color,
            Color::Estimated { dispersion, .. } if dispersion.is_infinite()
        ));
    }
}

#[test]
fn canonical_receipts_are_input_order_independent_and_distance_scored() {
    let registry = vec![card("z-card", 10.0, 100.0), card("a-card", 1.0, 10.0)];
    let mut first_claim = claim(&["z-card", "a-card"], None);
    first_claim.qoi = "z-qoi".to_string();
    let mut second_claim = claim(&["a-card"], None);
    second_claim.qoi = "a-qoi".to_string();
    let points = vec![
        point("z-point", &[("Re", 1_000.0)]),
        point("a-point", &[("Re", 5.0)]),
    ];

    let first = audit_product_output(
        &registry,
        &points,
        &[first_claim.clone(), second_claim.clone()],
    )
    .expect("first audit");
    let second = audit_product_output(
        &[registry[1].clone(), registry[0].clone()],
        &[points[1].clone(), points[0].clone()],
        &[second_claim, first_claim],
    )
    .expect("reordered audit");

    assert_eq!(first, second);
    assert_eq!(first.receipts[0].qoi, "a-qoi");
    assert!(
        first
            .receipts
            .iter()
            .flat_map(|receipt| &receipt.violations)
            .all(|violation| violation.distance > 0.0)
    );
    let decade = first
        .receipts
        .iter()
        .flat_map(|receipt| &receipt.violations)
        .find(|violation| violation.point == "z-point" && violation.card == "z-card")
        .expect("decade violation");
    assert!((decade.distance - 1.0).abs() < 1.0e-12);
    assert_eq!(
        first.receipts[0].to_canonical_json(),
        second.receipts[0].to_canonical_json()
    );
}

#[test]
fn missing_axis_is_a_named_unit_distance_violation() {
    let audit = audit_product_output(
        &[card("closure", 10.0, 100.0)],
        &[OperatingPoint {
            id: "missing-re".to_string(),
            groups: BTreeMap::new(),
        }],
        &[claim(&["closure"], None)],
    )
    .expect("missing axes demote instead of disappearing");
    let violation = &audit.receipts[0].violations[0];

    assert_eq!(violation.kind, AxisViolationKind::Missing);
    assert_eq!(violation.observed, None);
    assert_eq!(violation.distance, 1.0);
}
