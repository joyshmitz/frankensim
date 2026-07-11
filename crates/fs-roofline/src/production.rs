//! Sealed production-run protocol (bead fz2.5): the only path to citable
//! roofline evidence.
//!
//! The public `RooflineKernel`/`run_registry`/`record_run` surface treats
//! caller-supplied kernel implementations and `MachineAxes` as a trust root:
//! a caller can clone a valid GEMM `KernelExecutionBinding` into a fake
//! custom kernel named `gemm-f64`, or discard a drifted post-probe and pass
//! the pre-probe twice. That surface stays available for harness tests and
//! exploration, but everything it records is stamped
//! `"protocol":"custom-registry"` and is explicitly NON-CITABLE.
//!
//! The protocol is two opaque stages:
//!
//! 1. [`ProductionProbe::observe`] performs the pre-run axis probe and mints
//!    the per-run nonce. The caller may READ the observed axes (baseline
//!    selection needs them) but can never supply its own.
//! 2. [`ProductionProbe::run`] owns production registry selection, timed
//!    warmup/repetitions, the post-run axis probe (observed strictly after
//!    the timed loop), aggregate admission, and tune finalization, yielding
//!    a [`ProductionRun`]. [`ProductionRun::record`] commits atomically and
//!    consumes the run; the operation `ir` carries
//!    `"protocol":"production-v1"`, the nonce, and content hashes of both
//!    observed axis receipts.
//!
//! Trust model, per the workspace capability pattern: the nonce is a
//! process-unique challenge, not cryptographic proof — unforgeability comes
//! from type opacity (private fields, no public constructor, `pub(crate)`
//! seams only reachable from this crate's own tests), exactly like
//! `fs-checker` signatures and the fz2.7 promotion authority. A party who
//! can rebuild this crate can mint anything; the protocol guarantees that no
//! API CONSUMER can.

use fs_ledger::{Ledger, LedgerError};

use crate::kernels::production_registry_with_ledger;
use crate::{
    Attainment, AxisBaselinePolicy, FinalizedRegistryRun, MachineAxes, RooflineKernel,
    finalize_registry_tuning, record_run_with_protocol, run_admission_error, run_registry,
};

const RUN_NONCE_DOMAIN: &str = "org.frankensim.fs-roofline.production-run-nonce.v1";
const AXES_RECEIPT_DOMAIN: &str = "org.frankensim.fs-roofline.production-axes-receipt.v1";

static NONCE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Sizing and repetition parameters for one production run.
#[derive(Debug, Clone, Copy)]
pub struct ProductionRunConfig {
    /// Vector-kernel element count (GEMM derives its edge from this).
    pub n: usize,
    /// Untimed warmup repetitions per kernel.
    pub warmup: usize,
    /// Timed repetitions per kernel.
    pub reps: usize,
}

/// Stage one of the sealed protocol: a pre-run axis probe this crate
/// performed itself, plus the minted per-run nonce.
///
/// No public constructor accepts axes; the only public way in is
/// [`ProductionProbe::observe`], which probes the actual machine.
pub struct ProductionProbe {
    axes: MachineAxes,
    nonce: fs_blake3::ContentHash,
}

impl ProductionProbe {
    /// Probe the machine and mint this run's nonce.
    #[must_use]
    pub fn observe() -> Self {
        Self::from_observed(MachineAxes::probe())
    }

    /// Test seam (`pub(crate)`): inject a synthetic pre-probe. Unreachable
    /// by API consumers, so a forged probe cannot enter the protocol.
    pub(crate) fn from_observed(axes: MachineAxes) -> Self {
        let nonce = mint_nonce(&axes);
        Self { axes, nonce }
    }

    /// The observed pre-run axes (read-only; baseline selection needs them).
    #[must_use]
    pub fn axes(&self) -> &MachineAxes {
        &self.axes
    }

    /// Run the production registry and finalize, consuming the probe.
    ///
    /// The tune ledger (optional) lets the GEMM kernel adopt a previously
    /// validated row; the registry (and with it fsqlite's `!Send`
    /// connection) is dropped before this returns, so the caller may reopen
    /// the same database for [`ProductionRun::record`].
    ///
    /// # Errors
    /// Structured diagnostics from tuning finalization; admission refusal is
    /// NOT an error — the run comes back with `citable() == false` and can
    /// be recorded as an explicit rejection.
    pub fn run(
        self,
        config: ProductionRunConfig,
        baseline: AxisBaselinePolicy<'_>,
        tune_ledger: Option<Ledger>,
    ) -> Result<ProductionRun<'_>, String> {
        let registry = production_registry_with_ledger(config.n, &self.axes, tune_ledger);
        self.run_with_parts(config, baseline, registry, MachineAxes::probe)
    }

    /// Protocol core with injected registry and post-probe (`pub(crate)`
    /// test seam: drifted-post and finalizer-failure paths need determinism;
    /// API consumers cannot reach this to forge a run).
    pub(crate) fn run_with_parts(
        self,
        config: ProductionRunConfig,
        baseline: AxisBaselinePolicy<'_>,
        mut registry: Vec<Box<dyn RooflineKernel>>,
        post_probe: impl FnOnce() -> MachineAxes,
    ) -> Result<ProductionRun<'_>, String> {
        let results = run_registry(&mut registry, config.warmup, config.reps, &self.axes);
        let post_axes = post_probe();
        let finalized =
            finalize_registry_tuning(&mut registry, &self.axes, &post_axes, baseline, &results)?;
        drop(registry);
        Ok(ProductionRun {
            axes: self.axes,
            post_axes,
            baseline,
            nonce: self.nonce,
            results,
            finalized,
        })
    }
}

/// One complete, sealed production registry run.
///
/// Fields are private, there is no public constructor, and the value is
/// neither `Clone` nor `Copy`: the only way to obtain one is
/// [`ProductionProbe::run`], which performed both probes and timed the
/// production registry itself. [`ProductionRun::record`] consumes the run.
pub struct ProductionRun<'a> {
    axes: MachineAxes,
    post_axes: MachineAxes,
    baseline: AxisBaselinePolicy<'a>,
    nonce: fs_blake3::ContentHash,
    results: Vec<Attainment>,
    finalized: FinalizedRegistryRun,
}

impl std::fmt::Debug for ProductionRun<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductionRun")
            .field("fingerprint", &format_args!("{:016x}", self.axes.fingerprint))
            .field("kernels", &self.results.len())
            .field("nonce", &self.nonce)
            .field("citable", &self.finalized.admitted())
            .finish_non_exhaustive()
    }
}

impl ProductionRun<'_> {
    /// The pre-run axis probe observed by the protocol.
    #[must_use]
    pub fn axes(&self) -> &MachineAxes {
        &self.axes
    }

    /// The post-run axis probe, observed strictly after the timed loop.
    #[must_use]
    pub fn post_axes(&self) -> &MachineAxes {
        &self.post_axes
    }

    /// The measured result set in registry order.
    #[must_use]
    pub fn results(&self) -> &[Attainment] {
        &self.results
    }

    /// The per-run nonce bound into the recorded operation.
    #[must_use]
    pub fn nonce(&self) -> fs_blake3::ContentHash {
        self.nonce
    }

    /// Whether this run passed aggregate admission and may be cited once
    /// recorded.
    #[must_use]
    pub fn citable(&self) -> bool {
        self.finalized.admitted()
    }

    /// Why admission refused this run, if it did.
    #[must_use]
    pub fn admission_error(&self) -> Option<String> {
        run_admission_error(&self.axes, &self.post_axes, self.baseline, &self.results)
    }

    /// The baseline-admission receipt for this run's exact probe pair.
    #[must_use]
    pub fn receipt_json(&self) -> String {
        self.baseline.receipt_json(&self.axes, &self.post_axes)
    }

    /// Record the run atomically, consuming it. The operation `ir` carries
    /// `"protocol":"production-v1"`, the per-run nonce, and content hashes of
    /// both observed axis receipts.
    ///
    /// # Errors
    /// Ledger errors propagate and roll back the whole write set; the run is
    /// consumed either way (a failed transaction cannot be replayed into a
    /// different ledger with edited results).
    pub fn record(mut self, ledger: &Ledger) -> Result<i64, LedgerError> {
        let protocol_fields = format!(
            "\"protocol\":\"production-v1\",\"run_nonce\":\"{}\",\"pre_axes_receipt\":\"{}\",\"post_axes_receipt\":\"{}\"",
            self.nonce,
            axes_receipt(&self.axes),
            axes_receipt(&self.post_axes),
        );
        record_run_with_protocol(
            ledger,
            &self.axes,
            &self.post_axes,
            self.baseline,
            &mut self.finalized,
            &mut self.results,
            &protocol_fields,
        )
    }
}

/// Content hash of one observed probe's canonical JSONL receipt.
fn axes_receipt(axes: &MachineAxes) -> fs_blake3::ContentHash {
    fs_blake3::hash_domain(AXES_RECEIPT_DOMAIN, axes.to_jsonl().as_bytes())
}

/// Process-unique per-run challenge: wall clock, pid, a monotone counter,
/// and the pre-probe receipt. Uniqueness, not secrecy — see the module docs.
fn mint_nonce(axes: &MachineAxes) -> fs_blake3::ContentHash {
    let count = NONCE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut material = Vec::new();
    material.extend_from_slice(&fs_ledger::now_wall_ns().to_le_bytes());
    material.extend_from_slice(&u64::from(std::process::id()).to_le_bytes());
    material.extend_from_slice(&count.to_le_bytes());
    material.extend_from_slice(axes.to_jsonl().as_bytes());
    fs_blake3::hash_domain(RUN_NONCE_DOMAIN, &material)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::kernels::default_registry;
    use crate::{
        BaselineAxes, BaselineCandidate, BaselineIdentity, KernelSpec, Staleness, TargetAxis,
        Threading, promote_baseline, staleness_at,
    };

    fn synthetic_axes(fingerprint: u64) -> MachineAxes {
        // Roofs far above any real machine (bead xjhz): cache-resident test
        // kernels must never outrun the fixture roof.
        MachineAxes {
            fingerprint,
            cpu_brand: "synthetic".to_string(),
            logical_cpus: 8,
            bandwidth_single_gbs: 100_000.0,
            bandwidth_all_core_gbs: 400_000.0,
            peak_single_gflops: 50_000.0,
            peak_all_core_gflops: 300_000.0,
        }
    }

    fn trusted_baseline(axes: &MachineAxes) -> (BaselineAxes, BaselineIdentity) {
        let identity =
            BaselineIdentity::current(axes, "test-firmware").expect("valid synthetic identity");
        let candidates: Vec<_> = (0_u64..3)
            .map(|ordinal| {
                BaselineCandidate::from_receipt(
                    axes.clone(),
                    identity.clone(),
                    fs_blake3::hash_domain(
                        "fs-roofline.production-baseline-source.v1",
                        &ordinal.to_le_bytes(),
                    ),
                )
                .expect("valid synthetic candidate")
            })
            .collect();
        let baseline = promote_baseline(
            &candidates,
            "test-operator",
            "deterministic production-protocol fixture",
            20_000,
            90,
        )
        .expect("valid synthetic baseline");
        (baseline, identity)
    }

    fn temp_db(tag: &str) -> String {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "fs-roofline-prod-{tag}-{}-{n}.db",
                std::process::id()
            ))
            .display()
            .to_string()
    }

    const CONFIG: ProductionRunConfig = ProductionRunConfig {
        n: 1 << 10,
        warmup: 0,
        reps: 1,
    };

    struct CountingKernel {
        runs: Rc<Cell<usize>>,
        value: u64,
    }

    impl crate::RooflineKernel for CountingKernel {
        fn spec(&self) -> KernelSpec {
            KernelSpec {
                name: "counting-kernel",
                version: "1",
                bytes_per_elem: 8.0,
                flops_per_elem: 1.0,
                threading: Threading::SingleThread,
                target_axis: TargetAxis::BindingRoof,
                target_fraction: None,
            }
        }

        fn elements(&self) -> usize {
            64
        }

        fn run_once(&mut self) {
            self.runs.set(self.runs.get() + 1);
            for _ in 0..64 {
                self.value = std::hint::black_box(
                    self.value
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1),
                );
            }
        }
    }

    struct FailingFinalizeKernel {
        inner: CountingKernel,
    }

    impl crate::RooflineKernel for FailingFinalizeKernel {
        fn spec(&self) -> KernelSpec {
            self.inner.spec()
        }
        fn elements(&self) -> usize {
            self.inner.elements()
        }
        fn run_once(&mut self) {
            self.inner.run_once();
        }
        fn finalize_tuning(&mut self, _admitted: bool) -> Result<(), String> {
            Err("tune ledger unavailable mid-finalize".to_string())
        }
    }

    #[test]
    fn nonces_are_unique_per_probe() {
        let a = ProductionProbe::from_observed(synthetic_axes(0xA));
        let b = ProductionProbe::from_observed(synthetic_axes(0xA));
        assert_ne!(a.nonce, b.nonce, "identical axes must still mint distinct nonces");
    }

    #[test]
    fn post_probe_is_observed_strictly_after_every_timed_repetition() {
        let axes = synthetic_axes(0xB);
        let (baseline, identity) = trusted_baseline(&axes);
        let policy = AxisBaselinePolicy::new(Some(&baseline), &identity, 20_010);
        let runs = Rc::new(Cell::new(0_usize));
        let registry: Vec<Box<dyn crate::RooflineKernel>> = vec![Box::new(CountingKernel {
            runs: Rc::clone(&runs),
            value: 1,
        })];
        let probe = ProductionProbe::from_observed(axes.clone());
        let runs_at_post = Rc::new(Cell::new(usize::MAX));
        let observed = Rc::clone(&runs_at_post);
        let counter = Rc::clone(&runs);
        let config = ProductionRunConfig {
            n: 64,
            warmup: 2,
            reps: 3,
        };
        let run = probe
            .run_with_parts(config, policy, registry, move || {
                observed.set(counter.get());
                axes.clone()
            })
            .expect("protocol run");
        // warmup(2) + reps(3): the post-probe fired only after all five.
        assert_eq!(runs_at_post.get(), 5);
        assert_eq!(run.results().len(), 1);
    }

    #[test]
    fn drifted_post_probe_refuses_citation_and_records_a_rejection() {
        let axes = synthetic_axes(0xC);
        let (baseline, identity) = trusted_baseline(&axes);
        let policy = AxisBaselinePolicy::new(Some(&baseline), &identity, 20_010);
        let mut drifted = axes.clone();
        drifted.bandwidth_single_gbs *= 0.3;
        drifted.bandwidth_all_core_gbs *= 0.3;
        let probe = ProductionProbe::from_observed(axes);
        let run = probe
            .run_with_parts(CONFIG, policy, default_registry(1 << 10), move || drifted)
            .expect("protocol run");
        assert!(!run.citable(), "drifted post-probe must refuse citation");
        let reason = run.admission_error().expect("admission diagnostic");
        assert!(
            reason.contains("baseline admission refused"),
            "unexpected diagnostic: {reason}"
        );

        let db = temp_db("drift");
        let ledger = Ledger::open(&db).expect("open ledger");
        let kernel = run.results()[0].kernel.clone();
        let version = run.results()[0].version.clone();
        let fingerprint = run.axes().fingerprint;
        let baseline_hash = policy.baseline_hash();
        let op = run.record(&ledger).expect("record rejection");
        let ir = ledger.op(op).unwrap().expect("op row").ir;
        assert!(ir.contains("\"protocol\":\"production-v1\""));
        assert!(ir.contains("\"admitted\":false"));
        // A rejected run publishes no tune evidence.
        assert_eq!(
            staleness_at(&ledger, &kernel, &version, fingerprint, baseline_hash, 1)
                .expect("staleness"),
            Staleness::NeverMeasured
        );
        cleanup_db(&db);
    }

    #[test]
    fn partial_finalizer_failure_yields_no_recordable_run() {
        let axes = synthetic_axes(0xD);
        let (baseline, identity) = trusted_baseline(&axes);
        let policy = AxisBaselinePolicy::new(Some(&baseline), &identity, 20_010);
        let registry: Vec<Box<dyn crate::RooflineKernel>> = vec![Box::new(FailingFinalizeKernel {
            inner: CountingKernel {
                runs: Rc::new(Cell::new(0)),
                value: 1,
            },
        })];
        let probe = ProductionProbe::from_observed(axes);
        let error = probe
            .run_with_parts(CONFIG, policy, registry, || synthetic_axes(0xD))
            .expect_err("finalizer failure must poison the whole run");
        assert!(
            error.contains("tune ledger unavailable mid-finalize"),
            "diagnostic must name the failing kernel's reason: {error}"
        );
        // No ProductionRun exists, so nothing can reach a ledger at all.
    }

    #[test]
    fn successful_production_run_records_nonce_and_both_axis_receipts() {
        let axes = synthetic_axes(0xE);
        let (baseline, identity) = trusted_baseline(&axes);
        let policy = AxisBaselinePolicy::new(Some(&baseline), &identity, 20_010);
        let probe = ProductionProbe::from_observed(axes.clone());
        let nonce = probe.nonce;
        let post = axes.clone();
        let run = probe
            .run_with_parts(CONFIG, policy, default_registry(1 << 10), move || post)
            .expect("protocol run");
        assert!(run.citable(), "stable synthetic probes must admit");
        assert_eq!(run.nonce(), nonce);

        let db = temp_db("ok");
        let ledger = Ledger::open(&db).expect("open ledger");
        let kernel = run.results()[0].kernel.clone();
        let version = run.results()[0].version.clone();
        let baseline_hash = policy.baseline_hash();
        let op = run.record(&ledger).expect("record production run");
        let row = ledger.op(op).unwrap().expect("op row");
        let recorded_at = row.t_end.expect("finished op");
        assert!(row.ir.contains("\"protocol\":\"production-v1\""));
        assert!(row.ir.contains(&format!("\"run_nonce\":\"{nonce}\"")));
        assert!(row.ir.contains(&format!(
            "\"pre_axes_receipt\":\"{}\"",
            axes_receipt(&axes)
        )));
        assert!(row.ir.contains(&format!(
            "\"post_axes_receipt\":\"{}\"",
            axes_receipt(&axes)
        )));
        assert!(row.ir.contains("\"admitted\":true"));
        assert_eq!(
            staleness_at(
                &ledger,
                &kernel,
                &version,
                axes.fingerprint,
                baseline_hash,
                recorded_at + 1,
            )
            .expect("staleness"),
            Staleness::Fresh
        );
        cleanup_db(&db);
    }

    fn cleanup_db(path: &str) {
        for suffix in ["", "-wal", "-shm", ".fsqlite-wal", ".fsqlite-shm"] {
            let _ = std::fs::remove_file(format!("{path}{suffix}"));
        }
    }
}
