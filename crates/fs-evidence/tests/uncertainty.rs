//! G0/G3 laws for the eight-term engineering uncertainty budget.

use fs_blake3::{ContentHash, hash_domain};
use fs_evidence::uncertainty::{
    BudgetTotal, CovarianceBlock, DistributionTerm, DominantEngineeringTerm,
    EngineeringUncertaintyBudget, EngineeringUncertaintyKind, EngineeringUncertaintyTerm,
    EnsembleTerm, NumericalUncertaintyUpdate, TermValue, UncertaintyArtifactRef, UncertaintyRule,
};

fn digest(label: &str) -> ContentHash {
    hash_domain("org.frankensim.test.uncertainty.v1", label.as_bytes())
}

fn artifact(label: &str) -> UncertaintyArtifactRef {
    UncertaintyArtifactRef::new(label, digest(label)).expect("valid artifact fixture")
}

fn term(kind: EngineeringUncertaintyKind, value: TermValue) -> EngineeringUncertaintyTerm {
    EngineeringUncertaintyTerm::try_new(kind, value, artifact(kind.name()))
        .expect("valid term fixture")
}

fn negligible_terms() -> Vec<EngineeringUncertaintyTerm> {
    EngineeringUncertaintyKind::ALL
        .into_iter()
        .map(|kind| {
            term(
                kind,
                TermValue::negligible(format!("{} is zero in this analytic fixture", kind.name()))
                    .expect("non-empty justification"),
            )
        })
        .collect()
}

fn replace_term(
    terms: &mut [EngineeringUncertaintyTerm],
    kind: EngineeringUncertaintyKind,
    value: TermValue,
) {
    let slot = terms
        .iter_mut()
        .find(|term| term.kind() == kind)
        .expect("all eight fixture terms exist");
    *slot = term(kind, value);
}

fn budget(terms: Vec<EngineeringUncertaintyTerm>) -> EngineeringUncertaintyBudget {
    EngineeringUncertaintyBudget::try_new("temperature:max", "kelvin", terms)
        .expect("valid complete budget")
}

#[test]
fn all_term_representations_round_trip_with_stable_identity() {
    let covariance = CovarianceBlock::try_new(
        "mesh-solver-joint",
        artifact("covariance:mesh-solver"),
        vec![
            EngineeringUncertaintyKind::SolverAlgebraic,
            EngineeringUncertaintyKind::Discretization,
        ],
        vec![4.0, 1.0, 1.0, 9.0],
    )
    .expect("analytic covariance is positive definite");
    let mut terms = negligible_terms();
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Roundoff,
        TermValue::interval(0.0, 0.01).expect("interval"),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::SolverAlgebraic,
        TermValue::CorrelatedBlock(covariance.clone()),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Discretization,
        TermValue::CorrelatedBlock(covariance),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Geometry,
        TermValue::Distribution(DistributionTerm {
            mean: 0.0,
            standard_deviation: 0.2,
            conservative_half_width: 0.6,
            level: 0.997,
            replay: artifact("distribution:geometry"),
        }),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Parameters,
        TermValue::Ensemble(EnsembleTerm {
            member_count: 32,
            conservative_half_width: 0.8,
            replay: artifact("ensemble:parameters"),
        }),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::BoundaryConditions,
        TermValue::unknown("fan tolerance has no retained population authority").expect("unknown"),
    );
    let original = budget(terms);
    let bytes = original.canonical_bytes();
    let decoded = EngineeringUncertaintyBudget::decode(&bytes).expect("canonical round trip");
    assert_eq!(decoded, original);
    assert_eq!(decoded.canonical_bytes(), bytes);
    assert_eq!(decoded.content_id(), original.content_id());

    let report = original.render_report();
    assert_eq!(report, original.render_report());
    for kind in EngineeringUncertaintyKind::ALL {
        assert!(report.contains(&format!("- {}:", kind.name())));
    }
    assert_eq!(report.matches("provenance=").count(), 8);
    assert!(report.contains("total=unknown"));

    let mut with_trailing_byte = bytes;
    with_trailing_byte.push(0);
    assert!(EngineeringUncertaintyBudget::decode(&with_trailing_byte).is_err());
}

#[test]
fn empty_unknown_and_negligible_content_refuses() {
    let unknown = TermValue::unknown(" \n").expect_err("blank unknown reason must refuse");
    assert_eq!(unknown.rule(), UncertaintyRule::TextBounds);
    let negligible =
        TermValue::negligible("").expect_err("empty negligible justification must refuse");
    assert_eq!(negligible.rule(), UncertaintyRule::TextBounds);
}

#[test]
fn explicit_covariance_block_matches_analytic_total() {
    let block = CovarianceBlock::try_new(
        "solver-discretization",
        artifact("covariance:analytic"),
        vec![
            EngineeringUncertaintyKind::SolverAlgebraic,
            EngineeringUncertaintyKind::Discretization,
        ],
        vec![4.0, 1.0, 1.0, 9.0],
    )
    .expect("positive definite block");
    let mut terms = negligible_terms();
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::SolverAlgebraic,
        TermValue::CorrelatedBlock(block.clone()),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Discretization,
        TermValue::CorrelatedBlock(block),
    );
    let total = budget(terms).total();
    let BudgetTotal::Bounded {
        conservative_half_width,
    } = total
    else {
        panic!("fully known covariance fixture must be bounded");
    };
    let analytic = 15.0_f64.sqrt();
    assert!(conservative_half_width >= analytic);
    assert!(conservative_half_width - analytic <= 2.0 * f64::EPSILON * analytic);
}

#[test]
fn incomplete_or_invalid_covariance_authority_refuses() {
    let asymmetric = CovarianceBlock::try_new(
        "asymmetric",
        artifact("covariance:asymmetric"),
        vec![
            EngineeringUncertaintyKind::SolverAlgebraic,
            EngineeringUncertaintyKind::Discretization,
        ],
        vec![1.0, 0.2, 0.1, 1.0],
    )
    .expect_err("asymmetric covariance must refuse");
    assert_eq!(asymmetric.rule(), UncertaintyRule::CovarianceMatrix);

    let indefinite = CovarianceBlock::try_new(
        "indefinite",
        artifact("covariance:indefinite"),
        vec![
            EngineeringUncertaintyKind::SolverAlgebraic,
            EngineeringUncertaintyKind::Discretization,
        ],
        vec![1.0, 2.0, 2.0, 1.0],
    )
    .expect_err("symmetric indefinite covariance must refuse");
    assert_eq!(indefinite.rule(), UncertaintyRule::CovarianceMatrix);

    let block = CovarianceBlock::try_new(
        "incomplete",
        artifact("covariance:incomplete"),
        vec![
            EngineeringUncertaintyKind::SolverAlgebraic,
            EngineeringUncertaintyKind::Discretization,
        ],
        vec![1.0, 0.0, 0.0, 1.0],
    )
    .expect("valid block declaration");
    let mut terms = negligible_terms();
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::SolverAlgebraic,
        TermValue::CorrelatedBlock(block),
    );
    let error = EngineeringUncertaintyBudget::try_new("temperature:max", "kelvin", terms)
        .expect_err("every declared block member must reference the block");
    assert_eq!(error.rule(), UncertaintyRule::CovarianceMembership);
}

#[test]
fn unknown_terms_poison_totals_and_dominance_without_erasing_known_work() {
    let mut terms = negligible_terms();
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Roundoff,
        TermValue::interval(0.1, 0.2).expect("interval"),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Geometry,
        TermValue::unknown("as-built registration receipt is absent").expect("unknown"),
    );
    let budget = budget(terms);
    assert_eq!(
        budget.total(),
        BudgetTotal::Unknown {
            known_conservative_half_width: 0.2,
            unknown_terms: vec![EngineeringUncertaintyKind::Geometry],
        }
    );
    assert_eq!(
        budget.dominant(),
        DominantEngineeringTerm::Unknown {
            terms: vec![EngineeringUncertaintyKind::Geometry],
        }
    );
    assert!(
        budget
            .project_legacy(300.0)
            .expect("nonzero reference")
            .breakdown()
            .numerical_rel
            .is_infinite()
    );
}

#[test]
fn legacy_projection_accounts_for_every_source_without_silent_zero() {
    let mut terms = negligible_terms();
    for (index, kind) in EngineeringUncertaintyKind::ALL.into_iter().enumerate() {
        replace_term(
            &mut terms,
            kind,
            TermValue::interval(0.0, (index + 1) as f64).expect("interval"),
        );
    }
    let projection = budget(terms)
        .project_legacy(1.0)
        .expect("unit reference preserves fixture magnitudes");
    let breakdown = projection.breakdown();
    assert!(breakdown.numerical_rel >= 10.0);
    assert!(breakdown.statistical_rel >= 19.0);
    assert!(breakdown.model_rel >= 7.0);
    assert_eq!(projection.numerical_sources().len(), 4);
    assert_eq!(projection.statistical_sources().len(), 3);
    assert_eq!(
        projection.model_sources(),
        &[EngineeringUncertaintyKind::ModelForm]
    );
    assert_ne!(projection.original_budget(), ContentHash([0; 32]));
    assert_eq!(projection.reference_magnitude(), 1.0);
}

#[test]
fn legacy_projection_requires_a_physical_reference_scale() {
    let budget = budget(negligible_terms());
    for invalid in [0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let error = budget
            .project_legacy(invalid)
            .expect_err("invalid reference magnitude must refuse");
        assert_eq!(error.rule(), UncertaintyRule::NumericDomain);
    }
}

#[test]
fn composition_is_monotone_and_never_launders_unknowns() {
    for left in [0.0, 0.125, 1.0, 16.0] {
        for right in [0.0, 0.25, 2.0, 32.0] {
            let mut left_terms = negligible_terms();
            replace_term(
                &mut left_terms,
                EngineeringUncertaintyKind::Parameters,
                TermValue::Distribution(DistributionTerm {
                    mean: 0.0,
                    standard_deviation: left / 3.0,
                    conservative_half_width: left,
                    level: 0.99,
                    replay: artifact("distribution:left"),
                }),
            );
            let mut right_terms = negligible_terms();
            replace_term(
                &mut right_terms,
                EngineeringUncertaintyKind::Parameters,
                TermValue::Ensemble(EnsembleTerm {
                    member_count: 4,
                    conservative_half_width: right,
                    replay: artifact("ensemble:right"),
                }),
            );
            let composed = budget(left_terms)
                .compose(&budget(right_terms))
                .expect("compatible budgets compose");
            let TermValue::IntervalBound { upper, .. } = composed
                .term(EngineeringUncertaintyKind::Parameters)
                .value()
            else {
                panic!("mixed rich terms must conservatively degrade to an interval");
            };
            assert!(*upper >= left);
            assert!(*upper >= right);
            assert!(*upper >= left + right);
        }
    }

    let known = budget(negligible_terms());
    let mut unknown_terms = negligible_terms();
    replace_term(
        &mut unknown_terms,
        EngineeringUncertaintyKind::Measurement,
        TermValue::unknown("sensor calibration is not retained").expect("unknown"),
    );
    let composed = known
        .compose(&budget(unknown_terms))
        .expect("compatible budgets compose");
    assert!(matches!(
        composed
            .term(EngineeringUncertaintyKind::Measurement)
            .value(),
        TermValue::Unknown { .. }
    ));
    assert!(matches!(composed.total(), BudgetTotal::Unknown { .. }));
}

#[test]
fn composition_provenance_binds_exact_parent_budgets() {
    let mut left_small_terms = negligible_terms();
    replace_term(
        &mut left_small_terms,
        EngineeringUncertaintyKind::Geometry,
        TermValue::interval(0.0, 0.5).expect("interval"),
    );
    let mut left_large_terms = negligible_terms();
    replace_term(
        &mut left_large_terms,
        EngineeringUncertaintyKind::Geometry,
        TermValue::interval(0.0, 1.5).expect("interval"),
    );
    let right = budget(negligible_terms());
    let small = budget(left_small_terms)
        .compose(&right)
        .expect("compatible budgets compose");
    let large = budget(left_large_terms)
        .compose(&right)
        .expect("compatible budgets compose");

    assert_ne!(small.content_id(), large.content_id());
    assert_ne!(
        small
            .term(EngineeringUncertaintyKind::Geometry)
            .provenance()
            .digest(),
        large
            .term(EngineeringUncertaintyKind::Geometry)
            .provenance()
            .digest()
    );
}

#[test]
fn numerical_updates_cannot_name_or_rewrite_model_or_measurement_sources() {
    let mut terms = negligible_terms();
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::ModelForm,
        TermValue::unknown("no held-out experiment anchors the closure model").expect("unknown"),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Measurement,
        TermValue::interval(0.4, 0.8).expect("measurement interval"),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Geometry,
        TermValue::Ensemble(EnsembleTerm {
            member_count: 8,
            conservative_half_width: 0.3,
            replay: artifact("ensemble:geometry-preserved"),
        }),
    );
    let original = budget(terms);
    let update = NumericalUncertaintyUpdate::try_new(vec![
        term(
            EngineeringUncertaintyKind::Discretization,
            TermValue::interval(0.0, 0.03).expect("discretization bound"),
        ),
        term(
            EngineeringUncertaintyKind::Roundoff,
            TermValue::interval(0.0, 0.01).expect("roundoff bound"),
        ),
        term(
            EngineeringUncertaintyKind::SolverAlgebraic,
            TermValue::interval(0.0, 0.02).expect("solver bound"),
        ),
    ])
    .expect("the three numerical sources admit in any input order");
    let updated = original
        .apply_numerical_update(&update)
        .expect("sealed update preserves the complete budget");

    for kind in [
        EngineeringUncertaintyKind::Geometry,
        EngineeringUncertaintyKind::Parameters,
        EngineeringUncertaintyKind::BoundaryConditions,
        EngineeringUncertaintyKind::ModelForm,
        EngineeringUncertaintyKind::Measurement,
    ] {
        assert_eq!(
            updated.term(kind),
            original.term(kind),
            "numerical evidence rewrote {}",
            kind.name()
        );
    }
    assert!(matches!(
        updated.term(EngineeringUncertaintyKind::ModelForm).value(),
        TermValue::Unknown { .. }
    ));

    let error = NumericalUncertaintyUpdate::try_new(vec![
        term(
            EngineeringUncertaintyKind::Roundoff,
            TermValue::interval(0.0, 0.01).expect("roundoff bound"),
        ),
        term(
            EngineeringUncertaintyKind::SolverAlgebraic,
            TermValue::interval(0.0, 0.02).expect("solver bound"),
        ),
        term(
            EngineeringUncertaintyKind::ModelForm,
            TermValue::interval(0.0, 0.03).expect("attempted model bound"),
        ),
    ])
    .expect_err("a numerical update has no representation for model authority");
    assert_eq!(error.rule(), UncertaintyRule::NumericalUpdate);
    assert!(error.detail().contains("model-form"));
}

#[test]
fn finite_overflow_never_masquerades_as_a_bounded_total() {
    let mut terms = negligible_terms();
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::Roundoff,
        TermValue::interval(0.0, f64::MAX).expect("finite maximum"),
    );
    replace_term(
        &mut terms,
        EngineeringUncertaintyKind::SolverAlgebraic,
        TermValue::interval(0.0, f64::MAX).expect("finite maximum"),
    );
    assert_eq!(
        budget(terms).total(),
        BudgetTotal::Unbounded {
            reason: "finite term aggregation overflowed",
        }
    );

    let mut one = negligible_terms();
    replace_term(
        &mut one,
        EngineeringUncertaintyKind::Roundoff,
        TermValue::interval(0.0, f64::MAX).expect("finite maximum"),
    );
    let composed = budget(one.clone())
        .compose(&budget(one))
        .expect("overflow becomes explicit unknown rather than a constructor failure");
    assert!(matches!(
        composed.term(EngineeringUncertaintyKind::Roundoff).value(),
        TermValue::Unknown { .. }
    ));
}
