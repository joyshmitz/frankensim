//! End-to-end battery for the CertQD-Thrust campaign: the physics self-propels,
//! the campaign illuminates a certified diverse family under certify-or-escalate,
//! the no-laundering color algebra holds, and the whole run is deterministic.

use fs_evidence::ColorRank;
use fs_thrust_e2e::{CampaignBudget, Design, run_campaign, simulate_thrust};

#[test]
fn a_four_vortex_thruster_self_propels_and_conserves_impulse() {
    // physics sanity: a leading dipole with a weak trailing pair drifts +x, and
    // a converged inviscid sim conserves the exact linear impulse.
    let d = Design {
        gamma: 1.0,
        d: 0.6,
        l: 1.5,
        ratio: 0.3,
    };
    let res = simulate_thrust(&d, 300, 0.02, 0.05);
    assert!(
        res.drift > 0.1,
        "thruster should self-propel +x, drift={}",
        res.drift
    );
    assert!(
        res.impulse_error < 1e-2,
        "impulse leaked: {}",
        res.impulse_error
    );
}

#[test]
fn the_campaign_illuminates_a_certified_diverse_family() {
    let report = run_campaign(&CampaignBudget::default());
    // surface the headline numbers FIRST (structured, for the run log).
    println!(
        "{{\"campaign\":\"certqd-thrust\",\"niches\":{},\"coverage\":{:.3},\"qd_score\":{:.3},\
         \"best_drift\":{:.3},\"best\":{:?},\"verified\":{},\"estimated\":{},\"full_sims\":{},\
         \"short_sims\":{},\"steps_spent\":{},\"steps_all_full\":{},\"step_savings\":{:.3},\
         \"band\":{:.4},\"envelope\":{:?}}}",
        report.num_elites,
        report.coverage,
        report.qd_score,
        report.best_drift,
        report.best,
        report.verified_elites,
        report.estimated_elites,
        report.full_sims,
        report.short_sims,
        report.steps_spent,
        report.steps_all_full,
        1.0 - report.steps_spent as f64 / report.steps_all_full as f64,
        report.band_half_width,
        report.certified_envelope,
    );
    for e in &report.atlas {
        println!(
            "ATLAS {:.4} {:.4} {:.5} {}",
            e.budget, e.length, e.drift, e.verified
        );
    }
    // ILLUMINATION: a diverse archive of niches, not a single optimum.
    assert!(
        report.num_elites >= 5,
        "too few niches: {}",
        report.num_elites
    );
    assert!(report.coverage > 0.0 && report.qd_score > 0.0);
    // the best thruster genuinely self-propels.
    assert!(report.best_drift > 0.0, "best drift {}", report.best_drift);
    // CERTIFY-OR-ESCALATE: both fidelities were used, and it beat all-full cost.
    assert!(report.short_sims > 0, "surrogate never used");
    assert!(report.full_sims > 0, "nothing escalated");
    assert!(
        report.steps_spent < report.steps_all_full,
        "no savings: {} vs {}",
        report.steps_spent,
        report.steps_all_full
    );
    // CERTIFICATES: escalated sims that conserved impulse earned Verified bands.
    assert!(report.verified_elites > 0, "no verified elites");
    assert_eq!(
        report.verified_elites + report.estimated_elites,
        report.num_elites
    );
    let (lo, hi) = report.certified_envelope.expect("a verified elite exists");
    assert!(lo <= hi, "envelope [{lo}, {hi}]");
    // NO LAUNDERING: with surrogate (Estimated) elites present, the campaign
    // claim cannot outrank Estimated.
    assert!(report.estimated_elites > 0);
    assert_eq!(report.campaign_rank, ColorRank::Estimated);
    // the reproducible notebook carries the story and is content-addressed.
    assert!(report.notebook_markdown.contains("CertQD-Thrust"));
    assert!(report.notebook_markdown.contains("best_drift"));
    assert_ne!(report.content_hash, 0);
}

#[test]
fn the_campaign_is_deterministic() {
    let a = run_campaign(&CampaignBudget::default());
    let b = run_campaign(&CampaignBudget::default());
    assert_eq!(a.content_hash, b.content_hash);
    assert_eq!(a.num_elites, b.num_elites);
    assert!((a.qd_score - b.qd_score).abs() < 1e-12);
    assert_eq!(a.best.gamma.to_bits(), b.best.gamma.to_bits());
}

#[test]
fn malformed_conservation_tolerance_cannot_mint_verified_elites() {
    // Keep the public-path witness cheap while forcing every design through a
    // full simulation. Before the fail-closed guard, +infinity authorized every
    // finite conservation ratio and produced false Verified archive entries.
    let budget = CampaignBudget {
        full_steps: 2,
        short_steps: 1,
        bins: 2,
        alpha: 0.1,
        decision_tol: 0.0,
        conserve_tol: f64::INFINITY,
        ..CampaignBudget::default()
    };
    let report = run_campaign(&budget);
    assert!(report.full_sims > 0);
    assert_eq!(report.verified_elites, 0);
    assert_eq!(report.estimated_elites, report.num_elites);
    assert_eq!(report.certified_envelope, None);
    assert_eq!(report.campaign_rank, ColorRank::Estimated);
}
