//! Proposer-zoo conformance (bead lmp4.2, feature
//! `certified-speculation`). The type-level safety invariant, advisory
//! confidence in both directions, neighbor extrapolation with and
//! without warm adjoints (plus the equidistant tie-break), coarse-rung
//! prolongation with the fp16 precision-discipline demo, the
//! adversarial-surrogate falsifier with zero incorrect accepts and
//! auto-demotion, and the end-to-end economics loop with ledger rows.
//! JSON-line verdicts; seeded cases carry seeds.

use fs_verify::estimator::verify;
use fs_verify::fem1d::{MmsProblem, Poly, solve_p1, true_energy_error};
use fs_verify::zoo::{
    CoarseRungProlongation, NeighborExtrapolation, Outcome, Proposal, Proposer, Registry,
    SpeculationQuery, ZooTelemetry, quantize_f16, speculate,
};

fn verdict(case: &str, pass: bool, detail: &str) {
    println!(
        "{{\"suite\":\"fs-verify/zoo\",\"case\":\"{case}\",\"verdict\":\"{}\",\
         \"detail\":\"{detail}\"}}",
        if pass { "pass" } else { "fail" }
    );
    assert!(pass, "case {case}: {detail}");
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// The parameterized design family: u(x; θ) = x(1−x)(x−θ).
fn family(theta: f64) -> Poly {
    // (x − x²)(x − θ) = −θx + (1+θ)x² − x³
    Poly(vec![0.0, -theta, 1.0 + theta, -1.0])
}

fn uniform(n: usize) -> Vec<f64> {
    (0..=n).map(|i| i as f64 / n as f64).collect()
}

fn query(theta: f64, n: usize, tol: f64) -> SpeculationQuery {
    SpeculationQuery {
        problem: MmsProblem::new("family", family(theta), uniform(n)),
        theta,
        tolerance: tol,
        regime: "wedge-v0".to_string(),
    }
}

/// A deliberately adversarial surrogate: garbage with max confidence.
struct AdversarialSurrogate;

impl Proposer for AdversarialSurrogate {
    fn name(&self) -> &'static str {
        "adversarial-surrogate"
    }

    fn propose(&self, q: &SpeculationQuery) -> Option<Proposal> {
        Some(Proposal {
            candidate: q.problem.mesh.iter().map(|x| (x * 941.0).sin()).collect(),
            confidence: 1.0,
        })
    }
}

/// A good proposer that self-reports NaN confidence (the advisory
/// property: it must still be tried and accepted when it verifies).
struct HumbleGood;

impl Proposer for HumbleGood {
    fn name(&self) -> &'static str {
        "humble-good"
    }

    fn propose(&self, q: &SpeculationQuery) -> Option<Proposal> {
        Some(Proposal {
            candidate: solve_p1(&q.problem),
            confidence: f64::NAN,
        })
    }
}

/// zoo-001 — the interface + THE SAFETY INVARIANT: answers exist only
/// through the verifier; empty registries are honest misses;
/// confidence is advisory in BOTH directions.
#[test]
fn zoo_001_interface_and_safety() {
    let mut telemetry = ZooTelemetry::default();
    // Empty registry: honest NoCandidates.
    let empty = Registry::new();
    let q = query(0.45, 16, 2e-1);
    let none = matches!(speculate(&q, &empty, &mut telemetry), Outcome::NoCandidates);
    // Register/deregister round-trip.
    let mut reg = Registry::new();
    reg.register(Box::new(CoarseRungProlongation));
    reg.register(Box::new(AdversarialSurrogate));
    let has_two = reg.names().len() == 2;
    reg.deregister("adversarial-surrogate");
    let has_one = reg.names() == vec!["coarse-rung-prolongation"];
    // The accepted answer carries a VERIFIED color and its bound; the
    // only constructor is the verifier's yes.
    let out = speculate(&q, &reg, &mut telemetry);
    let sound = match &out {
        Outcome::Accepted(ans) => {
            ans.report().accept
                && matches!(
                    ans.report().color,
                    Some(fs_evidence::Color::Verified { .. })
                )
                && ans.report().bound.hi <= q.tolerance
        }
        _ => false,
    };
    // Advisory-NaN: the humble-good proposer (NaN confidence) is tried
    // LAST but still accepted when the noisy one fails.
    let mut reg2 = Registry::new();
    reg2.register(Box::new(HumbleGood));
    reg2.register(Box::new(AdversarialSurrogate));
    let tight = query(0.45, 16, 5e-2);
    let out2 = speculate(&tight, &reg2, &mut telemetry);
    let nan_still_wins = matches!(&out2, Outcome::Accepted(ans) if ans.proposer() == "humble-good");
    verdict(
        "zoo-001",
        none && has_two && has_one && sound && nan_still_wins,
        "empty registries miss honestly, registration hot-swaps, accepted answers \
         carry the verifier's bound and VERIFIED color (no other constructor \
         exists), and a NaN-confidence good proposer is ordered last yet still \
         accepted — confidence is advisory in both directions",
    );
}

/// zoo-002 — neighbor extrapolation: warm adjoints beat zeroth-order
/// measurably; equidistant neighbors tie-break deterministically to
/// the smaller θ; verified accepts at honest tolerances.
#[test]
fn zoo_002_neighbor_extrapolation() {
    let n = 32;
    // Certified cache at θ ∈ {0.2, 0.5, 0.8} with FD sensitivities.
    let solved = |th: f64| solve_p1(&MmsProblem::new("f", family(th), uniform(n)));
    let sens = |th: f64| -> Vec<f64> {
        let h = 1e-4;
        let (up, dn) = (solved(th + h), solved(th - h));
        up.iter()
            .zip(&dn)
            .map(|(a, b)| (a - b) / (2.0 * h))
            .collect()
    };
    let cache_warm: Vec<(f64, Vec<f64>, Option<Vec<f64>>)> = [0.2, 0.5, 0.8]
        .iter()
        .map(|&t| (t, solved(t), Some(sens(t))))
        .collect();
    let cache_cold: Vec<(f64, Vec<f64>, Option<Vec<f64>>)> = cache_warm
        .iter()
        .map(|(t, u, _)| (*t, u.clone(), None))
        .collect();
    let q = query(0.45, n, 1e-1);
    let warm = NeighborExtrapolation { cache: cache_warm }
        .propose(&q)
        .expect("warm");
    let cold = NeighborExtrapolation { cache: cache_cold }
        .propose(&q)
        .expect("cold");
    let warm_err = true_energy_error(&q.problem, &warm.candidate);
    let cold_err = true_energy_error(&q.problem, &cold.candidate);
    let warm_wins = warm_err < 0.5 * cold_err;
    // Both still pass the verifier at a loose tolerance (graceful
    // degradation to zeroth-order remains USEFUL).
    let warm_accepts = verify(&q.problem, &warm.candidate, 1e-1).accept;
    let cold_accepts = verify(&q.problem, &cold.candidate, 2e-1).accept;
    // Equidistant tie: θ = 0.35 sits exactly between 0.2 and 0.5; the
    // rule picks the SMALLER θ, deterministically.
    let qt = query(0.35, n, 1e-1);
    let cache2: Vec<(f64, Vec<f64>, Option<Vec<f64>>)> =
        [0.2, 0.5].iter().map(|&t| (t, solved(t), None)).collect();
    let pick1 = NeighborExtrapolation {
        cache: cache2.clone(),
    }
    .propose(&qt)
    .expect("tie 1");
    let pick2 = NeighborExtrapolation { cache: cache2 }
        .propose(&qt)
        .expect("tie 2");
    let tie_deterministic = pick1.candidate == pick2.candidate && pick1.candidate == solved(0.2); // zeroth-order from θ=0.2
    verdict(
        "zoo-002",
        warm_wins && warm_accepts && cold_accepts && tie_deterministic,
        &format!(
            "the warm adjoint cuts extrapolation error to {warm_err:.1e} vs \
             zeroth-order {cold_err:.1e} (>2x), both degrade gracefully into \
             verified accepts at honest tolerances, and the equidistant tie \
             resolves deterministically to the smaller theta"
        ),
    );
}

/// zoo-003 — coarse-rung prolongation + the PRECISION DISCIPLINE:
/// accepts at loose tolerance, rejects honestly at tight tolerance,
/// and an fp16-quantized candidate still verifies — speculate LOW,
/// verify HIGH.
#[test]
fn zoo_003_coarse_rung_and_precision() {
    let q_loose = query(0.3, 32, 1e-1);
    let prop = CoarseRungProlongation.propose(&q_loose).expect("coarse");
    let loose = verify(&q_loose.problem, &prop.candidate, 1e-1);
    let tight = verify(&q_loose.problem, &prop.candidate, 1e-6);
    // fp16 quantization: the proposer's precision is nobody's business.
    let quantized: Vec<f64> = prop.candidate.iter().map(|&v| quantize_f16(v)).collect();
    let q_accept = verify(&q_loose.problem, &quantized, 1e-1);
    // Tiny meshes have no coarser rung: honest decline.
    let q_small = query(0.3, 3, 1e-1);
    let declines = CoarseRungProlongation.propose(&q_small).is_none();
    verdict(
        "zoo-003",
        loose.accept && !tight.accept && q_accept.accept && declines,
        &format!(
            "the prolongated coarse solve accepts at 1e-1 (bound {:.2e}) and rejects \
             honestly at 1e-6; the fp16-QUANTIZED candidate still accepts (speculate \
             low, verify high — the certificate inherits the VERIFIER's precision); \
             mesh-too-small declines honestly",
            loose.bound.hi
        ),
    );
}

/// zoo-004 — THE FALSIFIER: an adversarial surrogate never lands a
/// single incorrect accept over the battery, its accept-rate collapse
/// AUTO-DEMOTES it in the regime, and demoted proposers stop being
/// consulted.
#[test]
fn zoo_004_adversarial_falsifier() {
    let mut telemetry = ZooTelemetry::default();
    let mut reg = Registry::new();
    reg.register(Box::new(AdversarialSurrogate));
    reg.register(Box::new(CoarseRungProlongation));
    let mut rng = Lcg(0x1001_2026_0707_00A4);
    let mut incorrect_accepts = 0u32;
    for _ in 0..25 {
        let theta = 0.2 + 0.6 * rng.unit();
        let q = query(theta, 32, 5e-2);
        match speculate(&q, &reg, &mut telemetry) {
            Outcome::Accepted(ans) => {
                // Cross-check the accept against the oracle: an
                // accepted bound must dominate the true error.
                let truth = true_energy_error(&q.problem, ans.candidate());
                if truth > ans.report().bound.hi * (1.0 + 1e-9) {
                    incorrect_accepts += 1;
                }
                if ans.proposer() == "adversarial-surrogate" {
                    incorrect_accepts += 1; // garbage must never verify
                }
            }
            Outcome::AllRejected { .. } | Outcome::NoCandidates => {}
        }
    }
    let adv_rate = telemetry
        .accept_rate("adversarial-surrogate", "wedge-v0")
        .expect("tried");
    let demotions = telemetry.demote_collapsed(0.05, 10);
    let demoted = telemetry.is_demoted("adversarial-surrogate", "wedge-v0");
    // After demotion the adversary is not consulted (tries frozen).
    let tries_before = 25;
    let q = query(0.5, 32, 5e-2);
    let _ = speculate(&q, &reg, &mut telemetry);
    let frozen = telemetry
        .accept_rate("adversarial-surrogate", "wedge-v0")
        .is_some()
        && telemetry.rows().iter().any(|r| {
            r.contains("adversarial-surrogate") && r.contains(&format!("\"tries\":{tries_before}"))
        });
    verdict(
        "zoo-004",
        incorrect_accepts == 0 && adv_rate == 0.0 && !demotions.is_empty() && demoted && frozen,
        &format!(
            "zero incorrect accepts over 25 adversarial-first speculations (the \
             verifier gates everything), the adversary's accept rate is {adv_rate:.2}, \
             the collapse AUTO-DEMOTES it in the regime, and demoted proposers stop \
             being consulted; seed 0x1001_2026_0707_00A4"
        ),
    );
}

/// zoo-005 — the economics loop end-to-end: a mixed registry over a
/// seeded query stream stays sound, telemetry rows ship to the ledger,
/// and the accept-rate ordering matches proposer quality.
#[test]
fn zoo_005_economics_loop() {
    let n = 32;
    let solved = |th: f64| solve_p1(&MmsProblem::new("f", family(th), uniform(n)));
    let cache: Vec<(f64, Vec<f64>, Option<Vec<f64>>)> = [0.2, 0.4, 0.6, 0.8]
        .iter()
        .map(|&t| (t, solved(t), None))
        .collect();
    let mut reg = Registry::new();
    reg.register(Box::new(NeighborExtrapolation { cache }));
    reg.register(Box::new(CoarseRungProlongation));
    reg.register(Box::new(AdversarialSurrogate));
    let mut telemetry = ZooTelemetry::default();
    let mut rng = Lcg(0x1001_2026_0707_00A5);
    let mut accepted = 0u32;
    for _ in 0..30 {
        let theta = 0.25 + 0.5 * rng.unit();
        let q = query(theta, n, 8e-2);
        match speculate(&q, &reg, &mut telemetry) {
            Outcome::Accepted(ans) => {
                accepted += 1;
                assert!(ans.report().accept, "type invariant");
            }
            Outcome::AllRejected { .. } | Outcome::NoCandidates => {}
        }
    }
    let rows = telemetry.rows();
    let mut em = fs_obs::Emitter::new("fs-verify/zoo", "zoo-005/economics");
    let line = em
        .emit(
            fs_obs::Severity::Info,
            fs_obs::EventKind::Custom {
                name: "speculation-accept-rates".to_string(),
                json: format!("[{}]", rows.join(",")),
            },
            None,
        )
        .to_jsonl();
    fs_obs::validate_line(&line).expect("economics rows validate");
    println!("{line}");
    let adv = telemetry
        .accept_rate("adversarial-surrogate", "wedge-v0")
        .unwrap_or(1.0);
    let good_beats_bad = telemetry
        .accept_rate("neighbor-extrapolation", "wedge-v0")
        .into_iter()
        .chain(telemetry.accept_rate("coarse-rung-prolongation", "wedge-v0"))
        .any(|r| r > adv);
    verdict(
        "zoo-005",
        accepted > 15 && adv == 0.0 && good_beats_bad,
        &format!(
            "{accepted}/30 speculations accepted with certified bounds, the \
             adversary landed nothing, honest proposers out-rate it, and the \
             per-proposer-per-regime rows ship to the ledger; \
             seed 0x1001_2026_0707_00A5"
        ),
    );
}
