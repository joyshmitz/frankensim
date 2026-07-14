//! fs-rep-nurbs conformance (the wqd.5 bead). Acceptance: knot insertion
//! and degree elevation EXACT (rational equality at common parameters —
//! the definitive spline-algebra test); trimmed classification correct on
//! adversarial fixtures (tangent trims, slivers, nested loops); measured
//! closest-point brackets are cross-checked against a dense-sampling oracle;
//! partition-of-unity / endpoint / derivative-vs-dual G0 laws; the honest
//! Boolean policy refuses with the route named.

use fs_rep_nurbs::{
    BooleanOp, BooleanPolicy, Classification, KnotVector, NurbsCurve, NurbsSurface, Rat, Scalar,
    TrimLoop, TrimmedPatch, boolean, closest_point_curve, closest_point_surface,
};

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-rep-nurbs/conformance\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 11) as f64) / (1u64 << 53) as f64
}

/// A two-lane forward dual implementing the crate's own `Scalar` trait —
/// the derivative check runs through the SAME generic evaluation code.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TDual {
    re: f64,
    eps: f64,
}

impl TDual {
    fn var(v: f64) -> TDual {
        TDual { re: v, eps: 1.0 }
    }
    fn con(v: f64) -> TDual {
        TDual { re: v, eps: 0.0 }
    }
}

impl PartialOrd for TDual {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.re.partial_cmp(&other.re)
    }
}

impl core::ops::Add for TDual {
    type Output = TDual;
    fn add(self, o: TDual) -> TDual {
        TDual {
            re: self.re + o.re,
            eps: self.eps + o.eps,
        }
    }
}
impl core::ops::Sub for TDual {
    type Output = TDual;
    fn sub(self, o: TDual) -> TDual {
        TDual {
            re: self.re - o.re,
            eps: self.eps - o.eps,
        }
    }
}
impl core::ops::Mul for TDual {
    type Output = TDual;
    fn mul(self, o: TDual) -> TDual {
        TDual {
            re: self.re * o.re,
            eps: self.re * o.eps + self.eps * o.re,
        }
    }
}
impl core::ops::Div for TDual {
    type Output = TDual;
    fn div(self, o: TDual) -> TDual {
        TDual {
            re: self.re / o.re,
            eps: (self.eps * o.re - self.re * o.eps) / (o.re * o.re),
        }
    }
}
impl core::ops::Neg for TDual {
    type Output = TDual;
    fn neg(self) -> TDual {
        TDual {
            re: -self.re,
            eps: -self.eps,
        }
    }
}
impl Scalar for TDual {
    fn zero() -> Self {
        TDual::con(0.0)
    }
    fn one() -> Self {
        TDual::con(1.0)
    }
    fn from_int(v: i64) -> Self {
        #[allow(clippy::cast_precision_loss)]
        TDual::con(v as f64)
    }
}

/// A random exact-rational cubic curve fixture.
fn rat_curve(seed: &mut u64) -> NurbsCurve<Rat, 3> {
    let r = |s: &mut u64| Rat::new(i128::from((lcg(s) * 17.0) as i64) - 8, 4);
    let knots = KnotVector::new(
        vec![
            Rat::int(0),
            Rat::int(0),
            Rat::int(0),
            Rat::int(0),
            Rat::new(1, 2),
            Rat::int(1),
            Rat::int(1),
            Rat::int(1),
            Rat::int(1),
        ],
        3,
    )
    .expect("knots");
    let points: Vec<[Rat; 3]> = (0..5).map(|_| [r(seed), r(seed), r(seed)]).collect();
    let weights: Vec<Rat> = (0..5)
        .map(|_| Rat::new(1 + (lcg(seed) * 3.0) as i128, 1 + (lcg(seed) * 2.0) as i128))
        .collect();
    NurbsCurve::new(knots, &points, &weights).expect("curve")
}

/// The f64 shadow of a rational curve.
fn to_f64_curve(c: &NurbsCurve<Rat, 3>) -> NurbsCurve<f64, 3> {
    NurbsCurve {
        knots: KnotVector::new(
            c.knots.knots.iter().map(|r| r.to_f64()).collect(),
            c.knots.degree,
        )
        .expect("knots"),
        cpw: c
            .cpw
            .iter()
            .map(|h| [h[0].to_f64(), h[1].to_f64(), h[2].to_f64(), h[3].to_f64()])
            .collect(),
    }
}

#[test]
fn nb_001_g0_laws_and_dual_derivatives() {
    let mut seed = 0x9B_0001u64;
    let rc = rat_curve(&mut seed);
    let fc = to_f64_curve(&rc);
    // Partition of unity in EXACT arithmetic.
    for t in [
        Rat::int(0),
        Rat::new(1, 7),
        Rat::new(1, 2),
        Rat::new(9, 10),
        Rat::int(1),
    ] {
        let (_, basis) = rc.knots.basis(t).expect("basis");
        let sum = basis.iter().fold(Rat::int(0), |a, &b| a + b);
        assert_eq!(sum, Rat::int(1), "partition of unity must be exact");
    }
    // Endpoint interpolation (clamped): C(0) = P0, C(1) = Pn.
    let start = rc.eval(Rat::int(0)).expect("eval");
    let p0 = [
        rc.cpw[0][0] / rc.cpw[0][3],
        rc.cpw[0][1] / rc.cpw[0][3],
        rc.cpw[0][2] / rc.cpw[0][3],
    ];
    assert_eq!(start, p0, "clamped endpoint interpolation is exact");
    // Derivative via the crate's own generic eval over a test dual ==
    // the analytic derivative pipeline.
    for &t in &[0.15f64, 0.4, 0.55, 0.83] {
        let dual_curve = NurbsCurve::<TDual, 3> {
            knots: KnotVector::new(
                fc.knots.knots.iter().map(|&u| TDual::con(u)).collect(),
                fc.knots.degree,
            )
            .expect("knots"),
            cpw: fc
                .cpw
                .iter()
                .map(|h| {
                    [
                        TDual::con(h[0]),
                        TDual::con(h[1]),
                        TDual::con(h[2]),
                        TDual::con(h[3]),
                    ]
                })
                .collect(),
        };
        let dval = dual_curve.eval(TDual::var(t)).expect("dual eval");
        let ders = fc.derivatives(t, 1).expect("ders");
        for k in 0..3 {
            assert!(
                (dval[k].re - ders[0][k]).abs() < 1e-12,
                "value mismatch at {t}"
            );
            assert!(
                (dval[k].eps - ders[1][k]).abs() < 1e-8 * (1.0 + ders[1][k].abs()),
                "derivative-vs-dual mismatch at {t}: {} vs {}",
                dval[k].eps,
                ders[1][k]
            );
        }
    }
    verdict(
        "nb-001",
        "exact partition of unity, exact endpoints, derivative == dual",
    );
}

#[test]
fn nb_002_refinement_is_exact_in_rational_arithmetic() {
    let mut seed = 0x9B_0002u64;
    for round in 0..6 {
        let c = rat_curve(&mut seed);
        // Insert two knots, then elevate degree — all exact.
        let refined = c
            .insert_knot(Rat::new(1, 3))
            .expect("insert 1/3")
            .insert_knot(Rat::new(4, 5))
            .expect("insert 4/5");
        let elevated = refined.elevate_degree().expect("elevate");
        assert_eq!(elevated.knots.degree, c.knots.degree + 1);
        for t in [
            Rat::int(0),
            Rat::new(1, 7),
            Rat::new(1, 3),
            Rat::new(1, 2),
            Rat::new(4, 5),
            Rat::new(19, 20),
            Rat::int(1),
        ] {
            let orig = c.eval(t).expect("orig");
            let after_insert = refined.eval(t).expect("refined");
            let after_elevate = elevated.eval(t).expect("elevated");
            assert_eq!(
                orig, after_insert,
                "round {round}: insertion must be EXACT at {t:?}"
            );
            assert_eq!(
                orig, after_elevate,
                "round {round}: elevation must be EXACT at {t:?}"
            );
        }
        // Lossless round trip: inserting then removing recovers the curve
        // EXACTLY (control net equality, not evaluation tolerance).
        let inserted = c.insert_knot(Rat::new(2, 7)).expect("insert 2/7");
        let removed = inserted.remove_knot(Rat::new(2, 7)).expect("remove 2/7");
        assert_eq!(
            removed, c,
            "round {round}: insert/remove must be a lossless round trip"
        );
    }
    verdict(
        "nb-002",
        "6 random rational curves: insertion/elevation evaluation-EXACT; insert+remove \
         recovers the control net",
    );
}

/// A closed degree-1 rational polyline loop in parameter space.
fn poly_loop(pts: &[[i64; 2]], scale_den: i64) -> TrimLoop {
    let n = pts.len();
    let mut knots = vec![Rat::int(0), Rat::int(0)];
    for k in 1..n {
        knots.push(Rat::new(k as i128, n as i128));
    }
    knots.push(Rat::int(1));
    knots.push(Rat::int(1));
    let kv = KnotVector::new(knots, 1).expect("polyline knots");
    let mut points: Vec<[Rat; 2]> = pts
        .iter()
        .map(|p| {
            [
                Rat::new(i128::from(p[0]), i128::from(scale_den)),
                Rat::new(i128::from(p[1]), i128::from(scale_den)),
            ]
        })
        .collect();
    points.push(points[0]);
    let weights = vec![Rat::int(1); points.len()];
    TrimLoop::new(NurbsCurve::new(kv, &points, &weights).expect("loop")).expect("closed")
}

#[test]
fn nb_003_trim_classification_adversarial_battery() {
    // Outer square CCW, diamond hole CW, plus a nested island inside the
    // hole (winding parity through three levels).
    let outer = poly_loop(&[[0, 0], [10, 0], [10, 10], [0, 10]], 1);
    // (5,2)->(7,5)->(5,8)->(3,5) is CCW; holes wind clockwise.
    let hole_cw = poly_loop(&[[5, 2], [7, 5], [5, 8], [3, 5]], 1).reversed_for_hole();
    let island = poly_loop(&[[45, 45], [55, 45], [55, 55], [45, 55]], 10); // 4.5..5.5 square
    let patch = TrimmedPatch::new(vec![outer.clone(), hole_cw.clone(), island.clone()]);
    let q = |a: i64, b: i64, d: i64| {
        [
            Rat::new(i128::from(a), i128::from(d)),
            Rat::new(i128::from(b), i128::from(d)),
        ]
    };
    // Solid region between square and diamond.
    assert_eq!(
        patch.classify(q(1, 1, 1)).expect("c"),
        Classification::Inside
    );
    // Inside the diamond hole but outside the island: outside.
    assert_eq!(
        patch.classify(q(42, 50, 10)).expect("c"),
        Classification::Outside
    );
    // Inside the island: inside again (nonzero rule through 3 loops).
    assert_eq!(
        patch.classify(q(5, 5, 1)).expect("c"),
        Classification::Inside
    );
    // Clearly outside everything.
    assert_eq!(
        patch.classify(q(-3, 4, 1)).expect("c"),
        Classification::Outside
    );
    // ON the outer boundary: honestly Boundary, never a false in/out.
    assert_eq!(
        patch.classify(q(0, 5, 1)).expect("c"),
        Classification::Boundary
    );
    // Tangent trim: a query at the diamond's apex height, just outside
    // its vertex — separable by subdivision, certified Outside-of-hole
    // (i.e. Inside the solid).
    assert_eq!(
        patch.classify(q(71, 50, 10)).expect("c"),
        Classification::Inside,
        "just right of the diamond vertex is solid"
    );
    // Sliver: an extremely thin triangle hole; a point midway between its
    // long edges is genuinely inside the sliver (outside the solid), and
    // a point just outside is solid.
    let sliver = poly_loop(&[[80, 20], [80, 22], [20, 21]], 10).reversed_for_hole();
    let patch2 = TrimmedPatch::new(vec![outer, sliver]);
    assert_eq!(
        patch2.classify(q(50, 21, 10)).expect("c"),
        Classification::Outside,
        "inside the sliver hole"
    );
    assert_eq!(
        patch2.classify(q(50, 25, 10)).expect("c"),
        Classification::Inside,
        "above the sliver is solid"
    );
    assert_eq!(
        patch2
            .classify_box(q(1, 1, 1), q(2, 2, 1))
            .expect("inside box"),
        Classification::Inside,
        "a curve-separated connected box inherits one winding verdict"
    );
    assert_eq!(
        patch2
            .classify_box(q(-1, 5, 10), q(1, 5, 10))
            .expect("straddling box"),
        Classification::Boundary,
        "a cell that straddles a trim curve must never inherit point authority"
    );
    verdict(
        "nb-003",
        "square/diamond/island nesting, boundary honesty, near-tangent vertex, sliver hole",
    );
}

#[test]
fn nb_004_measured_closest_point_brackets_the_oracle() {
    let mut seed = 0x9B_0004u64;
    // Curves.
    for round in 0..5 {
        let rc = rat_curve(&mut seed);
        let c = to_f64_curve(&rc);
        let q = [
            lcg(&mut seed) * 8.0 - 4.0,
            lcg(&mut seed) * 8.0 - 4.0,
            lcg(&mut seed) * 8.0 - 4.0,
        ];
        if round == 0 {
            assert!(closest_point_curve(&c, [f64::NAN, 0.0, 0.0], 1e-7, 1).is_err());
            assert!(closest_point_curve(&c, q, -1.0, 1).is_err());
            assert!(closest_point_curve(&c, q, 1e-7, u32::MAX).is_err());
        }
        let estimate = closest_point_curve(&c, q, 1e-7, 4000).expect("estimate");
        // Dense-sampling oracle.
        let (lo, hi) = c.knots.domain();
        let mut oracle = f64::INFINITY;
        for k in 0..=100_000 {
            let t = lo + (hi - lo) * f64::from(k) / 100_000.0;
            let p = c.eval(t).expect("eval");
            let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
            oracle = oracle.min(d);
        }
        assert!(
            estimate.lower <= oracle + 1e-12 && oracle <= estimate.upper + 1e-9,
            "round {round}: measured bracket [{}, {}] missed oracle {oracle}",
            estimate.lower,
            estimate.upper
        );
        assert!(
            estimate.upper - estimate.lower < 1e-3,
            "round {round}: bracket width {} too loose",
            estimate.upper - estimate.lower
        );
        println!(
            "{{\"suite\":\"fs-rep-nurbs/conformance\",\"metric\":\"closest-curve\",\
             \"round\":{round},\"lb\":{},\"ub\":{},\"iters\":{}}}",
            estimate.lower, estimate.upper, estimate.iterations
        );
    }
    // Surface (biquadratic with a bump).
    let kv = KnotVector::new(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 2).expect("kv");
    let mut points = Vec::new();
    for i in 0..3 {
        let mut row = Vec::new();
        for j in 0..3 {
            let z = if i == 1 && j == 1 { 1.5 } else { 0.0 };
            row.push([f64::from(i), f64::from(j), z]);
        }
        points.push(row);
    }
    let weights = vec![vec![1.0; 3]; 3];
    let s = NurbsSurface::new(kv.clone(), kv, &points, &weights).expect("surface");
    let q = [1.0, 1.0, 2.0];
    let estimate = closest_point_surface(&s, q, 1e-4, 4000).expect("estimate");
    let mut oracle = f64::INFINITY;
    for a in 0..=300 {
        for b in 0..=300 {
            let (u, v) = (f64::from(a) / 300.0, f64::from(b) / 300.0);
            let p = s.eval(u, v).expect("eval");
            let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
            oracle = oracle.min(d);
        }
    }
    assert!(
        estimate.lower <= oracle + 1e-12 && oracle <= estimate.upper + 1e-9,
        "surface bracket [{}, {}] vs oracle {oracle}",
        estimate.lower,
        estimate.upper
    );
    verdict(
        "nb-004",
        "curve + surface measured brackets contain sampled dense oracles; no rigorous enclosure claim",
    );
}

#[test]
fn nb_004b_closest_point_numeric_edge_regressions() {
    let line_knots = KnotVector::new(vec![0.0, 0.0, 1.0, 1.0], 1).expect("line knots");
    let line = NurbsCurve::new(
        line_knots.clone(),
        &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        &[1.0, 1.0],
    )
    .expect("line");
    let line_estimate =
        closest_point_curve(&line, [1.0, 1.0, 0.0], 0.0, 64).expect("linear estimate");
    assert!(line_estimate.upper.is_finite(), "degree-1 Newton must not index C''");
    let line_point = line.eval(line_estimate.param[0]).expect("line witness");
    let witness_distance = ((line_point[0] - 1.0).powi(2)
        + (line_point[1] - 1.0).powi(2)
        + line_point[2].powi(2))
    .sqrt();
    assert!((witness_distance - line_estimate.upper).abs() <= 4.0 * f64::EPSILON);

    let large = NurbsCurve::new(
        line_knots,
        &[[1.0e200, 0.0, 0.0], [1.0e200, 0.0, 0.0]],
        &[1.0, 1.0],
    )
    .expect("large-coordinate line");
    let large_estimate = closest_point_curve(&large, [0.0, 1.0e200, 0.0], 0.0, 1)
        .expect("scaled norm estimate");
    assert!(
        large_estimate.upper.is_finite() && large_estimate.upper > 1.0e200,
        "representable large distance must not overflow during squaring"
    );

    let adjacent = f64::from_bits(1.0f64.to_bits() + 1);
    let adjacent_u =
        KnotVector::new(vec![1.0, 1.0, adjacent, adjacent], 1).expect("adjacent u");
    let ordinary_v = KnotVector::new(vec![0.0, 0.0, 1.0, 1.0], 1).expect("ordinary v");
    let points = vec![
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
    ];
    let weights = vec![vec![1.0; 2]; 2];
    let one_axis = NurbsSurface::new(adjacent_u.clone(), ordinary_v, &points, &weights)
        .expect("one splittable axis");
    let one_axis_estimate = closest_point_surface(&one_axis, [0.5, 1.0, 0.0], 0.0, 1)
        .expect("fallback-axis split");
    assert_eq!(
        one_axis_estimate.iterations, 1,
        "an unsplittable preferred axis must fall back to the other axis"
    );

    let adjacent_v =
        KnotVector::new(vec![1.0, 1.0, adjacent, adjacent], 1).expect("adjacent v");
    let neither = NurbsSurface::new(adjacent_u, adjacent_v, &points, &weights)
        .expect("unsplittable axes");
    let neither_estimate = closest_point_surface(&neither, [0.5, 1.0, 0.0], 0.0, 1)
        .expect("retained unsplittable frontier");
    assert_eq!(neither_estimate.iterations, 0);
    assert!(neither_estimate.lower <= neither_estimate.upper);

    verdict(
        "nb-004b",
        "degree-1 Newton, scaled large-coordinate norms, and adjacent-float split termination",
    );
}

#[test]
fn nb_005_boolean_policy_refuses_with_the_route() {
    let default_refusal = boolean(BooleanOp::Union, BooleanPolicy::default());
    assert_eq!(default_refusal.policy, BooleanPolicy::RouteThroughSdf);
    assert!(default_refusal.route.contains("convert-nurbs-sdf"));
    assert!(default_refusal.route.contains("convert-sdf-nurbs"));
    let gated = boolean(BooleanOp::Subtract, BooleanPolicy::DirectCertificateGated);
    assert!(
        gated.diagnostics.iter().any(|d| d.contains("certificate")),
        "gated refusal must teach the certificate requirement"
    );
    assert!(
        gated.route.contains("coverage-complete continuum"),
        "the direct route must require the successor continuum certificate, not sampled wqd.13 evidence"
    );
    verdict(
        "nb-005",
        "both policies refuse with teaching routes (the honest position)",
    );
}

#[test]
fn nb_006_surface_refinement_exact_and_partials_check() {
    // Exact surface insertion (Rat) leaves evaluation identical.
    let kv = |n: i64| {
        KnotVector::new(
            vec![
                Rat::int(0),
                Rat::int(0),
                Rat::int(0),
                Rat::int(n),
                Rat::int(n),
                Rat::int(n),
            ],
            2,
        )
        .expect("kv")
    };
    let mut points = Vec::new();
    for i in 0..3i64 {
        let mut row = Vec::new();
        for j in 0..3i64 {
            row.push([Rat::int(i), Rat::int(j), Rat::new(i128::from(i * j), 2)]);
        }
        points.push(row);
    }
    let weights = vec![vec![Rat::int(1), Rat::new(3, 2), Rat::int(1)]; 3];
    let s = NurbsSurface::new(kv(1), kv(1), &points, &weights).expect("surface");
    let refined = s
        .insert_knot_u(Rat::new(1, 3))
        .expect("u insert")
        .insert_knot_v(Rat::new(2, 5))
        .expect("v insert");
    for (u, v) in [
        (Rat::int(0), Rat::int(0)),
        (Rat::new(1, 3), Rat::new(2, 5)),
        (Rat::new(1, 2), Rat::new(1, 2)),
        (Rat::new(7, 8), Rat::new(1, 5)),
        (Rat::int(1), Rat::int(1)),
    ] {
        assert_eq!(
            s.eval(u, v).expect("orig"),
            refined.eval(u, v).expect("refined"),
            "surface refinement must be EXACT at ({u:?}, {v:?})"
        );
    }
    // f64 partials vs central differences.
    let fs = NurbsSurface::<f64> {
        knots_u: KnotVector::new(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 2).expect("kv"),
        knots_v: KnotVector::new(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 2).expect("kv"),
        cpw: s
            .cpw
            .iter()
            .map(|row| {
                row.iter()
                    .map(|h| [h[0].to_f64(), h[1].to_f64(), h[2].to_f64(), h[3].to_f64()])
                    .collect()
            })
            .collect(),
    };
    let (u, v) = (0.37, 0.61);
    let (val, su, sv) = fs.partials(u, v).expect("partials");
    let h = 1e-6;
    let up = fs.eval(u + h, v).expect("e");
    let un = fs.eval(u - h, v).expect("e");
    let vp = fs.eval(u, v + h).expect("e");
    let vn = fs.eval(u, v - h).expect("e");
    for k in 0..3 {
        assert!(
            (su[k] - (up[k] - un[k]) / (2.0 * h)).abs() < 1e-5,
            "S_u component {k}"
        );
        assert!(
            (sv[k] - (vp[k] - vn[k]) / (2.0 * h)).abs() < 1e-5,
            "S_v component {k}"
        );
    }
    let direct = fs.eval(u, v).expect("e");
    for k in 0..3 {
        assert!((val[k] - direct[k]).abs() < 1e-12);
    }
    verdict(
        "nb-006",
        "exact surface refinement; partials match central differences",
    );
}
