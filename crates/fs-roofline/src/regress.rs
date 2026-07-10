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
    /// MALFORMED EVIDENCE (bead fz2.4.1): non-finite or negative
    /// inputs, or an unusable spec. A proof-bearing gate never
    /// represents bad data as Green — it says so, with a diagnosis.
    Invalid {
        /// What was malformed (structured, human-readable).
        reason: String,
    },
}

/// First flaw in a night's fields, if any (the fail-closed screen).
fn night_flaw(idx: usize, night: &Night) -> Option<String> {
    if !night.attainment.is_finite() || night.attainment < 0.0 {
        return Some(format!(
            "night {idx} (index in history): attainment {} is not finite and non-negative",
            night.attainment
        ));
    }
    for (phase, &secs) in &night.phases {
        if !secs.is_finite() || secs < 0.0 {
            return Some(format!(
                "night {idx}: phase '{phase}' duration {secs} is not finite and non-negative"
            ));
        }
    }
    None
}

/// First flaw in a spec, if any.
fn spec_flaw(spec: GateSpec) -> Option<String> {
    if !(spec.k_sigma.is_finite() && spec.k_sigma > 0.0) {
        return Some(format!(
            "spec.k_sigma {} is not finite and positive",
            spec.k_sigma
        ));
    }
    if spec.min_baseline < 2 {
        return Some(format!(
            "spec.min_baseline {} cannot support a dispersion estimate (need >= 2)",
            spec.min_baseline
        ));
    }
    None
}

/// Phase-share attribution of `newest` against `baseline` (the
/// flame-graph diff reconstructed post hoc): phases ranked by share
/// growth vs the baseline median share, top offender first.
fn attribution_vs_baseline(baseline: &[Night], newest: &Night) -> Vec<(String, f64, f64)> {
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
    attribution
}

/// Gate the newest night against the preceding baseline, attributing
/// any red to the phases whose SHARE of the total grew most — the
/// event stream reconstructing the flame-graph diff post hoc.
///
/// FAIL-CLOSED (bead fz2.4.1): non-finite or negative attainment or
/// phase durations anywhere in the history, and unusable specs
/// (non-finite/non-positive k_sigma, baseline too short to estimate
/// dispersion), return [`GateVerdict::Invalid`] — never Green. NaN can
/// otherwise flip the red predicate false silently.
#[must_use]
pub fn gate(history: &[Night], spec: GateSpec) -> GateVerdict {
    if let Some(reason) = spec_flaw(spec) {
        return GateVerdict::Invalid { reason };
    }
    for (idx, night) in history.iter().enumerate() {
        if let Some(reason) = night_flaw(idx, night) {
            return GateVerdict::Invalid { reason };
        }
    }
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
    GateVerdict::Red {
        z,
        attribution: attribution_vs_baseline(baseline, newest),
    }
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
    ///
    /// FAIL-CLOSED (bead fz2.4.1): a detector with a non-finite or
    /// non-positive threshold cannot certify quiet — it alarms at
    /// index 0; a non-finite residual alarms at ITS index (NaN would
    /// otherwise silently reset the shortfall via `max`, suppressing
    /// detection). Malformed data can force an alarm; it can never
    /// suppress one.
    #[must_use]
    pub fn first_alarm(&self, z_scores: &[f64]) -> Option<usize> {
        if !(self.k.is_finite() && self.k >= 0.0 && self.h.is_finite() && self.h > 0.0) {
            return if z_scores.is_empty() { None } else { Some(0) };
        }
        let mut s = 0.0f64;
        for (i, &z) in z_scores.iter().enumerate() {
            if !z.is_finite() {
                return Some(i);
            }
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
///
/// FAIL-CLOSED (bead fz2.4.1): from the first non-finite input onward
/// every output is −∞ (the worst possible shortfall), so poisoned
/// history can never enter the expanding baseline as ordinary data or
/// read as good performance — downstream CUSUM alarms instead.
#[must_use]
pub fn standardize(history: &[f64], warmup: usize) -> Vec<f64> {
    let poisoned_from = history
        .iter()
        .position(|x| !x.is_finite())
        .unwrap_or(history.len());
    history
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            if i >= poisoned_from {
                f64::NEG_INFINITY
            } else if i < warmup {
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
/// FAIL-CLOSED (bead fz2.4.1): a kernel whose history contains
/// non-finite or negative fields is reported FIRST with an infinite
/// drop and the flaw as its "why" — malformed evidence is flagged
/// loudest, never silently skipped and never allowed to poison the
/// trend arithmetic of valid kernels.
#[must_use]
pub fn slower_this_month(
    kernels: &BTreeMap<String, Vec<Night>>,
    pct_floor: f64,
) -> Vec<(String, f64, String)> {
    let mut out = Vec::new();
    for (kernel, history) in kernels {
        if let Some(flaw) = history
            .iter()
            .enumerate()
            .find_map(|(idx, n)| night_flaw(idx, n))
        {
            out.push((kernel.clone(), f64::INFINITY, format!("INVALID: {flaw}")));
            continue;
        }
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
            // Top offender straight from the attribution machinery
            // (formerly routed through gate() with a degenerate
            // k_sigma = 0 spec, which validation now refuses).
            let (baseline, newest) = history.split_at(history.len() - 1);
            let why = attribution_vs_baseline(baseline, &newest[0])
                .first()
                .map_or_else(|| "unattributed".to_string(), |(p, _, _)| p.clone());
            out.push((kernel.clone(), drop_pct, why));
        }
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}
