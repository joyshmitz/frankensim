//! Accept/reject economics conformance (bead lmp4.3, feature
//! `certified-speculation`). Outright accepts, warm starts measured
//! with the worse-than-cold clamp, drift injection with localized
//! demotion and no-flap hysteresis, conservative zero-telemetry
//! priors, deterministic decisions, and the kernel × regime dashboard.
//! JSON-line verdicts; seeded cases carry seeds.

use fs_verify::economics::{DriftGuard, EconDecision, run_speculative, solve_node_record};
use fs_verify::fem1d::{MmsProblem, Poly, solve_p1};
use fs_verify::zoo::{
    NeighborExtrapolation, Proposal, Proposer, Registry, SpeculationQuery, ZooTelemetry,
};

fn verdict(case: &str, pass: bool, detail: &str) {
    println!(
        "{{\"suite\":\"fs-verify/economics\",\"case\":\"{case}\",\"verdict\":\"{}\",\
         \"detail\":\"{detail}\"}}",
        if pass { "pass" } else { "fail" }
    );
    assert!(pass, "case {case}: {detail}");
}

fn family(theta: f64) -> Poly {
    Poly(vec![0.0, -theta, 1.0 + theta, -1.0])
}

/// The amplified family (×40): the u³ term dominates, so Newton
/// iteration counts genuinely separate cold from warm starts (the
/// monotone cubic keeps Newton globally convergent, just slower from
/// far away).
fn family_big(theta: f64) -> Poly {
    Poly(vec![0.0, -40.0 * theta, 40.0 * (1.0 + theta), -40.0])
}

fn uniform(n: usize) -> Vec<f64> {
    (0..=n).map(|i| i as f64 / n as f64).collect()
}

fn query(theta: f64, n: usize, tol: f64, regime: &str) -> SpeculationQuery {
    SpeculationQuery {
        problem: MmsProblem::new("family", family(theta), uniform(n)),
        theta,
        tolerance: tol,
        regime: regime.to_string(),
    }
}

/// A proposer good near its cache, garbage far from it (the drift
/// fixture: regime B sits far away).
struct RegimeBound {
    cache_theta: f64,
    solution: Vec<f64>,
}

impl Proposer for RegimeBound {
    fn name(&self) -> &'static str {
        "regime-bound"
    }

    fn propose(&self, q: &SpeculationQuery) -> Option<Proposal> {
        if (q.theta - self.cache_theta).abs() < 0.15 {
            Some(Proposal {
                candidate: self.solution.clone(),
                confidence: 0.9,
            })
        } else {
            // Out of its depth: confidently wrong.
            Some(Proposal {
                candidate: self.solution.iter().map(|v| v * 25.0).collect(),
                confidence: 0.9,
            })
        }
    }
}

/// A proposer whose candidate is WORSE than a cold start (antithetical
/// warm start: huge wrong values slow Newton down).
struct Antithetical;

impl Proposer for Antithetical {
    fn name(&self) -> &'static str {
        "antithetical"
    }

    fn propose(&self, q: &SpeculationQuery) -> Option<Proposal> {
        Some(Proposal {
            candidate: q.problem.mesh.iter().map(|_| 50.0).collect(),
            confidence: 0.5,
        })
    }
}

/// econ-001 — outright accepts ship with their bound and no solve;
/// warm starts measure savings; the ledger record carries the four
/// solve-node fields.
#[test]
fn econ_001_outright_and_warm() {
    let n = 32;
    let solved = solve_p1(&MmsProblem::new("f", family_big(0.4), uniform(n)));
    let mut reg = Registry::new();
    reg.register(Box::new(NeighborExtrapolation {
        cache: vec![(0.4, solved, None)],
    }));
    let mut zt = ZooTelemetry::default();
    let mut guard = DriftGuard::default();
    // Near the cache at loose tolerance: outright accept.
    let q1 = SpeculationQuery {
        problem: MmsProblem::new("family", family_big(0.42), uniform(n)),
        theta: 0.42,
        tolerance: 1.0,
        regime: "wedge".to_string(),
    };
    let d1 = run_speculative(&q1, &reg, &mut zt, &mut guard, 50);
    let outright = matches!(&d1, EconDecision::AcceptedOutright { proposer, bound }
        if *proposer == "neighbor-extrapolation" && *bound <= 1.0);
    // Tight tolerance: rejected → warm start, savings measured > 0.
    let q2 = SpeculationQuery {
        problem: MmsProblem::new("family", family_big(0.42), uniform(n)),
        theta: 0.42,
        tolerance: 1e-9,
        regime: "wedge".to_string(),
    };
    let d2 = run_speculative(&q2, &reg, &mut zt, &mut guard, 50);
    let warm_saved = matches!(&d2, EconDecision::WarmStarted { saved, cold, warm, .. }
        if *saved > 0 && warm < cold);
    // The solve-node record carries all four fields.
    let row = solve_node_record("neighbor-extrapolation", false, 3.2e-4, 4);
    let fields = row.contains("proposer_id")
        && row.contains("\"accepted\":false")
        && row.contains("bound")
        && row.contains("iterations_saved");
    verdict(
        "econ-001",
        outright && warm_saved && fields,
        &format!(
            "the near-cache query accepts OUTRIGHT with its certified bound (no \
             solve at all); the tight-tolerance query warm-starts and MEASURES its \
             savings ({d2:?}); the solve-node record carries all four schema \
             fields: {row}"
        ),
    );
}

/// econ-002 — the worse-than-cold clamp: an antithetical warm start
/// records ZERO savings (never a win) while the raw negative delta is
/// preserved for the ledger.
#[test]
fn econ_002_worse_than_cold_clamps() {
    let mut reg = Registry::new();
    reg.register(Box::new(Antithetical));
    let mut zt = ZooTelemetry::default();
    let mut guard = DriftGuard::default();
    let q = query(0.4, 24, 1e-9, "wedge");
    let d = run_speculative(&q, &reg, &mut zt, &mut guard, 200);
    let clamped = matches!(&d, EconDecision::WarmStarted { saved, raw_delta, .. }
        if *saved == 0 && *raw_delta < 0);
    verdict(
        "econ-002",
        clamped,
        &format!(
            "the antithetical candidate makes Newton slower than cold; recorded \
             savings clamp to 0 (never a win) while the raw delta stays negative \
             for the ledger: {d:?}"
        ),
    );
}

/// econ-003 — drift injection with LOCALIZED demotion: the
/// regime-bound proposer collapses only in the far regime, is demoted
/// THERE, and stays active near its cache.
#[test]
fn econ_003_drift_localized() {
    let n = 32;
    let solved = solve_p1(&MmsProblem::new("f", family(0.3), uniform(n)));
    let mut reg = Registry::new();
    reg.register(Box::new(RegimeBound {
        cache_theta: 0.3,
        solution: solved,
    }));
    let mut zt = ZooTelemetry::default();
    let mut guard = DriftGuard::default();
    // Regime A (near): accepts. Regime B (far): garbage → rejects.
    for k in 0..12 {
        let ta = 0.3 + 0.01 * f64::from(k % 3);
        let qa = query(ta, n, 1e-1, "regime-a");
        let _ = run_speculative(&qa, &reg, &mut zt, &mut guard, 50);
        let tb = 0.7 + 0.01 * f64::from(k % 3);
        let qb = query(tb, n, 1e-1, "regime-b");
        let _ = run_speculative(&qb, &reg, &mut zt, &mut guard, 50);
    }
    let demotions = guard.check_drift(0.2, 10);
    let localized = demotions.len() == 1
        && demotions[0] == ("regime-bound".to_string(), "regime-b".to_string())
        && guard.is_demoted("regime-bound", "regime-b")
        && !guard.is_demoted("regime-bound", "regime-a");
    let rate_a = guard.accept_rate_or_prior("regime-bound", "regime-a");
    let rate_b = guard.accept_rate_or_prior("regime-bound", "regime-b");
    verdict(
        "econ-003",
        localized && rate_a > 0.8 && rate_b == 0.0,
        &format!(
            "the accept-rate collapse is a distribution-shift alarm with correct \
             LOCALIZATION: demoted in regime-b (rate {rate_b:.2}) and untouched in \
             regime-a (rate {rate_a:.2})"
        ),
    );
}

/// econ-004 — hysteresis: a single unlucky reject cannot demote (min
/// samples); a failed probation doubles the evidence bar (no
/// flapping); a genuinely recovered proposer re-promotes.
#[test]
fn econ_004_hysteresis_no_flap() {
    let mut guard = DriftGuard::default();
    // One unlucky reject: no demotion (min_tries = 10).
    guard.record("p", "r", false);
    let none = guard.check_drift(0.2, 10).is_empty();
    // Collapse with enough samples: demoted.
    for _ in 0..11 {
        guard.record("p", "r", false);
    }
    let demoted = !guard.check_drift(0.2, 10).is_empty() && guard.is_demoted("p", "r");
    // Failed probation (insufficient probe rate): STAYS demoted and
    // the next probation needs double the evidence.
    let fail1 = !guard.probation("p", "r", 1, 5, 0.5);
    let still = guard.is_demoted("p", "r");
    // A 5-try probe is no longer enough after one failure (needs 10).
    let fail2 = !guard.probation("p", "r", 5, 5, 0.5);
    // Two failures → the bar doubles twice (needs 20); a genuine
    // recovery with that evidence re-promotes.
    let promoted = guard.probation("p", "r", 18, 20, 0.5) && !guard.is_demoted("p", "r");
    // And the reset window means it is not instantly re-demoted.
    let stable = guard.check_drift(0.2, 10).is_empty();
    verdict(
        "econ-004",
        none && demoted && fail1 && still && fail2 && promoted && stable,
        "one unlucky reject cannot demote; collapse with samples does; a failed \
         probation doubles the next evidence bar (no flapping); genuine recovery \
         re-promotes with a reset window that prevents instant re-demotion",
    );
}

/// econ-005 — conservative priors, determinism, and the dashboard: a
/// zero-telemetry regime reports rate 0.0 (never optimism, never a
/// divide-by-zero); identical runs give identical decisions and rows;
/// the kernel × regime dashboard ships via fs-obs.
#[test]
fn econ_005_priors_determinism_dashboard() {
    let prior = DriftGuard::default().accept_rate_or_prior("nobody", "nowhere");
    let conservative = prior == 0.0;
    let run = || -> (Vec<String>, String) {
        let n = 32;
        let solved = solve_p1(&MmsProblem::new("f", family(0.4), uniform(n)));
        let mut reg = Registry::new();
        reg.register(Box::new(NeighborExtrapolation {
            cache: vec![(0.4, solved, None)],
        }));
        let mut zt = ZooTelemetry::default();
        let mut guard = DriftGuard::default();
        let mut decisions = Vec::new();
        for k in 0..6 {
            let q = query(
                0.38 + 0.02 * f64::from(k % 3),
                n,
                if k % 2 == 0 { 1e-1 } else { 1e-9 },
                "wedge",
            );
            let d = run_speculative(&q, &reg, &mut zt, &mut guard, 50);
            decisions.push(format!("{d:?}"));
        }
        (decisions, guard.dashboard("poisson-1d").join(","))
    };
    let (d1, rows1) = run();
    let (d2, rows2) = run();
    let deterministic = d1 == d2 && rows1 == rows2;
    let mut em = fs_obs::Emitter::new("fs-verify/economics", "econ-005/dashboard");
    let line = em
        .emit(
            fs_obs::Severity::Info,
            fs_obs::EventKind::Custom {
                name: "speculation-economics-dashboard".to_string(),
                json: format!("[{rows1}]"),
            },
            None,
        )
        .to_jsonl();
    fs_obs::validate_line(&line).expect("dashboard validates");
    println!("{line}");
    let dashboard_ok = rows1.contains("\"kernel\":\"poisson-1d\"")
        && rows1.contains("median_savings")
        && rows1.contains("rate");
    verdict(
        "econ-005",
        conservative && deterministic && dashboard_ok,
        "zero-telemetry regimes report the conservative 0.0 prior, identical runs \
         give identical decisions and dashboard rows, and the kernel x regime x \
         proposer dashboard ships via fs-obs",
    );
}
