//! fs-truss conformance battery (bead 7tv.13).
//!
//! - truss-001: ground-structure rules hold member-by-member;
//!   generation is bitwise-reproducible; stats ledgered.
//! - truss-002: PDHG vs PROVABLE-BY-STATICS oracles (aligned tie,
//!   symmetric two-bar) with objective-separation and KKT diagnostics —
//!   independent truth on instances where the optimum is hand-computable.
//! - truss-003: ground-structure refinement — volume non-increasing
//!   under densification within declared numerical tolerances (the
//!   Michell-catalogue comparison row is ledgered as pending its
//!   vetted constants — stated, never skipped silently).
//! - truss-004: scale trend — PDHG cost per iteration tracks nnz;
//!   warm starts cut iterations on a perturbed load case.
//! - truss-005: sizing + catalog snapping — mandatory post-prune
//!   equilibrium re-verification, Euler floors on compression
//!   members, UP-snapping preserves feasibility, member-by-member
//!   code audit all-pass.
//! - truss-006: the fs-solid rod spot check — the critical compression
//!   member is stable at 1.3× design with catalog area, and the same
//!   member at a fraction of the area FAILS (the check has teeth).

use fs_truss::{
    ESTIMATED_GRAPH_BYTES_PER_MEMBER, ESTIMATED_GRAPH_BYTES_PER_NODE, GroundLimits, GroundRules,
    GroundStructure, LayoutCase, LayoutLimits, LayoutLp, MAX_PDHG_ITERS, PdhgError, PdhgSettings,
    TrussConstructionError, rod_buckling_check, size_and_snap,
};
use std::{fmt::Write as _, mem::size_of};

use fs_exec::{Budget, CancelGate, Cx, ExecMode, StreamKey};

fn verdict(name: &str, pass: bool, details: &str) {
    println!(
        "{{\"test\":\"{name}\",\"verdict\":\"{}\",{details}}}",
        if pass { "pass" } else { "fail" }
    );
    assert!(pass, "{name} failed: {details}");
}

fn with_cx<R>(gate: &CancelGate, f: impl FnOnce(&Cx<'_>) -> R) -> R {
    let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
    pool.scope(|arena| {
        let cx = Cx::new(
            gate,
            arena,
            StreamKey {
                seed: 0x7472_7573_7300_0001,
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

fn with_active_cx<R>(f: impl FnOnce(&Cx<'_>) -> R) -> R {
    with_cx(&CancelGate::new(), f)
}

/// Hand-built admitted ground structure.
fn hand_structure(cx: &Cx<'_>, nodes: &[[f64; 2]], members: &[(usize, usize)]) -> GroundStructure {
    let lengths = members
        .iter()
        .map(|&(a, b)| (nodes[b][0] - nodes[a][0]).hypot(nodes[b][1] - nodes[a][1]))
        .collect::<Vec<_>>();
    GroundStructure::try_from_parts(nodes, members, &lengths, GroundLimits::default(), cx)
        .expect("valid hand-built ground structure")
}

fn layout_lp(
    gs: &GroundStructure,
    cx: &Cx<'_>,
    supported: impl Fn(usize, usize) -> bool,
    load: impl Fn(usize) -> [f64; 2],
) -> LayoutLp {
    let case = LayoutCase::try_new(
        (0..gs.nodes().len())
            .map(|node| [supported(node, 0), supported(node, 1)])
            .collect(),
        (0..gs.nodes().len()).map(load).collect(),
        gs.nodes().len(),
    )
    .expect("valid layout case");
    LayoutLp::try_assemble(gs, &case, 1.0, LayoutLimits::default(), cx)
        .expect("valid layout construction")
}

fn construction_error<T>(result: Result<T, TrussConstructionError>) -> TrussConstructionError {
    result.err().expect("construction unexpectedly succeeded")
}

type MalformedParts = (&'static str, Vec<[f64; 2]>, Vec<(usize, usize)>, Vec<f64>);

// ---------------------------------------------------------------- truss-001

#[test]
fn truss_001_ground_rules_and_determinism() {
    with_active_cx(|cx| {
        let rules = GroundRules::try_new(0.2, 1.5, vec![0.0, 45.0, 90.0, 135.0], 0.5)
            .expect("valid fabrication rules");
        let a = GroundStructure::try_grid(5, 3, 2.0, 1.0, &rules, GroundLimits::default(), cx)
            .expect("valid ground structure");
        let b = GroundStructure::try_grid(5, 3, 2.0, 1.0, &rules, GroundLimits::default(), cx)
            .expect("valid replayed ground structure");
        let deterministic = a.stats() == b.stats();
        // Rule audit, member by member.
        let mut violations = 0usize;
        for (k, &(i, j)) in a.members().iter().enumerate() {
            let l = a.lengths()[k];
            if !(rules.min_len()..=rules.max_len()).contains(&l) {
                violations += 1;
            }
            let dx = a.nodes()[j][0] - a.nodes()[i][0];
            let dy = a.nodes()[j][1] - a.nodes()[i][1];
            let ang = dy.atan2(dx).to_degrees().rem_euclid(180.0);
            let ok = rules
                .angles()
                .iter()
                .any(|&w| ((ang - w).abs()).min(180.0 - (ang - w).abs()) <= rules.angle_tol());
            if !ok {
                violations += 1;
            }
        }
        let graph_is_replayable =
            a.graph().evidence_ledger().is_empty() && b.graph().evidence_ledger().is_empty();
        let pass =
            deterministic && graph_is_replayable && violations == 0 && !a.members().is_empty();
        verdict(
            "truss-001",
            pass,
            &format!(
                "\"detail\":\"fabrication rules member-by-member; bitwise reproducible\",\
                 \"stats\":{},\"violations\":{violations},\"deterministic\":{deterministic},\
                 \"graph_clock_state_cleared\":{graph_is_replayable}",
                a.stats()
            ),
        );
    });
}

#[test]
#[allow(clippy::too_many_lines)]
fn truss_001b_ground_construction_boundary_is_bounded_and_replayable() {
    with_active_cx(|cx| {
        for (min_len, max_len, angles, angle_tol) in [
            (-0.0, 1.0, vec![], 0.0),
            (-1.0, 1.0, vec![], 0.0),
            (f64::NAN, 1.0, vec![], 0.0),
            (1.0, f64::INFINITY, vec![], 0.0),
            (1.0, 2.0, vec![f64::NAN], 0.0),
            (1.0, 2.0, vec![-0.0], 0.0),
            (1.0, 2.0, vec![45.0, 45.0], 0.0),
            (1.0, 2.0, vec![], f64::INFINITY),
        ] {
            let run = || {
                construction_error(GroundRules::try_new(
                    min_len,
                    max_len,
                    angles.clone(),
                    angle_tol,
                ))
            };
            let first = run();
            assert!(matches!(first, TrussConstructionError::InvalidInput { .. }));
            assert_eq!(run(), first);
        }
        for (w, h) in [
            (-0.0, 1.0),
            (1.0, -0.0),
            (f64::INFINITY, 1.0),
            (1.0, f64::NAN),
            (f64::MAX, 1.0),
        ] {
            let run = || {
                construction_error(GroundStructure::try_grid(
                    2,
                    2,
                    w,
                    h,
                    &GroundRules::default(),
                    GroundLimits::default(),
                    cx,
                ))
            };
            let first = run();
            assert!(matches!(first, TrussConstructionError::InvalidInput { .. }));
            assert_eq!(run(), first);
        }
        let overflow = construction_error(GroundStructure::try_grid(
            usize::MAX,
            2,
            1.0,
            1.0,
            &GroundRules::default(),
            GroundLimits::default(),
            cx,
        ));
        assert!(matches!(
            overflow,
            TrussConstructionError::WorkBudget {
                resource: "nodes",
                observed: usize::MAX,
                ..
            }
        ));

        // A 2x2 grid has exactly four nodes, six candidate/member pairs,
        // twelve conservative through-node checks, and the following exact
        // conservative retained vector/graph footprint on the target ABI.
        let retained_bytes = 4 * (size_of::<[f64; 2]>() + ESTIMATED_GRAPH_BYTES_PER_NODE)
            + 6 * (size_of::<(usize, usize)>()
                + size_of::<f64>()
                + ESTIMATED_GRAPH_BYTES_PER_MEMBER);
        let exact = GroundLimits::try_new(4, 6, 12, 6, retained_bytes).expect("exact ground caps");
        let admitted =
            GroundStructure::try_grid(2, 2, 1.0, 1.0, &GroundRules::default(), exact, cx)
                .expect("work exactly at every cap is admitted");
        assert_eq!(admitted.nodes().len(), 4);
        assert_eq!(admitted.members().len(), 6);

        for (limits, expected) in [
            (
                GroundLimits::try_new(3, 6, 12, 6, retained_bytes).expect("node limit"),
                TrussConstructionError::WorkBudget {
                    resource: "nodes",
                    limit: 3,
                    observed: 4,
                },
            ),
            (
                GroundLimits::try_new(4, 5, 12, 6, retained_bytes).expect("pair limit"),
                TrussConstructionError::WorkBudget {
                    resource: "candidate_pairs",
                    limit: 5,
                    observed: 6,
                },
            ),
            (
                GroundLimits::try_new(4, 6, 11, 6, retained_bytes).expect("triplet limit"),
                TrussConstructionError::WorkBudget {
                    resource: "through_node_checks",
                    limit: 11,
                    observed: 12,
                },
            ),
            (
                GroundLimits::try_new(4, 6, 12, 5, retained_bytes).expect("member limit"),
                TrussConstructionError::WorkBudget {
                    resource: "members",
                    limit: 5,
                    observed: 6,
                },
            ),
            (
                GroundLimits::try_new(4, 6, 12, 6, retained_bytes - 1).expect("byte limit"),
                TrussConstructionError::WorkBudget {
                    resource: "retained_bytes",
                    limit: retained_bytes - 1,
                    observed: retained_bytes,
                },
            ),
        ] {
            let run = || {
                construction_error(GroundStructure::try_grid(
                    2,
                    2,
                    1.0,
                    1.0,
                    &GroundRules::default(),
                    limits,
                    cx,
                ))
            };
            assert_eq!(run(), expected);
            assert_eq!(run(), expected, "refusal must replay exactly");
        }
    });
}

#[test]
fn truss_001c_parts_admission_rejects_every_malformed_identity() {
    with_active_cx(|cx| {
        let limits = GroundLimits::default();
        let nodes = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];

        let cases: Vec<MalformedParts> = vec![
            ("length mismatch", nodes.to_vec(), vec![(0, 1)], vec![]),
            (
                "out-of-range endpoint",
                nodes.to_vec(),
                vec![(0, 3)],
                vec![3.0],
            ),
            ("self edge", nodes.to_vec(), vec![(1, 1)], vec![1.0]),
            ("reversed endpoint", nodes.to_vec(), vec![(1, 0)], vec![1.0]),
            (
                "duplicate member",
                nodes.to_vec(),
                vec![(0, 1), (0, 1)],
                vec![1.0, 1.0],
            ),
            (
                "noncanonical order",
                nodes.to_vec(),
                vec![(1, 2), (0, 1)],
                vec![1.0, 1.0],
            ),
            (
                "nonfinite coordinate",
                vec![[0.0, 0.0], [f64::NAN, 0.0]],
                vec![(0, 1)],
                vec![1.0],
            ),
            (
                "nonfinite length",
                nodes.to_vec(),
                vec![(0, 1)],
                vec![f64::INFINITY],
            ),
            ("zero length", nodes.to_vec(), vec![(0, 1)], vec![0.0]),
            ("negative length", nodes.to_vec(), vec![(0, 1)], vec![-1.0]),
            ("incorrect length", nodes.to_vec(), vec![(0, 1)], vec![1.5]),
        ];

        for (name, bad_nodes, bad_members, bad_lengths) in cases {
            let run = || {
                construction_error(GroundStructure::try_from_parts(
                    &bad_nodes,
                    &bad_members,
                    &bad_lengths,
                    limits,
                    cx,
                ))
            };
            let first = run();
            assert!(
                matches!(
                    first,
                    TrussConstructionError::InvalidInput { .. }
                        | TrussConstructionError::VectorLength { .. }
                ),
                "{name}: {first}"
            );
            assert_eq!(run(), first, "{name} refusal must replay exactly");
        }

        let near_length = 1.0 + 5.0e-13;
        let canonical =
            GroundStructure::try_from_parts(&nodes[..2], &[(0, 1)], &[near_length], limits, cx)
                .expect("a within-tolerance supplied length is canonicalized");
        assert_eq!(canonical.lengths()[0].to_bits(), 1.0f64.to_bits());
    });
}

#[test]
fn truss_001d_pre_cancelled_ground_construction_publishes_nothing() {
    let gate = CancelGate::new();
    gate.request();
    with_cx(&gate, |cx| {
        let run = || {
            construction_error(GroundStructure::try_grid(
                2,
                2,
                1.0,
                1.0,
                &GroundRules::default(),
                GroundLimits::default(),
                cx,
            ))
        };
        let first = run();
        assert!(matches!(first, TrussConstructionError::Cancelled { .. }));
        assert_eq!(run(), first);
    });
}

// ---------------------------------------------------------------- truss-002

#[test]
fn truss_002_provable_oracles_with_diagnostics() {
    with_active_cx(|cx| {
        // (a) Aligned tie: one member along the load; V* = P·L/σ.
        let tie = hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0]], &[(0, 1)]);
        let lp_tie = layout_lp(
            &tie,
            cx,
            |node, _| node == 0,
            |node| {
                if node == 1 { [1.0, 0.0] } else { [0.0, 0.0] }
            },
        );
        let (x_tie, y_tie, rep_tie) = lp_tie
            .solve(None, None, PdhgSettings::default())
            .expect("valid cold-start fixture");
        let tie_dev = (rep_tie.volume - 1.0).abs();
        // (b) Symmetric two-bar: supports (0,0), (2,0); load (1,1)
        // downward P=1; V* = 2·P·L·cos⁻…= 2P/σ (per-bar |q| = P/√2·√2…).
        let two = hand_structure(cx, &[[0.0, 0.0], [2.0, 0.0], [1.0, 1.0]], &[(0, 2), (1, 2)]);
        let lp_two = layout_lp(
            &two,
            cx,
            |node, _| node <= 1,
            |node| {
                if node == 2 { [0.0, -1.0] } else { [0.0, 0.0] }
            },
        );
        let (_, _, rep_two) = lp_two
            .solve(None, None, PdhgSettings::default())
            .expect("valid cold-start fixture");
        let two_dev = (rep_two.volume - 2.0).abs();
        // KKT on the tie: complementary slackness + dual feasibility.
        let mut aty = vec![0.0f64; lp_tie.c().len()];
        lp_tie.at().spmv(&y_tie, &mut aty);
        let mut comp_slack = 0.0f64;
        let mut dual_viol = 0.0f64;
        for i in 0..lp_tie.c().len() {
            comp_slack = comp_slack.max((x_tie[i] * (lp_tie.c()[i] + aty[i])).abs());
            dual_viol = dual_viol.max((-(lp_tie.c()[i] + aty[i])).max(0.0));
        }
        let pass = tie_dev < 1e-4
            && two_dev < 1e-4
            && rep_tie.gap < 1e-5
            && rep_two.gap < 1e-5
            && rep_tie.eq_residual < 1e-5
            && comp_slack < 1e-4
            && dual_viol < 1e-4;
        verdict(
            "truss-002",
            pass,
            &format!(
                "\"detail\":\"hand-provable optima with objective-separation + KKT diagnostics\",\
                 \"tie\":{},\"two_bar\":{},\"tie_dev\":{tie_dev:.2e},\"two_dev\":{two_dev:.2e},\
                 \"comp_slack\":{comp_slack:.2e},\"dual_viol\":{dual_viol:.2e}",
                rep_tie.to_json(),
                rep_two.to_json()
            ),
        );
    });
}

#[test]
fn truss_002b_solver_admission_refuses_malformed_controls_and_warm_starts() {
    with_active_cx(|cx| {
        let tie = hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0]], &[(0, 1)]);
        let lp = layout_lp(
            &tie,
            cx,
            |node, _| node == 0,
            |node| {
                if node == 1 { [1.0, 0.0] } else { [0.0, 0.0] }
            },
        );
        for settings in [
            PdhgSettings {
                max_iters: 0,
                ..PdhgSettings::default()
            },
            PdhgSettings {
                max_iters: MAX_PDHG_ITERS + 1,
                ..PdhgSettings::default()
            },
            PdhgSettings {
                check_every: 0,
                ..PdhgSettings::default()
            },
            PdhgSettings {
                gap_tol: f64::NAN,
                ..PdhgSettings::default()
            },
        ] {
            assert!(matches!(
                lp.solve(None, None, settings),
                Err(PdhgError::InvalidSetting { .. })
            ));
        }
        assert!(matches!(
            lp.solve(
                Some(vec![0.0; lp.c().len() - 1]),
                None,
                PdhgSettings::default()
            ),
            Err(PdhgError::VectorLength { vector: "x", .. })
        ));
        let mut negative_x = vec![0.0; lp.c().len()];
        negative_x[0] = -1.0;
        assert!(matches!(
            lp.solve(Some(negative_x), None, PdhgSettings::default()),
            Err(PdhgError::InvalidVector { vector: "x", .. })
        ));
        assert!(matches!(
            lp.solve(
                None,
                Some(vec![0.0; lp.b().len() - 1]),
                PdhgSettings::default()
            ),
            Err(PdhgError::VectorLength { vector: "y", .. })
        ));
        let mut invalid_y = vec![0.0; lp.b().len()];
        invalid_y[0] = f64::INFINITY;
        assert!(matches!(
            lp.solve(None, Some(invalid_y), PdhgSettings::default()),
            Err(PdhgError::InvalidVector { vector: "y", .. })
        ));
        assert!(matches!(
            lp.diagnostics(&[], &[], 1.0),
            Err(PdhgError::VectorLength { vector: "x", .. })
        ));
        assert!(matches!(
            lp.diagnostics(&vec![0.0; lp.c().len()], &vec![0.0; lp.b().len()], 0.0),
            Err(PdhgError::InvalidSetting { field: "bnorm", .. })
        ));
    });
}

#[test]
fn truss_002c_layout_construction_boundary_is_bounded_and_replayable() {
    with_active_cx(|cx| {
        let tie = hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0]], &[(0, 1)]);
        let case = LayoutCase::try_new(
            vec![[true, true], [false, false]],
            vec![[0.0, 0.0], [1.0, 0.0]],
            2,
        )
        .expect("valid tie case");

        // This fixture has exactly two free DOFs, two split variables, four
        // staged triplets, and this exact conservative footprint on the ABI.
        let retained_bytes = 4 * size_of::<Option<usize>>()
            + 2 * size_of::<f64>()
            + 2 * size_of::<f64>()
            + 2 * 4 * (size_of::<usize>() + size_of::<f64>())
            + 3 * size_of::<usize>()
            + 3 * size_of::<usize>()
            + size_of::<f64>();
        let exact = LayoutLimits::try_new(2, 2, 4, retained_bytes).expect("exact layout caps");
        let admitted = LayoutLp::try_assemble(&tie, &case, 1.0, exact, cx)
            .expect("work exactly at every layout cap is admitted");
        assert_eq!(admitted.a().nrows(), 2);
        assert_eq!(admitted.a().ncols(), 2);
        assert_eq!(admitted.a().nnz(), 4);
        assert_eq!(admitted.at().nrows(), 2);
        assert_eq!(admitted.at().ncols(), 2);
        assert_eq!(admitted.at().nnz(), 4);

        for (limits, expected) in [
            (
                LayoutLimits::try_new(1, 2, 4, retained_bytes).expect("DOF limit"),
                TrussConstructionError::WorkBudget {
                    resource: "layout free degrees of freedom",
                    limit: 1,
                    observed: 2,
                },
            ),
            (
                LayoutLimits::try_new(2, 1, 4, retained_bytes).expect("variable limit"),
                TrussConstructionError::WorkBudget {
                    resource: "layout split variables",
                    limit: 1,
                    observed: 2,
                },
            ),
            (
                LayoutLimits::try_new(2, 2, 3, retained_bytes).expect("triplet limit"),
                TrussConstructionError::WorkBudget {
                    resource: "layout staged triplets",
                    limit: 3,
                    observed: 4,
                },
            ),
            (
                LayoutLimits::try_new(2, 2, 4, retained_bytes - 1).expect("retained-byte limit"),
                TrussConstructionError::WorkBudget {
                    resource: "layout retained bytes",
                    limit: retained_bytes - 1,
                    observed: retained_bytes,
                },
            ),
        ] {
            let run = || construction_error(LayoutLp::try_assemble(&tie, &case, 1.0, limits, cx));
            assert_eq!(run(), expected);
            assert_eq!(run(), expected, "layout refusal must replay exactly");
        }
    });
}

#[test]
#[allow(clippy::too_many_lines)]
fn truss_002d_layout_case_and_scalar_admission_refuse_hostile_inputs() {
    assert!(matches!(
        LayoutCase::try_new(vec![[false, false]], vec![[0.0, 0.0]; 2], 2),
        Err(TrussConstructionError::VectorLength {
            field: "supported",
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        LayoutCase::try_new(vec![[false, false]; 2], vec![[0.0, 0.0]], 2),
        Err(TrussConstructionError::VectorLength {
            field: "loads",
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        LayoutCase::try_new(
            vec![[false, false]; 2],
            vec![[0.0, 0.0], [f64::NAN, 0.0]],
            2
        ),
        Err(TrussConstructionError::InvalidInput { field: "loads", .. })
    ));

    with_active_cx(|cx| {
        let tie = hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0]], &[(0, 1)]);
        let valid_case = LayoutCase::try_new(
            vec![[true, true], [false, false]],
            vec![[0.0, 0.0], [1.0, 0.0]],
            2,
        )
        .expect("valid tie case");
        for sigma_y in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY] {
            let first = construction_error(LayoutLp::try_assemble(
                &tie,
                &valid_case,
                sigma_y,
                LayoutLimits::default(),
                cx,
            ));
            assert!(matches!(
                first,
                TrussConstructionError::InvalidInput {
                    field: "sigma_y",
                    ..
                }
            ));
            assert_eq!(
                construction_error(LayoutLp::try_assemble(
                    &tie,
                    &valid_case,
                    sigma_y,
                    LayoutLimits::default(),
                    cx,
                )),
                first
            );
        }

        for extreme_load in [f64::MIN_POSITIVE, 1e-100, f64::MAX] {
            let case = LayoutCase::try_new(
                vec![[true, true], [false, false]],
                vec![[0.0, 0.0], [extreme_load, 0.0]],
                2,
            )
            .expect("finite load shape is admitted before LP-scale validation");
            let first = construction_error(LayoutLp::try_assemble(
                &tie,
                &case,
                1.0,
                LayoutLimits::default(),
                cx,
            ));
            assert!(matches!(
                first,
                TrussConstructionError::InvalidInput {
                    field: "free load vector",
                    ..
                }
            ));
            assert_eq!(
                construction_error(LayoutLp::try_assemble(
                    &tie,
                    &case,
                    1.0,
                    LayoutLimits::default(),
                    cx,
                )),
                first
            );
        }

        let eliminated_case = LayoutCase::try_new(
            vec![[true, true], [true, false]],
            vec![[0.0, 0.0], [1.0, 0.0]],
            2,
        )
        .expect("finite but eliminated load case");
        let run = || {
            construction_error(LayoutLp::try_assemble(
                &tie,
                &eliminated_case,
                1.0,
                LayoutLimits::default(),
                cx,
            ))
        };
        let first = run();
        assert!(matches!(
            first,
            TrussConstructionError::InvalidInput { field: "loads", .. }
        ));
        assert_eq!(run(), first);

        let all_supported =
            LayoutCase::try_new(vec![[true, true]; 2], vec![[0.0, 0.0], [1.0, 0.0]], 2)
                .expect("finite all-supported load case");
        assert!(matches!(
            LayoutLp::try_assemble(&tie, &all_supported, 1.0, LayoutLimits::default(), cx,),
            Err(TrussConstructionError::InvalidInput {
                field: "supports",
                ..
            })
        ));
    });
}

#[test]
fn truss_002e_sparse_admission_rejects_degenerate_and_disconnected_load_rows() {
    with_active_cx(|cx| {
        let tie = hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0]], &[(0, 1)]);
        let degenerate_case = LayoutCase::try_new(
            vec![[true, true], [true, false]],
            vec![[0.0, 0.0], [0.0, -1.0]],
            2,
        )
        .expect("finite perpendicular load case");
        let run_degenerate = || {
            construction_error(LayoutLp::try_assemble(
                &tie,
                &degenerate_case,
                1.0,
                LayoutLimits::default(),
                cx,
            ))
        };
        let degenerate = run_degenerate();
        assert!(matches!(
            degenerate,
            TrussConstructionError::InvalidInput {
                field: "equilibrium matrix",
                ..
            }
        ));
        assert_eq!(run_degenerate(), degenerate);

        let disconnected = hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], &[(0, 1)]);
        let disconnected_case = LayoutCase::try_new(
            vec![[true, true], [false, false], [false, true]],
            vec![[0.0, 0.0], [0.0, 0.0], [1.0, 0.0]],
            3,
        )
        .expect("finite disconnected load case");
        let run_disconnected = || {
            construction_error(LayoutLp::try_assemble(
                &disconnected,
                &disconnected_case,
                1.0,
                LayoutLimits::default(),
                cx,
            ))
        };
        let disconnected_error = run_disconnected();
        assert!(matches!(
            disconnected_error,
            TrussConstructionError::InvalidInput {
                field: "free load vector",
                ..
            }
        ));
        assert_eq!(run_disconnected(), disconnected_error);
    });
}

#[test]
fn truss_002f_pre_cancelled_layout_construction_publishes_nothing() {
    let tie = with_active_cx(|cx| hand_structure(cx, &[[0.0, 0.0], [1.0, 0.0]], &[(0, 1)]));
    let case = LayoutCase::try_new(
        vec![[true, true], [false, false]],
        vec![[0.0, 0.0], [1.0, 0.0]],
        2,
    )
    .expect("valid tie case");
    let gate = CancelGate::new();
    gate.request();
    with_cx(&gate, |cx| {
        let run = || {
            construction_error(LayoutLp::try_assemble(
                &tie,
                &case,
                1.0,
                LayoutLimits::default(),
                cx,
            ))
        };
        let first = run();
        assert!(matches!(first, TrussConstructionError::Cancelled { .. }));
        assert_eq!(run(), first);
    });
}

#[test]
fn truss_002g_norm_seed_falls_back_outside_the_split_nullspace() {
    with_active_cx(|cx| {
        let nodes: Vec<[f64; 2]> = (0_u32..8).map(|index| [f64::from(index), 0.0]).collect();
        let members: Vec<(usize, usize)> = (0..7).map(|index| (index, index + 1)).collect();
        let ground = hand_structure(cx, &nodes, &members);
        let case = LayoutCase::try_new(
            (0..8).map(|node| [node == 0; 2]).collect(),
            (0..8)
                .map(|node| if node == 7 { [1.0, 0.0] } else { [0.0, 0.0] })
                .collect(),
            8,
        )
        .expect("valid seven-member chain case");
        let lp = LayoutLp::try_assemble(&ground, &case, 1.0, LayoutLimits::default(), cx)
            .expect("period-seven split seed uses a deterministic nonzero-column fallback");
        assert!(lp.norm_est().is_finite() && lp.norm_est() > 0.0);
    });
}

// ---------------------------------------------------------------- truss-003

fn cantilever_volume(nx: usize, ny: usize, settings: PdhgSettings) -> (f64, f64, usize) {
    with_active_cx(|cx| {
        let rules = GroundRules::default();
        let gs = GroundStructure::try_grid(nx, ny, 2.0, 1.0, &rules, GroundLimits::default(), cx)
            .expect("valid cantilever ground structure");
        let tip = gs
            .nodes()
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (a[0] - 2.0).abs() + (a[1] - 0.5).abs();
                let db = (b[0] - 2.0).abs() + (b[1] - 0.5).abs();
                da.partial_cmp(&db).expect("finite")
            })
            .expect("nodes")
            .0;
        let lp = layout_lp(
            &gs,
            cx,
            |node, _| gs.nodes()[node][0] < 1e-9,
            move |node| {
                if node == tip { [0.0, -1.0] } else { [0.0, 0.0] }
            },
        );
        let (_, _, rep) = lp
            .solve(None, None, settings)
            .expect("valid cantilever settings");
        (rep.volume, rep.gap, gs.members().len())
    })
}

#[test]
fn truss_003_refinement_monotonicity() {
    let settings = PdhgSettings {
        max_iters: 60_000,
        gap_tol: 5e-4,
        check_every: 500,
    };
    let mut rows = String::new();
    let mut vols = Vec::new();
    for (nx, ny) in [(3usize, 2usize), (5, 3), (7, 4)] {
        let (v, gap, members) = cantilever_volume(nx, ny, settings);
        let _ = write!(
            rows,
            "{{\"grid\":\"{nx}x{ny}\",\"members\":{members},\"volume\":{v:.5},\"gap\":{gap:.2e}}},"
        );
        vols.push((v, gap));
    }
    // Non-increasing within the measured diagnostic tolerance.
    let mono = vols
        .windows(2)
        .all(|w| w[1].0 <= w[0].0 * (1.0 + w[0].1 + w[1].1 + 5e-4));
    let diagnostics_small = vols.iter().all(|&(_, g)| g < 5e-3);
    verdict(
        "truss-003",
        mono && diagnostics_small,
        &format!(
            "\"detail\":\"denser ground structures do not worsen returned-iterate volume within declared diagnostics; \
             michell_catalogue_row: pending vetted constants (fs-fab oracle spec), \
             stated not skipped\",\"rows\":[{}]",
            rows.trim_end_matches(',')
        ),
    );
}

// ---------------------------------------------------------------- truss-004

#[test]
#[allow(clippy::too_many_lines)]
fn truss_004_scale_trend_and_warm_start() {
    with_active_cx(|cx| {
        use std::time::Instant;
        let mut rows = String::new();
        let mut per_nnz = Vec::new();
        for (nx, ny) in [(7usize, 4usize), (9, 5), (11, 6)] {
            let gs = GroundStructure::try_grid(
                nx,
                ny,
                2.0,
                1.0,
                &GroundRules::default(),
                GroundLimits::default(),
                cx,
            )
            .expect("valid scale ground structure");
            let lp = layout_lp(
                &gs,
                cx,
                |node, _| gs.nodes()[node][0] < 1e-9,
                |node| {
                    if node % 7 == 3 {
                        [0.0, -0.2]
                    } else {
                        [0.0, 0.0]
                    }
                },
            );
            let settings = PdhgSettings {
                max_iters: 3000,
                gap_tol: 0.0, // run exactly max_iters for the timing row
                check_every: 3000,
            };
            let t0 = Instant::now();
            let (_, _, rep) = lp
                .solve(None, None, settings)
                .expect("valid fixed-iteration settings");
            let dt = t0.elapsed().as_secs_f64();
            let nnz = lp.a().nnz();
            #[allow(clippy::cast_precision_loss)]
            per_nnz.push(dt / (nnz as f64 * rep.iters as f64));
            let _ = write!(
                rows,
                "{{\"members\":{},\"nnz\":{nnz},\"iters\":{},\"seconds\":{dt:.3}}},",
                gs.members().len(),
                rep.iters
            );
        }
        let spread = per_nnz.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            / per_nnz.iter().copied().fold(f64::INFINITY, f64::min);
        // Warm start: perturbed load converges in fewer iterations.
        let gs = GroundStructure::try_grid(
            7,
            4,
            2.0,
            1.0,
            &GroundRules::default(),
            GroundLimits::default(),
            cx,
        )
        .expect("valid warm-start ground structure");
        let tip = gs.nodes().len() - 1;
        let mk = |scale: f64| {
            layout_lp(
                &gs,
                cx,
                |node, _| gs.nodes()[node][0] < 1e-9,
                move |node| {
                    if node == tip {
                        [0.0, -scale]
                    } else {
                        [0.0, 0.0]
                    }
                },
            )
        };
        let settings = PdhgSettings {
            max_iters: 60_000,
            gap_tol: 1e-4,
            check_every: 200,
        };
        let (x0, y0, rep_cold) = mk(1.0)
            .solve(None, None, settings)
            .expect("valid cold start");
        let (_, _, rep_warm) = mk(1.05)
            .solve(
                Some(x0.iter().map(|v| v * 1.05).collect()),
                Some(y0),
                settings,
            )
            .expect("shape-compatible warm start");
        let warm_wins = rep_warm.iters < rep_cold.iters;
        let pass = spread < 3.0 && warm_wins;
        verdict(
            "truss-004",
            pass,
            &format!(
                "\"detail\":\"cost per (iteration x nnz) flat across sizes; warm start wins \
                 (1e6-member wall-clock = perf-lane scope)\",\"rows\":[{}],\
                 \"per_nnz_spread\":{spread:.2},\"cold_iters\":{},\"warm_iters\":{}",
                rows.trim_end_matches(','),
                rep_cold.iters,
                rep_warm.iters
            ),
        );
    });
}

// ---------------------------------------------------------------- truss-005

#[test]
fn truss_005_sizing_and_catalog_audit() {
    with_active_cx(|cx| {
        let gs = GroundStructure::try_grid(
            5,
            3,
            2.0,
            1.0,
            &GroundRules::default(),
            GroundLimits::default(),
            cx,
        )
        .expect("valid sizing ground structure");
        let tip = gs
            .nodes()
            .iter()
            .position(|p| (p[0] - 2.0).abs() < 1e-9 && (p[1] - 0.5).abs() < 1e-9)
            .expect("tip node");
        let lp = layout_lp(
            &gs,
            cx,
            |node, _| gs.nodes()[node][0] < 1e-9,
            move |node| {
                if node == tip { [0.0, -1.0] } else { [0.0, 0.0] }
            },
        );
        let (x, _, rep) = lp
            .solve(
                None,
                None,
                PdhgSettings {
                    max_iters: 120_000,
                    gap_tol: 1e-5,
                    check_every: 500,
                },
            )
            .expect("valid sizing solve");
        let catalog = [0.05, 0.1, 0.2, 0.35, 0.5, 0.75, 1.0, 1.5, 2.5, 5.0];
        let audit = size_and_snap(&gs, &lp, &x, 1.0, 1000.0, &catalog, 1e-3);
        let compression = audit.members.iter().filter(|m| m.force < 0.0).count();
        let euler_active = audit
            .members
            .iter()
            .filter(|m| m.area_buckling > m.area_yield)
            .count();
        let pass = audit.all_pass
            && audit.eq_residual < 1e-6
            && audit.pruned > 0
            && !audit.members.is_empty()
            && compression > 0;
        let mut rows = String::new();
        for r in audit.rows.iter().take(6) {
            let _ = write!(rows, "{r},");
        }
        verdict(
            "truss-005",
            pass,
            &format!(
                "\"detail\":\"prune -> reverify -> Euler floors -> up-snap -> code audit\",\
                 \"survivors\":{},\"pruned\":{},\"eq_residual\":{:.2e},\"compression\":{compression},\
                 \"euler_governed\":{euler_active},\"lp\":{},\"sample_rows\":[{}]",
                audit.members.len(),
                audit.pruned,
                audit.eq_residual,
                rep.to_json(),
                rows.trim_end_matches(',')
            ),
        );
    });
}

// ---------------------------------------------------------------- truss-006

#[test]
fn truss_006_rod_spot_check() {
    // A slender compression member: catalog-sized area must survive
    // 1.3× design; a fraction of that area must buckle.
    let (length, youngs, design) = (1.0f64, 1000.0f64, 0.5f64);
    // Catalog area from the sizing rule.
    let a_euler =
        (12.0 * design * length * length / (std::f64::consts::PI.powi(2) * youngs)).sqrt();
    let a_catalog = a_euler * 1.4; // next size up
    let (stable, bow) = rod_buckling_check(length, a_catalog, youngs, design, 1.3, 0.002);
    let (unstable, bow_thin) =
        rod_buckling_check(length, 0.4 * a_euler, youngs, design, 1.3, 0.002);
    let pass = stable && bow < 0.05 && (!unstable || bow_thin > 5.0 * bow);
    verdict(
        "truss-006",
        pass,
        &format!(
            "\"detail\":\"critical member: catalog area stable at 1.3x design; \
             under-sized area fails (the check has teeth)\",\
             \"a_catalog\":{a_catalog:.4e},\"bow_over_l\":{bow:.4},\
             \"undersized_stable\":{unstable},\"undersized_bow\":{bow_thin:.3}"
        ),
    );
}
