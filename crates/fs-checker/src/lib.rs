//! fs-checker — the standalone evidence-package checker (plan addendum,
//! Proposal 12). Layer: L6.
//!
//! "Don't trust us; here is the checker." A third party — an auditor or a
//! regulator — runs THIS to re-verify a FrankenSim [`EvidencePackage`] without
//! trusting the vendor and, crucially, WITHOUT running any solver. The whole
//! proposition rides the check/produce asymmetry (P6): re-verification is cheap
//! and needs neither the solver stack nor a license.
//!
//! # Hard distribution constraint
//! This crate's entire dependency graph is `fs-package` → `fs-evidence`. There
//! is NO solver, NO geometry kernel, NO license gate anywhere in it — by
//! construction the checker CANNOT run a solve, which is exactly the point. It
//! carries its own protocol version ([`CHECKER_PROTOCOL_VERSION`]) because it
//! is distributed independently of the rest of FrankenSim.
//!
//! What it re-verifies: format support + per-claim completeness (delegated to
//! [`EvidencePackage::verify`]), the content address (Merkle root, optionally
//! against an expected value — tamper detection), and signature presence. It
//! renders the by-color budget pie. Everything is deterministic.

pub use fs_package::{ColorBreakdown, EvidencePackage, PackageError};

/// The checker's own protocol version (it is distributed independently).
pub const CHECKER_PROTOCOL_VERSION: u32 = 1;

/// Whether the package carried a detached signature. Cryptographic
/// verification awaits a Franken-compliant signature primitive; until then the
/// bundle is trusted by CONTENT ADDRESS, and a present signature is recorded
/// but not asserted valid (honest, not silently "verified").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// No signature — the bundle stands on its content address alone.
    Unsigned,
    /// A detached signature is present (not cryptographically checked here).
    Present(String),
}

/// The checker's overall verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The package re-verified.
    Pass,
    /// The package failed re-verification (see the findings).
    Fail,
}

/// One reason a check failed.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// A short kind slug.
    pub kind: &'static str,
    /// Human detail.
    pub detail: String,
}

/// The result of running the checker over a package.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckReport {
    /// Pass/Fail.
    pub verdict: Verdict,
    /// The recomputed content address.
    pub merkle_root: u64,
    /// The by-color budget pie.
    pub breakdown: ColorBreakdown,
    /// Signature presence.
    pub signature: SignatureStatus,
    /// Reasons for failure (empty on Pass).
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// Did the package pass?
    #[must_use]
    pub fn passed(&self) -> bool {
        matches!(self.verdict, Verdict::Pass)
    }

    /// Render the by-color budget pie as a deterministic text chart.
    #[must_use]
    pub fn render_pie(&self) -> String {
        use core::fmt::Write as _;
        let b = &self.breakdown;
        let total = b.verified + b.validated + b.estimated;
        if total == 0 {
            return "budget pie: no claims".to_string();
        }
        let mut out = format!("budget pie ({total} claims):\n");
        for (label, count) in [
            ("verified ", b.verified),
            ("validated", b.validated),
            ("estimated", b.estimated),
        ] {
            // ten-cell bar, deterministic integer rounding.
            let filled = (count * 10 + total / 2) / total;
            let pct = (count * 100 + total / 2) / total;
            let bar: String = (0..10)
                .map(|i| if i < filled { '#' } else { '.' })
                .collect();
            writeln!(out, "  {label} {bar} {count} ({pct}%)").expect("write to String");
        }
        out
    }
}

/// Re-verify a package (no expected content address supplied).
#[must_use]
pub fn check(pkg: &EvidencePackage) -> CheckReport {
    build_report(pkg, None)
}

/// Re-verify a package AND confirm its content address matches `expected_root`
/// — a mismatch (tamper, or the wrong package) fails the check.
#[must_use]
pub fn check_against_root(pkg: &EvidencePackage, expected_root: u64) -> CheckReport {
    build_report(pkg, Some(expected_root))
}

fn build_report(pkg: &EvidencePackage, expected_root: Option<u64>) -> CheckReport {
    let mut findings = Vec::new();

    // 1. delegate format + per-claim completeness to the package format.
    let breakdown = match pkg.verify() {
        Ok(report) => report.breakdown,
        Err(e) => {
            findings.push(describe(&e));
            pkg.color_breakdown()
        }
    };

    // 2. content address (recomputed here, independently).
    let merkle_root = pkg.merkle_root();
    if let Some(expected) = expected_root
        && merkle_root != expected
    {
        findings.push(Finding {
            kind: "content-address-mismatch",
            detail: format!("recomputed root {merkle_root:016x} != expected {expected:016x}"),
        });
    }

    // 3. signature presence (recorded, not cryptographically asserted).
    let signature = match &pkg.signature {
        None => SignatureStatus::Unsigned,
        Some(s) => SignatureStatus::Present(s.clone()),
    };

    let verdict = if findings.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    CheckReport {
        verdict,
        merkle_root,
        breakdown,
        signature,
        findings,
    }
}

/// Translate a package error into a checker finding.
fn describe(e: &PackageError) -> Finding {
    match e {
        PackageError::IncompleteValidatedClaim { claim, missing } => Finding {
            kind: "incomplete-validated-claim",
            detail: format!("claim '{claim}' is missing its {missing}"),
        },
        PackageError::IncompleteVerifiedClaim { claim } => Finding {
            kind: "incomplete-verified-claim",
            detail: format!("claim '{claim}' has no valid certificate interval"),
        },
        PackageError::UnsupportedFormat { found } => Finding {
            kind: "unsupported-format",
            detail: format!("package format version {found} is not supported"),
        },
    }
}
