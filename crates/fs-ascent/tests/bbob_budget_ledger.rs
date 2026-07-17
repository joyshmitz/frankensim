//! Bead 7tv.21.5: BBOB-class ERT-style budget ledger.
//!
//! nfev is MACHINE-INDEPENDENT given seeds and options, so these rows
//! CI-gate optimizer regressions with no perf lane: "an optimizer
//! regression is a perf regression". Stochastic families use the COCO
//! ERT estimator (total nfev across seeded runs ÷ number of successes),
//! so failed seeds honestly inflate the budget instead of vanishing;
//! deterministic DFO engines report single-run nfev.
//!
//! Every kernel asserts (1) its success gate (all seeds for the
//! separable/unimodal fixtures; ≥ 3/5 documented for DE on Rastrigin —
//! see the measured seed-1 finding below), (2) ERT/nfev at or under a
//! pinned ceiling (measured value + ~30% headroom, recorded per row),
//! and (3) an nfev sanity floor (vacuous-success guard). Rows are
//! emitted as fs-obs `BenchmarkResult` events (`machine: 0` =
//! machine-independent) and wire-validated — the ledger seed for trend
//! tracking.
//!
//! MEASURED FINDINGS kept on record:
//! - fsci `differential_evolution` under default options on Rastrigin-2
//!   over its CANONICAL domain [-5.12, 5.12]² converges to the
//!   neighboring local well (f ≈ 0.99496 at (0, −0.995)) on seed 1 and
//!   stays there at any maxiter; seeds 2–5 reach the origin (4/5).
//!   Shrinking the domain to [-5, 5]² drops that to 2/5 — the domain is
//!   part of the fixture identity, so the canonical bounds are pinned.
//! - fsci Powell on Rosenbrock-5 from the shared start stalls at
//!   f = 3.93e-8 (nfev 2194) independent of tol/maxiter; its target is
//!   therefore 1e-6, not the 1e-8 the simplex engines reach.

use fsci_opt::{
    DifferentialEvolutionOptions, MinimizeOptions, OptimizeMethod, differential_evolution,
    minimize, rosen,
};

fn sphere5(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum()
}

fn rastrigin2(x: &[f64]) -> f64 {
    20.0 + x
        .iter()
        .map(|&v| v * v - 10.0 * (2.0 * core::f64::consts::PI * v).cos())
        .sum::<f64>()
}

/// One ledger row: pinned ceiling + measured budget, emitted and gated.
struct Row {
    kernel: &'static str,
    /// ERT for stochastic kernels; single-run nfev for deterministic.
    budget: usize,
    ceiling: usize,
    successes: usize,
    attempts: usize,
    min_successes: usize,
}

fn ledger_and_gate(rows: &[Row]) {
    let mut em = fs_obs::Emitter::new("fs-ascent/bbob-budget", "ledger-v1");
    for row in rows {
        assert!(
            row.successes >= row.min_successes,
            "{}: {}/{} successes is below the documented gate of {}",
            row.kernel,
            row.successes,
            row.attempts,
            row.min_successes
        );
        assert!(
            row.budget > 10,
            "{}: {} evals is vacuous — sanity floor",
            row.kernel,
            row.budget
        );
        assert!(
            row.budget <= row.ceiling,
            "{}: budget {} exceeds the pinned ceiling {} — optimizer regression",
            row.kernel,
            row.budget,
            row.ceiling
        );
        for (metric, value) in [
            ("ert_nfev", row.budget as f64),
            ("success_rate", row.successes as f64 / row.attempts as f64),
        ] {
            let event = em.emit(
                fs_obs::Severity::Info,
                fs_obs::EventKind::BenchmarkResult {
                    kernel: row.kernel.to_string(),
                    metric: metric.to_string(),
                    value,
                    machine: 0, // machine-independent by construction
                },
                None,
            );
            let line = event.to_jsonl();
            fs_obs::validate_line(&line).expect("budget rows stay wire-valid");
            println!("{line}");
        }
    }
}

#[test]
fn de_family_ert_rows_hold_their_ceilings() {
    // Seeded DE, DEFAULT options, canonical per-fixture domains, seeds
    // 1..=5. ERT = total nfev / successes (COCO estimator). Ceilings are
    // the measured ERT + ~30%: sphere5 6645 -> 8700; rosen5 21810 ->
    // 28500; rastrigin2 1898 (7590/4, seed-1 local trap on record) ->
    // 2500.
    let mut rows = Vec::new();
    for (kernel, f, n, bound, target, min_successes, ceiling) in [
        (
            "de/sphere5",
            &sphere5 as &(dyn Fn(&[f64]) -> f64),
            5usize,
            5.0f64,
            1e-8,
            5usize,
            8_700usize,
        ),
        (
            "de/rosen5",
            &(|x: &[f64]| rosen(x)),
            5,
            5.0,
            1e-8,
            5,
            28_500,
        ),
        ("de/rastrigin2", &rastrigin2, 2, 5.12, 1e-6, 3, 2_500),
    ] {
        let bounds = vec![(-bound, bound); n];
        let mut total_nfev = 0usize;
        let mut successes = 0usize;
        for seed in 1u64..=5 {
            let res = differential_evolution(
                f,
                &bounds,
                DifferentialEvolutionOptions {
                    seed: Some(seed),
                    ..DifferentialEvolutionOptions::default()
                },
            )
            .expect("DE runs");
            total_nfev += res.nfev;
            if f(&res.x) < target {
                successes += 1;
            }
        }
        rows.push(Row {
            kernel,
            budget: total_nfev / successes.max(1),
            ceiling,
            successes,
            attempts: 5,
            min_successes,
        });
    }
    ledger_and_gate(&rows);
}

#[test]
fn dfo_family_budget_rows_hold_their_ceilings() {
    // Deterministic DFO engines from fixed starts; single-run nfev.
    // Success is judged on f(res.x) directly (some engines return
    // fun: None). Ceilings = measured nfev + ~30%, recorded per row.
    let start5 = [2.0f64, -1.5, 1.0, -0.5, 2.5];
    let start2 = [0.4f64, -0.3]; // rastrigin: global-basin start (DFO is local)
    let mut rows = Vec::new();
    for (kernel, f, x0, target, ceiling) in [
        (
            "nelder-mead/sphere5",
            &sphere5 as &(dyn Fn(&[f64]) -> f64),
            &start5[..],
            1e-8,
            1_100usize,
        ),
        (
            "nelder-mead/rosen5",
            &(|x: &[f64]| rosen(x)),
            &start5[..],
            1e-8,
            2_600,
        ),
        (
            "nelder-mead/rastrigin2-local",
            &rastrigin2,
            &start2[..],
            1e-6,
            400,
        ),
        ("powell/sphere5", &sphere5, &start5[..], 1e-8, 700),
        // Target 1e-6: Powell stalls at f = 3.93e-8 here (see module doc).
        (
            "powell/rosen5",
            &(|x: &[f64]| rosen(x)),
            &start5[..],
            1e-6,
            2_900,
        ),
    ] {
        let method = if kernel.starts_with("powell") {
            OptimizeMethod::Powell
        } else {
            OptimizeMethod::NelderMead
        };
        let res = minimize(
            f,
            x0,
            MinimizeOptions {
                method: Some(method),
                tol: Some(1e-12),
                maxiter: Some(20_000),
                ..MinimizeOptions::default()
            },
        )
        .expect("DFO engine runs");
        let reached = f(&res.x) < target;
        rows.push(Row {
            kernel,
            budget: res.nfev,
            ceiling,
            successes: usize::from(reached),
            attempts: 1,
            min_successes: 1,
        });
    }
    ledger_and_gate(&rows);
}
