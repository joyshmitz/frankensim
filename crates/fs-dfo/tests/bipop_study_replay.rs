//! G5 study-scale replay for the production BIPOP-CMA path (7tv.21.22).
//!
//! The fixture captures every objective call and binds that trace together
//! with every public `BipopReport`/`CmaReport` field. A test-local algebraic
//! oracle independently checks objective semantics, reconstructs every restart
//! boundary and BIPOP budget decision, verifies the declared restart/sample
//! streams at their observable boundaries, and links the first stable global
//! minimum to its exact winning run. A disclosed seeded mutation changes one
//! returned coordinate bit. The unsealed edit is refused as a stale payload,
//! the self-consistently resealed edit is refused both against the retained
//! reference and by semantic admission, and the resulting red fs-obs evidence
//! is independently reproducible. This is one finite deterministic study, not
//! an optimizer-quality or performance claim.

use fs_dfo::{BipopReport, bipop_cmaes};
use fs_obs::ident::{IdentityBuilder, ReplayIdentity};
use fs_obs::{Emitter, Event, EventKind, Severity};
use fs_rand::StreamKey;
use std::fmt::Write as _;
use std::panic::catch_unwind;

const SUITE: &str = "fs-dfo/bipop-study-replay";
const CASE: &str = "shifted-rastrigin-4d-full-public-state";
const RED_CASE: &str = "seeded-returned-coordinate-corruption";

const INPUT_SEED: u64 = 0xDF0A_2100_0000_0001;
const CORRUPTION_SEED: u64 = 0xDF0A_F11E_0000_0001;
const DIMENSION: usize = 4;
const X0: [f64; DIMENSION] = [2.75, -1.25, 3.5, -2.0];
const SHIFT: [f64; DIMENSION] = [0.25, -0.5, 1.0, -1.5];
const SIGMA0: f64 = 1.25;
const TOTAL_BUDGET: usize = 6_000;
// Shifted Rastrigin is non-negative, so this target keeps every restart in
// the non-converged evidence path without depending on solution quality.
const F_TARGET: f64 = -1.0;
const PER_RESTART_GENERATIONS: usize = 250;
const LARGE_RUN_CAP_TRIGGER: u32 = 8;
const CMA_EIGEN_INTERVAL: usize = 1;
const CMA_SPREAD_RELATIVE_LIMIT: f64 = 1e-12;
const CMA_MEANINGFUL_IMPROVEMENT_RELATIVE: f64 = 1e-12;
const CMA_STAGNATION_GENERATIONS: usize = 120;
const OBJECTIVE_ORACLE_ROUNDOFF_SCALE: f64 = 64.0;
const SEMANTIC_ORACLE_VERSION: &str =
    "bipop-independent-objective-restart-stream-schedule-accounting-v1";

// These are the logical stream coordinates and restart rule used by
// `fs_dfo::cma`; recording them makes the private implementation choice
// explicit in the fixture identity.  A change also changes the captured trace.
const CMA_STREAM_KERNEL: u32 = 0xD1F0;
const CMA_SAMPLE_TILE: u32 = 0;
const CMA_RESTART_TILE: u32 = 1;
const RESTART_SEED_STRIDE: u64 = 0x9E37_79B9;

#[derive(Debug, Clone)]
struct Evaluation {
    x: Vec<f64>,
    value: f64,
}

#[derive(Debug, Clone)]
struct StudyRun {
    input_seed: u64,
    fixture: ReplayIdentity,
    report: BipopReport,
    evaluations: Vec<Evaluation>,
    result: ReplayIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdmissionError {
    FixtureIdentityMismatch { declared: u64, computed: u64 },
    PayloadIdentityMismatch { declared: u64, computed: u64 },
    ReferenceIdentityMismatch { expected: u64, found: u64 },
}

#[derive(Debug, Clone, Copy)]
struct RestartSlice {
    start: usize,
    end: usize,
    lambda: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mutation {
    seed: u64,
    coordinate: usize,
    mantissa_bit: u32,
    before: u64,
    after: u64,
}

#[derive(Debug)]
struct SeededCorruption {
    run: StudyRun,
    mutation: Mutation,
    stale_error: AdmissionError,
    reference_error: AdmissionError,
    mismatch: String,
}

fn usize_u64(value: usize) -> u64 {
    u64::try_from(value).expect("fixture cardinality fits u64")
}

fn shifted_rastrigin_callback(x: &[f64]) -> f64 {
    assert_eq!(
        x.len(),
        DIMENSION,
        "fixture dimension is part of the contract"
    );
    x.iter()
        .zip(SHIFT)
        .map(|(&value, shift)| {
            let z = value - shift;
            10.0 + z.mul_add(z, -10.0 * fs_math::det::cos(std::f64::consts::TAU * z))
        })
        .sum()
}

fn shifted_rastrigin_oracle(x: &[f64]) -> f64 {
    assert_eq!(
        x.len(),
        DIMENSION,
        "oracle dimension is part of the fixture contract"
    );
    x.iter()
        .zip(SHIFT)
        .map(|(&value, shift)| {
            let z = value - shift;
            let periodic_penalty = 10.0 * (1.0 - fs_math::det::cos(std::f64::consts::TAU * z));
            z * z + periodic_penalty
        })
        .sum()
}

fn objective_oracle_tolerance(recorded: f64, oracle: f64) -> f64 {
    let scale = recorded.abs().max(oracle.abs()).max(1.0);
    OBJECTIVE_ORACLE_ROUNDOFF_SCALE * f64::EPSILON * (DIMENSION as f64) * scale
}

fn expected_base_lambda() -> usize {
    4 + (3.0 * fs_math::det::ln(DIMENSION as f64)).floor() as usize
}

fn fixture_identity(seed: u64) -> ReplayIdentity {
    let mut builder = IdentityBuilder::new("fs-dfo-bipop-study-fixture-v2")
        .str("suite", SUITE)
        .str("case", CASE)
        .str("red-case", RED_CASE)
        .str("semantic-oracle-version", SEMANTIC_ORACLE_VERSION)
        .str("algorithm", "fs_dfo::bipop_cmaes")
        .str("objective", "shifted-rastrigin")
        .str("units", "dimensionless")
        .u64("dimension", usize_u64(DIMENSION))
        .f64_bits("sigma0", SIGMA0)
        .u64("total-evaluation-budget", usize_u64(TOTAL_BUDGET))
        .f64_bits("f-target", F_TARGET)
        .u64("input-seed", seed)
        .u64("corruption-seed", CORRUPTION_SEED)
        .u64("base-lambda", usize_u64(expected_base_lambda()))
        .str("base-lambda-rule", "4+floor(3*ln(dimension))")
        .u64(
            "per-restart-generations",
            usize_u64(PER_RESTART_GENERATIONS),
        )
        .str(
            "per-restart-budget-rule",
            "min(lambda*per-restart-generations,remaining)",
        )
        .str("large-restart-rule", "large-budget-used<=small-budget-used")
        .str("large-population-rule", "base-lambda*2^large-runs")
        .u64("large-run-cap-trigger", u64::from(LARGE_RUN_CAP_TRIGGER))
        .u64("cma-eigen-interval", usize_u64(CMA_EIGEN_INTERVAL))
        .str(
            "stagnation-rule",
            "spread<relative-limit*sigma0 OR generations-since-meaningful-improvement>limit",
        )
        .f64_bits("cma-spread-relative-limit", CMA_SPREAD_RELATIVE_LIMIT)
        .f64_bits(
            "cma-meaningful-improvement-relative",
            CMA_MEANINGFUL_IMPROVEMENT_RELATIVE,
        )
        .u64(
            "cma-stagnation-generations",
            usize_u64(CMA_STAGNATION_GENERATIONS),
        )
        .f64_bits(
            "objective-oracle-roundoff-scale",
            OBJECTIVE_ORACLE_ROUNDOFF_SCALE,
        )
        .u64("cma-stream-kernel", u64::from(CMA_STREAM_KERNEL))
        .u64("sample-stream-tile", u64::from(CMA_SAMPLE_TILE))
        .u64("restart-stream-tile", u64::from(CMA_RESTART_TILE))
        .u64("restart-seed-stride", RESTART_SEED_STRIDE)
        .u64(
            "fs-rand-stream-semantics-version",
            u64::from(fs_rand::STREAM_SEMANTICS_VERSION),
        )
        .str(
            "fs-rand-stream-position-domain",
            fs_rand::STREAM_POSITION_IDENTITY_DOMAIN,
        )
        .str(
            "capabilities",
            "safe-rust;strict-fs-math;keyed-fs-rand;canonical-fs-obs",
        )
        .str("execution-context", "single-threaded-direct-test-no-Cx")
        .str("fs-dfo-version", fs_dfo::VERSION)
        .str("fs-la-version", fs_la::VERSION)
        .str("fs-math-version", fs_math::VERSION)
        .str("fs-rand-version", fs_rand::VERSION)
        .str("fs-obs-version", fs_obs::VERSION);
    for (coordinate, (&x0, &shift)) in X0.iter().zip(&SHIFT).enumerate() {
        builder = builder
            .u64("coordinate-index", usize_u64(coordinate))
            .f64_bits("initial-coordinate", x0)
            .f64_bits("objective-shift", shift);
    }
    builder.finish()
}

fn result_identity(
    fixture: &ReplayIdentity,
    report: &BipopReport,
    evaluations: &[Evaluation],
) -> ReplayIdentity {
    let mut builder = IdentityBuilder::new("fs-dfo-bipop-study-result-v1")
        .child("fixture", fixture)
        .u64("total-evals", usize_u64(report.total_evals))
        .u64("schedule-length", usize_u64(report.schedule.len()))
        .u64("best-x-length", usize_u64(report.best.x_best.len()))
        .f64_bits("best-f", report.best.f_best)
        .u64("best-run-evals", usize_u64(report.best.evals))
        .u64("best-run-generations", usize_u64(report.best.generations))
        .flag("best-run-converged", report.best.converged)
        .f64_bits("best-run-sigma", report.best.sigma)
        .u64("evaluation-trace-length", usize_u64(evaluations.len()));
    for (restart, &lambda) in report.schedule.iter().enumerate() {
        builder = builder
            .u64("restart-index", usize_u64(restart))
            .u64("restart-lambda", usize_u64(lambda));
    }
    for (coordinate, &value) in report.best.x_best.iter().enumerate() {
        builder = builder
            .u64("best-coordinate-index", usize_u64(coordinate))
            .f64_bits("best-coordinate", value);
    }
    for (evaluation_index, evaluation) in evaluations.iter().enumerate() {
        builder = builder
            .u64("evaluation-index", usize_u64(evaluation_index))
            .u64("evaluation-dimension", usize_u64(evaluation.x.len()));
        for (coordinate, &value) in evaluation.x.iter().enumerate() {
            builder = builder
                .u64("evaluation-coordinate-index", usize_u64(coordinate))
                .f64_bits("evaluation-coordinate", value);
        }
        builder = builder.f64_bits("evaluation-objective", evaluation.value);
    }
    builder.finish()
}

fn run_study(seed: u64) -> StudyRun {
    let mut evaluations = Vec::with_capacity(TOTAL_BUDGET);
    let report = {
        let mut objective = |x: &[f64]| {
            let value = shifted_rastrigin_callback(x);
            evaluations.push(Evaluation {
                x: x.to_vec(),
                value,
            });
            value
        };
        bipop_cmaes(&mut objective, &X0, SIGMA0, TOTAL_BUDGET, F_TARGET, seed)
    };
    let fixture = fixture_identity(seed);
    let result = result_identity(&fixture, &report, &evaluations);
    StudyRun {
        input_seed: seed,
        fixture,
        report,
        evaluations,
        result,
    }
}

fn same_point_bits(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| a.to_bits() == b.to_bits())
}

fn expected_restart_starts(input_seed: u64, restart_count: usize) -> Vec<Vec<f64>> {
    let mut stream = StreamKey {
        seed: input_seed,
        kernel: CMA_STREAM_KERNEL,
        tile: CMA_RESTART_TILE,
    }
    .stream();
    (0..restart_count)
        .map(|restart| {
            if restart == 0 {
                X0.to_vec()
            } else {
                X0.iter()
                    .map(|&value| SIGMA0.mul_add(stream.next_normal(), value))
                    .collect()
            }
        })
        .collect()
}

fn first_generation_matches(
    run: &StudyRun,
    restart: usize,
    slice: RestartSlice,
    start: &[f64],
) -> bool {
    let run_evals = slice.end - slice.start;
    if run_evals < 1 + slice.lambda {
        return true;
    }
    let restart_offset = usize_u64(restart)
        .checked_mul(RESTART_SEED_STRIDE)
        .expect("fixture restart seed offset fits u64");
    let mut stream = StreamKey {
        seed: run.input_seed.wrapping_add(restart_offset),
        kernel: CMA_STREAM_KERNEL,
        tile: CMA_SAMPLE_TILE,
    }
    .stream();
    (0..slice.lambda).all(|candidate| {
        let expected: Vec<f64> = start
            .iter()
            .map(|&mean| SIGMA0.mul_add(stream.next_normal(), mean))
            .collect();
        same_point_bits(&expected, &run.evaluations[slice.start + 1 + candidate].x)
    })
}

fn reconstruct_restart_slices(run: &StudyRun) -> Result<Vec<RestartSlice>, String> {
    let expected_starts = expected_restart_starts(run.input_seed, run.report.schedule.len());
    let mut boundaries = Vec::with_capacity(expected_starts.len());
    for (restart, expected) in expected_starts.iter().enumerate() {
        let matches: Vec<usize> = run
            .evaluations
            .iter()
            .enumerate()
            .filter_map(|(index, evaluation)| {
                same_point_bits(&evaluation.x, expected).then_some(index)
            })
            .collect();
        if matches.len() != 1 {
            return Err(format!(
                "restart[{restart}]-start-occurrences:{}!=1",
                matches.len()
            ));
        }
        boundaries.push(matches[0]);
    }
    if boundaries.first().copied() != Some(0) {
        return Err(format!(
            "first-restart-boundary:{:?}!=0",
            boundaries.first()
        ));
    }
    if boundaries.windows(2).any(|window| window[0] >= window[1]) {
        return Err(format!(
            "restart-boundaries-not-strictly-increasing:{boundaries:?}"
        ));
    }

    let mut slices = Vec::with_capacity(boundaries.len());
    let mut large_runs = 0u32;
    let mut small_budget_used = 0usize;
    let mut large_budget_used = 0usize;
    let base_lambda = expected_base_lambda();
    for (restart, (&start, &lambda)) in boundaries.iter().zip(&run.report.schedule).enumerate() {
        let end = boundaries
            .get(restart + 1)
            .copied()
            .unwrap_or(run.evaluations.len());
        let run_large = large_budget_used <= small_budget_used;
        let expected_lambda = if run_large {
            let multiplier = 1usize.checked_shl(large_runs).ok_or_else(|| {
                format!("restart[{restart}]-large-population-shift-overflow:{large_runs}")
            })?;
            base_lambda.checked_mul(multiplier).ok_or_else(|| {
                format!("restart[{restart}]-large-population-multiplication-overflow")
            })?
        } else {
            base_lambda
        };
        if lambda != expected_lambda {
            return Err(format!(
                "schedule[{restart}]={lambda}!=reconstructed-{expected_lambda}"
            ));
        }
        if start >= end {
            return Err(format!(
                "restart[{restart}]-empty-or-reversed-slice:{start}..{end}"
            ));
        }
        let run_evals = end - start;
        let remaining = TOTAL_BUDGET.checked_sub(start).ok_or_else(|| {
            format!("restart[{restart}]-start-{start}-exceeds-budget-{TOTAL_BUDGET}")
        })?;
        let nominal_budget = lambda
            .checked_mul(PER_RESTART_GENERATIONS)
            .ok_or_else(|| format!("restart[{restart}]-nominal-budget-overflow"))?;
        let admitted_budget = nominal_budget.min(remaining);
        if run_evals > admitted_budget {
            return Err(format!(
                "restart[{restart}]-evals-{run_evals}>admitted-budget-{admitted_budget}"
            ));
        }
        if (run_evals - 1) % lambda != 0 {
            return Err(format!(
                "restart[{restart}]-evals-{run_evals}-not-1-plus-whole-generations-of-{lambda}"
            ));
        }
        if admitted_budget >= 1 + lambda && run_evals < 1 + lambda {
            return Err(format!(
                "restart[{restart}]-omitted-admissible-first-generation"
            ));
        }
        let slice = RestartSlice { start, end, lambda };
        if !first_generation_matches(run, restart, slice, &expected_starts[restart]) {
            return Err(format!(
                "restart[{restart}]-first-generation-does-not-match-declared-stream"
            ));
        }
        slices.push(slice);

        if run_large {
            large_budget_used = large_budget_used
                .checked_add(run_evals)
                .ok_or_else(|| format!("restart[{restart}]-large-budget-overflow"))?;
            large_runs += 1;
        } else {
            small_budget_used = small_budget_used
                .checked_add(run_evals)
                .ok_or_else(|| format!("restart[{restart}]-small-budget-overflow"))?;
        }
        let has_next = restart + 1 < boundaries.len();
        if has_next && (end >= TOTAL_BUDGET || large_runs > LARGE_RUN_CAP_TRIGGER) {
            return Err(format!(
                "restart[{restart}]-schedule-continued-after-terminal-condition"
            ));
        }
    }
    if run.evaluations.len() < TOTAL_BUDGET && large_runs <= LARGE_RUN_CAP_TRIGGER {
        return Err(format!(
            "schedule-ended-before-budget-or-large-run-cap:evals={};large-runs={large_runs}",
            run.evaluations.len()
        ));
    }
    Ok(slices)
}

#[allow(clippy::too_many_lines)] // Complete trace and public-report accounting is the oracle.
fn accounting_mismatch(run: &StudyRun) -> Option<String> {
    if run.report.total_evals != run.evaluations.len() {
        return Some(format!(
            "reported-total-evals:{}!=closure-count:{}",
            run.report.total_evals,
            run.evaluations.len()
        ));
    }
    if !(1..=TOTAL_BUDGET).contains(&run.report.total_evals) {
        return Some(format!(
            "total-evals:{} not in 1..={TOTAL_BUDGET}",
            run.report.total_evals
        ));
    }
    if run.report.schedule.len() < 2 {
        return Some(format!(
            "schedule-too-short:{};fixture-must-exercise-a-restart",
            run.report.schedule.len()
        ));
    }
    if run.report.schedule.len() > run.report.total_evals {
        return Some(format!(
            "schedule-length:{}>total-evals:{}",
            run.report.schedule.len(),
            run.report.total_evals
        ));
    }
    let restart_slices = match reconstruct_restart_slices(run) {
        Ok(slices) => slices,
        Err(mismatch) => return Some(mismatch),
    };

    let best = &run.report.best;
    if best.x_best.len() != DIMENSION {
        return Some(format!(
            "best-point-dimension:{}!=expected-{DIMENSION}",
            best.x_best.len()
        ));
    }
    if best.x_best.iter().any(|value| !value.is_finite()) {
        return Some(format!(
            "best-point-non-finite:{:016x?}",
            best.x_best
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        ));
    }
    if !best.f_best.is_finite() || best.f_best <= F_TARGET {
        return Some(format!(
            "best-objective-invalid:0x{:016x};target=0x{:016x}",
            best.f_best.to_bits(),
            F_TARGET.to_bits()
        ));
    }
    if !(1..=run.report.total_evals).contains(&best.evals) {
        return Some(format!(
            "best-run-evals:{} not in 1..={}",
            best.evals, run.report.total_evals
        ));
    }
    if best.converged {
        return Some("impossible-target-was-reported-converged".to_string());
    }
    if !best.sigma.is_finite() || best.sigma <= 0.0 {
        return Some(format!(
            "best-run-invalid-sigma:0x{:016x}",
            best.sigma.to_bits()
        ));
    }

    let mut first_trace_best: Option<(usize, &Evaluation)> = None;
    for (evaluation_index, evaluation) in run.evaluations.iter().enumerate() {
        if evaluation.x.len() != DIMENSION {
            return Some(format!(
                "trace[{evaluation_index}]-dimension:{}!=expected-{DIMENSION}",
                evaluation.x.len()
            ));
        }
        if evaluation.x.iter().any(|value| !value.is_finite()) {
            return Some(format!(
                "trace[{evaluation_index}]-non-finite-point:{:016x?}",
                evaluation
                    .x
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            ));
        }
        if !evaluation.value.is_finite() {
            return Some(format!(
                "trace[{evaluation_index}]-non-finite-objective:0x{:016x}",
                evaluation.value.to_bits()
            ));
        }
        let oracle = shifted_rastrigin_oracle(&evaluation.x);
        let tolerance = objective_oracle_tolerance(evaluation.value, oracle);
        if !oracle.is_finite() || (evaluation.value - oracle).abs() > tolerance {
            return Some(format!(
                "trace[{evaluation_index}]-objective:recorded=0x{:016x};oracle=0x{:016x};tolerance=0x{:016x}",
                evaluation.value.to_bits(),
                oracle.to_bits(),
                tolerance.to_bits()
            ));
        }
        if first_trace_best.is_none_or(|(_, current)| evaluation.value < current.value) {
            first_trace_best = Some((evaluation_index, evaluation));
        }
    }
    let (first_trace_best_index, first_trace_best) =
        first_trace_best.expect("positive total-eval accounting makes trace nonempty");
    if first_trace_best.value.to_bits() != best.f_best.to_bits() {
        return Some(format!(
            "complete-trace-minimum=0x{:016x};reported-best=0x{:016x}",
            first_trace_best.value.to_bits(),
            best.f_best.to_bits()
        ));
    }
    let best_oracle = shifted_rastrigin_oracle(&best.x_best);
    let best_tolerance = objective_oracle_tolerance(best.f_best, best_oracle);
    if !best_oracle.is_finite() || (best.f_best - best_oracle).abs() > best_tolerance {
        return Some(format!(
            "best-point-objective:oracle=0x{:016x};reported=0x{:016x};tolerance=0x{:016x}",
            best_oracle.to_bits(),
            best.f_best.to_bits(),
            best_tolerance.to_bits()
        ));
    }
    if !same_point_bits(&first_trace_best.x, &best.x_best) {
        return Some("reported-best-is-not-the-first-stable-trace-minimum".to_string());
    }
    let Some((winning_restart, winning_slice)) =
        restart_slices.iter().enumerate().find(|(_, slice)| {
            slice.start <= first_trace_best_index && first_trace_best_index < slice.end
        })
    else {
        return Some(format!(
            "trace-minimum-index-{first_trace_best_index}-is-outside-restart-slices"
        ));
    };
    let winning_run_evals = winning_slice.end - winning_slice.start;
    let winning_generations = (winning_run_evals - 1) / winning_slice.lambda;
    if best.evals != winning_run_evals || best.generations != winning_generations {
        return Some(format!(
            "best-run-accounting:reported-evals-{}-generations-{}!=restart-{winning_restart}-evals-{winning_run_evals}-generations-{winning_generations}",
            best.evals, best.generations
        ));
    }
    None
}

#[allow(clippy::too_many_lines)] // Exhaustive field-by-field public-state audit.
fn first_public_mismatch(left: &StudyRun, right: &StudyRun) -> Option<String> {
    if left.input_seed != right.input_seed {
        return Some(format!(
            "input-seed:0x{:016x}!=0x{:016x}",
            left.input_seed, right.input_seed
        ));
    }
    if left.fixture.canonical_bytes() != right.fixture.canonical_bytes() {
        return Some("fixture-identity".to_string());
    }
    if left.report.total_evals != right.report.total_evals {
        return Some(format!(
            "total-evals:{}!={}",
            left.report.total_evals, right.report.total_evals
        ));
    }
    if left.report.schedule.len() != right.report.schedule.len() {
        return Some(format!(
            "schedule-length:{}!={}",
            left.report.schedule.len(),
            right.report.schedule.len()
        ));
    }
    for (restart, (&a, &b)) in left
        .report
        .schedule
        .iter()
        .zip(&right.report.schedule)
        .enumerate()
    {
        if a != b {
            return Some(format!("schedule[{restart}]:{a}!={b}"));
        }
    }

    let a = &left.report.best;
    let b = &right.report.best;
    if a.x_best.len() != b.x_best.len() {
        return Some(format!(
            "best.x-length:{}!={}",
            a.x_best.len(),
            b.x_best.len()
        ));
    }
    for (coordinate, (&x, &y)) in a.x_best.iter().zip(&b.x_best).enumerate() {
        if x.to_bits() != y.to_bits() {
            return Some(format!(
                "best.x[{coordinate}]:0x{:016x}!=0x{:016x}",
                x.to_bits(),
                y.to_bits()
            ));
        }
    }
    if a.f_best.to_bits() != b.f_best.to_bits() {
        return Some(format!(
            "best.f:0x{:016x}!=0x{:016x}",
            a.f_best.to_bits(),
            b.f_best.to_bits()
        ));
    }
    if a.evals != b.evals {
        return Some(format!("best.evals:{}!={}", a.evals, b.evals));
    }
    if a.generations != b.generations {
        return Some(format!(
            "best.generations:{}!={}",
            a.generations, b.generations
        ));
    }
    if a.converged != b.converged {
        return Some(format!("best.converged:{}!={}", a.converged, b.converged));
    }
    if a.sigma.to_bits() != b.sigma.to_bits() {
        return Some(format!(
            "best.sigma:0x{:016x}!=0x{:016x}",
            a.sigma.to_bits(),
            b.sigma.to_bits()
        ));
    }

    if left.evaluations.len() != right.evaluations.len() {
        return Some(format!(
            "trace-length:{}!={}",
            left.evaluations.len(),
            right.evaluations.len()
        ));
    }
    for (evaluation_index, (a, b)) in left.evaluations.iter().zip(&right.evaluations).enumerate() {
        if a.x.len() != b.x.len() {
            return Some(format!(
                "trace[{evaluation_index}].x-length:{}!={}",
                a.x.len(),
                b.x.len()
            ));
        }
        for (coordinate, (&x, &y)) in a.x.iter().zip(&b.x).enumerate() {
            if x.to_bits() != y.to_bits() {
                return Some(format!(
                    "trace[{evaluation_index}].x[{coordinate}]:0x{:016x}!=0x{:016x}",
                    x.to_bits(),
                    y.to_bits()
                ));
            }
        }
        if a.value.to_bits() != b.value.to_bits() {
            return Some(format!(
                "trace[{evaluation_index}].f:0x{:016x}!=0x{:016x}",
                a.value.to_bits(),
                b.value.to_bits()
            ));
        }
    }
    if left.result.canonical_bytes() != right.result.canonical_bytes() {
        return Some("result-identity".to_string());
    }
    None
}

fn validate_payload(run: &StudyRun) -> Result<(), AdmissionError> {
    let expected_fixture = fixture_identity(run.input_seed);
    if run.fixture.canonical_bytes() != expected_fixture.canonical_bytes() {
        return Err(AdmissionError::FixtureIdentityMismatch {
            declared: run.fixture.root(),
            computed: expected_fixture.root(),
        });
    }
    let computed = result_identity(&run.fixture, &run.report, &run.evaluations);
    if computed.canonical_bytes() == run.result.canonical_bytes() {
        Ok(())
    } else {
        Err(AdmissionError::PayloadIdentityMismatch {
            declared: run.result.root(),
            computed: computed.root(),
        })
    }
}

fn admit_against(run: &StudyRun, reference: &ReplayIdentity) -> Result<(), AdmissionError> {
    validate_payload(run)?;
    if run.result.canonical_bytes() == reference.canonical_bytes() {
        Ok(())
    } else {
        Err(AdmissionError::ReferenceIdentityMismatch {
            expected: reference.root(),
            found: run.result.root(),
        })
    }
}

fn exact_returned_bit_delta(reference: &StudyRun, mutant: &StudyRun, mutation: Mutation) -> bool {
    let Some(mask) = 1u64.checked_shl(mutation.mantissa_bit) else {
        return false;
    };
    let Some(reference_coordinate) = reference.report.best.x_best.get(mutation.coordinate) else {
        return false;
    };
    let Some(mutant_coordinate) = mutant.report.best.x_best.get(mutation.coordinate) else {
        return false;
    };
    if reference_coordinate.to_bits() != mutation.before
        || mutant_coordinate.to_bits() != mutation.after
        || mutation.before ^ mutation.after != mask
    {
        return false;
    }

    let mut expected = reference.clone();
    expected.report.best.x_best[mutation.coordinate] = f64::from_bits(mutation.after);
    expected.result = result_identity(&expected.fixture, &expected.report, &expected.evaluations);
    first_public_mismatch(&expected, mutant).is_none()
}

fn schedule_json(schedule: &[usize]) -> String {
    let mut json = String::from("[");
    for (index, lambda) in schedule.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        write!(&mut json, "{lambda}").expect("String writes are infallible");
    }
    json.push(']');
    json
}

fn emit_green_receipt(run: &StudyRun) {
    let mut emitter = Emitter::new(SUITE, CASE);
    let event = emitter.emit(
        Severity::Info,
        EventKind::Custom {
            name: "bipop-cma-full-study-replay-receipt".to_string(),
            json: format!(
                concat!(
                    "{{\"fixture_identity\":\"{}\",\"result_identity\":\"{}\",",
                    "\"algorithm\":\"fs_dfo::bipop_cmaes\",\"objective\":\"shifted-rastrigin\",",
                    "\"semantic_oracle\":\"{}\",\"units\":\"dimensionless\",",
                    "\"input_seed\":{},\"corruption_seed\":{},\"dimension\":{},",
                    "\"total_budget\":{},\"total_evals\":{},\"schedule\":{},",
                    "\"best\":{{\"x_len\":{},\"f_bits\":\"0x{:016x}\",",
                    "\"evals\":{},\"generations\":{},\"converged\":{},",
                    "\"sigma_bits\":\"0x{:016x}\"}},\"trace_len\":{},",
                    "\"stream_semantics_version\":{},\"versions\":{{",
                    "\"fs_dfo\":\"{}\",\"fs_la\":\"{}\",\"fs_math\":\"{}\",",
                    "\"fs_rand\":\"{}\",\"fs_obs\":\"{}\"}},",
                    "\"no_claims\":[\"optimizer-quality\",\"all-objectives\",",
                    "\"all-dimensions\",\"all-budgets\",\"all-seeds\",",
                    "\"cross-ISA-equality\",\"cancellation\",\"checkpointing\",",
                    "\"performance\"]}}"
                ),
                run.fixture.hex(),
                run.result.hex(),
                SEMANTIC_ORACLE_VERSION,
                run.input_seed,
                CORRUPTION_SEED,
                DIMENSION,
                TOTAL_BUDGET,
                run.report.total_evals,
                schedule_json(&run.report.schedule),
                run.report.best.x_best.len(),
                run.report.best.f_best.to_bits(),
                run.report.best.evals,
                run.report.best.generations,
                run.report.best.converged,
                run.report.best.sigma.to_bits(),
                run.evaluations.len(),
                fs_rand::STREAM_SEMANTICS_VERSION,
                fs_dfo::VERSION,
                fs_la::VERSION,
                fs_math::VERSION,
                fs_rand::VERSION,
                fs_obs::VERSION,
            ),
        },
        None,
    );
    let line = event.to_jsonl();
    fs_obs::validate_line(&line).expect("BIPOP study receipt must use the fs-obs wire schema");
    let receipt = event.content_identity_receipt();
    event
        .admit_content_identity(&receipt)
        .expect("fresh retained event identity must admit exactly");
    println!("{line}");
}

fn green_verdict_detail(run: &StudyRun) -> String {
    format!(
        "fixture={}; result={}; total_evals={}; restarts={}; trace=bit-exact; public_report=fully-bound; semantic_oracle={SEMANTIC_ORACLE_VERSION}",
        run.fixture.hex(),
        run.result.hex(),
        run.report.total_evals,
        run.report.schedule.len()
    )
}

fn emit_green_verdict(run: &StudyRun) -> Event {
    let detail = green_verdict_detail(run);
    let mut emitter = Emitter::new(SUITE, format!("{CASE}/verdict"));
    let event = emitter.emit(
        Severity::Info,
        EventKind::ConformanceCase {
            suite: SUITE.to_string(),
            case: CASE.to_string(),
            pass: true,
            detail,
            seed: run.input_seed,
        },
        None,
    );
    fs_obs::lint_failure_record(&event).expect("BIPOP study verdict must be replayable");
    let line = event.to_jsonl();
    fs_obs::validate_line(&line).expect("BIPOP study verdict must use the fs-obs wire schema");
    println!("{line}");
    event
}

fn failure_event(detail: &str, corruption_seed: u64) -> Event {
    let mut emitter = Emitter::new(SUITE, RED_CASE);
    emitter.emit(
        Severity::Error,
        EventKind::ConformanceCase {
            suite: SUITE.to_string(),
            case: RED_CASE.to_string(),
            pass: false,
            detail: detail.to_string(),
            seed: corruption_seed,
        },
        None,
    )
}

fn assert_mergeable(run: &StudyRun, reference: &ReplayIdentity, event: &Event) {
    let EventKind::ConformanceCase {
        suite,
        case,
        pass,
        detail,
        seed,
    } = &event.kind
    else {
        panic!("merge gate accepts only ConformanceCase evidence");
    };
    if let Err(error) = admit_against(run, reference) {
        panic!("merge gate refused {case}: {error:?}; {detail}");
    }
    if let Some(mismatch) = accounting_mismatch(run) {
        panic!("merge gate refused {case}: semantic mismatch {mismatch}; {detail}");
    }
    assert_eq!(
        event.session.as_str(),
        SUITE,
        "merge gate refused an event from the wrong session"
    );
    let expected_scope = format!("{CASE}/verdict");
    assert_eq!(
        event.scope.as_str(),
        expected_scope.as_str(),
        "merge gate refused an event from the wrong scope"
    );
    assert_eq!(
        event.seq, 0,
        "merge gate requires the canonical verdict slot"
    );
    assert_eq!(
        event.severity,
        Severity::Info,
        "merge gate requires an informational green verdict"
    );
    assert!(
        event.wall_ns.is_none(),
        "merge gate requires deterministic evidence without a wall-clock envelope"
    );
    assert_eq!(suite.as_str(), SUITE, "merge gate refused the wrong suite");
    assert_eq!(case.as_str(), CASE, "merge gate refused the wrong case");
    assert_eq!(
        *seed, run.input_seed,
        "merge gate refused a verdict with the wrong causal input seed"
    );
    let expected_detail = green_verdict_detail(run);
    assert_eq!(
        detail.as_str(),
        expected_detail.as_str(),
        "merge gate refused a verdict that does not name the admitted run"
    );
    assert!(*pass, "merge gate refused {case}: {detail}");
}

fn assert_resealed_semantic_refusal(
    mut mutant: StudyRun,
    event: &Event,
    expected_mismatch_fragment: &str,
) {
    mutant.result = result_identity(&mutant.fixture, &mutant.report, &mutant.evaluations);
    validate_payload(&mutant).expect("resealed semantic mutant must be identity-consistent");
    let panic = catch_unwind(|| {
        assert_mergeable(&mutant, &mutant.result, event);
    })
    .expect_err("resealed semantic mutant must fail admission against its own identity");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("semantic-refusal panic carries text");
    assert!(message.contains("semantic mismatch"), "{message}");
    assert!(message.contains(expected_mismatch_fragment), "{message}");
}

fn seeded_corruption(canonical: &StudyRun, seed: u64) -> SeededCorruption {
    let coordinate =
        usize::try_from(seed % usize_u64(DIMENSION)).expect("corruption coordinate fits usize");
    let mantissa_bit = u32::try_from((seed >> 32) & 0x1f).expect("corruption bit fits u32");

    let mut run = canonical.clone();
    let before = run.report.best.x_best[coordinate].to_bits();
    let after = before ^ (1_u64 << mantissa_bit);
    run.report.best.x_best[coordinate] = f64::from_bits(after);
    assert!(run.report.best.x_best[coordinate].is_finite());
    let stale_error = validate_payload(&run).expect_err("unsealed result mutation must refuse");
    run.result = result_identity(&run.fixture, &run.report, &run.evaluations);
    validate_payload(&run).expect("resealed mutation must be internally self-consistent");
    let reference_error = admit_against(&run, &canonical.result)
        .expect_err("resealed semantic mutation must not match the retained reference");

    let mismatch = first_public_mismatch(canonical, &run)
        .expect("the disclosed mutation must change public replay state");
    SeededCorruption {
        run,
        mutation: Mutation {
            seed,
            coordinate,
            mantissa_bit,
            before,
            after,
        },
        stale_error,
        reference_error,
        mismatch,
    }
}

fn corruption_detail(canonical: &StudyRun, corruption: &SeededCorruption) -> String {
    format!(
        "input_seed=0x{:016x}; corruption_seed=0x{:016x}; fixture={}; coordinate={}; mantissa_bit={}; before=0x{:016x}; after=0x{:016x}; stale_gate={:?}; reference_gate={:?}; first_mismatch={}; canonical={}; corrupted={}",
        canonical.input_seed,
        corruption.mutation.seed,
        canonical.fixture.hex(),
        corruption.mutation.coordinate,
        corruption.mutation.mantissa_bit,
        corruption.mutation.before,
        corruption.mutation.after,
        corruption.stale_error,
        corruption.reference_error,
        corruption.mismatch,
        canonical.result.hex(),
        corruption.run.result.hex()
    )
}

fn exercise_disclosed_corruption(canonical: &StudyRun, replay: &StudyRun) {
    let first_corruption = seeded_corruption(canonical, CORRUPTION_SEED);
    let replay_corruption = seeded_corruption(replay, CORRUPTION_SEED);
    assert_eq!(
        (
            first_corruption.mutation.coordinate,
            first_corruption.mutation.mantissa_bit
        ),
        (1, 30)
    );
    assert!(
        first_corruption.mismatch.starts_with("best.x[1]"),
        "unexpected mismatch: {}",
        first_corruption.mismatch
    );
    assert_eq!(first_corruption.mutation, replay_corruption.mutation);
    assert_eq!(first_corruption.stale_error, replay_corruption.stale_error);
    assert_eq!(
        first_corruption.reference_error,
        replay_corruption.reference_error
    );
    assert_eq!(first_corruption.mismatch, replay_corruption.mismatch);
    assert!(exact_returned_bit_delta(
        canonical,
        &first_corruption.run,
        first_corruption.mutation
    ));
    assert!(exact_returned_bit_delta(
        replay,
        &replay_corruption.run,
        replay_corruption.mutation
    ));
    assert!(matches!(
        first_corruption.stale_error,
        AdmissionError::PayloadIdentityMismatch { declared, computed }
            if declared == canonical.result.root()
                && computed == first_corruption.run.result.root()
    ));
    assert!(matches!(
        first_corruption.reference_error,
        AdmissionError::ReferenceIdentityMismatch { expected, found }
            if expected == canonical.result.root()
                && found == first_corruption.run.result.root()
    ));
    assert_eq!(validate_payload(&first_corruption.run), Ok(()));
    assert!(matches!(
        admit_against(&first_corruption.run, &canonical.result),
        Err(AdmissionError::ReferenceIdentityMismatch { expected, found })
            if expected == canonical.result.root()
                && found == first_corruption.run.result.root()
    ));
    assert_ne!(
        first_corruption.mutation.before,
        first_corruption.mutation.after
    );
    assert!(f64::from_bits(first_corruption.mutation.after).is_finite());
    assert_ne!(canonical.result.root(), first_corruption.run.result.root());
    assert_ne!(replay.result.root(), replay_corruption.run.result.root());
    assert_eq!(
        first_public_mismatch(&first_corruption.run, &replay_corruption.run),
        None,
        "the corruption seed must independently reproduce the complete red state"
    );
    assert_eq!(
        first_corruption.run.result.canonical_bytes(),
        replay_corruption.run.result.canonical_bytes()
    );

    let first_detail = corruption_detail(canonical, &first_corruption);
    let replay_detail = corruption_detail(replay, &replay_corruption);
    assert_eq!(first_detail, replay_detail);
    let first_event = failure_event(&first_detail, first_corruption.mutation.seed);
    let replay_event = failure_event(&replay_detail, replay_corruption.mutation.seed);
    for event in [&first_event, &replay_event] {
        fs_obs::lint_failure_record(event)
            .expect("disclosed BIPOP corruption must retain its replay seed and detail");
        fs_obs::validate_line(&event.to_jsonl())
            .expect("disclosed BIPOP corruption must remain wire-valid");
    }
    assert_eq!(
        first_event, replay_event,
        "independent seeded red evidence construction replays"
    );
    assert_eq!(
        first_event.content_identity().canonical_bytes(),
        replay_event.content_identity().canonical_bytes()
    );
    let retained = first_event.content_identity_receipt();
    first_event
        .admit_content_identity(&retained)
        .expect("red evidence identity must admit exactly");
    println!("{}", first_event.to_jsonl());

    let panic = catch_unwind(|| {
        assert_mergeable(&first_corruption.run, &canonical.result, &first_event);
    })
    .expect_err("the merge gate must reject the disclosed returned-bit corruption");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("merge-gate panic carries text");
    assert!(message.contains(RED_CASE));
    assert!(message.contains(&format!("0x{CORRUPTION_SEED:016x}")));
    assert!(message.contains("best.x[1]"));
    assert!(message.contains("ReferenceIdentityMismatch"));

    let semantic_panic = catch_unwind(|| {
        assert_mergeable(
            &first_corruption.run,
            &first_corruption.run.result,
            &first_event,
        );
    })
    .expect_err("a resealed mutant must fail semantic admission even against itself");
    let semantic_message = semantic_panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| semantic_panic.downcast_ref::<&str>().copied())
        .expect("semantic merge-gate panic carries text");
    assert!(semantic_message.contains(RED_CASE));
    assert!(semantic_message.contains("semantic mismatch"));
}

#[test]
fn bipop_full_study_replays_and_seeded_failure_is_refused() {
    let first = run_study(INPUT_SEED);
    let replay = run_study(INPUT_SEED);
    let first_accounting = accounting_mismatch(&first);
    let replay_accounting = accounting_mismatch(&replay);
    assert_eq!(first_accounting, None, "original accounting failed");
    assert_eq!(replay_accounting, None, "replay accounting failed");
    assert_eq!(validate_payload(&first), Ok(()));
    assert_eq!(validate_payload(&replay), Ok(()));
    assert_eq!(admit_against(&first, &first.result), Ok(()));
    assert_eq!(admit_against(&replay, &first.result), Ok(()));

    let mismatch = first_public_mismatch(&first, &replay);
    assert_eq!(
        mismatch, None,
        "same-seed study must replay every public bit"
    );
    assert_eq!(first.fixture.root(), replay.fixture.root());
    assert_eq!(first.result.root(), replay.result.root());
    assert_eq!(
        first.result.canonical_bytes(),
        replay.result.canonical_bytes(),
        "the retained complete result frame must replay byte-for-byte"
    );

    emit_green_receipt(&first);
    let green_verdict = emit_green_verdict(&first);
    assert_mergeable(&first, &first.result, &green_verdict);

    let mut wrong_seed_verdict = green_verdict.clone();
    let EventKind::ConformanceCase { seed, .. } = &mut wrong_seed_verdict.kind else {
        unreachable!("green verdict constructor always returns ConformanceCase evidence");
    };
    *seed ^= 1;
    let panic = catch_unwind(|| {
        assert_mergeable(&first, &first.result, &wrong_seed_verdict);
    })
    .expect_err("a green verdict with the wrong causal seed must not merge");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("verdict-binding panic carries text");
    assert!(message.contains("wrong causal input seed"));

    let mut objective_mutant = first.clone();
    objective_mutant.evaluations[0].value += 0.25;
    assert_resealed_semantic_refusal(objective_mutant, &green_verdict, "trace[0]-objective");

    let mut schedule_mutant = first.clone();
    schedule_mutant.report.schedule[0] = schedule_mutant.report.schedule[0]
        .checked_mul(2)
        .expect("fixture schedule mutation fits usize");
    assert_resealed_semantic_refusal(schedule_mutant, &green_verdict, "schedule[0]");

    let mut causal_seed_mutant = first.clone();
    causal_seed_mutant.input_seed ^= 1;
    assert!(matches!(
        validate_payload(&causal_seed_mutant),
        Err(AdmissionError::FixtureIdentityMismatch { declared, computed })
            if declared == first.fixture.root()
                && computed == fixture_identity(causal_seed_mutant.input_seed).root()
    ));
    causal_seed_mutant.fixture = fixture_identity(causal_seed_mutant.input_seed);
    assert_resealed_semantic_refusal(causal_seed_mutant, &green_verdict, "restart[");

    exercise_disclosed_corruption(&first, &replay);
}
