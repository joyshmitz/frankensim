//! G0/G3 conformance for the Phase 0B-A evidence-contract algebra.

use fs_blake3::ContentHash;
use fs_govern::{
    LaneCharter,
    evidence_contract::{
        ADJUDICATION_IDENTITY_DOMAIN, ASSUMPTION_SET_IDENTITY_DOMAIN, ATTACK_EDGE_IDENTITY_DOMAIN,
        AUTHORITY_ALGEBRA_VERSION, AUTHORITY_CATALOG_ROWS, AUTHORITY_HEAD_IDENTITY_DOMAIN,
        AUTHORITY_MIGRATION_IDENTITY_DOMAIN, AuthorityBudget, AuthorityError, AuthorityState,
        CapabilityBinding, CapabilityPolicy, CheckerDecisionCandidate, CheckerVerdict,
        ClaimInstance, ClaimLaneBinding, ClaimStatement, CounterexampleAdjudication,
        CounterexampleCandidate, CounterexampleVerdict, DomainVariable, EvidenceKind,
        EvidenceLifecycle, EvidenceRef, EvidenceState, ExactInstanceAdmission, FiveExplicits,
        InferenceRule, InvalidationState, KernelState, LEGACY_AUTHORITY_SCHEMA_VERSION,
        LegacyAuthorityRankV0, LegacyAuthorityV0, MAX_AUTHORITY_LOG_BYTES,
        NONVACUITY_EVIDENCE_IDENTITY_DOMAIN, NoClaimBoundary, NonvacuityEvidence, NonvacuityState,
        NonvacuityStrength, QuantifiedDomain, Quantifier, ReproductionState,
        SEMANTIC_CLAIM_IDENTITY_DOMAIN, SatisfiabilityEvidence, SatisfiabilityState, ScaleState,
        TruthRequirement, TruthState, UnitFactor, UnitSystem, VersionBinding,
        assess_runtime_candidate, authority_catalog_json, authority_catalog_markdown_rows,
        authority_log_json, migrate_legacy_v0,
    },
};

fn hash(label: &str) -> ContentHash {
    fs_blake3::hash_domain(
        "frankensim.fs-govern.test-evidence-contract.v1",
        label.as_bytes(),
    )
}

fn scale_family_strength() -> NonvacuityStrength {
    NonvacuityStrength::scale_family(hash("scale-space"), hash("quantity-fibre"))
        .expect("scale-family nonvacuity strength")
}

fn lane() -> LaneCharter {
    LaneCharter::new(
        "drag error <= 2 percent",
        "airfoil operating envelope",
        &["steady inflow"],
        "decision-grade",
        "coarse-grid baseline",
        "mesh-refinement falsifier",
        "airfoil-drag-v1",
    )
    .expect("lane")
}

fn claim_with(
    assumptions: &[&str],
    quantifier: Quantifier,
    budget_work: u64,
    capabilities: Vec<CapabilityBinding>,
    no_claim: &[&str],
) -> ClaimInstance {
    let statement =
        ClaimStatement::new(&["lift remains positive", "drag error is at most two percent"])
            .expect("statement");
    let domain = QuantifiedDomain::new(
        vec![
            DomainVariable::new("mach", quantifier, "[0.1, 0.3]").expect("mach"),
            DomainVariable::new("alpha", Quantifier::ForAll, "[-2 deg, 8 deg]").expect("alpha"),
        ],
        &["reynolds >= 1e6", "steady inflow"],
    )
    .expect("domain");
    let units = UnitSystem::new(
        1,
        2,
        vec![
            UnitFactor::new("s", -1).expect("seconds"),
            UnitFactor::new("m", 1).expect("metres"),
        ],
    )
    .expect("units");
    let explicits = FiveExplicits::new(
        units,
        17,
        AuthorityBudget {
            work_units: budget_work,
            memory_bytes: 4096,
            wall_time_millis: 500,
            reviewer_slots: 1,
        },
        vec![
            VersionBinding::new("solver", "3.2.1").expect("solver version"),
            VersionBinding::new("mesh", "7").expect("mesh version"),
        ],
        capabilities,
    )
    .expect("explicits");
    let assumptions =
        fs_govern::evidence_contract::AssumptionSet::new(assumptions).expect("assumptions");
    let lane = lane();
    let binding = ClaimLaneBinding::new(
        &statement,
        &domain,
        &assumptions,
        &lane,
        hash("claim-lane-binding-artifact"),
        hash("claim-lane-binder"),
    )
    .expect("claim/lane binding");
    ClaimInstance::new(
        statement,
        domain,
        assumptions,
        binding,
        explicits,
        NoClaimBoundary::new(no_claim).expect("no-claim"),
    )
    .expect("claim")
}

fn basic_claim() -> ClaimInstance {
    claim_with(
        &[],
        Quantifier::ForAll,
        100,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("capability")],
        &["not cross-ISA bit stability"],
    )
}

fn evidence(claim: &ClaimInstance, kind: EvidenceKind, label: &str) -> EvidenceRef {
    EvidenceRef::new(
        kind,
        claim.identity(),
        hash(&format!("{label}-artifact")),
        hash(&format!("{label}-checker")),
        AUTHORITY_ALGEBRA_VERSION,
    )
    .expect("evidence")
}

fn full_state(claim: ClaimInstance, truth: TruthState) -> AuthorityState {
    let sat =
        SatisfiabilityEvidence::new(claim.identity(), hash("sat-artifact"), hash("sat-checker"))
            .expect("satisfiability evidence");
    let nonvacuity = NonvacuityEvidence::new(
        claim.identity(),
        hash("nonvacuity-artifact"),
        hash("nonvacuity-checker"),
        scale_family_strength(),
    )
    .expect("nonvacuity evidence");
    let kernel = evidence(&claim, EvidenceKind::KernelProof, "kernel");
    let scale = evidence(&claim, EvidenceKind::ScaleQualification, "scale");
    let reproduction = evidence(&claim, EvidenceKind::Reproduction, "reproduction");
    AuthorityState::new(
        claim,
        truth,
        SatisfiabilityState::Satisfiable(sat),
        NonvacuityState::Nonvacuous(nonvacuity),
        ExactInstanceAdmission::Admitted(hash("exact-admission")),
        KernelState::KernelChecked(kernel),
        ScaleState::ScaleQualified(scale),
        ReproductionState::Reproduced(reproduction),
        InvalidationState::Clear,
    )
    .expect("full authority state")
}

fn full_proved_state(claim: ClaimInstance) -> AuthorityState {
    full_state(claim, TruthState::Proved)
}

#[test]
#[allow(clippy::too_many_lines)] // One G3 mutation table pins the complete exact identity.
fn g3_semantic_reordering_and_exact_unit_equivalence_are_identity_stable() {
    let cap_a = CapabilityBinding::new("runtime-admit", 1).expect("cap a");
    let cap_b = CapabilityBinding::new("portable-checker", 2).expect("cap b");
    let first = claim_with(
        &["bounded residual", "trusted mesh"],
        Quantifier::ForAll,
        100,
        vec![cap_a.clone(), cap_b.clone()],
        &["no transient claim", "not cross-ISA bit stability"],
    );

    let statement = ClaimStatement::new(&[
        " drag   error is at most two percent ",
        "lift remains positive",
    ])
    .expect("statement");
    let domain = QuantifiedDomain::new(
        vec![
            DomainVariable::new("alpha", Quantifier::ForAll, "[-2 deg, 8 deg]").expect("alpha"),
            DomainVariable::new("mach", Quantifier::ForAll, "[0.1, 0.3]").expect("mach"),
        ],
        &["steady   inflow", "reynolds >= 1e6"],
    )
    .expect("domain");
    let units = UnitSystem::new(
        2,
        4,
        vec![
            UnitFactor::new("m", 1).expect("m"),
            UnitFactor::new("s", -2).expect("s-2"),
            UnitFactor::new("s", 1).expect("s1"),
        ],
    )
    .expect("equivalent units");
    let explicits = FiveExplicits::new(
        units,
        17,
        AuthorityBudget {
            work_units: 100,
            memory_bytes: 4096,
            wall_time_millis: 500,
            reviewer_slots: 1,
        },
        vec![
            VersionBinding::new("mesh", "7").expect("mesh"),
            VersionBinding::new("solver", "3.2.1").expect("solver"),
        ],
        vec![cap_b, cap_a],
    )
    .expect("explicits");
    let assumptions =
        fs_govern::evidence_contract::AssumptionSet::new(&["trusted mesh", "bounded   residual"])
            .expect("assumptions");
    let lane = lane();
    let binding = ClaimLaneBinding::new(
        &statement,
        &domain,
        &assumptions,
        &lane,
        hash("claim-lane-binding-artifact"),
        hash("claim-lane-binder"),
    )
    .expect("binding");
    let equivalent = ClaimInstance::new(
        statement,
        domain,
        assumptions,
        binding,
        explicits,
        NoClaimBoundary::new(&["not cross-ISA bit stability", "no transient claim"])
            .expect("no claim"),
    )
    .expect("equivalent claim");

    assert_eq!(first.semantic_identity(), equivalent.semantic_identity());
    assert_eq!(first.identity(), equivalent.identity());

    let changed_quantifier = claim_with(
        &["bounded residual", "trusted mesh"],
        Quantifier::Exists,
        100,
        vec![
            CapabilityBinding::new("runtime-admit", 1).expect("cap"),
            CapabilityBinding::new("portable-checker", 2).expect("cap"),
        ],
        &["no transient claim", "not cross-ISA bit stability"],
    );
    assert_ne!(
        first.semantic_identity(),
        changed_quantifier.semantic_identity()
    );

    let changed_assumption = claim_with(
        &["trusted mesh"],
        Quantifier::ForAll,
        100,
        vec![
            CapabilityBinding::new("runtime-admit", 1).expect("cap"),
            CapabilityBinding::new("portable-checker", 2).expect("cap"),
        ],
        &["no transient claim", "not cross-ISA bit stability"],
    );
    assert_ne!(
        first.semantic_identity(),
        changed_assumption.semantic_identity()
    );

    let changed_budget = claim_with(
        &["bounded residual", "trusted mesh"],
        Quantifier::ForAll,
        101,
        vec![
            CapabilityBinding::new("runtime-admit", 1).expect("cap"),
            CapabilityBinding::new("portable-checker", 2).expect("cap"),
        ],
        &["no transient claim", "not cross-ISA bit stability"],
    );
    assert_eq!(
        first.semantic_identity(),
        changed_budget.semantic_identity()
    );
    assert_ne!(first.identity(), changed_budget.identity());
}

#[test]
fn g3_unit_factor_permutations_cannot_change_acceptance() {
    let first = UnitSystem::new(
        1,
        1,
        vec![
            UnitFactor::new("s", 127).expect("127"),
            UnitFactor::new("s", 1).expect("plus one"),
            UnitFactor::new("s", -1).expect("minus one"),
        ],
    )
    .expect("final exponent is representable");
    let reordered = UnitSystem::new(
        1,
        1,
        vec![
            UnitFactor::new("s", 127).expect("127"),
            UnitFactor::new("s", -1).expect("minus one"),
            UnitFactor::new("s", 1).expect("plus one"),
        ],
    )
    .expect("same multiset must accept");
    assert_eq!(first, reordered);
    assert_eq!(first.factors()[0].exponent(), 127);
}

#[test]
fn g0_claim_instance_rejects_a_binding_for_another_structured_claim() {
    let original = basic_claim();
    let changed_statement = ClaimStatement::new(&[
        "lift remains positive",
        "drag error is at most three percent",
    ])
    .expect("changed statement");
    assert!(matches!(
        ClaimInstance::new(
            changed_statement,
            original.domain().clone(),
            original.assumptions().clone(),
            original.lane_binding(),
            original.explicits().clone(),
            original.no_claim().clone(),
        ),
        Err(AuthorityError::IdentityMismatch { .. })
    ));
}

#[test]
fn g0_satisfiability_and_nonvacuity_are_distinct_axes_and_typed_evidence() {
    let claim = basic_claim();
    let sat = SatisfiabilityEvidence::new(
        claim.identity(),
        hash("shared-artifact"),
        hash("shared-checker"),
    )
    .expect("sat");
    let nonvacuity = NonvacuityEvidence::new(
        claim.identity(),
        hash("shared-artifact"),
        hash("shared-checker"),
        scale_family_strength(),
    )
    .expect("nonvacuity");
    assert_eq!(sat.evidence().kind(), EvidenceKind::Satisfiability);
    assert_eq!(nonvacuity.evidence().kind(), EvidenceKind::Nonvacuity);
    assert_ne!(sat.evidence().identity(), nonvacuity.evidence().identity());

    let sat_only = AuthorityState::new(
        claim.clone(),
        TruthState::Unknown,
        SatisfiabilityState::Satisfiable(sat),
        NonvacuityState::Unknown,
        ExactInstanceAdmission::NotEvaluated,
        KernelState::NotChecked,
        ScaleState::NotQualified,
        ReproductionState::NotAttempted,
        InvalidationState::Clear,
    )
    .expect("sat-only state");
    let nonvacuity_only = AuthorityState::new(
        claim.clone(),
        TruthState::Unknown,
        SatisfiabilityState::Unknown,
        NonvacuityState::Nonvacuous(nonvacuity),
        ExactInstanceAdmission::NotEvaluated,
        KernelState::NotChecked,
        ScaleState::NotQualified,
        ReproductionState::NotAttempted,
        InvalidationState::Clear,
    )
    .expect("nonvacuity-only state");
    assert_ne!(sat_only.identity(), nonvacuity_only.identity());

    assert!(matches!(
        AuthorityState::new(
            claim,
            TruthState::Unknown,
            SatisfiabilityState::Unsatisfiable(sat),
            NonvacuityState::Nonvacuous(nonvacuity),
            ExactInstanceAdmission::NotEvaluated,
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
            InvalidationState::Clear,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));
}

#[test]
fn g0_nonvacuity_strength_is_exact_and_every_source_moves_identity() {
    let claim = basic_claim();
    let context = hash("strength-context");
    let fibre = hash("strength-fibre");
    let scale = NonvacuityStrength::scale_family(context, fibre).expect("scale");
    let same = NonvacuityStrength::scale_family(context, fibre).expect("same scale");
    let point = NonvacuityStrength::point(context, fibre).expect("point");
    let other_context =
        NonvacuityStrength::scale_family(hash("other-context"), fibre).expect("context");
    let other_fibre =
        NonvacuityStrength::scale_family(context, hash("other-fibre")).expect("fibre");
    assert!(scale.satisfies(&same));
    for mismatch in [point, other_context, other_fibre] {
        assert!(!scale.satisfies(&mismatch));
    }

    let make = |strength| {
        NonvacuityEvidence::new(
            claim.identity(),
            hash("strength-artifact"),
            hash("strength-checker"),
            strength,
        )
        .expect("nonvacuity evidence")
    };
    let baseline = make(scale);
    assert_eq!(baseline.identity(), make(same).identity());
    assert_ne!(baseline.identity(), make(point).identity());
    assert_ne!(baseline.identity(), make(other_context).identity());
    assert_ne!(baseline.identity(), make(other_fibre).identity());
}

#[test]
fn g0_truth_partial_order_and_product_meet_never_widen() {
    assert!(TruthState::Unknown.leq(TruthState::ConditionalProof));
    assert!(TruthState::ConditionalProof.leq(TruthState::Proved));
    assert!(!TruthState::Refuted.leq(TruthState::Proved));
    assert!(!TruthState::Proved.leq(TruthState::Refuted));

    let claim = basic_claim();
    let unknown = AuthorityState::unknown(claim.clone()).expect("unknown");
    let proved = AuthorityState::new(
        claim.clone(),
        TruthState::Proved,
        SatisfiabilityState::Unknown,
        NonvacuityState::Unknown,
        ExactInstanceAdmission::NotEvaluated,
        KernelState::NotChecked,
        ScaleState::NotQualified,
        ReproductionState::NotAttempted,
        InvalidationState::Clear,
    )
    .expect("proved");
    assert!(proved.dominates(&unknown));
    assert!(!unknown.dominates(&proved));
    let meet = proved.conservative_meet(&unknown).expect("meet");
    assert_eq!(meet.identity(), unknown.identity());
    assert!(proved.dominates(&meet));
    assert!(unknown.dominates(&meet));

    let refuted = AuthorityState::new(
        claim,
        TruthState::Refuted,
        SatisfiabilityState::Unknown,
        NonvacuityState::Unknown,
        ExactInstanceAdmission::NotEvaluated,
        KernelState::NotChecked,
        ScaleState::NotQualified,
        ReproductionState::NotAttempted,
        InvalidationState::Clear,
    )
    .expect("refuted");
    let contradiction_bottom = proved
        .conservative_meet(&refuted)
        .expect("incomparable truth branches share Unknown bottom");
    assert_eq!(contradiction_bottom.truth(), TruthState::Unknown);
    assert!(proved.dominates(&contradiction_bottom));
    assert!(refuted.dominates(&contradiction_bottom));
}

#[test]
fn g0_truth_order_is_reflexive_antisymmetric_and_transitive() {
    let states = [
        TruthState::Unknown,
        TruthState::ConditionalProof,
        TruthState::Proved,
        TruthState::Refuted,
    ];
    for left in states {
        assert!(left.leq(left), "truth order is not reflexive at {left:?}");
        for right in states {
            if left.leq(right) && right.leq(left) {
                assert_eq!(left, right, "truth order is not antisymmetric");
            }
            for upper in states {
                if left.leq(right) && right.leq(upper) {
                    assert!(left.leq(upper), "truth order is not transitive");
                }
            }
        }
    }
}

#[test]
fn g0_product_meet_demotes_different_positive_receipts_monotonically() {
    let left = full_proved_state(basic_claim());
    let right = AuthorityState::new(
        left.claim().clone(),
        left.truth(),
        left.satisfiability(),
        left.nonvacuity(),
        ExactInstanceAdmission::Admitted(hash("different-admission-receipt")),
        KernelState::KernelChecked(evidence(
            left.claim(),
            EvidenceKind::KernelProof,
            "different-kernel-proof",
        )),
        left.scale(),
        left.reproduction(),
        InvalidationState::Clear,
    )
    .expect("second positive state");
    let meet = left.conservative_meet(&right).expect("product meet");
    assert_eq!(meet.exact_admission(), ExactInstanceAdmission::NotEvaluated);
    assert_eq!(meet.kernel(), KernelState::NotChecked);
    assert!(left.dominates(&meet));
    assert!(right.dominates(&meet));
    assert!(!meet.dominates(&left));
    assert!(!meet.dominates(&right));
}

#[test]
#[allow(clippy::too_many_lines)] // One finite product sweep proves the represented meet laws.
fn g0_product_meet_is_commutative_idempotent_associative_and_a_lower_bound() {
    let claim = basic_claim();
    let sat_a =
        SatisfiabilityEvidence::new(claim.identity(), hash("law-sat-a"), hash("law-sat-checker"))
            .expect("sat a");
    let sat_b =
        SatisfiabilityEvidence::new(claim.identity(), hash("law-sat-b"), hash("law-sat-checker"))
            .expect("sat b");
    let nonvacuity_a = NonvacuityEvidence::new(
        claim.identity(),
        hash("law-nonvacuity-a"),
        hash("law-nonvacuity-checker"),
        scale_family_strength(),
    )
    .expect("nonvacuity a");
    let nonvacuity_b = NonvacuityEvidence::new(
        claim.identity(),
        hash("law-nonvacuity-b"),
        hash("law-nonvacuity-checker"),
        scale_family_strength(),
    )
    .expect("nonvacuity b");
    let kernel_a = evidence(&claim, EvidenceKind::KernelProof, "law-kernel-a");
    let kernel_b = evidence(&claim, EvidenceKind::KernelProof, "law-kernel-b");
    let scale_a = evidence(&claim, EvidenceKind::ScaleQualification, "law-scale-a");
    let scale_b = evidence(&claim, EvidenceKind::ScaleQualification, "law-scale-b");
    let reproduction_a = evidence(&claim, EvidenceKind::Reproduction, "law-reproduction-a");
    let reproduction_b = evidence(&claim, EvidenceKind::Reproduction, "law-reproduction-b");

    let make = |truth, satisfiability, nonvacuity, admission, kernel, scale, reproduction| {
        AuthorityState::new(
            claim.clone(),
            truth,
            satisfiability,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
            InvalidationState::Clear,
        )
        .expect("law state")
    };
    let bottom_axes = || {
        (
            SatisfiabilityState::Unknown,
            NonvacuityState::Unknown,
            ExactInstanceAdmission::NotEvaluated,
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
        )
    };
    let mut states = Vec::new();
    for truth in [TruthState::Unknown, TruthState::Proved, TruthState::Refuted] {
        let (sat, nonvacuity, admission, kernel, scale, reproduction) = bottom_axes();
        states.push(make(
            truth,
            sat,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    for satisfiability in [
        SatisfiabilityState::Satisfiable(sat_a),
        SatisfiabilityState::Satisfiable(sat_b),
        SatisfiabilityState::Unsatisfiable(sat_b),
    ] {
        let (_, nonvacuity, admission, kernel, scale, reproduction) = bottom_axes();
        states.push(make(
            TruthState::Unknown,
            satisfiability,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    for nonvacuity in [
        NonvacuityState::Nonvacuous(nonvacuity_a),
        NonvacuityState::Nonvacuous(nonvacuity_b),
        NonvacuityState::Vacuous(nonvacuity_b),
    ] {
        let (sat, _, admission, kernel, scale, reproduction) = bottom_axes();
        states.push(make(
            TruthState::Unknown,
            sat,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    for admission in [
        ExactInstanceAdmission::Admitted(hash("law-admitted-a")),
        ExactInstanceAdmission::Admitted(hash("law-admitted-b")),
        ExactInstanceAdmission::Refused(hash("law-refused")),
    ] {
        let (sat, nonvacuity, _, kernel, scale, reproduction) = bottom_axes();
        states.push(make(
            TruthState::Unknown,
            sat,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    for kernel in [
        KernelState::KernelChecked(kernel_a),
        KernelState::KernelChecked(kernel_b),
    ] {
        let (sat, nonvacuity, admission, _, scale, reproduction) = bottom_axes();
        states.push(make(
            TruthState::Unknown,
            sat,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    for scale in [
        ScaleState::ScaleQualified(scale_a),
        ScaleState::ScaleQualified(scale_b),
    ] {
        let (sat, nonvacuity, admission, kernel, _, reproduction) = bottom_axes();
        states.push(make(
            TruthState::Unknown,
            sat,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    for reproduction in [
        ReproductionState::Reproduced(reproduction_a),
        ReproductionState::Reproduced(reproduction_b),
        ReproductionState::Failed(reproduction_b),
    ] {
        let (sat, nonvacuity, admission, kernel, scale, _) = bottom_axes();
        states.push(make(
            TruthState::Unknown,
            sat,
            nonvacuity,
            admission,
            kernel,
            scale,
            reproduction,
        ));
    }
    let revocation_target = full_proved_state(claim.clone());
    let counterexample = CounterexampleCandidate::new(
        revocation_target.claim(),
        evidence(
            revocation_target.claim(),
            EvidenceKind::Counterexample,
            "law-counterexample",
        ),
    )
    .expect("counterexample");
    let adjudication = CounterexampleAdjudication::new(
        &counterexample,
        CounterexampleVerdict::GenuineCounterexample,
        evidence(
            revocation_target.claim(),
            EvidenceKind::Adjudication,
            "law-adjudication",
        ),
    )
    .expect("adjudication");
    let tombstone = fs_govern::evidence_contract::RevocationTombstone::new(
        &revocation_target,
        &adjudication,
        "law revocation",
        evidence(
            revocation_target.claim(),
            EvidenceKind::Revocation,
            "law-revocation",
        ),
    )
    .expect("tombstone");
    states.push(
        revocation_target
            .invalidate(&tombstone)
            .expect("invalidated law state"),
    );

    for left in &states {
        assert_eq!(
            left.conservative_meet(left).expect("idempotent").identity(),
            left.identity()
        );
        for right in &states {
            let lr = left.conservative_meet(right).expect("left/right meet");
            let rl = right.conservative_meet(left).expect("right/left meet");
            assert_eq!(lr.identity(), rl.identity(), "meet must commute");
            assert!(left.dominates(&lr), "meet must be below left");
            assert!(right.dominates(&lr), "meet must be below right");
            for represented_lower_bound in &states {
                if left.dominates(represented_lower_bound)
                    && right.dominates(represented_lower_bound)
                {
                    assert!(
                        lr.dominates(represented_lower_bound),
                        "meet must dominate every represented common lower bound"
                    );
                }
            }
            for third in &states {
                let left_grouped = lr.conservative_meet(third).expect("left-associated meet");
                let right_pair = right.conservative_meet(third).expect("right inner meet");
                let right_grouped = left
                    .conservative_meet(&right_pair)
                    .expect("right-associated meet");
                assert_eq!(
                    left_grouped.identity(),
                    right_grouped.identity(),
                    "meet must associate"
                );
            }
        }
    }

    let other_claim = claim_with(
        &[],
        Quantifier::ForAll,
        101,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("capability")],
        &["not cross-ISA bit stability"],
    );
    let other = AuthorityState::unknown(other_claim).expect("other claim state");
    assert!(matches!(
        states[0].conservative_meet(&other),
        Err(AuthorityError::IdentityMismatch { .. })
    ));
}

#[test]
#[allow(clippy::too_many_lines)] // Keep every incompatible product witness adjacent.
fn g0_invalid_truth_and_admission_combinations_refuse() {
    let assumption_claim = claim_with(&["trusted mesh"], Quantifier::ForAll, 100, vec![], &[]);
    assert!(matches!(
        AuthorityState::new(
            assumption_claim,
            TruthState::Proved,
            SatisfiabilityState::Unknown,
            NonvacuityState::Unknown,
            ExactInstanceAdmission::NotEvaluated,
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
            InvalidationState::Clear,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));

    let claim = basic_claim();
    assert!(matches!(
        AuthorityState::new(
            claim,
            TruthState::Refuted,
            SatisfiabilityState::Unknown,
            NonvacuityState::Unknown,
            ExactInstanceAdmission::Admitted(hash("bad-admission")),
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
            InvalidationState::Clear,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));

    let impossible_claim = basic_claim();
    let unsatisfiable = SatisfiabilityEvidence::new(
        impossible_claim.identity(),
        hash("unsatisfiable-artifact"),
        hash("unsatisfiable-checker"),
    )
    .expect("unsatisfiable evidence");
    assert!(matches!(
        AuthorityState::new(
            impossible_claim.clone(),
            TruthState::Unknown,
            SatisfiabilityState::Unsatisfiable(unsatisfiable),
            NonvacuityState::Unknown,
            ExactInstanceAdmission::Admitted(hash("unsatisfiable-admission")),
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
            InvalidationState::Clear,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));
    let vacuity = NonvacuityEvidence::new(
        impossible_claim.identity(),
        hash("vacuous-artifact"),
        hash("vacuous-checker"),
        scale_family_strength(),
    )
    .expect("vacuity evidence");
    assert!(matches!(
        AuthorityState::new(
            impossible_claim,
            TruthState::Unknown,
            SatisfiabilityState::Unknown,
            NonvacuityState::Vacuous(vacuity),
            ExactInstanceAdmission::Admitted(hash("vacuous-admission")),
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
            InvalidationState::Clear,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));

    let unconditional = basic_claim();
    assert!(matches!(
        AuthorityState::new(
            unconditional,
            TruthState::ConditionalProof,
            SatisfiabilityState::Unknown,
            NonvacuityState::Unknown,
            ExactInstanceAdmission::NotEvaluated,
            KernelState::NotChecked,
            ScaleState::NotQualified,
            ReproductionState::NotAttempted,
            InvalidationState::Clear,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));

    let base = full_proved_state(basic_claim());
    let candidate = CounterexampleCandidate::new(
        base.claim(),
        evidence(base.claim(), EvidenceKind::Counterexample, "invalid-combo"),
    )
    .expect("candidate");
    let adjudication = CounterexampleAdjudication::new(
        &candidate,
        CounterexampleVerdict::GenuineCounterexample,
        evidence(base.claim(), EvidenceKind::Adjudication, "invalid-combo"),
    )
    .expect("adjudication");
    let tombstone = fs_govern::evidence_contract::RevocationTombstone::new(
        &base,
        &adjudication,
        "invalid combination witness",
        evidence(base.claim(), EvidenceKind::Revocation, "invalid-combo"),
    )
    .expect("tombstone");
    assert!(matches!(
        AuthorityState::new(
            base.claim().clone(),
            base.truth(),
            base.satisfiability(),
            base.nonvacuity(),
            ExactInstanceAdmission::Admitted(hash("stale-admission")),
            base.kernel(),
            base.scale(),
            base.reproduction(),
            InvalidationState::Invalidated(tombstone.identity()),
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));
}

#[test]
fn g0_cancellation_is_request_drain_finalize_and_terminal() {
    let receipt = fs_govern::evidence_contract::CancellationReceipt::new(
        hash("cancel-request"),
        hash("cancel-drain"),
        hash("cancel-finalize"),
    )
    .expect("cancellation receipt");
    let cancelled = EvidenceState::Proposed
        .transition(EvidenceState::Cancelled(receipt))
        .expect("cancel transition");
    assert!(cancelled.is_terminal());
    assert!(matches!(
        cancelled.transition(EvidenceState::Checked),
        Err(AuthorityError::TerminalState { state: "cancelled" })
    ));
    assert!(matches!(
        EvidenceState::Proposed.transition(EvidenceState::Adjudicated),
        Err(AuthorityError::IllegalTransition { .. })
    ));
    assert_eq!(
        EvidenceState::Proposed
            .transition(EvidenceState::Checked)
            .expect("check")
            .transition(EvidenceState::Adjudicated)
            .expect("adjudicate"),
        EvidenceState::Adjudicated
    );

    let claim = basic_claim();
    let mut lifecycle =
        EvidenceLifecycle::proposed(evidence(&claim, EvidenceKind::Support, "lifecycle"));
    let proposed_id = lifecycle.identity();
    lifecycle
        .transition(EvidenceState::Cancelled(receipt))
        .expect("lifecycle cancellation");
    assert_ne!(proposed_id, lifecycle.identity());
    assert_eq!(lifecycle.predecessor(), Some(proposed_id));
    assert_eq!(lifecycle.state(), EvidenceState::Cancelled(receipt));
    let terminal_id = lifecycle.identity();
    assert!(matches!(
        lifecycle.transition(EvidenceState::Checked),
        Err(AuthorityError::TerminalState { .. })
    ));
    assert_eq!(lifecycle.identity(), terminal_id);
}

#[test]
fn g0_edges_adjudication_and_revocation_bind_exact_instances() {
    let claim = basic_claim();
    let state = full_proved_state(claim.clone());
    let rule = InferenceRule::new("interval enclosure", 1, hash("rule-definition")).expect("rule");
    let support = fs_govern::evidence_contract::SupportEdge::new(
        &state,
        &claim,
        &rule,
        evidence(&claim, EvidenceKind::Support, "support"),
    )
    .expect("support edge");
    assert_eq!(support.target(), claim.identity());
    assert_eq!(support.rule(), rule.identity());

    let candidate = CounterexampleCandidate::new(
        &claim,
        evidence(&claim, EvidenceKind::Counterexample, "counterexample"),
    )
    .expect("candidate");
    let attack = fs_govern::evidence_contract::AttackEdge::new(
        &candidate,
        &claim,
        evidence(&claim, EvidenceKind::Attack, "attack"),
    )
    .expect("attack");
    assert_eq!(attack.candidate(), candidate.identity());

    let out_of_domain = CounterexampleAdjudication::new(
        &candidate,
        CounterexampleVerdict::OutOfDomain,
        evidence(&claim, EvidenceKind::Adjudication, "out-of-domain"),
    )
    .expect("adjudication");
    assert!(matches!(
        fs_govern::evidence_contract::RevocationTombstone::new(
            &state,
            &out_of_domain,
            "not genuine",
            evidence(&claim, EvidenceKind::Revocation, "invalid-revocation"),
        ),
        Err(AuthorityError::AdjudicationNotRevocable)
    ));

    let genuine = CounterexampleAdjudication::new(
        &candidate,
        CounterexampleVerdict::GenuineCounterexample,
        evidence(&claim, EvidenceKind::Adjudication, "genuine"),
    )
    .expect("genuine adjudication");
    let tombstone = fs_govern::evidence_contract::RevocationTombstone::new(
        &state,
        &genuine,
        "admitted in-domain counterexample",
        evidence(&claim, EvidenceKind::Revocation, "revocation"),
    )
    .expect("revocation");
    let invalidated = state.invalidate(&tombstone).expect("invalidate");
    assert_eq!(
        invalidated.invalidation(),
        InvalidationState::Invalidated(tombstone.identity())
    );
    assert_eq!(
        invalidated.exact_admission(),
        ExactInstanceAdmission::NotEvaluated
    );
    assert!(matches!(
        fs_govern::evidence_contract::SupportEdge::new(
            &invalidated,
            &claim,
            &rule,
            evidence(&claim, EvidenceKind::Support, "post-revocation"),
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));

    let other_tombstone = fs_govern::evidence_contract::RevocationTombstone::new(
        &state,
        &genuine,
        "independent revocation receipt",
        evidence(&claim, EvidenceKind::Revocation, "other-revocation"),
    )
    .expect("other revocation");
    let other_invalidated = state
        .invalidate(&other_tombstone)
        .expect("other invalidated state");
    assert!(matches!(
        invalidated.conservative_meet(&other_invalidated),
        Err(AuthorityError::CompositionConflict {
            axis: "invalidation"
        })
    ));
}

fn strict_policy(required_capabilities: Vec<CapabilityBinding>) -> CapabilityPolicy {
    CapabilityPolicy::new(
        TruthRequirement::ProvedOnly,
        true,
        Some(scale_family_strength()),
        true,
        true,
        true,
        required_capabilities,
        &[],
        &["not cross-ISA bit stability"],
    )
    .expect("policy")
}

fn accepted_candidate(
    state: &AuthorityState,
    policy: &CapabilityPolicy,
    checker: ContentHash,
) -> CheckerDecisionCandidate {
    CheckerDecisionCandidate::new(
        state.claim().identity(),
        state.identity(),
        policy.identity(),
        checker,
        CheckerVerdict::Accept,
        hash("checker-decision-artifact"),
        None,
    )
    .expect("candidate")
}

#[test]
fn g0_runtime_assessment_is_explicitly_candidate_only_and_checks_policy_axes() {
    let state = full_proved_state(basic_claim());
    let policy = strict_policy(vec![
        CapabilityBinding::new("runtime-admit", 1).expect("capability"),
    ]);
    let checker = hash("accepting-checker");
    let candidate = accepted_candidate(&state, &policy, checker);
    let assessment =
        assess_runtime_candidate(&state, &policy, &candidate).expect("eligible candidate");
    assert_eq!(assessment.claim(), state.claim().identity());
    assert_eq!(assessment.authority(), state.identity());
    assert_eq!(assessment.policy(), policy.identity());
    assert_eq!(assessment.decision_candidate(), candidate.identity());

    let impossible_policy = strict_policy(vec![
        CapabilityBinding::new("missing-capability", 9).expect("missing cap"),
    ]);
    let impossible_candidate = accepted_candidate(&state, &impossible_policy, checker);
    assert!(matches!(
        assess_runtime_candidate(&state, &impossible_policy, &impossible_candidate),
        Err(AuthorityError::CapabilityMissing { .. })
    ));

    let point_evidence = NonvacuityEvidence::new(
        state.claim().identity(),
        hash("point-only-artifact"),
        hash("point-only-checker"),
        NonvacuityStrength::point(hash("one-point"), hash("quantity-fibre")).expect("point"),
    )
    .expect("point evidence");
    let point_only = AuthorityState::new(
        state.claim().clone(),
        state.truth(),
        state.satisfiability(),
        NonvacuityState::Nonvacuous(point_evidence),
        state.exact_admission(),
        state.kernel(),
        state.scale(),
        state.reproduction(),
        state.invalidation(),
    )
    .expect("point-only state");
    let point_candidate = accepted_candidate(&point_only, &policy, checker);
    assert_eq!(
        assess_runtime_candidate(&point_only, &policy, &point_candidate)
            .expect_err("point evidence cannot satisfy scale-family policy"),
        AuthorityError::RuntimeRequirementNotMet {
            requirement: "nonvacuity-strength"
        }
    );
}

#[test]
#[allow(clippy::too_many_lines)] // Keep independent identity and verdict refusals explicit.
fn g0_runtime_candidate_identity_and_verdict_matrix_fails_closed() {
    let state = full_proved_state(basic_claim());
    let policy = strict_policy(vec![
        CapabilityBinding::new("runtime-admit", 1).expect("capability"),
    ]);
    let checker = hash("identity-matrix-checker");
    let other_claim = claim_with(
        &[],
        Quantifier::ForAll,
        101,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("capability")],
        &["not cross-ISA bit stability"],
    );
    let other_state = AuthorityState::unknown(state.claim().clone()).expect("other state");
    let other_policy = strict_policy(vec![]);
    for candidate in [
        CheckerDecisionCandidate::new(
            other_claim.identity(),
            state.identity(),
            policy.identity(),
            checker,
            CheckerVerdict::Accept,
            hash("wrong-claim"),
            None,
        )
        .expect("wrong claim candidate"),
        CheckerDecisionCandidate::new(
            state.claim().identity(),
            other_state.identity(),
            policy.identity(),
            checker,
            CheckerVerdict::Accept,
            hash("wrong-state"),
            None,
        )
        .expect("wrong state candidate"),
        CheckerDecisionCandidate::new(
            state.claim().identity(),
            state.identity(),
            other_policy.identity(),
            checker,
            CheckerVerdict::Accept,
            hash("wrong-policy"),
            None,
        )
        .expect("wrong policy candidate"),
    ] {
        assert!(matches!(
            assess_runtime_candidate(&state, &policy, &candidate),
            Err(AuthorityError::IdentityMismatch { .. })
        ));
    }

    for verdict in [CheckerVerdict::Refuse, CheckerVerdict::Indeterminate] {
        let candidate = CheckerDecisionCandidate::new(
            state.claim().identity(),
            state.identity(),
            policy.identity(),
            checker,
            verdict,
            hash(&format!("verdict-{verdict:?}")),
            None,
        )
        .expect("non-accept candidate");
        assert_eq!(
            assess_runtime_candidate(&state, &policy, &candidate)
                .expect_err("non-accept verdict must refuse"),
            AuthorityError::CheckerRefused { verdict }
        );
    }
    let cancellation = fs_govern::evidence_contract::CancellationReceipt::new(
        hash("checker-cancel-request"),
        hash("checker-cancel-drain"),
        hash("checker-cancel-finalize"),
    )
    .expect("cancellation");
    let cancelled = CheckerDecisionCandidate::new(
        state.claim().identity(),
        state.identity(),
        policy.identity(),
        checker,
        CheckerVerdict::Cancelled,
        hash("cancelled-verdict"),
        Some(cancellation),
    )
    .expect("cancelled candidate");
    assert_eq!(
        assess_runtime_candidate(&state, &policy, &cancelled)
            .expect_err("cancelled verdict must refuse"),
        AuthorityError::CheckerRefused {
            verdict: CheckerVerdict::Cancelled
        }
    );
    assert!(matches!(
        CheckerDecisionCandidate::new(
            state.claim().identity(),
            state.identity(),
            policy.identity(),
            checker,
            CheckerVerdict::Cancelled,
            hash("cancelled-without-cancellation"),
            None,
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));
    assert!(matches!(
        CheckerDecisionCandidate::new(
            state.claim().identity(),
            state.identity(),
            policy.identity(),
            checker,
            CheckerVerdict::Accept,
            hash("accept-with-cancellation"),
            Some(cancellation),
        ),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));
}

#[test]
#[allow(clippy::too_many_lines)] // Keep every runtime widening guard in one mutation matrix.
fn g0_every_runtime_widening_guard_has_a_negative_witness() {
    let base = full_proved_state(basic_claim());
    let policy = strict_policy(vec![
        CapabilityBinding::new("runtime-admit", 1).expect("capability"),
    ]);
    let checker = hash("guard-checker");
    let missing_states = [
        (
            AuthorityState::new(
                base.claim().clone(),
                TruthState::Proved,
                SatisfiabilityState::Unknown,
                base.nonvacuity(),
                base.exact_admission(),
                base.kernel(),
                base.scale(),
                base.reproduction(),
                InvalidationState::Clear,
            )
            .expect("missing satisfiability"),
            "satisfiable",
        ),
        (
            AuthorityState::new(
                base.claim().clone(),
                TruthState::Proved,
                base.satisfiability(),
                NonvacuityState::Unknown,
                base.exact_admission(),
                base.kernel(),
                base.scale(),
                base.reproduction(),
                InvalidationState::Clear,
            )
            .expect("missing nonvacuity"),
            "nonvacuity-strength",
        ),
        (
            AuthorityState::new(
                base.claim().clone(),
                TruthState::Proved,
                base.satisfiability(),
                base.nonvacuity(),
                base.exact_admission(),
                KernelState::NotChecked,
                base.scale(),
                base.reproduction(),
                InvalidationState::Clear,
            )
            .expect("missing kernel"),
            "kernel-checked",
        ),
        (
            AuthorityState::new(
                base.claim().clone(),
                TruthState::Proved,
                base.satisfiability(),
                base.nonvacuity(),
                base.exact_admission(),
                base.kernel(),
                ScaleState::NotQualified,
                base.reproduction(),
                InvalidationState::Clear,
            )
            .expect("missing scale"),
            "scale-qualified",
        ),
        (
            AuthorityState::new(
                base.claim().clone(),
                TruthState::Proved,
                base.satisfiability(),
                base.nonvacuity(),
                base.exact_admission(),
                base.kernel(),
                base.scale(),
                ReproductionState::NotAttempted,
                InvalidationState::Clear,
            )
            .expect("missing reproduction"),
            "reproduced",
        ),
        (
            AuthorityState::new(
                base.claim().clone(),
                TruthState::Proved,
                base.satisfiability(),
                base.nonvacuity(),
                ExactInstanceAdmission::NotEvaluated,
                base.kernel(),
                base.scale(),
                base.reproduction(),
                InvalidationState::Clear,
            )
            .expect("missing admission"),
            "exact-instance-admitted",
        ),
    ];
    for (state, requirement) in missing_states {
        let candidate = accepted_candidate(&state, &policy, checker);
        let error = assess_runtime_candidate(&state, &policy, &candidate)
            .expect_err("missing axis must refuse");
        assert_eq!(
            error,
            AuthorityError::RuntimeRequirementNotMet { requirement }
        );
    }

    let conditional_claim = claim_with(
        &["trusted mesh"],
        Quantifier::ForAll,
        100,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("cap")],
        &["not cross-ISA bit stability"],
    );
    let conditional = full_state(conditional_claim, TruthState::ConditionalProof);
    let conditional_candidate = accepted_candidate(&conditional, &policy, checker);
    assert_eq!(
        assess_runtime_candidate(&conditional, &policy, &conditional_candidate)
            .expect_err("proved-only must reject conditional truth"),
        AuthorityError::RuntimeRequirementNotMet {
            requirement: "proved-only"
        }
    );

    let conditional_policy = CapabilityPolicy::new(
        TruthRequirement::ConditionalOrProved,
        true,
        Some(scale_family_strength()),
        true,
        true,
        true,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("cap")],
        &["trusted mesh"],
        &["not cross-ISA bit stability"],
    )
    .expect("conditional policy");
    let accepted_conditional = accepted_candidate(&conditional, &conditional_policy, checker);
    assess_runtime_candidate(&conditional, &conditional_policy, &accepted_conditional)
        .expect("exactly accepted assumption is eligible candidate data");

    let unaccepted_assumption_policy = CapabilityPolicy::new(
        TruthRequirement::ConditionalOrProved,
        true,
        Some(scale_family_strength()),
        true,
        true,
        true,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("cap")],
        &[],
        &["not cross-ISA bit stability"],
    )
    .expect("missing-assumption policy");
    let unaccepted_assumption =
        accepted_candidate(&conditional, &unaccepted_assumption_policy, checker);
    assert!(matches!(
        assess_runtime_candidate(
            &conditional,
            &unaccepted_assumption_policy,
            &unaccepted_assumption,
        ),
        Err(AuthorityError::AssumptionNotAccepted { .. })
    ));

    let unaccepted_boundary_policy = CapabilityPolicy::new(
        TruthRequirement::ProvedOnly,
        true,
        Some(scale_family_strength()),
        true,
        true,
        true,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("cap")],
        &[],
        &[],
    )
    .expect("unaccepted-boundary policy");
    let boundary_candidate = accepted_candidate(&base, &unaccepted_boundary_policy, checker);
    assert!(matches!(
        assess_runtime_candidate(&base, &unaccepted_boundary_policy, &boundary_candidate,),
        Err(AuthorityError::NoClaimNotAccepted { .. })
    ));

    let refused_candidate = CheckerDecisionCandidate::new(
        base.claim().identity(),
        base.identity(),
        policy.identity(),
        checker,
        CheckerVerdict::Refuse,
        hash("refusal-artifact"),
        None,
    )
    .expect("refusal candidate");
    assert_eq!(
        assess_runtime_candidate(&base, &policy, &refused_candidate)
            .expect_err("refused checker verdict cannot be eligible"),
        AuthorityError::CheckerRefused {
            verdict: CheckerVerdict::Refuse
        }
    );

    let candidate = CounterexampleCandidate::new(
        base.claim(),
        evidence(
            base.claim(),
            EvidenceKind::Counterexample,
            "guard-counterexample",
        ),
    )
    .expect("candidate");
    let adjudication = CounterexampleAdjudication::new(
        &candidate,
        CounterexampleVerdict::GenuineCounterexample,
        evidence(
            base.claim(),
            EvidenceKind::Adjudication,
            "guard-adjudication",
        ),
    )
    .expect("adjudication");
    let tombstone = fs_govern::evidence_contract::RevocationTombstone::new(
        &base,
        &adjudication,
        "guard revocation",
        evidence(base.claim(), EvidenceKind::Revocation, "guard-revocation"),
    )
    .expect("tombstone");
    let invalidated = base.invalidate(&tombstone).expect("invalidate");
    let safety_meet = base
        .conservative_meet(&invalidated)
        .expect("clear/invalidated meet");
    assert_eq!(safety_meet.identity(), invalidated.identity());
    assert!(base.dominates(&safety_meet));
    assert!(invalidated.dominates(&safety_meet));
    let invalidated_candidate = accepted_candidate(&invalidated, &policy, checker);
    assert_eq!(
        assess_runtime_candidate(&invalidated, &policy, &invalidated_candidate)
            .expect_err("invalidation must refuse"),
        AuthorityError::RuntimeRequirementNotMet {
            requirement: "not-invalidated"
        }
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One closed policy-preimage mutation table prevents omissions.
fn g0_checker_and_policy_identity_mutations_fail_closed() {
    let state = full_proved_state(basic_claim());
    let baseline = strict_policy(vec![
        CapabilityBinding::new("runtime-admit", 1).expect("capability"),
    ]);
    let variants = [
        CapabilityPolicy::new(
            TruthRequirement::ConditionalOrProved,
            true,
            Some(scale_family_strength()),
            true,
            true,
            true,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("truth mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            false,
            Some(scale_family_strength()),
            true,
            true,
            true,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("sat mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            None,
            true,
            true,
            true,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("nonvacuity mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            Some(scale_family_strength()),
            false,
            true,
            true,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("kernel mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            Some(scale_family_strength()),
            true,
            false,
            true,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("scale mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            Some(scale_family_strength()),
            true,
            true,
            false,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("reproduction mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            Some(scale_family_strength()),
            true,
            true,
            true,
            vec![CapabilityBinding::new("runtime-admit", 2).expect("mutated cap")],
            &[],
            &["not cross-ISA bit stability"],
        )
        .expect("capability mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            Some(scale_family_strength()),
            true,
            true,
            true,
            baseline.required_capabilities().to_vec(),
            &["trusted but irrelevant"],
            &["not cross-ISA bit stability"],
        )
        .expect("accepted-assumption mutation"),
        CapabilityPolicy::new(
            TruthRequirement::ProvedOnly,
            true,
            Some(scale_family_strength()),
            true,
            true,
            true,
            baseline.required_capabilities().to_vec(),
            &[],
            &["not a production release", "not cross-ISA bit stability"],
        )
        .expect("no-claim mutation"),
    ];
    for variant in &variants {
        assert_ne!(baseline.identity(), variant.identity());
    }

    let candidate = accepted_candidate(&state, &baseline, hash("checker-mutation"));
    let other_policy = &variants[0];
    assert!(matches!(
        assess_runtime_candidate(&state, other_policy, &candidate),
        Err(AuthorityError::IdentityMismatch { .. })
    ));
}

#[test]
#[allow(clippy::too_many_lines)] // One matrix pins every ambiguous v0 field and demotion.
fn g0_schema_versions_and_legacy_migration_never_widen() {
    let claim = basic_claim();
    assert!(matches!(
        EvidenceRef::new(
            EvidenceKind::Support,
            claim.identity(),
            hash("artifact"),
            hash("checker"),
            AUTHORITY_ALGEBRA_VERSION + 1,
        ),
        Err(AuthorityError::SchemaVersionRefused { .. })
    ));

    let ranks = [
        LegacyAuthorityRankV0::Unknown,
        LegacyAuthorityRankV0::Supported,
        LegacyAuthorityRankV0::Proved,
        LegacyAuthorityRankV0::Refuted,
    ];
    let mut matrix_identities = std::collections::BTreeSet::new();
    for rank in ranks {
        for admitted in [false, true] {
            for reproduced in [false, true] {
                let legacy = LegacyAuthorityV0::new(
                    claim.clone(),
                    rank,
                    admitted,
                    reproduced,
                    hash("legacy-matrix-source"),
                )
                .expect("legacy");
                let migration =
                    migrate_legacy_v0(LEGACY_AUTHORITY_SCHEMA_VERSION, legacy).expect("migration");
                assert_eq!(migration.state().truth(), TruthState::Unknown);
                assert_eq!(
                    migration.state().exact_admission(),
                    ExactInstanceAdmission::NotEvaluated
                );
                assert_eq!(
                    migration.state().reproduction(),
                    ReproductionState::NotAttempted
                );
                assert_eq!(migration.demotions().len(), 5);
                assert!(matrix_identities.insert(migration.identity()));
            }
        }
    }
    assert_eq!(matrix_identities.len(), ranks.len() * 2 * 2);

    let migration_identity = |claim: ClaimInstance,
                              rank: LegacyAuthorityRankV0,
                              admitted: bool,
                              reproduced: bool,
                              source: ContentHash| {
        migrate_legacy_v0(
            LEGACY_AUTHORITY_SCHEMA_VERSION,
            LegacyAuthorityV0::new(claim, rank, admitted, reproduced, source).expect("legacy"),
        )
        .expect("migration")
        .identity()
    };
    let baseline = migration_identity(
        claim.clone(),
        LegacyAuthorityRankV0::Unknown,
        false,
        false,
        hash("legacy-source"),
    );
    let changed_claim = claim_with(
        &[],
        Quantifier::ForAll,
        101,
        vec![CapabilityBinding::new("runtime-admit", 1).expect("capability")],
        &["not cross-ISA bit stability"],
    );
    for changed in [
        migration_identity(
            claim.clone(),
            LegacyAuthorityRankV0::Supported,
            false,
            false,
            hash("legacy-source"),
        ),
        migration_identity(
            claim.clone(),
            LegacyAuthorityRankV0::Unknown,
            true,
            false,
            hash("legacy-source"),
        ),
        migration_identity(
            claim.clone(),
            LegacyAuthorityRankV0::Unknown,
            false,
            true,
            hash("legacy-source"),
        ),
        migration_identity(
            claim.clone(),
            LegacyAuthorityRankV0::Unknown,
            false,
            false,
            hash("other-legacy-source"),
        ),
        migration_identity(
            changed_claim,
            LegacyAuthorityRankV0::Unknown,
            false,
            false,
            hash("legacy-source"),
        ),
    ] {
        assert_ne!(baseline, changed);
    }

    let future = LegacyAuthorityV0::new(
        claim,
        LegacyAuthorityRankV0::Unknown,
        false,
        false,
        hash("future"),
    )
    .expect("legacy");
    assert!(matches!(
        migrate_legacy_v0(LEGACY_AUTHORITY_SCHEMA_VERSION + 1, future),
        Err(AuthorityError::SchemaVersionRefused { .. })
    ));
}

#[test]
#[allow(clippy::too_many_lines)] // The closed ordered catalog is intentionally explicit.
fn g0_catalog_is_code_derived_unique_and_contract_drift_checked() {
    let first = authority_catalog_json();
    let second = authority_catalog_json();
    assert_eq!(first, second);
    assert!(first.contains("frankensim-authority-catalog-v1"));
    assert_eq!(
        first.matches("\"object_kind\":").count(),
        AUTHORITY_CATALOG_ROWS.len()
    );
    for row in AUTHORITY_CATALOG_ROWS {
        for exact_field in [
            format!("\"object_kind\":\"{}\"", row.object_kind),
            format!("\"identity_domain\":\"{}\"", row.identity_domain),
            format!("\"identity_sources\":\"{}\"", row.identity_sources),
            format!("\"binding\":\"{}\"", row.binding),
            format!("\"no_claim\":\"{}\"", row.no_claim),
        ] {
            assert!(
                first.contains(&exact_field),
                "catalog JSON omits exact field {exact_field}"
            );
        }
    }

    let mut kinds = std::collections::BTreeSet::new();
    let mut domains = std::collections::BTreeSet::new();
    let contract = include_str!("../CONTRACT.md");
    for row in AUTHORITY_CATALOG_ROWS {
        assert!(
            kinds.insert(row.object_kind),
            "duplicate kind {}",
            row.object_kind
        );
        assert!(
            domains.insert(row.identity_domain),
            "duplicate domain {}",
            row.identity_domain
        );
    }
    assert_eq!(
        AUTHORITY_CATALOG_ROWS.len(),
        22,
        "the closed v1 catalog changed; update the contract and migration policy explicitly"
    );
    let actual = AUTHORITY_CATALOG_ROWS
        .iter()
        .map(|row| (row.object_kind, row.identity_domain))
        .collect::<Vec<_>>();
    let expected = vec![
        ("claim-statement", "frankensim.fs-govern.claim-statement.v1"),
        (
            "quantified-domain",
            "frankensim.fs-govern.quantified-domain.v1",
        ),
        ("assumption-set", "frankensim.fs-govern.assumption-set.v1"),
        ("semantic-claim", "frankensim.fs-govern.semantic-claim.v1"),
        (
            "claim-lane-binding",
            "frankensim.fs-govern.claim-lane-binding.v1",
        ),
        ("claim-instance", "frankensim.fs-govern.claim-instance.v1"),
        ("proof-lane", "frankensim.fs-govern.proof-lane.v1"),
        ("evidence-ref", "frankensim.fs-govern.evidence-ref.v1"),
        (
            "nonvacuity-evidence",
            "frankensim.fs-govern.nonvacuity-evidence.v1",
        ),
        ("evidence-state", "frankensim.fs-govern.evidence-state.v1"),
        ("authority-state", "frankensim.fs-govern.authority-state.v1"),
        ("inference-rule", "frankensim.fs-govern.inference-rule.v1"),
        ("support-edge", "frankensim.fs-govern.support-edge.v1"),
        ("attack-edge", "frankensim.fs-govern.attack-edge.v1"),
        (
            "counterexample-candidate",
            "frankensim.fs-govern.counterexample.v1",
        ),
        (
            "counterexample-adjudication",
            "frankensim.fs-govern.counterexample-adjudication.v1",
        ),
        (
            "revocation-tombstone",
            "frankensim.fs-govern.revocation-tombstone.v1",
        ),
        (
            "capability-policy",
            "frankensim.fs-govern.capability-policy.v1",
        ),
        (
            "checker-decision",
            "frankensim.fs-govern.checker-decision.v1",
        ),
        ("authority-head", "frankensim.fs-govern.authority-head.v1"),
        (
            "runtime-admission",
            "frankensim.fs-govern.runtime-admission.v1",
        ),
        (
            "authority-migration",
            "frankensim.fs-govern.authority-migration.v1",
        ),
    ];
    assert_eq!(actual, expected, "closed catalog order/domain drift");
    let contract_table = contract
        .split("### Exact objects and identity bindings")
        .nth(1)
        .expect("Phase 0B-A catalog section")
        .split("All canonical encodings")
        .next()
        .expect("catalog table boundary");
    let contract_rows = contract_table
        .lines()
        .filter(|line| line.starts_with("| `"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(
        contract_rows,
        authority_catalog_markdown_rows(),
        "CONTRACT catalog must be the exact code-derived rows in both directions"
    );
    assert!(
        AUTHORITY_CATALOG_ROWS
            .iter()
            .all(|row| row.schema_version == AUTHORITY_ALGEBRA_VERSION),
        "every v1 catalog row must bind the current algebra version"
    );
}

#[test]
fn g0_authority_logs_are_complete_bounded_and_never_truncated() {
    let state = full_proved_state(basic_claim());
    let policy = strict_policy(vec![
        CapabilityBinding::new("runtime-admit", 1).expect("capability"),
    ]);
    let candidate = accepted_candidate(&state, &policy, hash("log-checker"));
    let log = authority_log_json(&state, Some(&policy), Some(&candidate), None).expect("log");
    for required in [
        "\"object_kind\":\"authority-state\"",
        "\"source_identity\"",
        "\"authority_identity\"",
        "\"schema_version\":1",
        "\"policy_version\":1",
        "\"checker_identity\"",
        "\"truth\":\"proved\"",
        "\"satisfiability\":\"satisfiable\"",
        "\"nonvacuity\":\"nonvacuous\"",
        "\"exact_instance\":\"admitted\"",
        "\"kernel\":\"kernel-checked\"",
        "\"scale\":\"scale-qualified\"",
        "\"reproduction\":\"reproduced\"",
        "\"invalidation\":\"clear\"",
        "not cross-ISA bit stability",
        "\"remedy\":\"none\"",
    ] {
        assert!(log.contains(required), "log misses {required}");
    }
    assert!(log.contains(&format!("\"checker_identity\":\"{}\"", hash("log-checker"))));
    assert!(log.len() <= MAX_AUTHORITY_LOG_BYTES);
    assert!(matches!(
        authority_log_json(&state, None, Some(&candidate), None),
        Err(AuthorityError::IncompatibleAxes { .. })
    ));
    let other_policy = strict_policy(vec![]);
    assert!(matches!(
        authority_log_json(&state, Some(&other_policy), Some(&candidate), None),
        Err(AuthorityError::IdentityMismatch { .. })
    ));
    let other_state = AuthorityState::unknown(state.claim().clone()).expect("other state");
    assert!(matches!(
        authority_log_json(&other_state, Some(&policy), Some(&candidate), None),
        Err(AuthorityError::IdentityMismatch { .. })
    ));

    let long_entries = (0..8)
        .map(|index| format!("boundary-{index}-{}", "x".repeat(3000)))
        .collect::<Vec<_>>();
    let refs = long_entries.iter().map(String::as_str).collect::<Vec<_>>();
    let oversized_claim = claim_with(&[], Quantifier::ForAll, 100, vec![], &refs);
    let oversized_state = AuthorityState::unknown(oversized_claim).expect("oversized state");
    assert!(matches!(
        authority_log_json(&oversized_state, None, None, None),
        Err(AuthorityError::LogCapacityExceeded {
            cap: MAX_AUTHORITY_LOG_BYTES,
            ..
        })
    ));
}

#[test]
fn g0_limits_duplicates_and_default_inference_rules_fail_closed() {
    assert!(fs_govern::evidence_contract::DEFAULT_INFERENCE_RULES.is_empty());
    assert!(matches!(
        UnitSystem::new(0, 1, vec![]),
        Err(AuthorityError::InvalidValue { .. })
    ));
    assert!(matches!(
        FiveExplicits::new(
            UnitSystem::dimensionless(),
            0,
            AuthorityBudget {
                work_units: 0,
                memory_bytes: 0,
                wall_time_millis: 0,
                reviewer_slots: 0,
            },
            vec![
                VersionBinding::new("solver", "1").expect("version"),
                VersionBinding::new("solver", "2").expect("duplicate"),
            ],
            vec![],
        ),
        Err(AuthorityError::DuplicateMember { .. })
    ));
    assert!(matches!(
        InferenceRule::new("future theorem", 0, hash("rule")),
        Err(AuthorityError::InvalidValue { .. })
    ));
}

#[test]
fn identity_domain_constants_remain_distinct() {
    let domains = [
        ADJUDICATION_IDENTITY_DOMAIN,
        ASSUMPTION_SET_IDENTITY_DOMAIN,
        ATTACK_EDGE_IDENTITY_DOMAIN,
        AUTHORITY_HEAD_IDENTITY_DOMAIN,
        AUTHORITY_MIGRATION_IDENTITY_DOMAIN,
        NONVACUITY_EVIDENCE_IDENTITY_DOMAIN,
        SEMANTIC_CLAIM_IDENTITY_DOMAIN,
    ];
    let unique = domains
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), domains.len());
}
