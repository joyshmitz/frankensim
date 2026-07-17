//! G5 study-scale replay and seeded-failure self-test for trust-region Newton.
//!
//! The production driver solves the existing six-dimensional Rosenbrock
//! fixture with an exact matrix-free Hessian-vector callback. The receipt binds
//! every objective and Hessian-vector callback input/output plus every public
//! `TrustRegionReport` field. A same-input repeat must reproduce it byte for
//! byte. A deterministic red mutation flips one finite mantissa bit in the
//! returned decision and is refused even after the payload is self-consistently
//! resealed.
//!
//! This is one objective/Hessian pair. It does not claim all objectives,
//! approximate-Hessian parity, cancellation, checkpointing, cross-ISA equality,
//! ledger persistence, or performance.

use core::cell::RefCell;

use fs_ascent::{TrustRegionReport, trust_region_newton};
use fs_obs::ident::{IdentityBuilder, ReplayIdentity, check_version};
use fs_obs::{Emitter, EventKind, Severity};

const SUITE: &str = "fs-ascent/trust-region-study-replay";
const INPUT_SEED: u64 = 0;
const MUTATION_SEED: u64 = 0x5452_5553_545F_5244;
const DIMENSION: usize = 6;
const GRADIENT_TOLERANCE: f64 = 1e-7;
const MAX_ITERATIONS: usize = 300;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectiveCall {
    point_bits: Vec<u64>,
    objective_bits: u64,
    gradient_bits: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HessianVectorCall {
    point_bits: Vec<u64>,
    direction_bits: Vec<u64>,
    product_bits: Vec<u64>,
}

#[derive(Debug)]
struct RunRecord {
    report: TrustRegionReport,
    objective_calls: Vec<ObjectiveCall>,
    hessian_vector_calls: Vec<HessianVectorCall>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReceiptPayload {
    input_seed: u64,
    start_bits: Vec<u64>,
    objective_calls: Vec<ObjectiveCall>,
    hessian_vector_calls: Vec<HessianVectorCall>,
    report_x_bits: Vec<u64>,
    report_f_bits: u64,
    report_gradient_norm_bits: u64,
    report_iterations: usize,
    report_evaluations: usize,
    report_hessian_vector_evaluations: usize,
    report_negative_curvature_hits: usize,
}

impl ReceiptPayload {
    fn identity(&self) -> ReplayIdentity {
        let mut builder = IdentityBuilder::new("fs-ascent-trust-region-study-receipt-v1")
            .str("fs-ascent-version", fs_ascent::VERSION)
            .str("engine", "trust_region_newton/Steihaug-CG")
            .str("objective", "Rosenbrock-chain")
            .str("hessian-vector", "exact-analytic")
            .u64("input-seed", self.input_seed)
            .u64("dimension", DIMENSION as u64)
            .f64_bits("gradient-tolerance", GRADIENT_TOLERANCE)
            .u64("maximum-iterations", MAX_ITERATIONS as u64)
            .u64("start-values", self.start_bits.len() as u64);
        for &value_bits in &self.start_bits {
            builder = builder.u64("start-value-bits", value_bits);
        }

        builder = builder.u64("objective-calls", self.objective_calls.len() as u64);
        for call in &self.objective_calls {
            builder = builder.u64("objective-point-values", call.point_bits.len() as u64);
            for &value_bits in &call.point_bits {
                builder = builder.u64("objective-point-bits", value_bits);
            }
            builder = builder
                .u64("objective-value-bits", call.objective_bits)
                .u64("gradient-values", call.gradient_bits.len() as u64);
            for &value_bits in &call.gradient_bits {
                builder = builder.u64("gradient-value-bits", value_bits);
            }
        }

        builder = builder.u64(
            "hessian-vector-calls",
            self.hessian_vector_calls.len() as u64,
        );
        for call in &self.hessian_vector_calls {
            builder = builder.u64("hessian-point-values", call.point_bits.len() as u64);
            for &value_bits in &call.point_bits {
                builder = builder.u64("hessian-point-bits", value_bits);
            }
            builder = builder.u64("hessian-direction-values", call.direction_bits.len() as u64);
            for &value_bits in &call.direction_bits {
                builder = builder.u64("hessian-direction-bits", value_bits);
            }
            builder = builder.u64("hessian-product-values", call.product_bits.len() as u64);
            for &value_bits in &call.product_bits {
                builder = builder.u64("hessian-product-bits", value_bits);
            }
        }

        builder = builder.u64("report-x-values", self.report_x_bits.len() as u64);
        for &value_bits in &self.report_x_bits {
            builder = builder.u64("report-x-bits", value_bits);
        }
        builder
            .u64("report-objective-bits", self.report_f_bits)
            .u64("report-gradient-norm-bits", self.report_gradient_norm_bits)
            .u64("report-iterations", self.report_iterations as u64)
            .u64("report-evaluations", self.report_evaluations as u64)
            .u64(
                "report-hessian-vector-evaluations",
                self.report_hessian_vector_evaluations as u64,
            )
            .u64(
                "report-negative-curvature-hits",
                self.report_negative_curvature_hits as u64,
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedReceipt {
    payload: ReceiptPayload,
    declared_identity: ReplayIdentity,
}

impl RetainedReceipt {
    fn new(payload: ReceiptPayload) -> Self {
        let declared_identity = payload.identity();
        Self {
            payload,
            declared_identity,
        }
    }

    fn reseal(&mut self) {
        self.declared_identity = self.payload.identity();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergeRefusal {
    UnsupportedIdentityVersion,
    PayloadIdentityMismatch,
    ReferenceIdentityMismatch,
}

fn admit_receipt(
    reference: &ReplayIdentity,
    candidate: &RetainedReceipt,
) -> Result<(), MergeRefusal> {
    check_version(candidate.declared_identity.version())
        .map_err(|_| MergeRefusal::UnsupportedIdentityVersion)?;
    if candidate.payload.identity() != candidate.declared_identity {
        return Err(MergeRefusal::PayloadIdentityMismatch);
    }
    if &candidate.declared_identity != reference {
        return Err(MergeRefusal::ReferenceIdentityMismatch);
    }
    Ok(())
}

fn bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|value| value.to_bits()).collect()
}

fn rosenbrock(x: &[f64]) -> (f64, Vec<f64>) {
    assert_eq!(x.len(), DIMENSION);
    let mut objective = 0.0;
    let mut gradient = vec![0.0; DIMENSION];
    for index in 0..DIMENSION - 1 {
        let center = 1.0 - x[index];
        let valley = x[index + 1] - x[index] * x[index];
        objective += center.mul_add(center, 100.0 * valley * valley);
        gradient[index] += (-2.0f64).mul_add(center, -400.0 * x[index] * valley);
        gradient[index + 1] += 200.0 * valley;
    }
    (objective, gradient)
}

fn rosenbrock_hessian_vector(x: &[f64], direction: &[f64]) -> Vec<f64> {
    assert_eq!(x.len(), DIMENSION);
    assert_eq!(direction.len(), DIMENSION);
    let mut product = vec![0.0; DIMENSION];
    for index in 0..DIMENSION - 1 {
        let xi = x[index];
        let valley = x[index + 1] - xi * xi;
        let diagonal = 2.0 - 400.0 * valley + 800.0 * xi * xi;
        let off_diagonal = -400.0 * xi;
        product[index] += diagonal.mul_add(direction[index], off_diagonal * direction[index + 1]);
        product[index + 1] += off_diagonal.mul_add(direction[index], 200.0 * direction[index + 1]);
    }
    product
}

fn run_once(start: &[f64]) -> RunRecord {
    let objective_calls = RefCell::new(Vec::new());
    let hessian_vector_calls = RefCell::new(Vec::new());
    let report = {
        let mut objective = |x: &[f64]| {
            let (value, gradient) = rosenbrock(x);
            objective_calls.borrow_mut().push(ObjectiveCall {
                point_bits: bits(x),
                objective_bits: value.to_bits(),
                gradient_bits: bits(&gradient),
            });
            (value, gradient)
        };
        let mut hessian_vector = |x: &[f64], direction: &[f64]| {
            let product = rosenbrock_hessian_vector(x, direction);
            hessian_vector_calls.borrow_mut().push(HessianVectorCall {
                point_bits: bits(x),
                direction_bits: bits(direction),
                product_bits: bits(&product),
            });
            product
        };
        trust_region_newton(
            start,
            &mut objective,
            &mut hessian_vector,
            GRADIENT_TOLERANCE,
            MAX_ITERATIONS,
        )
    };
    RunRecord {
        report,
        objective_calls: objective_calls.into_inner(),
        hessian_vector_calls: hessian_vector_calls.into_inner(),
    }
}

fn receipt(start: &[f64], run: &RunRecord) -> RetainedReceipt {
    RetainedReceipt::new(ReceiptPayload {
        input_seed: INPUT_SEED,
        start_bits: bits(start),
        objective_calls: run.objective_calls.clone(),
        hessian_vector_calls: run.hessian_vector_calls.clone(),
        report_x_bits: bits(&run.report.x),
        report_f_bits: run.report.f.to_bits(),
        report_gradient_norm_bits: run.report.grad_norm.to_bits(),
        report_iterations: run.report.iters,
        report_evaluations: run.report.evals,
        report_hessian_vector_evaluations: run.report.hv_evals,
        report_negative_curvature_hits: run.report.negative_curvature_hits,
    })
}

fn mutate_returned_decision(receipt: &RetainedReceipt) -> (RetainedReceipt, usize, u64) {
    let mut mutant = receipt.clone();
    let coordinate = (MUTATION_SEED as usize) % mutant.payload.report_x_bits.len();
    let mask = 1_u64 << ((MUTATION_SEED >> 8) % 52);
    mutant.payload.report_x_bits[coordinate] ^= mask;
    assert!(
        f64::from_bits(mutant.payload.report_x_bits[coordinate]).is_finite(),
        "mantissa-only mutation must remain a finite wire-valid decision"
    );
    mutant.reseal();
    (mutant, coordinate, mask)
}

fn emit_receipt(
    reference: &RetainedReceipt,
    mutant: &RetainedReceipt,
    coordinate: usize,
    mask: u64,
) {
    let json = format!(
        "{{\"input_seed\":{INPUT_SEED},\"mutation_seed\":{MUTATION_SEED},\
         \"reference_identity\":\"{}\",\"mutant_identity\":\"{}\",\
         \"mutated_coordinate\":{coordinate},\"mantissa_mask\":\"{mask:#018x}\",\
         \"merge_refusal\":\"reference-identity-mismatch\"}}",
        reference.declared_identity.hex(),
        mutant.declared_identity.hex(),
    );
    let mut emitter = Emitter::new(SUITE, "rosenbrock-exact-hessian-vector");
    let receipt_event = emitter.emit(
        Severity::Info,
        EventKind::Custom {
            name: "trust-region-study-replay-receipt".to_string(),
            json,
        },
        None,
    );
    let receipt_line = receipt_event.to_jsonl();
    fs_obs::validate_line(&receipt_line)
        .expect("trust-region receipt must use the fs-obs wire schema");
    println!("{receipt_line}");

    let verdict = emitter.emit(
        Severity::Info,
        EventKind::ConformanceCase {
            suite: SUITE.to_string(),
            case: "rosenbrock-exact-hessian-vector".to_string(),
            pass: true,
            detail: format!(
                "fixed input seed {INPUT_SEED} replayed every objective/Hv call and report bit; mutation seed {MUTATION_SEED:#018x} flipped coordinate {coordinate} mask {mask:#018x}, produced stable identity {}, and the merge gate refused it",
                mutant.declared_identity.hex(),
            ),
            seed: INPUT_SEED,
        },
        None,
    );
    fs_obs::lint_failure_record(&verdict)
        .expect("trust-region seeded-failure verdict must be replayable");
    let verdict_line = verdict.to_jsonl();
    fs_obs::validate_line(&verdict_line)
        .expect("trust-region verdict must use the fs-obs wire schema");
    println!("{verdict_line}");
}

#[test]
fn trust_region_study_replays_and_rejects_seeded_red_mutation() {
    let start = vec![-1.2; DIMENSION];
    let reference_run = run_once(&start);
    assert!(
        reference_run.report.grad_norm < GRADIENT_TOLERANCE,
        "trust-region run did not certify: {:?}",
        reference_run.report,
    );
    assert!(
        reference_run.report.f < 1e-10,
        "Rosenbrock objective remained {}",
        reference_run.report.f,
    );
    assert!(
        reference_run
            .report
            .x
            .iter()
            .all(|value| (value - 1.0).abs() < 1e-4),
        "trust-region result missed the analytic minimizer: {:?}",
        reference_run.report.x,
    );
    assert!(
        reference_run.report.negative_curvature_hits > 0,
        "the retained study must exercise Steihaug negative-curvature handling"
    );
    assert_eq!(
        reference_run.report.evals,
        reference_run.objective_calls.len(),
        "objective callback accounting must be exact"
    );
    assert_eq!(
        reference_run.report.hv_evals,
        reference_run.hessian_vector_calls.len(),
        "Hessian-vector callback accounting must be exact"
    );

    let reference = receipt(&start, &reference_run);
    admit_receipt(&reference.declared_identity, &reference)
        .expect("the internally consistent reference receipt must admit");
    let replay = receipt(&start, &run_once(&start));
    assert_eq!(
        replay, reference,
        "the complete callback trace and public report must replay exactly"
    );

    let (mutant, coordinate, mask) = mutate_returned_decision(&reference);
    let (mutant_repeat, repeat_coordinate, repeat_mask) = mutate_returned_decision(&reference);
    assert_eq!((coordinate, mask), (repeat_coordinate, repeat_mask));
    assert_eq!(
        mutant, mutant_repeat,
        "the seeded red mutation and evidence identity must be stable"
    );
    assert_ne!(mutant.declared_identity, reference.declared_identity);
    let mut stale_identity_mutant = mutant.clone();
    stale_identity_mutant.declared_identity = reference.declared_identity.clone();
    assert_eq!(
        admit_receipt(&reference.declared_identity, &stale_identity_mutant),
        Err(MergeRefusal::PayloadIdentityMismatch)
    );
    assert_eq!(
        admit_receipt(&reference.declared_identity, &mutant),
        Err(MergeRefusal::ReferenceIdentityMismatch)
    );

    emit_receipt(&reference, &mutant, coordinate, mask);
}
