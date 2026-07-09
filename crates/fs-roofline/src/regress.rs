//! PERF-REGRESSION CI (plan §14.4, bead fz2.4): performance
//! regressions are TEST FAILURES. This module is the statistics and
//! diagnosis layer over the roofline harness: DISPERSION-AWARE
//! tolerance bands (thermal jitter must not cry wolf), CUSUM
//! change-point alarms with calibrated thresholds, flame-graph-
//! equivalent attribution from the phase-annotated event stream (a
//! regression arrives WITH its own diagnosis), and the dashboard
//! one-liners ("what got slower this month, and why").

use std::collections::BTreeMap;

/// One nightly observation of a kernel: attainment plus its
/// phase-annotated event stream (phase name → seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct Night {
    /// Nightly index (logical time).
    pub night: u64,
    /// Roofline attainment in [0, 1]-ish.
    pub attainment: f64,
    /// Phase durations (the flame-graph-equivalent source).
    pub phases: BTreeMap<String, f64>,
}

/// The dispersion-aware gate: a run fails when attainment drops more
/// than `k_sigma` baseline standard deviations below the baseline
/// mean — the statistical band, not a naive threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateSpec {
    /// Band width in baseline sigmas.
    pub k_sigma: f64,
    /// Minimum baseline nights before the gate arms.
    pub min_baseline: usize,
}

impl Default for GateSpec {
    fn default() -> Self {
        GateSpec {
            k_sigma: 4.0,
            min_baseline: 8,
        }
    }
}

fn mean_std(xs: &[f64]) -> (f64, f64) {
    #[allow(clippy::cast_precision_loss)]
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n.max(1.0);
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1.0).max(1.0);
    (mean, var.sqrt())
}

/// The gate verdict for the newest night against its baseline.
#[derive(Debug, Clone, PartialEq)]
pub enum GateVerdict {
    /// Within the band (or the gate is not yet armed).
    Green {
        /// Standardized score of the newest night.
        z: f64,
    },
    /// RED: the regression, with its own diagnosis attached.
    Red {
        /// Standardized drop (negative).
        z: f64,
        /// The flame-graph-level attribution: phases ranked by their
        /// share growth vs the last-green baseline (top offender
        /// first), as (phase, baseline share, regressed share).
        attribution: Vec<(String, f64, f64)>,
    },
}

/// Gate the newest night against the preceding baseline, attributing
/// any red to the phases whose SHARE of the total grew most — the
/// event stream reconstructing the flame-graph diff post hoc.
#[must_use]
pub fn gate(history: &[Night], spec: GateSpec) -> GateVerdict {
    let n = history.len();
    if n < spec.min_baseline + 1 {
        return GateVerdict::Green { z: 0.0 };
    }
    let (baseline, newest) = history.split_at(n - 1);
    let newest = &newest[0];
    let xs: Vec<f64> = baseline.iter().map(|b| b.attainment).collect();
    let (mu, sigma) = mean_std(&xs);
    let z = (newest.attainment - mu) / sigma.max(1e-12);
    if z >= -spec.k_sigma {
        return GateVerdict::Green { z };
    }
    // Attribution: per-phase SHARE of total time, baseline median vs
    // the regressed night; rank by share growth.
    let mut base_shares: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for night in baseline {
        let total: f64 = night.phases.values().sum();
        for (phase, secs) in &night.phases {
            base_shares
                .entry(phase.as_str())
                .or_default()
                .push(secs / total.max(1e-12));
        }
    }
    let new_total: f64 = newest.phases.values().sum();
    let mut attribution: Vec<(String, f64, f64)> = newest
        .phases
        .iter()
        .map(|(phase, secs)| {
            let new_share = secs / new_total.max(1e-12);
            let base = base_shares.get(phase.as_str()).map_or(0.0, |v| {
                let mut s = v.clone();
                s.sort_by(f64::total_cmp);
                s[s.len() / 2]
            });
            (phase.clone(), base, new_share)
        })
        .collect();
    attribution.sort_by(|a, b| (b.2 - b.1).total_cmp(&(a.2 - a.1)).then(a.0.cmp(&b.0)));
    GateVerdict::Red { z, attribution }
}

/// A one-sided CUSUM change-point detector for slow drifts the
/// per-night gate misses: alarm when the cumulative standardized
/// shortfall crosses `h`. `k` is the slack (drift smaller than k·σ per
/// night is absorbed — the noise-robustness knob).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cusum {
    /// Slack per observation (in sigmas).
    pub k: f64,
    /// Alarm threshold (in cumulative sigmas).
    pub h: f64,
}

impl Default for Cusum {
    fn default() -> Self {
        Cusum { k: 0.5, h: 8.0 }
    }
}

impl Cusum {
    /// Run over standardized residuals (baseline-calibrated z-scores);
    /// returns the first alarm index, if any.
    #[must_use]
    pub fn first_alarm(&self, z_scores: &[f64]) -> Option<usize> {
        let mut s = 0.0f64;
        for (i, &z) in z_scores.iter().enumerate() {
            s = (s - z - self.k).max(0.0); // accumulate SHORTFALL
            if s > self.h {
                return Some(i);
            }
        }
        None
    }
}

/// Standardize a history against its own expanding baseline (each
/// night scored against the nights before it; the first `warmup`
/// nights score 0).
#[must_use]
pub fn standardize(history: &[f64], warmup: usize) -> Vec<f64> {
    history
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            if i < warmup {
                0.0
            } else {
                let (mu, sigma) = mean_std(&history[..i]);
                (x - mu) / sigma.max(1e-12)
            }
        })
        .collect()
}

/// THE DASHBOARD ONE-LINER: "what got slower this month, and why" —
/// kernels whose trailing-week mean attainment dropped more than
/// `pct_floor` percent below their opening-week mean, each with its
/// top-offender phase from the flame-graph diff.
#[must_use]
pub fn slower_this_month(
    kernels: &BTreeMap<String, Vec<Night>>,
    pct_floor: f64,
) -> Vec<(String, f64, String)> {
    let mut out = Vec::new();
    for (kernel, history) in kernels {
        if history.len() < 14 {
            continue;
        }
        let head: Vec<f64> = history[..7].iter().map(|n| n.attainment).collect();
        let tail: Vec<f64> = history[history.len() - 7..]
            .iter()
            .map(|n| n.attainment)
            .collect();
        let (mu_head, _) = mean_std(&head);
        let (mu_tail, _) = mean_std(&tail);
        let drop_pct = (mu_head - mu_tail) / mu_head.max(1e-12) * 100.0;
        if drop_pct > pct_floor {
            // Top offender via the gate's attribution machinery.
            let verdict = gate(
                history,
                GateSpec {
                    k_sigma: 0.0,
                    min_baseline: 7,
                },
            );
            let why = match verdict {
                GateVerdict::Red { attribution, .. } => attribution
                    .first()
                    .map_or_else(|| "unattributed".to_string(), |(p, _, _)| p.clone()),
                GateVerdict::Green { .. } => "trend-only (no single-night red)".to_string(),
            };
            out.push((kernel.clone(), drop_pct, why));
        }
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}
