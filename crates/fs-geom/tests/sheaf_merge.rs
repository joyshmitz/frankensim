//! Sheaf-merge conformance (the lmp4.12 crown jewel; runs under the
//! `sheaf-merge` feature). Acceptance: gauge-fit mismatches auto-reconcile
//! only after a nominal post-state residual check; dominant fixed-iteration
//! remainders remain candidate conflicts localized to the right cells with
//! both caller-supplied parent labels; type-level collisions are caught before any
//! decomposition; degraded-gap merges are flagged low-confidence; the
//! Sev-0 adversarial case (reconciliation cannot reach a passing state)
//! ESCALATES rather than reporting a resolved state falsely; trivial merges
//! take the fast paths; the candidate diagnostic measures retained conflicts.
#![cfg(feature = "sheaf-merge")]

use std::collections::BTreeMap;

use fs_geom::sheaf_merge::{
    BranchState, CandidateRateError, Confidence, MergeOutcome, candidate_remainder_conflict_rate,
    spectral_gap, three_way_merge, type_conflicts,
};
use fs_geom::sheaf_repair::{SheafSkeleton, apply_gauge, hodge_decompose};

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-geom/sheaf-merge\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

/// 3-patch triangle (contractible: no harmonic space).
fn triangle() -> SheafSkeleton {
    SheafSkeleton {
        n_patches: 3,
        edges: vec![(0, 1), (1, 2), (0, 2)],
        triangles: vec![(0, 1, 2)],
    }
}

/// 4-patch ring (one cycle: harmonic space is 1-dimensional).
fn ring() -> SheafSkeleton {
    SheafSkeleton {
        n_patches: 4,
        edges: vec![(0, 1), (1, 2), (2, 3), (0, 3)],
        triangles: vec![],
    }
}

fn branch(name: &str, mismatch: Vec<f64>) -> BranchState {
    BranchState {
        provenance: name.to_string(),
        mismatch,
        assignments: BTreeMap::new(),
    }
}

fn norm_inf(values: &[f64]) -> f64 {
    assert!(
        values.iter().all(|value| value.is_finite()),
        "test norm requires finite values: {values:?}"
    );
    values
        .iter()
        .fold(0.0f64, |largest, value| largest.max(value.abs()))
}

#[test]
fn sm_001_gauge_fit_auto_reconciles_with_checked_residual() {
    let sk = triangle();
    let base = vec![0.0; 3];
    // X re-gauges patch 1, Y re-gauges patch 2: pure coboundary edits.
    let x = branch("agent-x@c1", sk.d0(&[0.0, 0.02, 0.0]));
    let y = branch("agent-y@c2", sk.d0(&[0.0, 0.0, -0.015]));
    let out = three_way_merge(&sk, &base, &x, &y, None, 1e-9, 1e-6);
    match out {
        MergeOutcome::Resolved {
            merged,
            gauge,
            residual_receipt,
            confidence,
        } => {
            assert!(
                residual_receipt.post_norm <= residual_receipt.tol,
                "nominal residual was checked"
            );
            assert!(
                merged.iter().all(|v| v.abs() < 1e-9),
                "nominal residual-passing state: {merged:?}"
            );
            // This fixture's deterministic gauge fit recovered both offsets.
            assert!((gauge[1] - 0.02).abs() < 1e-9, "{gauge:?}");
            assert!((gauge[2] + 0.015).abs() < 1e-9, "{gauge:?}");
            assert!(matches!(confidence, Confidence::Normal { .. }));
        }
        other => panic!("coboundary edits must auto-resolve: {other:?}"),
    }
    verdict(
        "sm-001",
        "two gauge edits auto-reconcile; the nominal post-norm is checked under tol \
         and the recovered gauge matches both branches",
    );
}

#[test]
fn sm_002_retained_ring_witness_escalates_candidate_with_parent_labels() {
    let sk = ring();
    let base = vec![0.0; 4];
    // X and Y reinforce the same oriented circulation around the cycle.
    let x = branch("agent-x@c7", vec![0.03, 0.03, 0.03, -0.03]);
    let y = branch("agent-y@c9", vec![0.01, 0.01, 0.01, -0.01]);
    let union: Vec<f64> = x
        .mismatch
        .iter()
        .zip(&y.mismatch)
        .zip(&base)
        .map(|((x_value, y_value), base_value)| x_value + y_value - base_value)
        .collect();
    let split = hodge_decompose(&sk, &union);
    assert!(norm_inf(&split.harmonic) > 0.03, "nonzero retained witness");
    assert!(
        norm_inf(&sk.d1(&split.harmonic)) < 1e-12,
        "ring witness is closed in the retained skeleton complex"
    );
    assert!(
        norm_inf(&sk.d0t(&split.harmonic)) < 1e-12,
        "ring witness is orthogonal to patch-gauge cochains"
    );
    let cycle_pairing =
        split.harmonic[0] + split.harmonic[1] + split.harmonic[2] - split.harmonic[3];
    assert!(
        (cycle_pairing - 0.16).abs() < 1e-12,
        "nonzero cycle pairing witnesses non-exactness: {cycle_pairing}"
    );
    let out = three_way_merge(&sk, &base, &x, &y, None, 1e-9, 1e-6);
    match out {
        MergeOutcome::Conflicted {
            candidate_remainders,
            type_conflicts,
            ..
        } => {
            assert!(type_conflicts.is_empty());
            assert_eq!(candidate_remainders.len(), 1);
            let c = &candidate_remainders[0];
            assert_eq!(
                c.cells.len(),
                4,
                "all four cycle cells exceed this fixture tolerance"
            );
            assert_eq!(
                c.parents,
                ("agent-x@c7".to_string(), "agent-y@c9".to_string()),
                "both caller-supplied parent labels attached"
            );
        }
        other => panic!("the witnessed ring remainder must block auto-merge: {other:?}"),
    }
    verdict(
        "sm-002",
        "the retained ring cochain has a closed non-exact skeleton witness; runtime \
         output remains a candidate conflict with full support and both parents",
    );
}

#[test]
fn sm_003_gauge_edits_resolve_and_cycle_candidates_block_auto_merge() {
    // Property sweep: these seeded gauge-only edits converge and resolve; an
    // injected cycle component blocks automatic merge on the ring.
    let sk = ring();
    let base = vec![0.0; 4];
    let mut state = 0xd00d_u64;
    let mut lcg = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64) / (1u64 << 53) as f64 - 0.5
    };
    for trial in 0..20 {
        let gx: Vec<f64> = (0..4).map(|_| 0.1 * lcg()).collect();
        let gy: Vec<f64> = (0..4).map(|_| 0.1 * lcg()).collect();
        let x = branch("x", sk.d0(&gx));
        let y = branch("y", sk.d0(&gy));
        let out = three_way_merge(&sk, &base, &x, &y, None, 1e-9, 1e-6);
        assert!(
            matches!(out, MergeOutcome::Resolved { .. }),
            "gauge-only trial {trial} must resolve"
        );
        // Now inject a cycle component into X.
        let mut mx = sk.d0(&gx);
        let eps = 0.02 + 0.05 * lcg().abs();
        for (k, v) in mx.iter_mut().enumerate() {
            *v += if k == 3 { -eps } else { eps };
        }
        let x2 = branch("x", mx);
        let y2 = branch("y", sk.d0(&gy));
        let out2 = three_way_merge(&sk, &base, &x2, &y2, None, 1e-9, 1e-6);
        assert!(
            matches!(
                out2,
                MergeOutcome::Conflicted {
                    ref candidate_remainders,
                    ..
                } if !candidate_remainders.is_empty()
            ),
            "cycle-tainted trial {trial} must remain a candidate conflict"
        );
    }
    verdict(
        "sm-003",
        "20 seeded trials: converged gauge edits resolve; cycle-tainted remainders \
         stay candidate conflicts rather than topology claims",
    );
}

#[test]
fn sm_003b_exact_tree_remainder_is_candidate_not_topology() {
    let n_patches = 32;
    let sk = SheafSkeleton {
        n_patches,
        edges: (0..n_patches - 1).map(|patch| (patch, patch + 1)).collect(),
        triangles: Vec::new(),
    };
    assert_eq!(
        sk.edges.len() + 1,
        sk.n_patches,
        "connected tree has H1 = 0"
    );
    let mut x_potential = vec![0.0; n_patches];
    let mut y_potential = vec![0.0; n_patches];
    for patch in 1..n_patches {
        x_potential[patch] = x_potential[patch - 1] + 0.1 / 31.0;
        y_potential[patch] = y_potential[patch - 1] + 0.05 / 31.0;
    }
    let x = branch("tree-x", sk.d0(&x_potential));
    let y = branch("tree-y", sk.d0(&y_potential));
    let base = vec![0.0; sk.edges.len()];
    let union: Vec<f64> = x
        .mismatch
        .iter()
        .zip(&y.mismatch)
        .map(|(x_value, y_value)| x_value + y_value)
        .collect();
    let known_gauge: Vec<f64> = x_potential
        .iter()
        .zip(&y_potential)
        .map(|(x_value, y_value)| x_value + y_value)
        .collect();
    assert!(
        norm_inf(&apply_gauge(&sk, &union, &known_gauge)) < 1e-15,
        "an explicit local patch-gauge repair exists"
    );

    match three_way_merge(&sk, &base, &x, &y, None, 1e-9, 1e-6) {
        MergeOutcome::Resolved {
            residual_receipt, ..
        } => assert!(
            residual_receipt.post_norm <= residual_receipt.tol,
            "a better converged solver may legitimately resolve this exact tree"
        ),
        MergeOutcome::Conflicted {
            candidate_remainders,
            type_conflicts,
            ..
        } => {
            assert!(type_conflicts.is_empty());
            assert!(
                !candidate_remainders.is_empty(),
                "bounded solve remainder must be retained only as a candidate"
            );
        }
        other => panic!(
            "the exact tree may resolve or retain an honest candidate, but must not take another path: {other:?}"
        ),
    }
}

#[test]
fn sm_004_sev0_escalates_instead_of_false_resolution() {
    // A COEXACT residue on the triangle: gauge reconciliation cannot
    // reach a passing state (circulation around the triple junction is
    // not a coboundary), and there is NO harmonic space here — the
    // naive path would report "resolved" despite a nominal residual above the
    // requested tolerance. The Sev-0 guard must escalate.
    let sk = triangle();
    let base = vec![0.0; 3];
    let circulation = sk.d1t(&[0.05]);
    let x = branch("x", circulation.clone());
    let y = branch("y", vec![0.0; 3]);
    // Wait: Y unchanged from base triggers the trivial path — perturb Y
    // slightly so the merge genuinely runs.
    let y = BranchState {
        mismatch: sk.d0(&[0.0, 1e-3, 0.0]),
        ..y
    };
    let out = three_way_merge(&sk, &base, &x, &y, None, 1e-6, 1e-6);
    match out {
        MergeOutcome::EscalatedUnresolved {
            post_norm,
            tol,
            fractions,
            confidence,
        } => {
            assert!(post_norm > tol, "the failure is real: {post_norm} > {tol}");
            assert!(
                fractions.1 > 0.5,
                "the residue is predominantly coexact: {fractions:?}"
            );
            assert!(matches!(confidence, Confidence::Normal { .. }));
        }
        other => panic!("Sev-0: must escalate, never report resolved falsely: {other:?}"),
    }
    // And the trivial fast paths themselves.
    let same = branch("s", sk.d0(&[0.0, 0.01, 0.0]));
    let t1 = three_way_merge(&sk, &base, &same, &same.clone(), None, 1e-9, 1e-6);
    assert!(matches!(t1, MergeOutcome::Trivial { reason, .. } if reason == "branches identical"));
    let unchanged = branch("u", base.clone());
    let t2 = three_way_merge(&sk, &base, &unchanged, &same, None, 1e-9, 1e-6);
    assert!(
        matches!(t2, MergeOutcome::Trivial { reason, .. } if reason == "X unchanged from base")
    );
    // Confidence must survive escalation too. Adding an isolated patch makes
    // the weighted graph disconnected (lambda_2 = 0) while leaving the
    // triangle circulation fixture intact.
    let low_gap = SheafSkeleton {
        n_patches: 4,
        edges: sk.edges.clone(),
        triangles: sk.triangles.clone(),
    };
    let x_low = branch("x-low", low_gap.d1t(&[0.05]));
    let y_low = branch("y-low", low_gap.d0(&[0.0, 1e-3, 0.0, 0.0]));
    assert!(matches!(
        three_way_merge(
            &low_gap,
            &base,
            &x_low,
            &y_low,
            None,
            1e-6,
            1e-6,
        ),
        MergeOutcome::EscalatedUnresolved {
            confidence: Confidence::LowGap { gap, .. },
            ..
        } if gap.abs() < f64::EPSILON
    ));
    verdict(
        "sm-004",
        "a coexact residue escalates unresolved (never a false resolution); trivial \
         fast paths report edit equality without a residual receipt",
    );
}

#[test]
fn sm_005_assignment_authority_refusal_and_degraded_gap() {
    let sk = triangle();
    let base = vec![0.0; 3];
    // Pairwise-different values are a useful candidate diagnostic, but without
    // a base assignment map either branch might be unchanged. The merge must
    // refuse rather than manufacture three-way conflict authority.
    let mut x = branch("x", sk.d0(&[0.0, 0.01, 0.0]));
    x.assignments
        .insert("loadcase/cruise".to_string(), "2.5g".to_string());
    let mut y = branch("y", sk.d0(&[0.0, 0.0, 0.01]));
    y.assignments
        .insert("loadcase/cruise".to_string(), "3.0g".to_string());
    let candidates = type_conflicts(&x, &y);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].key, "loadcase/cruise");
    assert_eq!(candidates[0].x_value, "2.5g");
    assert_eq!(candidates[0].y_value, "3.0g");
    assert!(matches!(
        three_way_merge(&sk, &base, &x, &y, None, 1e-9, 1e-6),
        MergeOutcome::Refused { reason }
            if reason == "base-aware assignment merge is not represented"
    ));
    // Degraded gap: two clusters joined by ONE weak interface — the
    // weighted algebraic connectivity collapses and the merge is
    // flagged low-confidence (R5).
    let barbell = SheafSkeleton {
        n_patches: 6,
        edges: vec![(0, 1), (0, 2), (1, 2), (3, 4), (3, 5), (4, 5), (2, 3)],
        triangles: vec![(0, 1, 2), (3, 4, 5)],
    };
    let weights = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1e-4];
    let gap = spectral_gap(&barbell, Some(&weights));
    assert!(gap < 1e-3, "weak-link gap is tiny: {gap}");
    let xb = branch("x", barbell.d0(&[0.0, 0.01, 0.0, 0.0, 0.0, 0.0]));
    let yb = branch("y", barbell.d0(&[0.0, 0.0, 0.0, 0.0, -0.01, 0.0]));
    let base_b = vec![0.0; 7];
    let out_b = three_way_merge(&barbell, &base_b, &xb, &yb, Some(&weights), 1e-9, 1e-3);
    match out_b {
        MergeOutcome::Resolved { confidence, .. } => {
            assert!(
                matches!(confidence, Confidence::LowGap { gap, threshold }
                    if gap < threshold),
                "degraded-gap merge must be flagged: {confidence:?}"
            );
        }
        other => panic!("the barbell gauge merge itself resolves: {other:?}"),
    }
    // A healthy complex at the same threshold is Normal.
    let healthy_gap = spectral_gap(&sk, None);
    assert!(
        healthy_gap > 1e-3,
        "triangle is well-coupled: {healthy_gap}"
    );
    let disconnected = SheafSkeleton {
        n_patches: 4,
        edges: vec![(0, 1), (2, 3)],
        triangles: Vec::new(),
    };
    assert!(
        spectral_gap(&disconnected, None).abs() < f64::EPSILON,
        "lambda_2 remains zero when another positive eigenvalue exists"
    );
    let zero_weight_cut = SheafSkeleton {
        n_patches: 3,
        edges: vec![(0, 1), (1, 2)],
        triangles: Vec::new(),
    };
    assert!(
        spectral_gap(&zero_weight_cut, Some(&[1.0, 0.0])).abs() < f64::EPSILON,
        "a zero-weight cut disconnects the weighted graph"
    );
    verdict(
        "sm-005",
        "base-less assignment payloads refuse despite a pairwise-difference candidate; \
         the weak-link barbell merge resolves but is FLAGGED LowGap (R5)",
    );
}

#[test]
fn sm_005a_nonconflicting_assignments_refuse_instead_of_disappearing() {
    let sk = triangle();
    let base = vec![0.0; 3];
    let mismatch = sk.d0(&[0.0, 0.01, 0.0]);
    let mut x = branch("x", mismatch.clone());
    x.assignments
        .insert("material/left".to_string(), "steel".to_string());
    let mut y = branch("y", mismatch);
    y.assignments
        .insert("loadcase/cruise".to_string(), "2.5g".to_string());
    assert!(matches!(
        three_way_merge(&sk, &base, &x, &y, None, 1e-9, 1e-6),
        MergeOutcome::Refused { reason }
            if reason == "base-aware assignment merge is not represented"
    ));
}

#[test]
fn sm_005b_nonfinite_or_malformed_inputs_refuse() {
    let sk = triangle();
    let base = vec![0.0; 3];
    let invalid = branch("invalid", vec![f64::NAN, 0.0, 0.0]);
    let valid = branch("valid", vec![0.0, 1e-3, 0.0]);
    assert!(spectral_gap(&sk, Some(&[f64::MAX; 3])).is_nan());
    assert!(matches!(
        three_way_merge(&sk, &base, &invalid, &valid, None, 1e-6, 1e-6),
        MergeOutcome::Refused { reason } if reason == "cochains must be finite"
    ));
    assert!(matches!(
        three_way_merge(
            &sk,
            &base,
            &valid,
            &valid,
            Some(&[1.0, 1.0]),
            1e-6,
            1e-6,
        ),
        MergeOutcome::Refused { reason }
            if reason == "weights must match edges and be finite non-negative values"
    ));
    assert!(matches!(
        three_way_merge(&sk, &base, &valid, &invalid, None, f64::INFINITY, 1e-6),
        MergeOutcome::Refused { .. }
    ));
    assert!(matches!(
        three_way_merge(&sk, &base, &valid, &invalid, None, 1e-6, 0.0),
        MergeOutcome::Refused { reason }
            if reason == "residual tolerance must be finite non-negative and gap threshold finite positive"
    ));
    let duplicate_triangle = SheafSkeleton {
        n_patches: 3,
        edges: sk.edges.clone(),
        triangles: vec![(0, 1, 2), (0, 1, 2)],
    };
    assert!(matches!(
        three_way_merge(
            &duplicate_triangle,
            &base,
            &valid,
            &valid,
            None,
            1e-6,
            1e-6,
        ),
        MergeOutcome::Refused { reason } if reason == "malformed sheaf skeleton"
    ));
    assert!(matches!(
        three_way_merge(&sk, &[0.0; 2], &valid, &valid, None, 1e-6, 1e-6),
        MergeOutcome::Refused { reason } if reason == "cochain length mismatch"
    ));

    let merged_overflow_x = branch("overflow-x", vec![f64::MAX, 0.0, 0.0]);
    let merged_overflow_y = branch("overflow-y", vec![f64::MAX, 1.0, 0.0]);
    assert!(matches!(
        three_way_merge(
            &sk,
            &base,
            &merged_overflow_x,
            &merged_overflow_y,
            None,
            1e-6,
            1e-6,
        ),
        MergeOutcome::Refused { reason }
            if reason == "merged cochain arithmetic is non-finite"
    ));

    let split_overflow_x = branch("split-overflow-x", vec![1e200, 0.0, 0.0]);
    let split_overflow_y = branch("split-overflow-y", vec![0.0, 1e200, 0.0]);
    assert!(matches!(
        three_way_merge(
            &sk,
            &base,
            &split_overflow_x,
            &split_overflow_y,
            None,
            1e-6,
            1e-6,
        ),
        MergeOutcome::Refused { reason }
            if reason == "decomposition arithmetic is non-finite"
    ));

    assert!(matches!(
        three_way_merge(
            &sk,
            &base,
            &valid,
            &branch("other", vec![1e-3, 0.0, 0.0]),
            Some(&[f64::MAX; 3]),
            1e-6,
            1e-6,
        ),
        MergeOutcome::Refused { reason }
            if reason == "spectral-gap arithmetic is non-finite"
    ));
}

#[test]
fn sm_006_candidate_diagnostic_harness() {
    // Candidate-remainder rate over seeded gauge-dominated edits. This is one
    // diagnostic input, not the full Proposal 10 unresolved-merge criterion.
    let ring4 = ring();
    let rate_ring = candidate_remainder_conflict_rate(&ring4, 60, 0.1, 0xfeed)
        .expect("well-formed seeded ring diagnostic");
    // Gauge-dominated edits on a cycle-bearing complex may leave a small
    // fixed-iteration candidate remainder. This fixture threshold catches
    // regressions in that narrow diagnostic; it is not the Proposal 10 kill
    // criterion.
    assert!(
        rate_ring < 0.25,
        "gauge-dominated merges must rarely retain candidate conflicts: {rate_ring}"
    );
    let tri = triangle();
    let rate_tri = candidate_remainder_conflict_rate(&tri, 60, 0.1, 0xfeed)
        .expect("well-formed seeded triangle diagnostic");
    assert!(
        rate_tri.abs() < f64::EPSILON,
        "no retained candidate conflicts in this triangle fixture: {rate_tri}"
    );
    println!(
        "{{\"metric\":\"candidate-remainder-conflict-rate\",\"ring\":{rate_ring:.3},\"triangle\":{rate_tri:.3},\
         \"fixture_threshold\":0.25}}"
    );
    verdict(
        "sm-006",
        "the candidate-conflict diagnostic runs: triangle rate 0 and ring rate below \
         this fixture's regression threshold",
    );

    assert_eq!(
        candidate_remainder_conflict_rate(&ring4, 0, 0.1, 0xfeed),
        Err(CandidateRateError::ZeroTrials)
    );
    assert_eq!(
        candidate_remainder_conflict_rate(&ring4, 1, f64::NAN, 0xfeed),
        Err(CandidateRateError::InvalidEditScale)
    );
    let malformed = SheafSkeleton {
        n_patches: 2,
        edges: vec![(1, 0)],
        triangles: Vec::new(),
    };
    assert_eq!(
        candidate_remainder_conflict_rate(&malformed, 1, 0.1, 0xfeed),
        Err(CandidateRateError::MalformedSkeleton)
    );
    assert!(matches!(
        candidate_remainder_conflict_rate(&ring4, 1, 1e200, 0xfeed),
        Err(CandidateRateError::TrialRefused { .. })
    ));
}
