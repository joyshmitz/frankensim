//! Package-grounded crosswalk coverage (bead qmao.6.1): which
//! regulatory concepts are EVIDENCED by the fields actually present in
//! a package — the static fs-crosswalk mapping alone can never produce
//! "covered".

// ---------------------------------------------------------------------------
// Coverage from ACTUAL package fields (bead qmao.6.1; moved here from
// fs-crosswalk under the layer rule — L6 may know UTIL, never the
// reverse): the static fs-crosswalk table maps concepts to standards; whether a concept is EVIDENCED must
// come from a parsed package, never from the mapping itself — a mapped
// concept with no evidence in hand is "mapped but absent", not covered.
// ---------------------------------------------------------------------------

use crate::{EvidencePackage, VerificationCapabilities};
use fs_crosswalk::{PackageConcept, Standard, crosswalk};
use fs_evidence::{Color, ColorRank};

/// Whether one concept is actually evidenced by a package, with the
/// reason either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptPresence {
    /// The concept.
    pub concept: PackageConcept,
    /// Is it evidenced by fields ACTUALLY PRESENT in this package?
    pub present: bool,
    /// Why / why not (teaching, deterministic).
    pub why: String,
}

fn anchoring_dataset_presence(pkg: &EvidencePackage) -> (bool, String) {
    let named = pkg
        .claims
        .iter()
        .filter(|claim| {
            matches!(&claim.color, Color::Validated { dataset, .. } if !dataset.trim().is_empty())
        })
        .count();
    let anchored: usize = pkg.claims.iter().map(|claim| claim.anchors.len()).sum();
    let matched = pkg
        .claims
        .iter()
        .filter(|claim| claim.has_matching_validated_anchor())
        .count();
    (
        matched > 0,
        format!(
            "{matched} validated claim(s) have matching content-hashed anchors; {named} named \
             dataset(s), {anchored} total anchor record(s) (schema v3)"
        ),
    )
}

fn signature_presence(pkg: &EvidencePackage) -> (bool, String) {
    match &pkg.signature {
        Some(_) => (
            false,
            "detached signature present but not authenticated; raw presence cannot establish \
             sign-off"
                .to_string(),
        ),
        None => (false, "no authenticated detached signature".to_string()),
    }
}

fn rank_presence(pkg: &EvidencePackage, rank: ColorRank, description: &str) -> (bool, String) {
    let count = pkg
        .claims
        .iter()
        .filter(|claim| claim.color.rank() == rank)
        .count();
    (count > 0, format!("{count} {description} claim(s)"))
}

fn certificate_presence(pkg: &EvidencePackage) -> (bool, String) {
    let present = !pkg.claims.is_empty();
    (
        present,
        if present {
            "every claim carries a complete, re-verified color payload".to_string()
        } else {
            "no claims to certify".to_string()
        },
    )
}

fn falsifier_presence(pkg: &EvidencePackage) -> (bool, String) {
    let count: usize = pkg.claims.iter().map(|claim| claim.falsifiers.len()).sum();
    (
        count > 0,
        if count > 0 {
            format!("{count} falsifier record(s) attached to claims")
        } else {
            "no falsifier records attached; absent evidence cannot claim coverage".to_string()
        },
    )
}

fn regime_presence(pkg: &EvidencePackage) -> (bool, String) {
    let count = pkg
        .claims
        .iter()
        .filter(|claim| {
            matches!(&claim.color, Color::Validated { regime, .. } if !regime.bounds().is_empty())
        })
        .count();
    (
        count > 0,
        format!("{count} validated claim(s) with regime bounds"),
    )
}

fn provenance_presence(pkg: &EvidencePackage) -> (bool, String) {
    let present = !pkg.provenance.code_version.trim().is_empty()
        && !pkg.provenance.constellation_lock.trim().is_empty();
    (
        present,
        if present {
            "code version + constellation lock present".to_string()
        } else {
            "provenance fields empty".to_string()
        },
    )
}

fn claim_origin_presence(pkg: &EvidencePackage) -> (bool, String) {
    let count = pkg.claims.len();
    (
        count > 0,
        if count > 0 {
            format!("{count} claim origin(s) passed package verification")
        } else {
            "no claims, therefore no verified claim origins".to_string()
        },
    )
}

fn waiver_authorization_presence(pkg: &EvidencePackage) -> (bool, String) {
    let count = pkg.waiver_claims();
    (
        count > 0,
        if count > 0 {
            format!("{count} waiver authorization(s) authenticated and unexpired")
        } else {
            "no authenticated waiver origins".to_string()
        },
    )
}

fn concept_presence(pkg: &EvidencePackage, concept: PackageConcept) -> ConceptPresence {
    let (present, why) = match concept {
        PackageConcept::VerifiedColor => {
            rank_presence(pkg, ColorRank::Verified, "verified interval-certificate")
        }
        PackageConcept::ValidatedColor => rank_presence(pkg, ColorRank::Validated, "validated"),
        PackageConcept::EstimatedColor => rank_presence(pkg, ColorRank::Estimated, "estimated"),
        PackageConcept::Certificate => certificate_presence(pkg),
        PackageConcept::FalsifierLog => falsifier_presence(pkg),
        PackageConcept::RegimeTag => regime_presence(pkg),
        PackageConcept::AnchoringDataset => anchoring_dataset_presence(pkg),
        PackageConcept::Provenance => provenance_presence(pkg),
        PackageConcept::MerkleRoot => (
            true,
            format!("content root {} recomputable", pkg.merkle_root()),
        ),
        PackageConcept::Signature => signature_presence(pkg),
        PackageConcept::ClaimOrigin => claim_origin_presence(pkg),
        PackageConcept::WaiverAuthorization => waiver_authorization_presence(pkg),
    };
    ConceptPresence {
        concept,
        present,
        why,
    }
}

/// Judge every concept against the fields actually present in `pkg`.
#[must_use]
pub fn package_presence(pkg: &EvidencePackage) -> Vec<ConceptPresence> {
    package_presence_with(pkg, &VerificationCapabilities::deny_all())
}

/// Judge every concept only after package verification with the supplied
/// source-certificate and waiver capabilities.
#[must_use]
pub fn package_presence_with(
    pkg: &EvidencePackage,
    capabilities: &VerificationCapabilities<'_>,
) -> Vec<ConceptPresence> {
    if pkg.verify_with(capabilities).is_err() {
        let why = "package verification failed; fail-closed coverage suppresses every concept"
            .to_string();
        return PackageConcept::ALL
            .iter()
            .map(|&concept| ConceptPresence {
                concept,
                present: false,
                why: why.clone(),
            })
            .collect();
    }

    PackageConcept::ALL
        .iter()
        .map(|&concept| concept_presence(pkg, concept))
        .collect()
}

/// One row of the package-grounded coverage report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageStatus {
    /// Mapped to the standard AND evidenced by this package.
    Covered,
    /// Mapped to the standard but the evidence is ABSENT from this
    /// package — never reported as covered.
    MappedButAbsent,
    /// Deliberately unmapped for this standard (see the table's reason).
    NoClaim,
}

/// Coverage of `standard` derived from the fields actually present in
/// `pkg` (bead qmao.6.1): the static mapping alone can never produce
/// `Covered` — only the intersection with parsed package evidence can.
#[must_use]
pub fn package_coverage(
    pkg: &EvidencePackage,
    standard: Standard,
) -> Vec<(PackageConcept, CoverageStatus, String)> {
    package_coverage_with(pkg, standard, &VerificationCapabilities::deny_all())
}

/// Package-grounded standards coverage after verification with explicit
/// origin capabilities.
#[must_use]
pub fn package_coverage_with(
    pkg: &EvidencePackage,
    standard: Standard,
    capabilities: &VerificationCapabilities<'_>,
) -> Vec<(PackageConcept, CoverageStatus, String)> {
    let presence = package_presence_with(pkg, capabilities);
    crosswalk()
        .iter()
        .filter(|e| e.standard == standard)
        .map(|e| {
            let p = presence
                .iter()
                .find(|pr| pr.concept == e.concept)
                .expect("every concept judged");
            let status = if !e.is_mapped() {
                CoverageStatus::NoClaim
            } else if p.present {
                CoverageStatus::Covered
            } else {
                CoverageStatus::MappedButAbsent
            };
            (e.concept, status, p.why.clone())
        })
        .collect()
}
