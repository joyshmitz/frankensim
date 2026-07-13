//! End-to-end battery: a deterministic PDHG cantilever iterate with an
//! advisory, endpoint-checked tropical load path from load to support.

use fs_evidence::Color;
use fs_exec::{Budget, CancelGate, Cx, ExecMode, StreamKey};
use fs_sparse::Csr;
use fs_truss::{
    LayoutCertificateProblem, LayoutCertificateRefusal, LayoutCertificateStatus, PdhgSettings,
};
use fs_truss_e2e::{
    TrussError, analyze_load_path, optimality_color_from_certificate, rescale_optimality_color,
    run_campaign,
};

fn with_gate_cx<R>(gate: &CancelGate, f: impl FnOnce(&Cx<'_>) -> R) -> R {
    let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
    pool.scope(|arena| {
        let cx = Cx::new(
            gate,
            arena,
            StreamKey {
                seed: 0x7A55,
                kernel_id: 1,
                tile: 0,
                iteration: 0,
            },
            Budget::INFINITE,
            ExecMode::Deterministic,
        );
        f(&cx)
    })
}

fn with_cx<R>(f: impl FnOnce(&Cx<'_>) -> R) -> R {
    with_gate_cx(&CancelGate::new(), f)
}

fn campaign(
    nx: usize,
    ny: usize,
    w: f64,
    h: f64,
    gap_tol: f64,
) -> Result<fs_truss_e2e::TrussReport, TrussError> {
    with_cx(|cx| run_campaign(nx, ny, w, h, gap_tol, cx))
}

#[test]
fn the_converged_truss_has_a_bounded_unique_load_path() {
    let report = campaign(4, 3, 4.0, 2.0, 1e-4).expect("valid tropical load path");
    // a real ground structure was optimized down to a sparse active set.
    assert!(report.num_members > report.num_active, "nothing was pruned");
    assert!(report.num_active > 0, "no active bars");
    assert!(report.total_volume > 0.0, "zero volume");
    // Solver convergence remains a diagnostic, independent of the outward
    // optimum certificate.
    assert!(
        report.solver_converged,
        "gap {} eq_res {}",
        report.gap, report.eq_residual
    );
    let Color::Verified { lo, hi } = report.optimality_color else {
        panic!("the repaired primal and checked dual must certify optimality");
    };
    assert!(lo.is_finite() && hi.is_finite());
    assert!(
        lo > 0.0,
        "the scaled solver dual must retain a useful bound"
    );
    assert!(lo <= hi, "inverted optimum interval [{lo}, {hi}]");
    // The advisory path is non-trivial and carries real rounded volume.
    assert!(
        report.critical_path.len() >= 2,
        "path too short: {:?}",
        report.critical_path
    );
    assert!(report.critical_path_volume > 0.0);
    assert!(report.bottleneck_member.is_some());
    assert!(
        report
            .critical_path
            .contains(&report.bottleneck_member.unwrap())
    );
    let Color::Estimated { dispersion, .. } = report.load_path_color else {
        panic!("thresholded load path must remain estimated");
    };
    assert!(dispersion.is_infinite());
    // the critical path carries no more than the whole structure.
    assert!(report.critical_path_volume <= report.total_volume + 1e-6);
    println!(
        "{{\"campaign\":\"trusspath\",\"members\":{},\"active\":{},\"volume\":{:.4},\"gap\":{:.2e},\
         \"eq_res\":{:.2e},\"iters\":{},\"path_len\":{},\"path_volume\":{:.4},\"bottleneck\":{:?}}}",
        report.num_members,
        report.num_active,
        report.total_volume,
        report.gap,
        report.eq_residual,
        report.iters,
        report.critical_path.len(),
        report.critical_path_volume,
        report.bottleneck_member,
    );
}

#[test]
fn the_campaign_is_deterministic() {
    let a = campaign(4, 3, 4.0, 2.0, 1e-4).expect("first run");
    let b = campaign(4, 3, 4.0, 2.0, 1e-4).expect("second run");
    assert_eq!(a.total_volume.to_bits(), b.total_volume.to_bits());
    assert_eq!(a.critical_path, b.critical_path);
    assert_eq!(a.bottleneck_member, b.bottleneck_member);
    assert_eq!(a.optimality_color, b.optimality_color);
}

#[test]
fn unavailable_certificate_never_promotes_finite_diagnostics() {
    let matrix = Csr::from_parts(1, 2, vec![0, 2], vec![0, 1], vec![1.0, -1.0]);
    let costs = [1.0, 1.0];
    let loads = [1.0];
    let problem = LayoutCertificateProblem::try_new(&matrix, &costs, &loads)
        .expect("well-formed paired fixture");
    let status = LayoutCertificateStatus::Unavailable(LayoutCertificateRefusal::RankDeficient {
        active_rows: 1,
        rank: 0,
    });
    let settings = PdhgSettings::default();
    with_cx(|cx| {
        assert!(matches!(
            optimality_color_from_certificate(
                &problem,
                &[0.0, 0.0],
                &[0.0],
                settings,
                &status,
                0.0,
                0.0,
                cx,
            )
            .expect("unavailable promotion fallback"),
            Color::Estimated {
                dispersion: 0.0,
                ..
            }
        ));
        assert!(matches!(
            optimality_color_from_certificate(
                &problem,
                &[0.0, 0.0],
                &[0.0],
                settings,
                &status,
                f64::NAN,
                0.0,
                cx,
            )
            .expect("non-finite diagnostic fallback"),
            Color::Estimated { dispersion, .. } if dispersion.is_infinite()
        ));
    });
}

#[test]
fn certificate_promotion_rejects_another_problem_and_rescales_outward() {
    let matrix = Csr::from_parts(1, 2, vec![0, 2], vec![0, 1], vec![1.0, -1.0]);
    let costs = [1.0, 1.0];
    let loads = [1.0];
    let other_loads = [2.0];
    let problem = LayoutCertificateProblem::try_new(&matrix, &costs, &loads)
        .expect("well-formed source problem");
    let other_problem = LayoutCertificateProblem::try_new(&matrix, &costs, &other_loads)
        .expect("well-formed distinct problem");
    let settings = PdhgSettings::default();
    with_cx(|cx| {
        let status = problem
            .certify_optimum(
                &[0.0, 0.0],
                &[0.0],
                settings,
                fs_truss::LayoutCertificateLimits::default(),
                cx,
            )
            .expect("source certificate attempt");
        assert!(matches!(status, LayoutCertificateStatus::Certified(_)));
        assert!(matches!(
            optimality_color_from_certificate(
                &other_problem,
                &[0.0, 0.0],
                &[0.0],
                settings,
                &status,
                0.0,
                0.0,
                cx,
            )
            .expect("context-mismatch preflight"),
            Color::Estimated { .. }
        ));
    });

    let scaled = rescale_optimality_color(&Color::Verified { lo: 1.0, hi: 2.0 }, 3.0);
    let Color::Verified { lo, hi } = scaled else {
        panic!("positive physical scaling must preserve Verified");
    };
    assert!(lo <= 1.0 / 3.0 && hi >= 2.0 / 3.0);
    assert!(matches!(
        rescale_optimality_color(&Color::Verified { lo: 1.0, hi: 2.0 }, 0.0),
        Color::Estimated { dispersion, .. } if dispersion.is_infinite()
    ));
}

#[test]
fn invalid_or_unbounded_campaigns_refuse_before_ground_structure_work() {
    assert!(matches!(
        campaign(1, 2, 1.0, 1.0, 1e-4),
        Err(TrussError::InvalidInput {
            field: "grid dimensions",
            ..
        })
    ));
    assert!(matches!(
        campaign(17, 16, 1.0, 1.0, 1e-4),
        Err(TrussError::InvalidInput {
            field: "grid node count",
            ..
        })
    ));
    for (width, height, tolerance) in [
        (f64::NAN, 1.0, 1e-4),
        (1.0, f64::INFINITY, 1e-4),
        (1.0, 1.0, 0.0),
    ] {
        assert!(matches!(
            campaign(2, 2, width, height, tolerance),
            Err(TrussError::InvalidInput { .. })
        ));
    }
    assert!(matches!(
        campaign(2, 2, 0.01, 0.01, 1e-4),
        Err(TrussError::NoCandidateMembers)
    ));

    // 64 nodes is the exact cubic-preflight boundary. It reaches the later
    // candidate/solver budget; 65 nodes is refused before construction.
    let boundary = campaign(8, 8, 4.0, 2.0, 1e-4);
    assert!(
        matches!(boundary, Err(TrussError::WorkBudget { resource, .. }) if resource != "ground-structure triplet checks")
    );
    assert!(matches!(
        campaign(13, 5, 4.0, 2.0, 1e-4),
        Err(TrussError::WorkBudget {
            resource: "ground-structure triplet checks",
            ..
        })
    ));
}

#[test]
fn pre_cancelled_campaign_refuses_without_a_partial_report() {
    let gate = CancelGate::new();
    gate.request();
    let result = with_gate_cx(&gate, |cx| run_campaign(4, 3, 4.0, 2.0, 1e-4, cx));
    assert!(matches!(
        result,
        Err(TrussError::Construction(
            fs_truss::TrussConstructionError::Cancelled { .. }
        ))
    ));
}

#[test]
fn tight_tolerance_does_not_mislabel_the_iteration_cap() {
    let report = campaign(4, 3, 4.0, 2.0, f64::MIN_POSITIVE)
        .expect("bounded campaign still returns its final iterate");
    assert_eq!(report.iters, 60_000);
    assert!(!report.solver_converged);
}

#[test]
fn support_selection_is_index_based_even_below_the_old_coordinate_tolerance() {
    match campaign(2, 4, 1e-10, 1.0, 1e-4) {
        Ok(report) => {
            assert!(report.total_volume > 0.0);
            assert!(report.critical_path.len() >= 2);
        }
        Err(TrussError::NoCandidateMembers | TrussError::NoCompleteLoadPath) => {}
        Err(error) => panic!("unexpected narrow-grid refusal: {error}"),
    }
}

#[test]
fn path_analysis_excludes_disconnected_heavy_components_and_checks_endpoints() {
    let nodes = [[3.0, 0.0], [2.0, 0.0], [0.0, 0.0], [2.0, 2.0], [1.0, 2.0]];
    let members = [(0, 1), (1, 2), (3, 4)];
    let path = analyze_load_path(&nodes, &members, &[0, 1, 2], &[1.0, 2.0, 100.0], 0, &[2])
        .expect("the connected load-support chain survives filtering");
    assert_eq!(path.members, vec![0, 1]);
    assert_eq!(path.weight.to_bits(), 3.0_f64.to_bits());
    assert!(!path.members.contains(&2));

    assert!(matches!(
        analyze_load_path(&nodes, &members, &[0], &[1.0, 2.0, 100.0], 0, &[1]),
        Err(TrussError::NoCompleteLoadPath)
    ));
    assert!(matches!(
        analyze_load_path(
            &nodes,
            &members,
            &[0, 1],
            &[1.0, 2.0, 100.0],
            0,
            &[1, 2, 3, 4, 4, 4]
        ),
        Err(TrussError::InvalidLoadPath {
            reason: "support count must be within 1..=node count"
        })
    ));
}
