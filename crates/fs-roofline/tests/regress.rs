//! Perf-regression-CI conformance (the fz2.4 bead): gate arithmetic,
//! change-point calibration on synthetic series (zero false alarms at
//! the declared confidence), seeded-regression attribution (the red
//! arrives WITH its flame-graph-level diagnosis), and the dashboard
//! one-liner answering the canonical question in one call.

use std::collections::BTreeMap;

use fs_roofline::regress::{Cusum, GateSpec, GateVerdict, gate, slower_this_month, standardize};

fn verdict(case: &str, detail: &str) {
    println!(
        "{{\"suite\":\"fs-roofline/regress\",\"case\":\"{case}\",\"verdict\":\"pass\",\
         \"detail\":\"{detail}\"}}"
    );
}

fn unit(seed: u64, k: u64) -> f64 {
    let mut z = seed ^ 0x9e37_79b9_7f4a_7c15u64.wrapping_mul(k + 1);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

fn gauss(seed: u64, k: u64) -> f64 {
    (0..12).map(|j| unit(seed, k * 12 + j)).sum::<f64>() - 6.0
}

/// A stable kernel: attainment ~ N(0.72, 0.01) with steady phases.
fn stable_night(night: u64, seed: u64) -> fs_roofline::regress::Night {
    let mut phases = BTreeMap::new();
    phases.insert(
        "assemble".to_string(),
        0.30 + 0.005 * gauss(seed, night * 3),
    );
    phases.insert(
        "solve".to_string(),
        0.55 + 0.005 * gauss(seed, night * 3 + 1),
    );
    phases.insert(
        "reduce".to_string(),
        0.15 + 0.003 * gauss(seed, night * 3 + 2),
    );
    fs_roofline::regress::Night {
        night,
        attainment: 0.72 + 0.01 * gauss(seed, night * 7 + 5),
        phases,
    }
}

#[test]
fn rg_001_noise_robustness_zero_false_alarms() {
    // 60 nights of stable code x 20 independent kernels: ZERO alarms
    // from the gate AND the CUSUM at the declared settings — thermal
    // jitter must not cry wolf.
    let mut gate_alarms = 0usize;
    let mut cusum_alarms = 0usize;
    for kernel in 0..20u64 {
        let history: Vec<_> = (0..60).map(|n| stable_night(n, 0xace + kernel)).collect();
        for t in 10..60 {
            if let GateVerdict::Red { .. } = gate(&history[..=t], GateSpec::default()) {
                gate_alarms += 1;
            }
        }
        let xs: Vec<f64> = history.iter().map(|n| n.attainment).collect();
        let z = standardize(&xs, 8);
        if Cusum::default().first_alarm(&z).is_some() {
            cusum_alarms += 1;
        }
    }
    println!(
        "{{\"metric\":\"false-alarms\",\"nights\":60,\"kernels\":20,\
         \"gate_alarms\":{gate_alarms},\"cusum_alarms\":{cusum_alarms}}}"
    );
    assert_eq!(gate_alarms, 0, "the 4-sigma band never cries wolf");
    assert_eq!(cusum_alarms, 0, "the CUSUM never cries wolf on stable code");
    verdict(
        "rg-001",
        "20 kernels x 60 stable nights: zero gate alarms and zero CUSUM alarms at the \
         declared settings — dispersion-aware bands hold",
    );
}

#[test]
fn rg_002_seeded_regression_red_with_attribution() {
    // Night 30 de-tunes the SOLVE phase (2x slower): the gate must go
    // RED with `solve` as the top attribution — the regression arrives
    // with its own diagnosis.
    let mut history: Vec<_> = (0..30).map(|n| stable_night(n, 0xbead)).collect();
    let mut bad = stable_night(30, 0xbead);
    bad.attainment = 0.48; // the de-tuned kernel's roofline drop
    *bad.phases.get_mut("solve").expect("solve") *= 2.0;
    history.push(bad);
    let verdict_ = gate(&history, GateSpec::default());
    let GateVerdict::Red { z, attribution } = verdict_ else {
        panic!("the seeded regression must gate red: {verdict_:?}")
    };
    println!(
        "{{\"metric\":\"seeded-regression\",\"z\":{z:.1},\"top\":\"{}\",\
         \"shares\":[{:.3},{:.3}]}}",
        attribution[0].0, attribution[0].1, attribution[0].2
    );
    assert!(z < -4.0, "far outside the band: z = {z:.1}");
    assert_eq!(
        attribution[0].0, "solve",
        "the flame-graph diff names the phase"
    );
    assert!(
        attribution[0].2 > attribution[0].1 + 0.1,
        "the share growth is visible: {:.3} -> {:.3}",
        attribution[0].1,
        attribution[0].2
    );
    verdict(
        "rg-002",
        "the de-tuned solve phase gates red at z < -4 with `solve` ranked first in the \
         flame-graph-equivalent attribution",
    );
}

#[test]
fn rg_003_cusum_catches_the_slow_drift() {
    // A 0.3-sigma-per-night drift never trips the single-night gate
    // but MUST trip the CUSUM within the month — the complementary
    // detector pair.
    let mut xs: Vec<f64> = (0..20).map(|n| 0.72 + 0.01 * gauss(0xd1f7, n)).collect();
    for n in 20..50u64 {
        let drift = 0.003 * (n - 19) as f64;
        xs.push(0.72 - drift + 0.01 * gauss(0xd1f7, n));
    }
    let z = standardize(&xs, 8);
    let single_night_reds = z.iter().skip(10).filter(|&&v| v < -4.0).count();
    let alarm = Cusum::default().first_alarm(&z);
    println!(
        "{{\"metric\":\"drift\",\"single_night_reds\":{single_night_reds},\
         \"cusum_alarm_at\":{alarm:?}}}"
    );
    let at = alarm.expect("the CUSUM catches the drift");
    assert!(at < 45, "caught within the month: night {at}");
    verdict(
        "rg-003",
        "a 0.3-sigma/night drift trips the CUSUM mid-month — the change-point detector \
         covers what the single-night band cannot",
    );
}

#[test]
fn rg_004_dashboard_one_liner() {
    // Three kernels: one regressed (12% drop, reduce-phase bloat), two
    // stable. The canonical question answers in ONE call, ranked, with
    // the why attached.
    let mut kernels = BTreeMap::new();
    kernels.insert(
        "gemm".to_string(),
        (0..30).map(|n| stable_night(n, 1)).collect::<Vec<_>>(),
    );
    kernels.insert(
        "spmv".to_string(),
        (0..30).map(|n| stable_night(n, 2)).collect::<Vec<_>>(),
    );
    let mut regressed: Vec<_> = (0..30).map(|n| stable_night(n, 3)).collect();
    for night in regressed.iter_mut().skip(20) {
        night.attainment *= 0.86;
        *night.phases.get_mut("reduce").expect("reduce") *= 3.0;
    }
    kernels.insert("fft".to_string(), regressed);
    let report = slower_this_month(&kernels, 5.0);
    println!("{{\"metric\":\"dashboard\",\"report\":{report:?}}}");
    assert_eq!(report.len(), 1, "only the regressed kernel is named");
    assert_eq!(report[0].0, "fft");
    assert!(
        report[0].1 > 10.0,
        "the drop percentage is right: {:.1}",
        report[0].1
    );
    assert_eq!(report[0].2, "reduce", "and the why names the bloated phase");
    verdict(
        "rg-004",
        "'what got slower this month, and why' answers in one call: fft, ~13%, reduce — \
         stable kernels stay unnamed",
    );
}
