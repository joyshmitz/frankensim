//! Gauntlet G3 relations for the live optimization-step surface.
//!
//! These checks supplement the fixed manifold and budget pins in the
//! conformance battery; they do not replace those cases.

use asupersync::types::Budget;
use fs_exec::{CancelGate, Cx, ExecMode, StreamKey};
use fs_opt::{DescentOptions, Manifold, descend_fn};
use fs_propcheck::metamorphic::{
    RelationCase, RelationObservation, Tolerance, check_relation, unit_rescaling,
};

type QuadraticCase = (f64, f64, f64);

const STEPS: u32 = 3;
const EXPECTED_EVALS: u64 = 1 + 2 * STEPS as u64 + 1;

#[derive(Debug)]
struct DescentScaleReceipt {
    x: f64,
    f0: f64,
    f_final: f64,
    evals: u64,
    steps_taken: u32,
    budget_stopped: bool,
}

fn with_cx<R>(f: impl FnOnce(&Cx<'_>) -> R) -> R {
    let gate = CancelGate::new();
    let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
    pool.scope(|arena| {
        let cx = Cx::new(
            &gate,
            arena,
            StreamKey {
                seed: 0x2ACE_0401,
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

fn scale_for_exponent(exponent: i64) -> f64 {
    2.0_f64.powi(i32::try_from(exponent).expect("generated exponent fits i32"))
}

#[test]
fn g3_descend_fn_is_equivariant_under_power_of_two_unit_rescaling() {
    with_cx(|cx| {
        let operator = |&(x0, target, fd_h): &QuadraticCase| {
            let objective = |x: &[f64]| {
                let residual = x[0] - target;
                residual * residual
            };
            let report = descend_fn(
                Manifold::Rn { dim: 1 },
                &objective,
                &[x0],
                DescentOptions {
                    steps: STEPS,
                    lr: 0.125,
                    fd_h,
                },
                0,
                cx,
            )
            .expect("generated quadratic descent is admitted");
            DescentScaleReceipt {
                x: report.x[0],
                f0: report.f0,
                f_final: report.f_final,
                evals: report.evals,
                steps_taken: report.steps_taken,
                budget_stopped: report.budget_stopped,
            }
        };
        let relation = unit_rescaling(
            "quadratic-descent-power-of-two-units",
            Tolerance::AbsoluteRelative {
                max_abs: 2.0e-12,
                max_relative: 2.0e-12,
            },
            |&(x0, target, fd_h): &QuadraticCase, &exponent: &i64| {
                let scale = scale_for_exponent(exponent);
                (x0 * scale, target * scale, fd_h * scale)
            },
            |base: &DescentScaleReceipt,
             transformed: &DescentScaleReceipt,
             &exponent: &i64,
             tolerance: Tolerance| {
                let scale = scale_for_exponent(exponent);
                let scale_sq = scale * scale;
                let x = tolerance.evaluate_scalar(base.x * scale, transformed.x);
                let f0 = tolerance.evaluate_scalar(base.f0 * scale_sq, transformed.f0);
                let f_final =
                    tolerance.evaluate_scalar(base.f_final * scale_sq, transformed.f_final);
                let discrete_receipts_match = base.evals == EXPECTED_EVALS
                    && transformed.evals == EXPECTED_EVALS
                    && base.steps_taken == STEPS
                    && transformed.steps_taken == STEPS
                    && !base.budget_stopped
                    && !transformed.budget_stopped;
                let discrete_margin = if discrete_receipts_match { 0.0 } else { -1.0 };
                RelationObservation::new(
                    x.margin()
                        .min(f0.margin())
                        .min(f_final.margin())
                        .min(discrete_margin),
                    "scaled descent preserves x, quadratic objectives, and discrete receipts",
                )
            },
        );
        let exponents = [-4_i64, -3, -2, -1, 1, 2, 3, 4];

        check_relation(
            "fs-opt::descend_fn",
            0x2ACE_0402,
            256,
            |stream| {
                let target = stream.f64_in(-4.0, 4.0);
                let magnitude = stream.f64_in(0.25, 2.0);
                let sign = if stream.next_u64().is_multiple_of(2) {
                    -1.0
                } else {
                    1.0
                };
                let exponent = exponents[(stream.next_u64() % exponents.len() as u64) as usize];
                RelationCase::new(
                    (target + sign * magnitude, target, 2.0_f64.powi(-10)),
                    exponent,
                )
            },
            &operator,
            &relation,
        );
    });
}
