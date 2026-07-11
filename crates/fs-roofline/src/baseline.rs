//! Trusted historical axis baselines (bead dfh3): sustained-contention
//! detection that pre/post agreement cannot provide.
//!
//! The hole this closes: [`MachineAxes::reprobe_error`] only checks that
//! the pre-run and post-run probes AGREE — a host that was already
//! crushed before the first probe and stayed crushed through the second
//! (6 GB/s pre AND post on a normally 100+ GB/s machine) self-normalizes
//! and passes. Absolute floors stay as coarse last-resort sanity, but
//! they cannot be tight enough to catch a 10x degradation on a fast
//! machine without refusing slow-but-honest reference machines.
//!
//! The fix is a separately ADMITTED baseline: a fingerprint-specific
//! record of what this machine's axes measure when quiet, carrying
//! provenance (who promoted it, from which runs, why), an age policy,
//! and the environment identity (OS/arch/firmware declaration) it is
//! valid for. Citable gates then require the CURRENT axes to sit inside
//! declared bands around the trusted baseline.
//!
//! Trust discipline (the acceptance's four laws):
//! 1. First-run measurements are CANDIDATE evidence — nothing a probe
//!    measures about itself can authorize itself.
//! 2. Promotion is explicit and governed: at least
//!    [`MIN_PROMOTION_RUNS`] mutually-agreeing candidate runs plus a
//!    named operator and a non-blank justification.
//! 3. Admission against the baseline refuses degraded, suspiciously
//!    fast, stale, and identity-drifted axes — each with a distinct,
//!    teaching verdict.
//! 4. Baseline updates go through the same promotion gate; there is no
//!    in-place mutation API.

use crate::axes::{MAX_AXIS_REPROBE_DRIFT, MachineAxes};
use std::fmt::Write as _;

/// Lower trust band: each current axis must be at least this fraction of
/// its baseline value. Below is SUSTAINED CONTENTION (the dfh3
/// counterexample measured 0.06 on the workbench).
pub const BASELINE_LOW_BAND: f64 = 0.70;

/// Upper trust band: each current axis must be at most this multiple of
/// its baseline value. Above means the machine is no longer the machine
/// the baseline describes (firmware/hardware change, or the baseline was
/// promoted from a degraded window) — re-promotion required either way.
pub const BASELINE_HIGH_BAND: f64 = 1.15;

/// Default and maximum baseline age policies, in days. A baseline older
/// than its policy is STALE: silent firmware/OS updates accumulate.
pub const DEFAULT_BASELINE_AGE_DAYS: u32 = 90;
/// Hard cap any policy must respect.
pub const MAX_BASELINE_AGE_DAYS: u32 = 365;

/// Minimum mutually-agreeing candidate runs behind one promotion.
pub const MIN_PROMOTION_RUNS: usize = 3;

/// Bounded store parsing (mirrors the tune-store discipline).
const MAX_BASELINE_STORE_BYTES: usize = 1024 * 1024;
const MAX_BASELINE_LINE_BYTES: usize = 16 * 1024;
const MAX_BASELINE_STRING_BYTES: usize = 4096;

/// The environment identity a baseline is valid for. `firmware` is a
/// DECLARED string (OS build / kernel release / SMC version — whatever
/// the operator's fleet discipline tracks): declared at promotion,
/// compared verbatim at admission. A mismatch is identity drift, never
/// a band question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineIdentity {
    /// fs-substrate topology fingerprint.
    pub fingerprint: u64,
    /// CPU brand string.
    pub cpu_brand: String,
    /// Logical CPU count.
    pub logical_cpus: u32,
    /// Operating system (`std::env::consts::OS` at promotion).
    pub os: String,
    /// ISA (`std::env::consts::ARCH` at promotion).
    pub arch: String,
    /// Declared firmware/OS-build identity.
    pub firmware: String,
}

impl BaselineIdentity {
    /// The identity of the current process's environment for `axes`,
    /// with the operator's declared firmware string.
    #[must_use]
    pub fn current(axes: &MachineAxes, firmware: impl Into<String>) -> BaselineIdentity {
        BaselineIdentity {
            fingerprint: axes.fingerprint,
            cpu_brand: axes.cpu_brand.clone(),
            logical_cpus: axes.logical_cpus,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            firmware: firmware.into(),
        }
    }
}

/// Who promoted a baseline, from what, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineProvenance {
    /// Named operator (non-blank; "governed" means someone signs).
    pub promoted_by: String,
    /// Non-blank justification recorded with the promotion.
    pub justification: String,
    /// Promotion day (days since the Unix epoch — see
    /// [`days_since_epoch_now`]).
    pub promoted_day: u64,
    /// How many mutually-agreeing candidate runs backed the promotion.
    pub source_runs: u32,
}

/// A trusted, admitted baseline for one machine fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct BaselineAxes {
    /// The environment this baseline describes.
    pub identity: BaselineIdentity,
    /// Trusted single-thread STREAM bandwidth, GB/s.
    pub bandwidth_single_gbs: f64,
    /// Trusted all-core STREAM bandwidth, GB/s.
    pub bandwidth_all_core_gbs: f64,
    /// Trusted single-thread FMA throughput, GFLOP/s.
    pub peak_single_gflops: f64,
    /// Trusted all-core FMA throughput, GFLOP/s.
    pub peak_all_core_gflops: f64,
    /// Promotion provenance.
    pub provenance: BaselineProvenance,
    /// This baseline's age policy in days (≤ [`MAX_BASELINE_AGE_DAYS`]).
    pub age_policy_days: u32,
}

/// Why a promotion was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionError {
    /// The refusal, in teaching form.
    pub detail: String,
}

impl core::fmt::Display for PromotionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "baseline promotion refused: {}", self.detail)
    }
}

impl core::error::Error for PromotionError {}

/// The verdict of checking current axes against a trusted baseline.
/// Only [`BaselineVerdict::Trusted`] supports citable gates.
#[derive(Debug, Clone, PartialEq)]
pub enum BaselineVerdict {
    /// Every axis sits inside the declared bands: the host is behaving
    /// like its trusted self.
    Trusted,
    /// No admitted baseline exists for this fingerprint: current
    /// measurements are CANDIDATE evidence only.
    Unbaselined,
    /// An axis fell below [`BASELINE_LOW_BAND`] of its baseline —
    /// sustained contention or thermal collapse; pre/post agreement is
    /// irrelevant.
    Degraded {
        /// Which axis.
        axis: &'static str,
        /// current / baseline.
        ratio: f64,
    },
    /// An axis exceeded [`BASELINE_HIGH_BAND`] of its baseline — the
    /// machine is not the machine the baseline describes; re-promote.
    Suspect {
        /// Which axis.
        axis: &'static str,
        /// current / baseline.
        ratio: f64,
    },
    /// The baseline is older than its age policy.
    Stale {
        /// Days since promotion.
        age_days: u64,
        /// The policy that was exceeded.
        limit_days: u32,
    },
    /// Fingerprint/topology/OS/arch/firmware mismatch: the baseline
    /// does not describe this environment at all.
    IdentityDrift {
        /// The first field that differed.
        field: &'static str,
    },
}

impl BaselineVerdict {
    /// True only for [`BaselineVerdict::Trusted`].
    #[must_use]
    pub fn trusted(&self) -> bool {
        matches!(self, BaselineVerdict::Trusted)
    }

    /// One-line JSON for reports/ledger.
    #[must_use]
    pub fn to_jsonl(&self) -> String {
        match self {
            BaselineVerdict::Trusted => "{\"baseline\":\"trusted\"}".to_string(),
            BaselineVerdict::Unbaselined => "{\"baseline\":\"unbaselined\"}".to_string(),
            BaselineVerdict::Degraded { axis, ratio } => {
                format!("{{\"baseline\":\"degraded\",\"axis\":\"{axis}\",\"ratio\":{ratio:.3}}}")
            }
            BaselineVerdict::Suspect { axis, ratio } => {
                format!("{{\"baseline\":\"suspect\",\"axis\":\"{axis}\",\"ratio\":{ratio:.3}}}")
            }
            BaselineVerdict::Stale {
                age_days,
                limit_days,
            } => format!(
                "{{\"baseline\":\"stale\",\"age_days\":{age_days},\"limit_days\":{limit_days}}}"
            ),
            BaselineVerdict::IdentityDrift { field } => {
                format!("{{\"baseline\":\"identity-drift\",\"field\":\"{field}\"}}")
            }
        }
    }
}

/// Days since the Unix epoch, from the system clock. Tests inject their
/// own day; production callers use this.
#[must_use]
pub fn days_since_epoch_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}

fn axes_quad(axes: &MachineAxes) -> [(&'static str, f64); 4] {
    [
        ("bandwidth_single_gbs", axes.bandwidth_single_gbs),
        ("bandwidth_all_core_gbs", axes.bandwidth_all_core_gbs),
        ("peak_single_gflops", axes.peak_single_gflops),
        ("peak_all_core_gflops", axes.peak_all_core_gflops),
    ]
}

impl BaselineAxes {
    fn baseline_quad(&self) -> [(&'static str, f64); 4] {
        [
            ("bandwidth_single_gbs", self.bandwidth_single_gbs),
            ("bandwidth_all_core_gbs", self.bandwidth_all_core_gbs),
            ("peak_single_gflops", self.peak_single_gflops),
            ("peak_all_core_gflops", self.peak_all_core_gflops),
        ]
    }

    /// Check `current` axes (already floor-plausible) against this
    /// baseline on day `now_day`.
    #[must_use]
    pub fn verdict(
        &self,
        current: &MachineAxes,
        identity: &BaselineIdentity,
        now_day: u64,
    ) -> BaselineVerdict {
        if self.identity.fingerprint != current.fingerprint {
            return BaselineVerdict::IdentityDrift {
                field: "fingerprint",
            };
        }
        if self.identity.logical_cpus != current.logical_cpus {
            return BaselineVerdict::IdentityDrift {
                field: "logical_cpus",
            };
        }
        if self.identity.cpu_brand != current.cpu_brand {
            return BaselineVerdict::IdentityDrift { field: "cpu_brand" };
        }
        if self.identity.os != identity.os {
            return BaselineVerdict::IdentityDrift { field: "os" };
        }
        if self.identity.arch != identity.arch {
            return BaselineVerdict::IdentityDrift { field: "arch" };
        }
        if self.identity.firmware != identity.firmware {
            return BaselineVerdict::IdentityDrift { field: "firmware" };
        }
        let age_days = now_day.saturating_sub(self.provenance.promoted_day);
        if age_days > u64::from(self.age_policy_days) {
            return BaselineVerdict::Stale {
                age_days,
                limit_days: self.age_policy_days,
            };
        }
        for ((axis, current_value), (_, trusted_value)) in
            axes_quad(current).into_iter().zip(self.baseline_quad())
        {
            let ratio = current_value / trusted_value;
            if !ratio.is_finite() || ratio < BASELINE_LOW_BAND {
                return BaselineVerdict::Degraded { axis, ratio };
            }
            if ratio > BASELINE_HIGH_BAND {
                return BaselineVerdict::Suspect { axis, ratio };
            }
        }
        BaselineVerdict::Trusted
    }

    /// One canonical JSON line (bit-exact axes for lossless round-trip).
    #[must_use]
    pub fn to_jsonl(&self) -> String {
        let mut s = String::with_capacity(512);
        let _ = write!(
            s,
            "{{\"fingerprint\":\"{:016x}\",\"cpu_brand\":",
            self.identity.fingerprint
        );
        push_json_string(&mut s, &self.identity.cpu_brand);
        let _ = write!(
            s,
            ",\"logical_cpus\":{},\"os\":",
            self.identity.logical_cpus
        );
        push_json_string(&mut s, &self.identity.os);
        s.push_str(",\"arch\":");
        push_json_string(&mut s, &self.identity.arch);
        s.push_str(",\"firmware\":");
        push_json_string(&mut s, &self.identity.firmware);
        let _ = write!(
            s,
            ",\"bandwidth_single_bits\":\"{:016x}\",\"bandwidth_all_core_bits\":\"{:016x}\",\
             \"peak_single_bits\":\"{:016x}\",\"peak_all_core_bits\":\"{:016x}\",\"promoted_by\":",
            self.bandwidth_single_gbs.to_bits(),
            self.bandwidth_all_core_gbs.to_bits(),
            self.peak_single_gflops.to_bits(),
            self.peak_all_core_gflops.to_bits(),
        );
        push_json_string(&mut s, &self.provenance.promoted_by);
        s.push_str(",\"justification\":");
        push_json_string(&mut s, &self.provenance.justification);
        let _ = write!(
            s,
            ",\"promoted_day\":{},\"source_runs\":{},\"age_policy_days\":{}}}",
            self.provenance.promoted_day, self.provenance.source_runs, self.age_policy_days
        );
        s
    }
}

/// Promote a trusted baseline from candidate runs — THE only way a
/// baseline comes to exist.
///
/// Requirements (each refused with a teaching detail):
/// - at least [`MIN_PROMOTION_RUNS`] candidate runs;
/// - every run floor-plausible, same fingerprint/topology, and every
///   axis pair within [`MAX_AXIS_REPROBE_DRIFT`] of the run minimum
///   (mutual agreement — one quiet run among crushed ones cannot
///   launder the set);
/// - a named operator and non-blank justification;
/// - an age policy within [`MAX_BASELINE_AGE_DAYS`].
///
/// The promoted axes are the per-axis MAXIMUM over the runs: the best
/// mutually-corroborated measurement is the closest to the machine's
/// true quiet capability, and a too-low baseline would inflate every
/// later attainment claim.
///
/// # Errors
/// [`PromotionError`] naming the failed requirement.
pub fn promote_baseline(
    runs: &[MachineAxes],
    identity: BaselineIdentity,
    promoted_by: impl Into<String>,
    justification: impl Into<String>,
    promoted_day: u64,
    age_policy_days: u32,
) -> Result<BaselineAxes, PromotionError> {
    let promoted_by = promoted_by.into();
    let justification = justification.into();
    if promoted_by.trim().is_empty() {
        return Err(PromotionError {
            detail: "promotion requires a named operator".to_string(),
        });
    }
    if justification.trim().is_empty() {
        return Err(PromotionError {
            detail: "promotion requires a non-blank justification".to_string(),
        });
    }
    if age_policy_days == 0 || age_policy_days > MAX_BASELINE_AGE_DAYS {
        return Err(PromotionError {
            detail: format!(
                "age policy {age_policy_days} days is outside 1..={MAX_BASELINE_AGE_DAYS}"
            ),
        });
    }
    if runs.len() < MIN_PROMOTION_RUNS {
        return Err(PromotionError {
            detail: format!(
                "promotion requires at least {MIN_PROMOTION_RUNS} candidate runs, got {}",
                runs.len()
            ),
        });
    }
    for (index, run) in runs.iter().enumerate() {
        if let Some(reason) = run.plausibility_error() {
            return Err(PromotionError {
                detail: format!("candidate run {index} fails plausibility floors: {reason}"),
            });
        }
        if run.fingerprint != identity.fingerprint {
            return Err(PromotionError {
                detail: format!("candidate run {index} has a different machine fingerprint"),
            });
        }
        if run.logical_cpus != identity.logical_cpus || run.cpu_brand != identity.cpu_brand {
            return Err(PromotionError {
                detail: format!("candidate run {index} has a different topology identity"),
            });
        }
    }
    // Mutual agreement: for each axis, max/min across runs must sit
    // within the reprobe drift band.
    let mut promoted = [0.0f64; 4];
    for (axis_index, promoted_axis) in promoted.iter_mut().enumerate() {
        let values: Vec<f64> = runs
            .iter()
            .map(|run| axes_quad(run)[axis_index].1)
            .collect();
        let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = values.iter().copied().fold(0.0f64, f64::max);
        if (maximum - minimum) / maximum > MAX_AXIS_REPROBE_DRIFT {
            return Err(PromotionError {
                detail: format!(
                    "candidate runs disagree on {} beyond the {MAX_AXIS_REPROBE_DRIFT} drift \
                     band ({minimum:.2} .. {maximum:.2}) — measure on a quiet host",
                    axes_quad(&runs[0])[axis_index].0
                ),
            });
        }
        *promoted_axis = maximum;
    }
    let source_runs = u32::try_from(runs.len()).expect("a plausible promotion run count fits u32");
    Ok(BaselineAxes {
        identity,
        bandwidth_single_gbs: promoted[0],
        bandwidth_all_core_gbs: promoted[1],
        peak_single_gflops: promoted[2],
        peak_all_core_gflops: promoted[3],
        provenance: BaselineProvenance {
            promoted_by,
            justification,
            promoted_day,
            source_runs,
        },
        age_policy_days,
    })
}

/// The combined citable-axis admission: absolute floors (last-resort
/// sanity), pre/post agreement, AND baseline trust. `baseline = None`
/// yields [`BaselineVerdict::Unbaselined`] — measurements proceed as
/// candidate evidence but nothing citable may be minted from them.
#[must_use]
pub fn citable_axis_admission(
    pre: &MachineAxes,
    post: &MachineAxes,
    baseline: Option<&BaselineAxes>,
    identity: &BaselineIdentity,
    now_day: u64,
) -> BaselineVerdict {
    if pre.plausibility_error().is_some()
        || post.plausibility_error().is_some()
        || pre.reprobe_error(post).is_some()
    {
        // The coarse checks already refuse; report through the nearest
        // verdict (a degraded pair that also fails floors is degraded
        // with an unknown ratio — the floors' own error strings travel
        // in the existing environment-refusal path).
        return BaselineVerdict::Degraded {
            axis: "floors-or-reprobe",
            ratio: f64::NAN,
        };
    }
    match baseline {
        None => BaselineVerdict::Unbaselined,
        Some(trusted) => {
            let pre_verdict = trusted.verdict(pre, identity, now_day);
            if !pre_verdict.trusted() {
                return pre_verdict;
            }
            trusted.verdict(post, identity, now_day)
        }
    }
}

/// A strict JSON-lines baseline store: one admitted baseline per
/// fingerprint. Duplicate fingerprints, malformed lines, and oversized
/// stores are corruption (fail closed), mirroring the tune store.
#[derive(Debug, Default)]
pub struct BaselineStore {
    baselines: Vec<BaselineAxes>,
}

impl BaselineStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The admitted baseline for a fingerprint, if any.
    #[must_use]
    pub fn for_fingerprint(&self, fingerprint: u64) -> Option<&BaselineAxes> {
        self.baselines
            .iter()
            .find(|b| b.identity.fingerprint == fingerprint)
    }

    /// Admit a promoted baseline, REPLACING any previous baseline for
    /// the same fingerprint (updates go through promotion, so the new
    /// record carries its own provenance).
    pub fn admit(&mut self, baseline: BaselineAxes) {
        self.baselines
            .retain(|b| b.identity.fingerprint != baseline.identity.fingerprint);
        self.baselines.push(baseline);
    }

    /// Serialize as JSON lines.
    #[must_use]
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for baseline in &self.baselines {
            out.push_str(&baseline.to_jsonl());
            out.push('\n');
        }
        out
    }

    /// Parse a JSON-lines store STRICTLY: every line must be a canonical
    /// baseline record; duplicate fingerprints are corruption.
    ///
    /// # Errors
    /// [`PromotionError`] (the store shares the promotion trust domain)
    /// naming the offending line.
    pub fn from_jsonl(text: &str) -> Result<Self, PromotionError> {
        if text.len() > MAX_BASELINE_STORE_BYTES {
            return Err(PromotionError {
                detail: format!("baseline store exceeds the {MAX_BASELINE_STORE_BYTES}-byte bound"),
            });
        }
        let mut store = BaselineStore::new();
        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let baseline = parse_baseline_line(line).ok_or_else(|| PromotionError {
                detail: format!("baseline store line {} is not canonical", line_number + 1),
            })?;
            if store
                .for_fingerprint(baseline.identity.fingerprint)
                .is_some()
            {
                return Err(PromotionError {
                    detail: format!(
                        "baseline store line {} duplicates fingerprint {:016x}",
                        line_number + 1,
                        baseline.identity.fingerprint
                    ),
                });
            }
            store.baselines.push(baseline);
        }
        Ok(store)
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Minimal strict line parser for the canonical writer grammar above.
struct LineParser<'a> {
    rest: &'a str,
}

impl LineParser<'_> {
    fn take(&mut self, token: &str) -> Option<()> {
        self.rest = self.rest.strip_prefix(token)?;
        Some(())
    }

    fn string(&mut self) -> Option<String> {
        self.take("\"")?;
        let mut out = String::new();
        let mut chars = self.rest.char_indices();
        loop {
            let (index, ch) = chars.next()?;
            match ch {
                '"' => {
                    self.rest = &self.rest[index + 1..];
                    if out.len() > MAX_BASELINE_STRING_BYTES {
                        return None;
                    }
                    return Some(out);
                }
                '\\' => {
                    let (_, escaped) = chars.next()?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                let (_, hex) = chars.next()?;
                                code = code * 16 + hex.to_digit(16)?;
                            }
                            out.push(char::from_u32(code)?);
                        }
                        _ => return None,
                    }
                }
                c if c.is_control() => return None,
                c => out.push(c),
            }
        }
    }

    fn hex_u64(&mut self) -> Option<u64> {
        let raw = self.string()?;
        if raw.len() != 16
            || !raw
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        u64::from_str_radix(&raw, 16).ok()
    }

    fn decimal_u64(&mut self) -> Option<u64> {
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(self.rest.len());
        if end == 0 {
            return None;
        }
        let (digits, rest) = self.rest.split_at(end);
        if digits.len() > 1 && digits.starts_with('0') {
            return None; // canonical integers only
        }
        self.rest = rest;
        digits.parse().ok()
    }
}

fn parse_baseline_line(line: &str) -> Option<BaselineAxes> {
    if line.len() > MAX_BASELINE_LINE_BYTES {
        return None;
    }
    let mut p = LineParser { rest: line };
    p.take("{\"fingerprint\":")?;
    let fingerprint = p.hex_u64()?;
    p.take(",\"cpu_brand\":")?;
    let cpu_brand = p.string()?;
    p.take(",\"logical_cpus\":")?;
    let logical_cpus = u32::try_from(p.decimal_u64()?).ok()?;
    p.take(",\"os\":")?;
    let os = p.string()?;
    p.take(",\"arch\":")?;
    let arch = p.string()?;
    p.take(",\"firmware\":")?;
    let firmware = p.string()?;
    p.take(",\"bandwidth_single_bits\":")?;
    let bandwidth_single_gbs = f64::from_bits(p.hex_u64()?);
    p.take(",\"bandwidth_all_core_bits\":")?;
    let bandwidth_all_core_gbs = f64::from_bits(p.hex_u64()?);
    p.take(",\"peak_single_bits\":")?;
    let peak_single_gflops = f64::from_bits(p.hex_u64()?);
    p.take(",\"peak_all_core_bits\":")?;
    let peak_all_core_gflops = f64::from_bits(p.hex_u64()?);
    p.take(",\"promoted_by\":")?;
    let promoted_by = p.string()?;
    p.take(",\"justification\":")?;
    let justification = p.string()?;
    p.take(",\"promoted_day\":")?;
    let promoted_day = p.decimal_u64()?;
    p.take(",\"source_runs\":")?;
    let source_runs = u32::try_from(p.decimal_u64()?).ok()?;
    p.take(",\"age_policy_days\":")?;
    let age_policy_days = u32::try_from(p.decimal_u64()?).ok()?;
    p.take("}")?;
    if !p.rest.is_empty() {
        return None;
    }
    // Semantic refusals: the store only carries records promotion could
    // have produced.
    if promoted_by.trim().is_empty()
        || justification.trim().is_empty()
        || source_runs < MIN_PROMOTION_RUNS as u32
        || age_policy_days == 0
        || age_policy_days > MAX_BASELINE_AGE_DAYS
        || logical_cpus == 0
    {
        return None;
    }
    let axes = [
        bandwidth_single_gbs,
        bandwidth_all_core_gbs,
        peak_single_gflops,
        peak_all_core_gflops,
    ];
    if axes.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    Some(BaselineAxes {
        identity: BaselineIdentity {
            fingerprint,
            cpu_brand,
            logical_cpus,
            os,
            arch,
            firmware,
        },
        bandwidth_single_gbs,
        bandwidth_all_core_gbs,
        peak_single_gflops,
        peak_all_core_gflops,
        provenance: BaselineProvenance {
            promoted_by,
            justification,
            promoted_day,
            source_runs,
        },
        age_policy_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_axes() -> MachineAxes {
        MachineAxes {
            fingerprint: 0xF1,
            cpu_brand: "synthetic-m".to_string(),
            logical_cpus: 8,
            bandwidth_single_gbs: 100.0,
            bandwidth_all_core_gbs: 220.0,
            peak_single_gflops: 45.0,
            peak_all_core_gflops: 300.0,
        }
    }

    fn identity() -> BaselineIdentity {
        BaselineIdentity::current(&quiet_axes(), "build-24F74")
    }

    fn promoted() -> BaselineAxes {
        promote_baseline(
            &[quiet_axes(), quiet_axes(), quiet_axes()],
            identity(),
            "operator-a",
            "three quiet-window runs on the reference host",
            20_000,
            90,
        )
        .expect("canonical promotion")
    }

    /// QUIET DRILL: axes at the baseline are trusted end-to-end.
    #[test]
    fn quiet_axes_within_bands_are_trusted() {
        let baseline = promoted();
        let current = quiet_axes();
        assert_eq!(
            baseline.verdict(&current, &identity(), 20_010),
            BaselineVerdict::Trusted
        );
        assert!(
            citable_axis_admission(&current, &current, Some(&baseline), &identity(), 20_010)
                .trusted()
        );
    }

    /// DEGRADED DRILL — the dfh3 counterexample: 6 GB/s pre AND post on
    /// a 100 GB/s host passes floors and pre/post agreement but must be
    /// refused by the baseline.
    #[test]
    fn sustained_contention_is_refused_despite_pre_post_agreement() {
        let baseline = promoted();
        let mut crushed = quiet_axes();
        crushed.bandwidth_single_gbs = 6.0;
        crushed.bandwidth_all_core_gbs = 13.0;
        // Floors pass (6 > 5) and the pair self-agrees...
        assert!(crushed.plausible());
        assert!(crushed.reprobe_error(&crushed).is_none());
        // ...but the baseline sees a 0.06 ratio.
        let verdict =
            citable_axis_admission(&crushed, &crushed, Some(&baseline), &identity(), 20_010);
        assert!(
            matches!(
                verdict,
                BaselineVerdict::Degraded {
                    axis: "bandwidth_single_gbs",
                    ..
                }
            ),
            "{verdict:?}"
        );
        assert!(!verdict.trusted());
    }

    /// A suspiciously FAST axis is not a pass either: this is not the
    /// machine the baseline describes.
    #[test]
    fn faster_than_baseline_beyond_band_is_suspect() {
        let baseline = promoted();
        let mut upgraded = quiet_axes();
        upgraded.peak_single_gflops = 45.0 * 1.4;
        upgraded.peak_all_core_gflops = 300.0 * 1.4;
        let verdict = baseline.verdict(&upgraded, &identity(), 20_010);
        assert!(
            matches!(
                verdict,
                BaselineVerdict::Suspect {
                    axis: "peak_single_gflops",
                    ..
                }
            ),
            "{verdict:?}"
        );
    }

    /// STALE DRILL: a baseline past its age policy refuses.
    #[test]
    fn stale_baseline_is_refused_by_age_policy() {
        let baseline = promoted();
        let verdict = baseline.verdict(&quiet_axes(), &identity(), 20_000 + 91);
        assert_eq!(
            verdict,
            BaselineVerdict::Stale {
                age_days: 91,
                limit_days: 90
            }
        );
        // The day before the boundary still trusts.
        assert!(
            baseline
                .verdict(&quiet_axes(), &identity(), 20_000 + 90)
                .trusted()
        );
    }

    /// FIRMWARE-DRIFT DRILL: a changed firmware declaration is identity
    /// drift, refused before any band math.
    #[test]
    fn firmware_drift_is_identity_refusal() {
        let baseline = promoted();
        let mut moved = identity();
        moved.firmware = "build-25A01".to_string();
        assert_eq!(
            baseline.verdict(&quiet_axes(), &moved, 20_010),
            BaselineVerdict::IdentityDrift { field: "firmware" }
        );
        let mut other_machine = quiet_axes();
        other_machine.fingerprint = 0xF2;
        assert_eq!(
            baseline.verdict(&other_machine, &identity(), 20_010),
            BaselineVerdict::IdentityDrift {
                field: "fingerprint"
            }
        );
    }

    /// FIRST-RUN LAW: no baseline → Unbaselined, never Trusted; the
    /// measurement is candidate evidence and cannot authorize itself.
    #[test]
    fn first_run_measurements_are_candidates_not_baselines() {
        let current = quiet_axes();
        let verdict = citable_axis_admission(&current, &current, None, &identity(), 20_010);
        assert_eq!(verdict, BaselineVerdict::Unbaselined);
        assert!(!verdict.trusted());
    }

    /// GOVERNED PROMOTION: blank operator/justification, too few runs,
    /// disagreeing runs, foreign-fingerprint runs, and out-of-policy age
    /// are each refused with a teaching detail.
    #[test]
    fn promotion_is_governed_and_fails_closed() {
        let runs = [quiet_axes(), quiet_axes(), quiet_axes()];
        let refuse = |result: Result<BaselineAxes, PromotionError>, needle: &str| {
            let err = result.expect_err(needle);
            assert!(err.detail.contains(needle), "{}", err.detail);
        };
        refuse(
            promote_baseline(&runs, identity(), "  ", "why", 1, 90),
            "named operator",
        );
        refuse(
            promote_baseline(&runs, identity(), "op", " ", 1, 90),
            "justification",
        );
        refuse(
            promote_baseline(&runs[..2], identity(), "op", "why", 1, 90),
            "at least 3",
        );
        refuse(
            promote_baseline(&runs, identity(), "op", "why", 1, 0),
            "age policy",
        );
        refuse(
            promote_baseline(&runs, identity(), "op", "why", 1, 9999),
            "age policy",
        );
        // One crushed run among quiet ones: mutual agreement refuses.
        let mut mixed = vec![quiet_axes(), quiet_axes(), quiet_axes()];
        mixed[2].bandwidth_single_gbs = 6.0;
        refuse(
            promote_baseline(&mixed, identity(), "op", "why", 1, 90),
            "disagree",
        );
        // A foreign-fingerprint run cannot join a promotion.
        let mut foreign = vec![quiet_axes(), quiet_axes(), quiet_axes()];
        foreign[1].fingerprint = 0xF2;
        refuse(
            promote_baseline(&foreign, identity(), "op", "why", 1, 90),
            "fingerprint",
        );
        // An implausible run cannot join a promotion.
        let mut implausible = vec![quiet_axes(), quiet_axes(), quiet_axes()];
        implausible[0].peak_single_gflops = f64::NAN;
        refuse(
            promote_baseline(&implausible, identity(), "op", "why", 1, 90),
            "plausibility",
        );
    }

    /// Store round-trip is lossless; corruption and duplicates refuse.
    #[test]
    fn store_round_trips_and_fails_closed() {
        let mut store = BaselineStore::new();
        store.admit(promoted());
        let text = store.to_jsonl();
        let back = BaselineStore::from_jsonl(&text).expect("canonical store parses");
        assert_eq!(back.for_fingerprint(0xF1), Some(&promoted()));
        assert!(back.for_fingerprint(0xF2).is_none());
        // Tampered line: refused.
        assert!(BaselineStore::from_jsonl(&text.replace("operator-a", "")).is_err());
        assert!(BaselineStore::from_jsonl("{\"not\":\"a baseline\"}\n").is_err());
        // Duplicate fingerprint: corruption.
        let duplicated = format!("{text}{text}");
        assert!(BaselineStore::from_jsonl(&duplicated).is_err());
        // Admitting a re-promotion REPLACES (single record per machine).
        let mut refreshed = promoted();
        refreshed.provenance.promoted_day = 21_000;
        store.admit(refreshed.clone());
        assert_eq!(store.for_fingerprint(0xF1), Some(&refreshed));
        assert_eq!(store.to_jsonl().lines().count(), 1);
    }
}
