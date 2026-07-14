//! Certified geometric-moments battery (bead rjnd, part 1).
//!
//! - gm-001 G0: box moments contain the closed forms with bounded widths.
//! - gm-002 G0/G2: sphere moments and COM enclosures contain the analytic
//!   values (parallel-axis closed form about the origin).
//! - gm-003 G3: translation-covariance metamorphic — the outward-rounded
//!   covariance law and a direct recomputation on a shifted chart must
//!   overlap componentwise (both enclose the same truth), and volume is
//!   translation-invariant bitwise.
//! - gm-004 G0: capability and input refusals — weaker trace claims,
//!   Estimate-class samples, malformed spacing/domains, and excessive
//!   work all refuse with the named typed error.
//! - gm-005 G4: cancellation surfaces as `QueryError::Cancelled`.
//!
//! JSON-line verdicts log every admitted/refused decision.

use asupersync::types::Budget;
use fs_evidence::NumericalCertificate;
use fs_exec::{CancelGate, Cx, ExecMode, StreamKey};
use fs_geom::fixtures::{BoxChart, SphereChart};
use fs_geom::{Aabb, Chart, ChartSample, Point3, TraceStepClaim, Vec3};
use fs_query::{GeometricMoments, MomentEnclosure, QueryError, geometric_moments};
use std::f64::consts::PI;

fn verdict(case: &str, pass: bool, detail: &str) {
    println!(
        "{{\"suite\":\"fs-query/moments\",\"case\":\"{case}\",\"verdict\":\"{}\",\
         \"detail\":\"{detail}\"}}",
        if pass { "pass" } else { "fail" }
    );
    assert!(pass, "case {case}: {detail}");
}

fn with_cx<R>(f: impl FnOnce(&Cx<'_>) -> R) -> R {
    let gate = CancelGate::new();
    let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
    pool.scope(|arena| {
        let cx = Cx::new(
            &gate,
            arena,
            StreamKey {
                seed: 0x60E5,
                kernel_id: 9,
                tile: 0,
                iteration: 0,
            },
            Budget::INFINITE,
            ExecMode::Deterministic,
        );
        f(&cx)
    })
}

fn contained(name: &str, enclosure: MomentEnclosure, truth: f64, max_width: f64) -> String {
    assert!(
        enclosure.contains(truth),
        "{name}: [{}, {}] must contain {truth}",
        enclosure.lo,
        enclosure.hi
    );
    assert!(
        enclosure.width() <= max_width,
        "{name}: width {} exceeds {max_width}",
        enclosure.width()
    );
    format!(
        "\\\"{name}\\\":{{\\\"lo\\\":{:.6e},\\\"hi\\\":{:.6e},\\\"truth\\\":{truth:.6e}}}",
        enclosure.lo, enclosure.hi
    )
}

/// The fixture chart rigidly shifted by `d` (still an exact distance).
struct Shifted<C: Chart> {
    inner: C,
    d: Vec3,
}

impl<C: Chart> Chart for Shifted<C> {
    fn eval(&self, x: Point3, cx: &Cx<'_>) -> ChartSample {
        self.inner
            .eval(x.offset(Vec3::new(-self.d.x, -self.d.y, -self.d.z)), cx)
    }

    fn support(&self) -> Aabb {
        let s = self.inner.support();
        Aabb::new(s.min.offset(self.d), s.max.offset(self.d))
    }

    fn trace_step_claim(&self) -> TraceStepClaim {
        self.inner.trace_step_claim()
    }

    fn name(&self) -> &'static str {
        "test/shifted"
    }
}

/// Claims only LipschitzImplicit: certified moments must refuse it.
struct ImplicitOnly<C: Chart>(C);

impl<C: Chart> Chart for ImplicitOnly<C> {
    fn eval(&self, x: Point3, cx: &Cx<'_>) -> ChartSample {
        self.0.eval(x, cx)
    }

    fn support(&self) -> Aabb {
        self.0.support()
    }

    fn trace_step_claim(&self) -> TraceStepClaim {
        TraceStepClaim::LipschitzImplicit
    }

    fn name(&self) -> &'static str {
        "test/implicit-only"
    }
}

/// Claims ExactDistance but serves Estimate-class evidence: each sample
/// must refuse (the claim alone cannot launder the certificate).
struct EstimatingBox(BoxChart);

impl Chart for EstimatingBox {
    fn eval(&self, x: Point3, cx: &Cx<'_>) -> ChartSample {
        let mut s = self.0.eval(x, cx);
        s.error = NumericalCertificate::estimate(s.signed_distance, s.signed_distance);
        s
    }

    fn support(&self) -> Aabb {
        self.0.support()
    }

    fn trace_step_claim(&self) -> TraceStepClaim {
        TraceStepClaim::ExactDistance
    }

    fn name(&self) -> &'static str {
        "test/estimating-box"
    }
}

fn unit_domain() -> Aabb {
    Aabb::new(Point3::new(-1.0, -1.0, -1.0), Point3::new(1.0, 1.0, 1.0))
}

fn test_box() -> BoxChart {
    BoxChart {
        aabb: Aabb::new(Point3::new(-0.25, -0.25, -0.25), Point3::new(0.5, 0.5, 0.5)),
    }
}

#[test]
fn gm_001_box_moments_contain_closed_forms() {
    let chart = test_box();
    let m = with_cx(|cx| geometric_moments(&chart, &unit_domain(), 0.02, cx)).expect("box moments");
    // w = 0.75, c = 0.125 per axis (dyadic: closed forms are exact).
    let volume = 0.75 * 0.75 * 0.75;
    let m1 = volume * 0.125;
    let m2_diag = volume * (0.125 * 0.125 + 0.75 * 0.75 / 12.0);
    let m2_cross = volume * 0.125 * 0.125;
    let mut rows = Vec::new();
    rows.push(contained("volume", m.volume, volume, 0.30));
    for (axis, label) in ["m1x", "m1y", "m1z"].into_iter().enumerate() {
        rows.push(contained(label, m.first[axis], m1, 0.20));
    }
    for (enclosure, label) in [
        (m.second.xx, "m2xx"),
        (m.second.yy, "m2yy"),
        (m.second.zz, "m2zz"),
    ] {
        rows.push(contained(label, enclosure, m2_diag, 0.20));
    }
    for (enclosure, label) in [
        (m.second.xy, "m2xy"),
        (m.second.xz, "m2xz"),
        (m.second.yz, "m2yz"),
    ] {
        rows.push(contained(label, enclosure, m2_cross, 0.20));
    }
    verdict(
        "gm-001",
        m.sure_cells > 0 && m.band_cells > 0,
        &format!(
            "box closed forms contained; sure={} band={} {}",
            m.sure_cells,
            m.band_cells,
            rows.join(",")
        ),
    );
}

#[test]
fn gm_002_sphere_moments_and_com() {
    let center = [0.25, -0.125, 0.0];
    let chart = SphereChart {
        center: Point3::new(center[0], center[1], center[2]),
        radius: 0.5,
    };
    let m =
        with_cx(|cx| geometric_moments(&chart, &unit_domain(), 0.02, cx)).expect("sphere moments");
    let volume = 4.0 / 3.0 * PI * (0.5 * 0.5 * 0.5);
    let mut rows = Vec::new();
    rows.push(contained("volume", m.volume, volume, 0.30));
    for a in 0..3 {
        rows.push(contained(
            ["m1x", "m1y", "m1z"][a],
            m.first[a],
            volume * center[a],
            0.25,
        ));
    }
    // Parallel axis about the origin: ∫x² = V(r²/5 + c_x²).
    for (a, (enclosure, label)) in [
        (m.second.xx, "m2xx"),
        (m.second.yy, "m2yy"),
        (m.second.zz, "m2zz"),
    ]
    .into_iter()
    .enumerate()
    {
        let truth = volume * (0.5 * 0.5 / 5.0 + center[a] * center[a]);
        rows.push(contained(label, enclosure, truth, 0.25));
    }
    // Central cross moments vanish for a ball: ∫xy = V·c_x·c_y.
    let cross_truth = [
        volume * center[0] * center[1],
        volume * center[0] * center[2],
        volume * center[1] * center[2],
    ];
    for (slot, (enclosure, label)) in [
        (m.second.xy, "m2xy"),
        (m.second.xz, "m2xz"),
        (m.second.yz, "m2yz"),
    ]
    .into_iter()
    .enumerate()
    {
        rows.push(contained(label, enclosure, cross_truth[slot], 0.25));
    }
    let com = m.com_enclosure().expect("volume lower bound is positive");
    for a in 0..3 {
        assert!(
            com[a].contains(center[a]),
            "com axis {a}: [{}, {}] must contain {}",
            com[a].lo,
            com[a].hi,
            center[a]
        );
        assert!(
            com[a].width() < 0.6,
            "com axis {a} width {}",
            com[a].width()
        );
    }
    verdict(
        "gm-002",
        true,
        &format!(
            "sphere moments + COM contained; sure={} band={} {}",
            m.sure_cells,
            m.band_cells,
            rows.join(",")
        ),
    );
}

#[test]
fn gm_003_translation_covariance_metamorphic() {
    let d = [0.3, -0.2, 0.1];
    let chart = test_box();
    let shifted = Shifted {
        inner: test_box(),
        d: Vec3::new(d[0], d[1], d[2]),
    };
    let (law, direct): (GeometricMoments, GeometricMoments) = with_cx(|cx| {
        let base = geometric_moments(&chart, &unit_domain(), 0.04, cx).expect("base moments");
        let direct =
            geometric_moments(&shifted, &unit_domain(), 0.04, cx).expect("shifted moments");
        (base.translated(d), direct)
    });
    // The shifted region meets different band cells, so the direct
    // volume enclosure differs; both still contain the true volume.
    let pairs = [
        ("volume", law.volume, direct.volume),
        ("m1x", law.first[0], direct.first[0]),
        ("m1y", law.first[1], direct.first[1]),
        ("m1z", law.first[2], direct.first[2]),
        ("m2xx", law.second.xx, direct.second.xx),
        ("m2yy", law.second.yy, direct.second.yy),
        ("m2zz", law.second.zz, direct.second.zz),
        ("m2xy", law.second.xy, direct.second.xy),
        ("m2xz", law.second.xz, direct.second.xz),
        ("m2yz", law.second.yz, direct.second.yz),
    ];
    for (label, a, b) in pairs {
        assert!(
            a.overlaps(&b),
            "{label}: covariance law [{}, {}] and direct [{}, {}] must overlap",
            a.lo,
            a.hi,
            b.lo,
            b.hi
        );
    }
    verdict(
        "gm-003",
        true,
        "translated law and direct recomputation overlap on every component",
    );
}

#[test]
fn gm_004_capability_and_input_refusals() {
    let box_chart = test_box();
    let implicit = ImplicitOnly(test_box());
    let estimating = EstimatingBox(test_box());
    let tiny_sphere = SphereChart {
        center: Point3::new(0.0, 0.0, 0.0),
        radius: 0.01,
    };
    // Aabb::new normalizes corners, so an inverted or non-finite domain
    // must be constructed literally to reach the refusal path.
    let inverted = Aabb {
        min: Point3::new(1.0, -1.0, -1.0),
        max: Point3::new(-1.0, 1.0, 1.0),
    };
    let non_finite = Aabb {
        min: Point3::new(f64::NAN, -1.0, -1.0),
        max: Point3::new(1.0, 1.0, 1.0),
    };
    let excludes_support = Aabb::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
    let outcomes = with_cx(|cx| {
        [
            (
                "weaker-claim-refuses",
                matches!(
                    geometric_moments(&implicit, &unit_domain(), 0.1, cx),
                    Err(QueryError::MomentsUncertifiedChart {
                        claim: TraceStepClaim::LipschitzImplicit
                    })
                ),
            ),
            (
                "estimate-sample-refuses",
                matches!(
                    geometric_moments(&estimating, &unit_domain(), 0.1, cx),
                    Err(QueryError::MomentsInvalidSample { .. })
                ),
            ),
            (
                "zero-spacing-refuses",
                matches!(
                    geometric_moments(&box_chart, &unit_domain(), 0.0, cx),
                    Err(QueryError::MomentsInvalidSpacing { .. })
                ),
            ),
            (
                "nan-spacing-refuses",
                matches!(
                    geometric_moments(&box_chart, &unit_domain(), f64::NAN, cx),
                    Err(QueryError::MomentsInvalidSpacing { .. })
                ),
            ),
            (
                "inverted-domain-refuses",
                matches!(
                    geometric_moments(&box_chart, &inverted, 0.1, cx),
                    Err(QueryError::MomentsInvalidDomain { .. })
                ),
            ),
            (
                "non-finite-domain-refuses",
                matches!(
                    geometric_moments(&box_chart, &non_finite, 0.1, cx),
                    Err(QueryError::MomentsInvalidDomain { .. })
                ),
            ),
            (
                "support-excluding-domain-refuses",
                matches!(
                    geometric_moments(&box_chart, &excludes_support, 0.1, cx),
                    Err(QueryError::MomentsInvalidDomain { .. })
                ),
            ),
            (
                "per-axis-work-refuses",
                matches!(
                    geometric_moments(&box_chart, &unit_domain(), 1e-7, cx),
                    Err(QueryError::MomentsExcessiveWork { .. })
                ),
            ),
            (
                "total-work-refuses",
                matches!(
                    geometric_moments(&box_chart, &unit_domain(), 1e-6, cx),
                    Err(QueryError::MomentsExcessiveWork { .. })
                ),
            ),
            (
                "unproven-volume-refuses-com",
                match geometric_moments(&tiny_sphere, &unit_domain(), 0.5, cx) {
                    Ok(m) => matches!(
                        m.com_enclosure(),
                        Err(QueryError::MomentsVolumeUnproven { .. })
                    ),
                    Err(_) => false,
                },
            ),
        ]
    });
    for (name, pass) in outcomes {
        verdict("gm-004", pass, name);
    }
}

#[test]
fn gm_005_cancellation_fails_closed() {
    let gate = CancelGate::new();
    gate.request();
    let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
    let refused = pool.scope(|arena| {
        let cx = Cx::new(
            &gate,
            arena,
            StreamKey {
                seed: 0x60E5,
                kernel_id: 10,
                tile: 0,
                iteration: 0,
            },
            Budget::INFINITE,
            ExecMode::Deterministic,
        );
        geometric_moments(&test_box(), &unit_domain(), 0.1, &cx)
    });
    verdict(
        "gm-005",
        matches!(refused, Err(QueryError::Cancelled)),
        "a pre-cancelled context refuses before publishing moment enclosures",
    );
}
