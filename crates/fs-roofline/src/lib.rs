//! fs-roofline: the roofline harness (plan §14; Decalogue P6).
//!
//! Performance claims as FALSIFIABLE targets: every registered kernel is
//! benchmarked against its arithmetic-intensity-derived limit on the actual
//! machine — measured axes, never spec-sheet numbers — with dispersion
//! reported and results ledgered under the machine fingerprint. "A target
//! that was never re-measured is a lie waiting to happen."
//!
//! Layer: L6 (consumes fs-substrate probes, fs-simd primitives, and writes
//! fs-ledger records). Reporting-only in v0: attainment verdicts inform;
//! gating bands belong to nightly runs on ledgered reference machines.

pub mod axes;
pub mod baseline;
pub mod kernels;
pub mod stats;

pub use axes::MachineAxes;
pub use baseline::{
    BaselineAxes, BaselineIdentity, BaselineProvenance, BaselineStore, BaselineVerdict,
    PromotionError, citable_axis_admission, promote_baseline,
};

use fs_ledger::{EventRow, FiveExplicits, Ledger, LedgerError, OpOutcome, now_wall_ns};

pub mod regress;

/// Crate version (compile-time stamp).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Shape-class prefix under which versioned roofline rows land in the ledger
/// `tune` table.
pub const TUNE_SHAPE_CLASS: &str = "roofline-v2";

/// Versioned ledger shape-class key for a kernel implementation.
#[must_use]
pub fn tune_shape_class(version: &str) -> String {
    format!("{TUNE_SHAPE_CLASS}:{version}")
}

/// Which machine axis a kernel is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threading {
    /// One thread: per-core bandwidth/compute axes.
    SingleThread,
    /// All logical cores: aggregate axes.
    AllCore,
}

/// Machine quantity against which a declared target fraction is evaluated.
///
/// This is independent of [`RoofSide`]: the binding roof remains part of every
/// report, while a plan target such as "75% of measured peak FLOPs" must stay
/// compute-relative even on a machine where memory bandwidth binds first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetAxis {
    /// Fraction of `min(bandwidth limit, compute limit)`.
    BindingRoof,
    /// Fraction of the measured floating-point compute peak.
    ComputePeak,
    /// Fraction of the measured memory-bandwidth peak.
    MemoryBandwidth,
}

impl TargetAxis {
    /// Stable receipt spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BindingRoof => "binding_roof",
            Self::ComputePeak => "compute_peak",
            Self::MemoryBandwidth => "memory_bandwidth",
        }
    }
}

/// Static description of a benchmarkable kernel: identity plus the
/// arithmetic-intensity model that derives its machine-specific limit.
#[derive(Debug, Clone, Copy)]
pub struct KernelSpec {
    /// Registry name (ledger key; kebab-case).
    pub name: &'static str,
    /// Kernel version (bumped when the implementation changes — attainment
    /// history is only comparable within one version).
    pub version: &'static str,
    /// Bytes moved to/from memory per element processed.
    pub bytes_per_elem: f64,
    /// Floating-point operations per element processed.
    pub flops_per_elem: f64,
    /// Measurement threading model.
    pub threading: Threading,
    /// Axis whose measured value defines `target_fraction`.
    pub target_axis: TargetAxis,
    /// Target as a fraction of [`KernelSpec::target_axis`]. `None` =
    /// report-only, no band claimed.
    pub target_fraction: Option<f64>,
}

/// One benchmarkable kernel: owns its buffers; `run_once` is the timed unit.
pub trait RooflineKernel {
    /// The kernel's spec (identity + intensity model + target).
    fn spec(&self) -> KernelSpec;
    /// Elements processed per `run_once` call.
    fn elements(&self) -> usize;
    /// Execute one timed repetition.
    fn run_once(&mut self);
    /// Sealed execution decision made by the most recent `run_once`, when this
    /// kernel has decision provenance beyond its static [`KernelSpec`].
    ///
    /// The default is `None`; the production GEMM route returns a binding that
    /// includes its exact tune key, plan, source, implementation tier, build,
    /// and validated tune-row identity.
    fn execution_binding(&self) -> Option<KernelExecutionBinding> {
        None
    }
    /// Newly measured, validated tune state awaiting the enclosing evidence
    /// transaction. Returning it does not publish it.
    fn pending_tune_publication(&self) -> Option<fs_session::ValidatedGemmTuneRow> {
        None
    }
    /// Complete process-local tuning state after the enclosing registry run's
    /// aggregate admission decision. The measured [`Attainment`] already owns
    /// any pending publication clone; kernels discard their pending marker on
    /// either outcome and additionally invalidate rejected local decisions.
    ///
    /// # Errors
    /// Returns a structured diagnostic if lifecycle cleanup fails. The default
    /// is a no-op for kernels without tune state.
    fn finalize_tuning(&mut self, _admitted: bool) -> Result<(), String> {
        Ok(())
    }
}

/// A sealed execution-decision receipt sampled after one timed repetition.
///
/// Fields are deliberately private. Built-in kernels construct this only from
/// producer-validated values; consumers can inspect the canonical JSON emitted
/// by [`Attainment::to_jsonl`] but cannot mint citable bindings directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelExecutionBinding {
    gemm: GemmDecisionBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GemmDecisionBinding {
    scoped_tune_key: String,
    shape_class: String,
    canonical_plan: String,
    source: &'static str,
    operation_tier: String,
    build_identity: String,
    tune_row_identity: fs_ledger::ContentHash,
    validated_row: fs_session::ValidatedGemmTuneRow,
    execution_path: fs_session::GemmExecutionReceipt,
    execution_path_identity: fs_ledger::ContentHash,
}

const GEMM_EXECUTION_PATH_DOMAIN: &str = "org.frankensim.fs-roofline.gemm-execution-path.v2";
const EXECUTION_BINDING_DOMAIN: &str = "org.frankensim.fs-roofline.execution-binding.v2";

fn execution_path_is_complete(path: &fs_session::GemmExecutionReceipt) -> bool {
    path.total_tiles > 0
        && path.completed_tiles == path.total_tiles
        && !path.panels.is_empty()
        && path.panels.iter().enumerate().all(|(ordinal, panel)| {
            u64::try_from(ordinal) == Ok(panel.declared_run)
                && !panel.kernel.is_empty()
                && !panel.mode.is_empty()
                && panel.total > 0
                && panel.completed == panel.total
        })
}

fn execution_path_json(path: &fs_session::GemmExecutionReceipt) -> String {
    let panels = path
        .panels
        .iter()
        .map(|panel| {
            format!(
                "{{\"kernel\":\"{}\",\"mode\":\"{}\",\"declared_run\":{},\"completed\":{},\"total\":{}}}",
                json_escape(&panel.kernel),
                json_escape(&panel.mode),
                panel.declared_run,
                panel.completed,
                panel.total,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"completed_tiles\":{},\"total_tiles\":{},\"panel_count\":{},\"panels\":[{}]}}",
        path.completed_tiles,
        path.total_tiles,
        path.panels.len(),
        panels,
    )
}

impl KernelExecutionBinding {
    #[allow(clippy::too_many_arguments)] // every independent provenance field stays explicit
    pub(crate) fn gemm(
        scoped_tune_key: String,
        shape_class: String,
        canonical_plan: String,
        source: &'static str,
        operation_tier: String,
        build_identity: String,
        validated_row: fs_session::ValidatedGemmTuneRow,
        machine: u64,
        execution_path: fs_session::GemmExecutionReceipt,
    ) -> Result<Self, String> {
        if !validated_row.matches_decision(&scoped_tune_key, &shape_class, machine, &canonical_plan)
        {
            return Err(
                "validated GEMM tune row does not bind the dispatched decision".to_string(),
            );
        }
        let tune_row_identity = validated_row.receipt_identity();
        let execution_path_identity = fs_blake3::hash_domain(
            GEMM_EXECUTION_PATH_DOMAIN,
            execution_path_json(&execution_path).as_bytes(),
        );
        let binding = Self {
            gemm: GemmDecisionBinding {
                scoped_tune_key,
                shape_class,
                canonical_plan,
                source,
                operation_tier,
                build_identity,
                tune_row_identity,
                validated_row,
                execution_path,
                execution_path_identity,
            },
        };
        if !binding.is_valid_for(machine) {
            return Err("GEMM execution binding is internally inconsistent".to_string());
        }
        Ok(binding)
    }

    fn is_valid_for(&self, machine: u64) -> bool {
        let gemm = &self.gemm;
        !gemm.scoped_tune_key.is_empty()
            && !gemm.shape_class.is_empty()
            && !gemm.canonical_plan.is_empty()
            && gemm.source == "tuned"
            && !gemm.operation_tier.is_empty()
            && !gemm.build_identity.is_empty()
            && gemm
                .scoped_tune_key
                .contains(&format!("/shape={}/", gemm.shape_class))
            && gemm
                .scoped_tune_key
                .contains(&format!("/tier={}/", gemm.operation_tier))
            && gemm
                .scoped_tune_key
                .ends_with(&format!("/build={}", gemm.build_identity))
            && gemm.tune_row_identity == gemm.validated_row.receipt_identity()
            && execution_path_is_complete(&gemm.execution_path)
            && gemm.execution_path_identity
                == fs_blake3::hash_domain(
                    GEMM_EXECUTION_PATH_DOMAIN,
                    execution_path_json(&gemm.execution_path).as_bytes(),
                )
            && gemm.validated_row.matches_decision(
                &gemm.scoped_tune_key,
                &gemm.shape_class,
                machine,
                &gemm.canonical_plan,
            )
    }

    fn canonical_json(&self) -> String {
        let gemm = &self.gemm;
        format!(
            "{{\"kind\":\"gemm-v2\",\"scoped_tune_key\":\"{}\",\"shape_class\":\"{}\",\"plan\":\"{}\",\"source\":\"{}\",\"operation_tier\":\"{}\",\"build_identity\":\"{}\",\"tune_row_identity\":\"{}\",\"tune_row\":{},\"execution_path_identity\":\"{}\",\"execution_path\":{}}}",
            json_escape(&gemm.scoped_tune_key),
            json_escape(&gemm.shape_class),
            json_escape(&gemm.canonical_plan),
            gemm.source,
            json_escape(&gemm.operation_tier),
            json_escape(&gemm.build_identity),
            gemm.tune_row_identity,
            gemm.validated_row.receipt_json(),
            gemm.execution_path_identity,
            execution_path_json(&gemm.execution_path),
        )
    }

    fn receipt_identity(&self) -> fs_ledger::ContentHash {
        fs_blake3::hash_domain(EXECUTION_BINDING_DOMAIN, self.canonical_json().as_bytes())
    }

    fn validated_row(&self) -> &fs_session::ValidatedGemmTuneRow {
        &self.gemm.validated_row
    }
}

/// Which side of the roofline binds the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoofSide {
    /// Memory-bandwidth-bound at this intensity.
    Bandwidth,
    /// Compute-bound at this intensity.
    Compute,
}

/// Verdict against the kernel's declared band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Attainment ≥ target fraction.
    WithinBand,
    /// Attainment < target fraction.
    BelowBand,
    /// No target declared: report-only.
    NoTarget,
    /// The MEASUREMENT ENVIRONMENT is invalid: the probed axes fail
    /// absolute plausibility floors, or the kernel "beat" its roofline
    /// by an impossible margin (stale/contention-crushed axes). A gate
    /// must never pass — or fail — on a machine that was useless while
    /// it was measured (bead 1n61: a load-68 window collapsed both the
    /// STREAM probe and the kernel ~1000× together, and the RATIO
    /// self-normalized to a vacuous within_band).
    EnvironmentInvalid,
}

impl Verdict {
    /// Stable lowercase name for logs and ledger rows.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Verdict::WithinBand => "within_band",
            Verdict::BelowBand => "below_band",
            Verdict::NoTarget => "no_target",
            Verdict::EnvironmentInvalid => "environment_invalid",
        }
    }
}

/// One kernel's measured attainment against the machine roofline.
#[derive(Debug, Clone)]
pub struct Attainment {
    /// Kernel name.
    pub kernel: String,
    /// Kernel version.
    pub version: String,
    /// Median elements/second across repetitions.
    pub elems_per_sec: f64,
    /// Achieved memory traffic, GB/s.
    pub achieved_gbs: f64,
    /// Achieved compute, GFLOP/s.
    pub achieved_gflops: f64,
    /// Roofline limit in elements/second for this machine + intensity.
    pub limit_elems_per_sec: f64,
    /// Which axis binds.
    pub roof: RoofSide,
    /// `elems_per_sec / limit_elems_per_sec` (1.0 = at the roof).
    pub attainment: f64,
    /// Fraction of the spec's declared target axis. This equals `attainment`
    /// only when `target_axis == binding_roof` (or the declared physical axis
    /// happens to bind).
    pub target_attainment: f64,
    /// Relative interquartile dispersion of the repetition times
    /// ((p75 − p25) / median): a benchmark without variance bars is
    /// folklore.
    pub dispersion: f64,
    /// Repetitions measured (after warmup).
    pub reps: usize,
    /// Verdict against the declared band.
    pub verdict: Verdict,
    /// Why this row cannot support a verdict. Present exactly when
    /// `verdict == EnvironmentInvalid`.
    pub invalid_reason: Option<String>,
    axis_binding: AxisBinding,
    spec_binding: SpecBinding,
    measurement_origin: MeasurementOrigin,
    pending_tune_publication: Option<fs_session::ValidatedGemmTuneRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MeasurementOrigin {
    Analytic,
    Timed {
        elements: usize,
        warmup_runs: usize,
        sample_seconds_bits: Vec<u64>,
        decision_bindings: Vec<Option<KernelExecutionBinding>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AxisBinding {
    fingerprint: u64,
    logical_cpus: u32,
    bandwidth_single_bits: u64,
    bandwidth_all_core_bits: u64,
    peak_single_bits: u64,
    peak_all_core_bits: u64,
}

impl AxisBinding {
    fn new(axes: &MachineAxes) -> Self {
        Self {
            fingerprint: axes.fingerprint,
            logical_cpus: axes.logical_cpus,
            bandwidth_single_bits: axes.bandwidth_single_gbs.to_bits(),
            bandwidth_all_core_bits: axes.bandwidth_all_core_gbs.to_bits(),
            peak_single_bits: axes.peak_single_gflops.to_bits(),
            peak_all_core_bits: axes.peak_all_core_gflops.to_bits(),
        }
    }

    fn matches(&self, axes: &MachineAxes) -> bool {
        self == &Self::new(axes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpecBinding {
    kernel: String,
    version: String,
    bytes_per_elem_bits: u64,
    flops_per_elem_bits: u64,
    threading: Threading,
    target_axis: TargetAxis,
    target_fraction_bits: Option<u64>,
}

impl SpecBinding {
    fn new(spec: &KernelSpec) -> Self {
        Self {
            kernel: spec.name.to_string(),
            version: spec.version.to_string(),
            bytes_per_elem_bits: spec.bytes_per_elem.to_bits(),
            flops_per_elem_bits: spec.flops_per_elem.to_bits(),
            threading: spec.threading,
            target_axis: spec.target_axis,
            target_fraction_bits: spec.target_fraction.map(f64::to_bits),
        }
    }
}

fn stable_decision_binding(
    bindings: &[Option<KernelExecutionBinding>],
) -> Option<&KernelExecutionBinding> {
    let first = bindings.first()?.as_ref()?;
    bindings
        .iter()
        .all(|candidate| candidate.as_ref() == Some(first))
        .then_some(first)
}

impl Attainment {
    /// One JSON line for logs/agents (stable field order).
    #[must_use]
    #[allow(clippy::too_many_lines)] // one canonical receipt schema, kept visibly ordered
    pub fn to_jsonl(&self) -> String {
        let invalid_reason = self.invalid_reason.as_ref().map_or_else(
            || "null".to_string(),
            |reason| format!("\"{}\"", json_escape(reason)),
        );
        let target_bits = self
            .spec_binding
            .target_fraction_bits
            .map_or_else(|| "null".to_string(), |bits| format!("\"{bits:016x}\""));
        let measurement = match &self.measurement_origin {
            MeasurementOrigin::Analytic => "{\"origin\":\"analytic\"}".to_string(),
            MeasurementOrigin::Timed {
                elements,
                warmup_runs,
                sample_seconds_bits,
                decision_bindings,
            } => {
                let sample = stats::sample_from_times(
                    sample_seconds_bits
                        .iter()
                        .copied()
                        .map(f64::from_bits)
                        .collect(),
                );
                let sample_bits = sample_seconds_bits
                    .iter()
                    .map(|bits| format!("\"{bits:016x}\""))
                    .collect::<Vec<_>>()
                    .join(",");
                let decision_bits = decision_bindings
                    .iter()
                    .map(|binding| {
                        binding.as_ref().map_or_else(
                            || "null".to_string(),
                            |binding| format!("\"{}\"", binding.receipt_identity()),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"origin\":\"timed\",\"elements\":{elements},\"warmup_runs\":{warmup_runs},\"sample_seconds_bits\":[{sample_bits}],\"decision_binding_hashes\":[{decision_bits}],\"median_seconds_bits\":\"{:016x}\",\"p25_seconds_bits\":\"{:016x}\",\"p75_seconds_bits\":\"{:016x}\",\"dispersion_bits\":\"{:016x}\"}}",
                    sample.median.to_bits(),
                    sample.p25.to_bits(),
                    sample.p75.to_bits(),
                    sample.dispersion.to_bits(),
                )
            }
        };
        let execution = match &self.measurement_origin {
            MeasurementOrigin::Timed {
                decision_bindings, ..
            } => stable_decision_binding(decision_bindings).map_or_else(
                || "null".to_string(),
                KernelExecutionBinding::canonical_json,
            ),
            MeasurementOrigin::Analytic => "null".to_string(),
        };
        format!(
            "{{\"receipt_version\":2,\"kernel\":\"{}\",\"version\":\"{}\",\"machine\":\"{:016x}\",\
             \"axes\":{{\"logical_cpus\":{},\"bandwidth_single_bits\":\"{:016x}\",\"bandwidth_all_core_bits\":\"{:016x}\",\"peak_single_bits\":\"{:016x}\",\"peak_all_core_bits\":\"{:016x}\"}},\
             \"spec\":{{\"bytes_per_elem_bits\":\"{:016x}\",\"flops_per_elem_bits\":\"{:016x}\",\"threading\":\"{}\",\"target_axis\":\"{}\",\"target_fraction_bits\":{}}},\"measurement\":{},\"execution\":{},\"elems_per_sec_bits\":\"{:016x}\",\"gbs_bits\":\"{:016x}\",\"gflops_bits\":\"{:016x}\",\"limit_elems_per_sec_bits\":\"{:016x}\",\"attainment_bits\":\"{:016x}\",\"target_attainment_bits\":\"{:016x}\",\"dispersion_bits\":\"{:016x}\",\"elems_per_sec_display\":{:.3e},\
             \"gbs\":{:.3},\"gflops\":{:.3},\"limit_elems_per_sec\":{:.3e},\
             \"roof\":\"{}\",\"attainment\":{:.4},\"target_attainment\":{:.4},\"dispersion\":{:.4},\
             \"reps\":{},\"verdict\":\"{}\",\"invalid_reason\":{}}}",
            json_escape(&self.kernel),
            json_escape(&self.version),
            self.axis_binding.fingerprint,
            self.axis_binding.logical_cpus,
            self.axis_binding.bandwidth_single_bits,
            self.axis_binding.bandwidth_all_core_bits,
            self.axis_binding.peak_single_bits,
            self.axis_binding.peak_all_core_bits,
            self.spec_binding.bytes_per_elem_bits,
            self.spec_binding.flops_per_elem_bits,
            match self.spec_binding.threading {
                Threading::SingleThread => "single_thread",
                Threading::AllCore => "all_core",
            },
            self.spec_binding.target_axis.name(),
            target_bits,
            measurement,
            execution,
            self.elems_per_sec.to_bits(),
            self.achieved_gbs.to_bits(),
            self.achieved_gflops.to_bits(),
            self.limit_elems_per_sec.to_bits(),
            self.attainment.to_bits(),
            self.target_attainment.to_bits(),
            self.dispersion.to_bits(),
            self.elems_per_sec,
            self.achieved_gbs,
            self.achieved_gflops,
            self.limit_elems_per_sec,
            match self.roof {
                RoofSide::Bandwidth => "bandwidth",
                RoofSide::Compute => "compute",
            },
            self.attainment,
            self.target_attainment,
            self.dispersion,
            self.reps,
            self.verdict.name(),
            invalid_reason,
        )
    }

    #[allow(clippy::too_many_lines)] // fail-closed rederivation is easier to audit as one flow
    fn is_citable_against(&self, axes: &MachineAxes) -> bool {
        if !self.axis_binding.matches(axes)
            || self.kernel != self.spec_binding.kernel
            || self.version != self.spec_binding.version
            || self.kernel.trim().is_empty()
            || self.version.trim().is_empty()
            || self.reps == 0
            || self.invalid_reason.is_some()
            || self.verdict == Verdict::EnvironmentInvalid
        {
            return false;
        }
        let MeasurementOrigin::Timed {
            elements,
            warmup_runs,
            sample_seconds_bits,
            decision_bindings,
        } = &self.measurement_origin
        else {
            return false;
        };
        let sample_times: Vec<_> = sample_seconds_bits
            .iter()
            .copied()
            .map(f64::from_bits)
            .collect();
        if *elements == 0
            || sample_times.is_empty()
            || sample_times.len() != self.reps
            || decision_bindings.len() != self.reps
            || sample_times
                .iter()
                .any(|seconds| !seconds.is_finite() || *seconds <= 0.0)
        {
            return false;
        }
        let sample = stats::sample_from_times(sample_times);
        if ((*elements as f64) / sample.median).to_bits() != self.elems_per_sec.to_bits()
            || sample.dispersion.to_bits() != self.dispersion.to_bits()
        {
            return false;
        }
        let bytes_per_elem = f64::from_bits(self.spec_binding.bytes_per_elem_bits);
        let flops_per_elem = f64::from_bits(self.spec_binding.flops_per_elem_bits);
        let target = self.spec_binding.target_fraction_bits.map(f64::from_bits);
        if !bytes_per_elem.is_finite()
            || !flops_per_elem.is_finite()
            || bytes_per_elem < 0.0
            || flops_per_elem < 0.0
            || (bytes_per_elem == 0.0 && flops_per_elem == 0.0)
            || target.is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 1.0)
            || !self.elems_per_sec.is_finite()
            || self.elems_per_sec < 0.0
            || !self.dispersion.is_finite()
            || self.dispersion < 0.0
        {
            return false;
        }
        let (bandwidth_gbs, peak_gflops) = match self.spec_binding.threading {
            Threading::SingleThread => (axes.bandwidth_single_gbs, axes.peak_single_gflops),
            Threading::AllCore => (axes.bandwidth_all_core_gbs, axes.peak_all_core_gflops),
        };
        let bandwidth_limit = if bytes_per_elem > 0.0 {
            bandwidth_gbs * 1e9 / bytes_per_elem
        } else {
            f64::INFINITY
        };
        let compute_limit = if flops_per_elem > 0.0 {
            peak_gflops * 1e9 / flops_per_elem
        } else {
            f64::INFINITY
        };
        let (limit, roof) = if bandwidth_limit <= compute_limit {
            (bandwidth_limit, RoofSide::Bandwidth)
        } else {
            (compute_limit, RoofSide::Compute)
        };
        if !limit.is_finite() || limit <= 0.0 {
            return false;
        }
        let attainment = self.elems_per_sec / limit;
        let achieved_gbs = self.elems_per_sec * bytes_per_elem / 1e9;
        let achieved_gflops = self.elems_per_sec * flops_per_elem / 1e9;
        let target_attainment = match self.spec_binding.target_axis {
            TargetAxis::BindingRoof => attainment,
            TargetAxis::ComputePeak => achieved_gflops / peak_gflops,
            TargetAxis::MemoryBandwidth => achieved_gbs / bandwidth_gbs,
        };
        if !attainment.is_finite()
            || attainment > 1.5
            || !target_attainment.is_finite()
            || !achieved_gbs.is_finite()
            || !achieved_gflops.is_finite()
        {
            return false;
        }
        let expected_verdict = match target {
            None => Verdict::NoTarget,
            Some(fraction) if target_attainment >= fraction => Verdict::WithinBand,
            Some(_) => Verdict::BelowBand,
        };
        let decision_valid = if self.kernel == "gemm-f64" {
            *warmup_runs > 0
                && stable_decision_binding(decision_bindings)
                    .is_some_and(|binding| binding.is_valid_for(axes.fingerprint))
        } else {
            decision_bindings.iter().all(Option::is_none)
        };
        let pending_valid = self
            .pending_tune_publication
            .as_ref()
            .is_none_or(|pending| {
                self.kernel == "gemm-f64"
                    && stable_decision_binding(decision_bindings).is_some_and(|binding| {
                        pending.receipt_identity() == binding.validated_row().receipt_identity()
                    })
            });
        self.roof == roof
            && self.limit_elems_per_sec.to_bits() == limit.to_bits()
            && self.attainment.to_bits() == attainment.to_bits()
            && self.target_attainment.to_bits() == target_attainment.to_bits()
            && self.achieved_gbs.to_bits() == achieved_gbs.to_bits()
            && self.achieved_gflops.to_bits() == achieved_gflops.to_bits()
            && self.verdict == expected_verdict
            && decision_valid
            && pending_valid
    }
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

fn spec_error(spec: &KernelSpec) -> Option<&'static str> {
    if spec.name.trim().is_empty() || spec.version.trim().is_empty() {
        return Some("kernel name and version must be non-empty");
    }
    if !spec.bytes_per_elem.is_finite()
        || !spec.flops_per_elem.is_finite()
        || spec.bytes_per_elem < 0.0
        || spec.flops_per_elem < 0.0
        || (spec.bytes_per_elem == 0.0 && spec.flops_per_elem == 0.0)
    {
        return Some("kernel intensity must be finite, non-negative, and exercise an axis");
    }
    if let Some(target) = spec.target_fraction
        && (!target.is_finite() || target <= 0.0 || target > 1.0)
    {
        return Some("target fraction must be finite and in (0, 1]");
    }
    if spec.target_fraction.is_some()
        && match spec.target_axis {
            TargetAxis::BindingRoof => false,
            TargetAxis::ComputePeak => spec.flops_per_elem == 0.0,
            TargetAxis::MemoryBandwidth => spec.bytes_per_elem == 0.0,
        }
    {
        return Some("declared target axis is not exercised by this kernel");
    }
    None
}

/// Compute attainment from a measured rate and the machine axes. Pure
/// arithmetic — meta-tested against hand calculations (`rf_002`).
#[must_use]
pub fn attainment_for(spec: &KernelSpec, elems_per_sec: f64, axes: &MachineAxes) -> Attainment {
    attainment_with_dispersion(spec, elems_per_sec, 0.0, 0, axes)
}

/// [`attainment_for`] with measured dispersion and repetition count.
#[must_use]
pub fn attainment_with_dispersion(
    spec: &KernelSpec,
    elems_per_sec: f64,
    dispersion: f64,
    reps: usize,
    axes: &MachineAxes,
) -> Attainment {
    attainment_with_origin(
        spec,
        elems_per_sec,
        dispersion,
        reps,
        axes,
        MeasurementOrigin::Analytic,
    )
}

#[allow(clippy::too_many_lines)] // one roof decision per branch; splitting obscures the min-roof flow
fn attainment_with_origin(
    spec: &KernelSpec,
    elems_per_sec: f64,
    dispersion: f64,
    reps: usize,
    axes: &MachineAxes,
    measurement_origin: MeasurementOrigin,
) -> Attainment {
    let spec_error = spec_error(spec);
    let measurement_error = if !elems_per_sec.is_finite() || elems_per_sec < 0.0 {
        Some("measured element rate is non-finite or negative")
    } else if !dispersion.is_finite() || dispersion < 0.0 {
        Some("measured dispersion is non-finite or negative")
    } else {
        None
    };
    let safe_rate = if measurement_error.is_none() {
        elems_per_sec
    } else {
        0.0
    };
    let safe_bytes = if spec.bytes_per_elem.is_finite() && spec.bytes_per_elem >= 0.0 {
        spec.bytes_per_elem
    } else {
        0.0
    };
    let safe_flops = if spec.flops_per_elem.is_finite() && spec.flops_per_elem >= 0.0 {
        spec.flops_per_elem
    } else {
        0.0
    };
    let (bandwidth_gbs, peak_gflops) = match spec.threading {
        Threading::SingleThread => (axes.bandwidth_single_gbs, axes.peak_single_gflops),
        Threading::AllCore => (axes.bandwidth_all_core_gbs, axes.peak_all_core_gflops),
    };
    // Limits in elements/second on each axis; +inf when the kernel does not
    // exercise an axis (zero bytes or zero flops per element).
    let bw_limit = if safe_bytes > 0.0 && bandwidth_gbs.is_finite() && bandwidth_gbs > 0.0 {
        bandwidth_gbs * 1e9 / safe_bytes
    } else {
        f64::INFINITY
    };
    let comp_limit = if safe_flops > 0.0 && peak_gflops.is_finite() && peak_gflops > 0.0 {
        peak_gflops * 1e9 / safe_flops
    } else {
        f64::INFINITY
    };
    let (limit, roof) = if bw_limit <= comp_limit {
        (bw_limit, RoofSide::Bandwidth)
    } else {
        (comp_limit, RoofSide::Compute)
    };
    let raw_attainment = if limit.is_finite() && limit > 0.0 {
        safe_rate / limit
    } else {
        0.0
    };
    let raw_achieved_gbs = safe_rate * safe_bytes / 1e9;
    let raw_achieved_gflops = safe_rate * safe_flops / 1e9;
    let raw_target_attainment = match spec.target_axis {
        TargetAxis::BindingRoof => raw_attainment,
        TargetAxis::ComputePeak if peak_gflops.is_finite() && peak_gflops > 0.0 => {
            raw_achieved_gflops / peak_gflops
        }
        TargetAxis::MemoryBandwidth if bandwidth_gbs.is_finite() && bandwidth_gbs > 0.0 => {
            raw_achieved_gbs / bandwidth_gbs
        }
        TargetAxis::ComputePeak | TargetAxis::MemoryBandwidth => f64::NAN,
    };
    let derived_error = if !raw_attainment.is_finite()
        || !raw_target_attainment.is_finite()
        || !raw_achieved_gbs.is_finite()
        || !raw_achieved_gflops.is_finite()
    {
        Some("derived roofline quantities overflowed or became non-finite")
    } else {
        None
    };
    let attainment = if raw_attainment.is_finite() {
        raw_attainment
    } else {
        0.0
    };
    let target_attainment = if raw_target_attainment.is_finite() {
        raw_target_attainment
    } else {
        0.0
    };
    // Environment validity BEFORE band comparison (bead 1n61): the
    // ratio is meaningless when the axes are implausible, and an
    // attainment materially above 1 means the kernel outran its own
    // roofline — the axes were probed under different (crushed)
    // conditions than the kernel run. Refuse to gate either way.
    let invalid_reason = axes
        .plausibility_error()
        .or(spec_error)
        .or(measurement_error)
        .or(derived_error)
        .or_else(|| (attainment > 1.5).then_some("attainment exceeds the credible roofline band"));
    let verdict = if invalid_reason.is_some() {
        Verdict::EnvironmentInvalid
    } else {
        match spec.target_fraction {
            None => Verdict::NoTarget,
            Some(t) if target_attainment >= t => Verdict::WithinBand,
            Some(_) => Verdict::BelowBand,
        }
    };
    Attainment {
        kernel: spec.name.to_string(),
        version: spec.version.to_string(),
        elems_per_sec: safe_rate,
        achieved_gbs: if raw_achieved_gbs.is_finite() {
            raw_achieved_gbs
        } else {
            0.0
        },
        achieved_gflops: if raw_achieved_gflops.is_finite() {
            raw_achieved_gflops
        } else {
            0.0
        },
        limit_elems_per_sec: if limit.is_finite() { limit } else { 0.0 },
        roof,
        attainment,
        target_attainment,
        dispersion: if dispersion.is_finite() && dispersion >= 0.0 {
            dispersion
        } else {
            0.0
        },
        reps,
        verdict,
        invalid_reason: invalid_reason.map(str::to_string),
        axis_binding: AxisBinding::new(axes),
        spec_binding: SpecBinding::new(spec),
        measurement_origin,
        pending_tune_publication: None,
    }
}

/// Measure one kernel (warmup + repetitions) and compute its attainment.
pub fn measure(
    kernel: &mut dyn RooflineKernel,
    warmup: usize,
    reps: usize,
    axes: &MachineAxes,
) -> Attainment {
    let spec = kernel.spec();
    let elements = kernel.elements();
    let elems = elements as f64;
    for _ in 0..warmup {
        kernel.run_once();
    }
    let measured_reps = reps.max(1);
    let mut times = Vec::with_capacity(measured_reps);
    let mut decision_bindings = Vec::with_capacity(measured_reps);
    for _ in 0..measured_reps {
        let start = std::time::Instant::now();
        kernel.run_once();
        times.push(start.elapsed().as_secs_f64());
        decision_bindings.push(kernel.execution_binding());
    }
    let sample = stats::sample_from_times(times);
    let elems_per_sec = if sample.median > 0.0 {
        elems / sample.median
    } else {
        0.0
    };
    let mut result = attainment_with_origin(
        &spec,
        elems_per_sec,
        sample.dispersion,
        measured_reps,
        axes,
        MeasurementOrigin::Timed {
            elements,
            warmup_runs: warmup,
            sample_seconds_bits: sample.times.iter().map(|value| value.to_bits()).collect(),
            decision_bindings,
        },
    );
    result.pending_tune_publication = kernel.pending_tune_publication();
    result
}

/// Run every kernel in the registry.
pub fn run_registry(
    registry: &mut [Box<dyn RooflineKernel>],
    warmup: usize,
    reps: usize,
    axes: &MachineAxes,
) -> Vec<Attainment> {
    let mut results: Vec<_> = registry
        .iter_mut()
        .map(|k| measure(k.as_mut(), warmup, reps, axes))
        .collect();
    poison_invalid_run(&mut results);
    results
}

/// Whether a registry result set is admissible as citable performance
/// evidence for these exact measured axes.
#[must_use]
pub fn run_is_citable(axes: &MachineAxes, post_axes: &MachineAxes, results: &[Attainment]) -> bool {
    run_admission_error(axes, post_axes, results).is_none()
}

/// Complete the registry's tuning lifecycle at the same admission boundary
/// used for citable performance evidence.
///
/// Every kernel sees the single aggregate decision. Publication belongs to
/// [`record_run`]'s evidence transaction; this hook clears kernel-owned pending
/// markers and invalidates rejected local decisions.
///
/// # Errors
/// Calls every kernel, then returns all lifecycle diagnostics in deterministic
/// registry order.
pub fn finalize_registry_tuning(
    registry: &mut [Box<dyn RooflineKernel>],
    axes: &MachineAxes,
    post_axes: &MachineAxes,
    results: &[Attainment],
) -> Result<bool, String> {
    let admitted = run_admission_error(axes, post_axes, results).is_none();
    let mut diagnostics = Vec::new();
    for (index, kernel) in registry.iter_mut().enumerate() {
        if let Err(error) = kernel.finalize_tuning(admitted) {
            diagnostics.push(format!("kernel[{index}]: {error}"));
        }
    }
    if !diagnostics.is_empty() {
        return Err(format!(
            "tuning lifecycle finalization failed for {} kernel(s): {}",
            diagnostics.len(),
            diagnostics.join("; ")
        ));
    }
    Ok(admitted)
}

/// Explain why a result set cannot be admitted as citable performance
/// evidence. `None` means every row is a bound timed measurement.
#[must_use]
pub fn run_admission_error(
    axes: &MachineAxes,
    post_axes: &MachineAxes,
    results: &[Attainment],
) -> Option<String> {
    if results.is_empty() {
        return Some("registry produced no measured kernels".to_string());
    }
    if let Some(reason) = axes.plausibility_error() {
        return Some(format!("machine axes are not admissible: {reason}"));
    }
    if let Some(reason) = axes.reprobe_error(post_axes) {
        return Some(format!(
            "post-run axis probe did not corroborate the run: {reason}"
        ));
    }
    let mut identities = std::collections::BTreeSet::new();
    for (index, result) in results.iter().enumerate() {
        if !identities.insert(result.kernel.as_str()) {
            return Some(format!(
                "duplicate kernel identity at row {index}: {}/{}",
                result.kernel, result.version
            ));
        }
        if !result.is_citable_against(axes) {
            return Some(format!(
                "row {index} ({}/{}) is not a bound timed measurement for these axes",
                result.kernel, result.version
            ));
        }
    }
    None
}

fn poison_invalid_run(results: &mut [Attainment]) {
    let Some(origin) = results
        .iter()
        .find(|result| result.verdict == Verdict::EnvironmentInvalid)
    else {
        return;
    };
    let reason = format!(
        "registry invalidated by {}: {}",
        origin.kernel,
        origin
            .invalid_reason
            .as_deref()
            .unwrap_or("invalid evidence")
    );
    for result in results {
        result.verdict = Verdict::EnvironmentInvalid;
        result.invalid_reason = Some(reason.clone());
    }
}

// ---------------------------------------------------------------------------
// §14.1 target table as data
// ---------------------------------------------------------------------------

/// One row of the plan §14.1 target table. `landed = false` rows are
/// visible from day one so nothing is silently uncovered — they flip as the
/// owning kernels register.
#[derive(Debug, Clone, Copy)]
pub struct TargetRow {
    /// Kernel family name.
    pub kernel: &'static str,
    /// What the target means (unit and roof context).
    pub statement: &'static str,
    /// Whether an implementation is registered in this harness yet.
    pub landed: bool,
}

/// The §14.1 table. Statements, not claims: every value must be re-measured
/// on a fingerprinted machine before anyone may cite it.
pub const SECTION_14_1_TARGETS: &[TargetRow] = &[
    TargetRow {
        kernel: "lbm-d3q19-stream-collide",
        statement: "≥1.0 GLUP/s (M-class) / ≥0.6 GLUP/s (TR-class), bandwidth-bound",
        landed: false,
    },
    TargetRow {
        kernel: "gemm-f64",
        statement: "≥75% of measured peak FLOPs for the selected SIMD tier",
        landed: true,
    },
    TargetRow {
        kernel: "spmv-sell-c-sigma",
        statement: "≥85% of measured STREAM-class bandwidth",
        landed: false,
    },
    TargetRow {
        kernel: "feec-apply-p4",
        statement: "≥30% of peak FLOPs, sum-factorized",
        landed: false,
    },
    TargetRow {
        kernel: "batched-small-dense",
        statement: "≥60% of peak FLOPs, SIMD-across-elements",
        landed: false,
    },
    TargetRow {
        kernel: "fft-3d-pencil",
        statement: "≥40% of the memory-bound limit",
        landed: false,
    },
    TargetRow {
        kernel: "sdf-primary-rays",
        statement: "≥80 Mray/s (M-class) / ≥120 Mray/s (TR-class)",
        landed: false,
    },
];

fn versions_json(code_version: &str) -> String {
    format!(
        "{{\"frankensim\":\"{}\",\"fs-roofline\":\"{VERSION}\"}}",
        json_escape(code_version)
    )
}

// ---------------------------------------------------------------------------
// Ledger integration and staleness
// ---------------------------------------------------------------------------

/// Record a harness run atomically in the ledger. Admitted timed rows receive
/// metrics, `benchmark_result` events, and versioned tune rows. Rejected input
/// receives one explicit rejection event and no normal-looking measurements.
/// A successful commit consumes each result's one-shot fresh-row marker;
/// rollback retains it so the same transaction can be retried.
///
/// # Errors
/// Ledger errors propagate and roll back the whole write set.
#[allow(clippy::too_many_lines)] // one auditable all-or-nothing evidence transaction
pub fn record_run(
    ledger: &Ledger,
    axes: &MachineAxes,
    post_axes: &MachineAxes,
    results: &mut [Attainment],
) -> Result<i64, LedgerError> {
    let admission_error = run_admission_error(axes, post_axes, results);
    let run_valid = admission_error.is_none();
    let code_version = std::env::var("GITHUB_SHA").unwrap_or_else(|_| "local".to_string());
    let versions = versions_json(&code_version);
    let explicits = FiveExplicits {
        seed: b"roofline",
        versions: &versions,
        budget: "{\"wall_s\":600}",
        capability: "{\"ops\":[\"perf.roofline\"]}",
    };
    let ir = format!(
        "{{\"op\":\"perf.roofline\",\"kernels\":{},\"fingerprint\":\"{:016x}\",\"post_fingerprint\":\"{:016x}\",\"admitted\":{run_valid}}}",
        results.len(),
        axes.fingerprint,
        post_axes.fingerprint,
    );
    ledger.begin()?;
    let write_result: Result<i64, LedgerError> = (|| {
        let op = ledger.begin_op(Some(b"roofline"), &ir, &explicits, now_wall_ns())?;
        if run_valid {
            let fp_bytes = axes.fingerprint.to_le_bytes();
            for r in results.iter() {
                if let MeasurementOrigin::Timed {
                    decision_bindings, ..
                } = &r.measurement_origin
                    && let Some(binding) = stable_decision_binding(decision_bindings)
                {
                    // Publish the exact row from the sealed binding even when
                    // this measurement adopted it from another ledger. This
                    // evidence transaction is deliberately insert-only: a
                    // clone or delayed receipt must never replace a newer
                    // cache row. Refresh/replacement is a separate explicit
                    // protocol, never authority carried by Attainment.
                    binding
                        .validated_row()
                        .publish_if_absent_or_identical(ledger)?;
                }
                ledger.record_metric(
                    op,
                    0,
                    &format!("{}.elems_per_sec", r.kernel),
                    r.elems_per_sec,
                )?;
                ledger.record_metric(op, 0, &format!("{}.attainment", r.kernel), r.attainment)?;
                ledger.record_metric(op, 0, &format!("{}.dispersion", r.kernel), r.dispersion)?;
                let payload = r.to_jsonl();
                ledger.append_event(&EventRow {
                    session: Some(b"roofline"),
                    t: 0,
                    kind: "benchmark_result",
                    payload: Some(&payload),
                })?;
                ledger.tune_put(
                    &r.kernel,
                    &tune_shape_class(&r.version),
                    &fp_bytes,
                    &format!(
                        "{{\"version\":\"{}\",\"reps\":{},\"post_bandwidth_single_bits\":\"{:016x}\",\"post_bandwidth_all_core_bits\":\"{:016x}\",\"post_peak_single_bits\":\"{:016x}\",\"post_peak_all_core_bits\":\"{:016x}\"}}",
                        json_escape(&r.version),
                        r.reps,
                        post_axes.bandwidth_single_gbs.to_bits(),
                        post_axes.bandwidth_all_core_gbs.to_bits(),
                        post_axes.peak_single_gflops.to_bits(),
                        post_axes.peak_all_core_gflops.to_bits(),
                    ),
                    &payload,
                )?;
            }
            ledger.finish_op(op, OpOutcome::Ok, None, now_wall_ns())?;
        } else {
            let reason = admission_error
                .as_deref()
                .unwrap_or("unknown admission failure");
            let payload = format!(
                "{{\"code\":\"roofline_evidence_rejected\",\"reason\":\"{}\",\"effect\":\"no_measurements_or_tune_rows_published\"}}",
                json_escape(reason)
            );
            ledger.append_event(&EventRow {
                session: Some(b"roofline"),
                t: 0,
                kind: "roofline_run_rejected",
                payload: Some(&payload),
            })?;
            let diagnostic = format!(
                "{{\"code\":\"roofline_evidence_rejected\",\"reason\":\"{}\"}}",
                json_escape(reason)
            );
            ledger.finish_op(op, OpOutcome::Error, Some(&diagnostic), now_wall_ns())?;
        }
        Ok(op)
    })();
    match write_result {
        Ok(op) => match ledger.commit() {
            Ok(()) => {
                for result in results.iter_mut() {
                    result.pending_tune_publication = None;
                }
                Ok(op)
            }
            Err(error) => {
                let _ = ledger.rollback();
                Err(error)
            }
        },
        Err(error) => {
            let _ = ledger.rollback();
            Err(error)
        }
    }
}

/// Staleness state of one kernel's ledgered attainment on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Staleness {
    /// A row matches kernel version and machine identity, but the tune schema
    /// carries no timestamp yet, so freshness cannot honestly be asserted.
    MatchingIdentityAgeUnknown,
    /// Rows exist, but none for the current fingerprint — the machine
    /// drifted and every cited number is stale until re-measured.
    FingerprintDrift,
    /// No roofline rows at all: never measured.
    NeverMeasured,
}

/// Check one kernel's staleness against the current fingerprint.
///
/// # Errors
/// Ledger errors propagate.
pub fn staleness(
    ledger: &Ledger,
    kernel: &str,
    version: &str,
    current_fingerprint: u64,
) -> Result<Staleness, LedgerError> {
    let rows = ledger.tune_rows(kernel)?;
    let shape_class = tune_shape_class(version);
    let roofline_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.shape_class == shape_class)
        .collect();
    if roofline_rows.is_empty() {
        return Ok(Staleness::NeverMeasured);
    }
    let fp = current_fingerprint.to_le_bytes();
    if roofline_rows.iter().any(|r| r.machine == fp) {
        Ok(Staleness::MatchingIdentityAgeUnknown)
    } else {
        Ok(Staleness::FingerprintDrift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReceiptKernel {
        elements: usize,
        value: u64,
    }

    impl RooflineKernel for ReceiptKernel {
        fn spec(&self) -> KernelSpec {
            KernelSpec {
                name: "receipt-kernel",
                version: "1",
                bytes_per_elem: 8.0,
                flops_per_elem: 1.0,
                threading: Threading::SingleThread,
                target_axis: TargetAxis::BindingRoof,
                target_fraction: None,
            }
        }

        fn elements(&self) -> usize {
            self.elements
        }

        fn run_once(&mut self) {
            for _ in 0..1024 {
                self.value = std::hint::black_box(
                    self.value
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1),
                );
            }
        }
    }

    struct AdmissionProbeKernel {
        observed: std::rc::Rc<std::cell::Cell<Option<bool>>>,
    }

    impl RooflineKernel for AdmissionProbeKernel {
        fn spec(&self) -> KernelSpec {
            ReceiptKernel {
                elements: 1,
                value: 0,
            }
            .spec()
        }

        fn elements(&self) -> usize {
            1
        }

        fn run_once(&mut self) {}

        fn finalize_tuning(&mut self, admitted: bool) -> Result<(), String> {
            self.observed.set(Some(admitted));
            Ok(())
        }
    }

    struct FallibleAdmissionProbeKernel {
        id: usize,
        observed: std::rc::Rc<std::cell::RefCell<Vec<(usize, bool)>>>,
        failure: Option<&'static str>,
    }

    impl RooflineKernel for FallibleAdmissionProbeKernel {
        fn spec(&self) -> KernelSpec {
            ReceiptKernel {
                elements: 1,
                value: 0,
            }
            .spec()
        }

        fn elements(&self) -> usize {
            1
        }

        fn run_once(&mut self) {}

        fn finalize_tuning(&mut self, admitted: bool) -> Result<(), String> {
            self.observed.borrow_mut().push((self.id, admitted));
            self.failure.map_or(Ok(()), |error| Err(error.to_string()))
        }
    }

    fn synthetic_axes() -> MachineAxes {
        MachineAxes {
            fingerprint: 0xABCD,
            cpu_brand: "synthetic".to_string(),
            logical_cpus: 8,
            bandwidth_single_gbs: 100.0,
            bandwidth_all_core_gbs: 400.0,
            peak_single_gflops: 50.0,
            peak_all_core_gflops: 300.0,
        }
    }

    #[test]
    fn crushed_axes_cannot_gate_vacuously() {
        // Bead 1n61 counterexample replay: on a load-68 host both the
        // STREAM probe and the FFT kernel collapsed ~1000× together
        // (axes 0.2 GB/s, kernel 0.156 GB/s on a ~200 GB/s machine) and
        // the RATIO self-normalized to 0.89 = a vacuous within_band.
        let crushed = MachineAxes {
            fingerprint: 0xDEAD,
            cpu_brand: "crushed".to_string(),
            logical_cpus: 128,
            bandwidth_single_gbs: 0.2,
            bandwidth_all_core_gbs: 0.4,
            peak_single_gflops: 0.05,
            peak_all_core_gflops: 0.4,
        };
        assert!(!crushed.plausible());
        let spec = KernelSpec {
            name: "fft-roundtrip",
            version: "1n61",
            bytes_per_elem: 672.0,
            flops_per_elem: 172.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: Some(0.40),
        };
        // 0.156 GB/s effective — 89% of the crushed axis.
        let a = attainment_for(&spec, 0.156e9 / 672.0, &crushed);
        assert_eq!(
            a.verdict,
            Verdict::EnvironmentInvalid,
            "a crushed environment must refuse to gate (got attainment {:.3})",
            a.attainment
        );
    }

    #[test]
    fn over_roof_attainment_poisons_the_gate() {
        // Healthy axes but the kernel 'beats' its roofline by 2× — the
        // axes are stale relative to the run; refuse.
        let spec = KernelSpec {
            name: "axpy",
            version: "1n61",
            bytes_per_elem: 24.0,
            flops_per_elem: 2.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: Some(0.6),
        };
        let a = attainment_for(&spec, 2.0 * 100.0e9 / 24.0, &synthetic_axes());
        assert_eq!(a.verdict, Verdict::EnvironmentInvalid);
        // Slightly over 1 (measurement jitter) still gates normally.
        let b = attainment_for(&spec, 1.2 * 100.0e9 / 24.0, &synthetic_axes());
        assert_eq!(b.verdict, Verdict::WithinBand);
    }

    #[test]
    fn invalid_numeric_inputs_fail_closed_and_remain_json() {
        let base = KernelSpec {
            name: "probe\"escaped",
            version: "1",
            bytes_per_elem: 8.0,
            flops_per_elem: 1.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: Some(0.5),
        };
        let bad_rate = attainment_with_dispersion(&base, f64::NAN, 0.0, 3, &synthetic_axes());
        assert_eq!(bad_rate.verdict, Verdict::EnvironmentInvalid);
        assert!(bad_rate.elems_per_sec.is_finite());
        assert!(bad_rate.to_jsonl().contains("probe\\\"escaped"));
        assert!(!bad_rate.to_jsonl().contains("NaN"));

        let bad_dispersion =
            attainment_with_dispersion(&base, 1.0, f64::INFINITY, 3, &synthetic_axes());
        assert_eq!(bad_dispersion.verdict, Verdict::EnvironmentInvalid);
        let bad_target = KernelSpec {
            target_fraction: Some(f64::NAN),
            ..base
        };
        assert_eq!(
            attainment_for(&bad_target, 1.0, &synthetic_axes()).verdict,
            Verdict::EnvironmentInvalid
        );
    }

    #[test]
    fn one_invalid_row_poisons_every_registry_verdict() {
        let normal = KernelSpec {
            name: "normal",
            version: "1",
            bytes_per_elem: 24.0,
            flops_per_elem: 2.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: Some(0.5),
        };
        let impossible = KernelSpec {
            name: "impossible",
            ..normal
        };
        let mut results = vec![
            attainment_for(&normal, 100.0e9 / 24.0 * 0.8, &synthetic_axes()),
            attainment_for(&impossible, 100.0e9 / 24.0 * 2.0, &synthetic_axes()),
        ];
        assert_eq!(results[0].verdict, Verdict::WithinBand);
        assert_eq!(results[1].verdict, Verdict::EnvironmentInvalid);
        poison_invalid_run(&mut results);
        assert!(
            results
                .iter()
                .all(|row| row.verdict == Verdict::EnvironmentInvalid)
        );
        assert!(results.iter().all(|row| {
            row.invalid_reason
                .as_deref()
                .is_some_and(|r| r.contains("impossible"))
        }));
    }

    #[test]
    fn only_bound_timed_receipts_are_citable() {
        let axes = synthetic_axes();
        let spec = ReceiptKernel {
            elements: 1,
            value: 0,
        }
        .spec();
        let analytic = attainment_with_dispersion(&spec, 1.0, 0.0, 1, &axes);
        assert!(
            !run_is_citable(&axes, &axes, &[analytic]),
            "an analytic helper result is not measurement evidence"
        );

        let mut kernel = ReceiptKernel {
            elements: 1,
            value: 0,
        };
        let timed = measure(&mut kernel, 0, 3, &axes);
        assert!(run_is_citable(&axes, &axes, std::slice::from_ref(&timed)));
        assert!(timed.to_jsonl().contains("\"sample_seconds_bits\""));
        let mut drifted_post = axes.clone();
        drifted_post.bandwidth_single_gbs = 60.0;
        assert!(!run_is_citable(
            &axes,
            &drifted_post,
            std::slice::from_ref(&timed)
        ));

        let mut tampered = timed.clone();
        tampered.dispersion += 0.01;
        assert!(!run_is_citable(&axes, &axes, &[tampered]));
        let mut tampered_target = timed.clone();
        tampered_target.target_attainment += 0.01;
        assert!(!run_is_citable(&axes, &axes, &[tampered_target]));
        let mut tampered_axis = timed.clone();
        tampered_axis.spec_binding.target_axis = TargetAxis::ComputePeak;
        assert!(!run_is_citable(&axes, &axes, &[tampered_axis]));
        assert!(run_admission_error(&axes, &axes, &[timed.clone(), timed]).is_some());

        let mut empty = ReceiptKernel {
            elements: 0,
            value: 0,
        };
        let empty_row = measure(&mut empty, 0, 1, &axes);
        assert!(!run_is_citable(&axes, &axes, &[empty_row]));
    }

    #[test]
    fn registry_tuning_hook_uses_the_complete_admission_decision() {
        let axes = synthetic_axes();
        let mut measured = ReceiptKernel {
            elements: 1,
            value: 0,
        };
        let result = measure(&mut measured, 0, 3, &axes);
        let observed = std::rc::Rc::new(std::cell::Cell::new(None));
        let mut registry: Vec<Box<dyn RooflineKernel>> = vec![Box::new(AdmissionProbeKernel {
            observed: std::rc::Rc::clone(&observed),
        })];

        assert!(
            finalize_registry_tuning(&mut registry, &axes, &axes, std::slice::from_ref(&result),)
                .expect("admitted hook")
        );
        assert_eq!(observed.get(), Some(true));

        observed.set(None);
        let mut drifted_post = axes.clone();
        drifted_post.bandwidth_single_gbs *= 0.5;
        assert!(
            !finalize_registry_tuning(&mut registry, &axes, &drifted_post, &[result])
                .expect("rejected hook")
        );
        assert_eq!(observed.get(), Some(false));
    }

    #[test]
    fn registry_tuning_hook_drains_every_kernel_after_a_middle_failure() {
        let axes = synthetic_axes();
        let mut measured = ReceiptKernel {
            elements: 1,
            value: 0,
        };
        let result = measure(&mut measured, 0, 3, &axes);
        let observed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut registry: Vec<Box<dyn RooflineKernel>> = (0..3)
            .map(|id| {
                Box::new(FallibleAdmissionProbeKernel {
                    id,
                    observed: std::rc::Rc::clone(&observed),
                    failure: (id == 1).then_some("middle cleanup failed"),
                }) as Box<dyn RooflineKernel>
            })
            .collect();

        let error =
            finalize_registry_tuning(&mut registry, &axes, &axes, std::slice::from_ref(&result))
                .expect_err("middle failure must be reported after every hook drains");
        assert_eq!(
            observed.borrow().as_slice(),
            &[(0, true), (1, true), (2, true)],
            "first, failing middle, and last kernel must see the same admission decision"
        );
        assert_eq!(
            error,
            "tuning lifecycle finalization failed for 1 kernel(s): kernel[1]: middle cleanup failed"
        );
    }

    #[test]
    fn attainment_matches_hand_calculation_bandwidth_bound() {
        // axpy: 24 B/elem, 2 flop/elem on axes (100 GB/s, 50 GFLOP/s):
        // bw limit = 100e9/24 = 4.1667e9 elem/s; compute = 50e9/2 = 25e9.
        // Bandwidth binds. At 2.0833e9 elem/s attainment = 0.5 exactly.
        let spec = KernelSpec {
            name: "axpy",
            version: "1",
            bytes_per_elem: 24.0,
            flops_per_elem: 2.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: Some(0.6),
        };
        let a = attainment_for(&spec, 100.0e9 / 24.0 / 2.0, &synthetic_axes());
        assert_eq!(a.roof, RoofSide::Bandwidth);
        assert!((a.attainment - 0.5).abs() < 1e-12, "got {}", a.attainment);
        assert!((a.achieved_gbs - 50.0).abs() < 1e-9);
        assert_eq!(a.verdict, Verdict::BelowBand);
    }

    #[test]
    fn attainment_matches_hand_calculation_compute_bound() {
        // High-intensity kernel: 1 B/elem, 100 flop/elem.
        // bw limit = 100e9 elem/s; compute = 50e9/100 = 0.5e9 → compute binds.
        let spec = KernelSpec {
            name: "dense",
            version: "1",
            bytes_per_elem: 1.0,
            flops_per_elem: 100.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: Some(0.5),
        };
        let a = attainment_for(&spec, 0.4e9, &synthetic_axes());
        assert_eq!(a.roof, RoofSide::Compute);
        assert!((a.attainment - 0.8).abs() < 1e-12);
        assert_eq!(a.verdict, Verdict::WithinBand);
        // All-core axes flip the limit.
        let all = KernelSpec {
            threading: Threading::AllCore,
            ..spec
        };
        let b = attainment_for(&all, 0.4e9, &synthetic_axes());
        assert!((b.limit_elems_per_sec - 3.0e9).abs() < 1.0);
    }

    #[test]
    fn compute_target_is_not_laundered_through_the_binding_bandwidth_roof() {
        let bandwidth_starved = MachineAxes {
            fingerprint: 0xC0DE,
            cpu_brand: "high-compute-low-bandwidth".to_string(),
            logical_cpus: 8,
            bandwidth_single_gbs: 10.0,
            bandwidth_all_core_gbs: 20.0,
            peak_single_gflops: 1_000.0,
            peak_all_core_gflops: 2_000.0,
        };
        let spec = KernelSpec {
            name: "gemm-target-model",
            version: "ss0n",
            bytes_per_elem: 8.0,
            flops_per_elem: 1.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::ComputePeak,
            target_fraction: Some(0.75),
        };
        let above_bandwidth_target = attainment_for(&spec, 1.0e9, &bandwidth_starved);
        assert_eq!(above_bandwidth_target.roof, RoofSide::Bandwidth);
        assert!((above_bandwidth_target.attainment - 0.8).abs() < 1e-12);
        assert!((above_bandwidth_target.target_attainment - 0.001).abs() < 1e-12);
        assert_eq!(above_bandwidth_target.verdict, Verdict::BelowBand);
        assert!(
            above_bandwidth_target
                .to_jsonl()
                .contains("\"target_axis\":\"compute_peak\"")
        );

        let compute_bound = KernelSpec {
            bytes_per_elem: 1.0,
            flops_per_elem: 100.0,
            ..spec
        };
        let exact_boundary = attainment_for(&compute_bound, 0.375e9, &synthetic_axes());
        assert_eq!(exact_boundary.roof, RoofSide::Compute);
        assert_eq!(
            exact_boundary.target_attainment.to_bits(),
            0.75f64.to_bits()
        );
        assert_eq!(exact_boundary.verdict, Verdict::WithinBand);
    }

    #[test]
    fn no_target_reports_without_verdict() {
        let spec = KernelSpec {
            name: "probe",
            version: "1",
            bytes_per_elem: 8.0,
            flops_per_elem: 1.0,
            threading: Threading::SingleThread,
            target_axis: TargetAxis::BindingRoof,
            target_fraction: None,
        };
        let a = attainment_for(&spec, 1.0e9, &synthetic_axes());
        assert_eq!(a.verdict, Verdict::NoTarget);
        assert!(a.to_jsonl().contains("\"verdict\":\"no_target\""));
    }

    #[test]
    fn section_14_1_table_is_complete_and_honest() {
        assert_eq!(SECTION_14_1_TARGETS.len(), 7, "all §14.1 families present");
        let registry = kernels::production_registry(1, &synthetic_axes());
        let registered: std::collections::BTreeSet<_> =
            registry.iter().map(|kernel| kernel.spec().name).collect();
        for row in SECTION_14_1_TARGETS {
            assert_eq!(
                row.landed,
                registered.contains(row.kernel),
                "{} target registration and landed status drifted",
                row.kernel,
            );
        }
    }

    #[test]
    fn versions_json_escapes_hostile_build_identifiers() {
        assert_eq!(
            versions_json("sha\\\"row\n\t\u{0001}"),
            format!(
                "{{\"frankensim\":\"sha\\\\\\\"row\\n\\t\\u0001\",\"fs-roofline\":\"{VERSION}\"}}"
            )
        );
    }
}
