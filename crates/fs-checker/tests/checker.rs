//! Battery for the standalone evidence-package checker (addendum Proposal 12).
//! Covers a clean pass, completeness-failure findings, content-address
//! (Merkle) tamper detection, signature-presence reporting, budget-pie
//! rendering (including the empty case), the protocol version, and
//! determinism. The checker uses only the package format — no solver.

use fs_checker::{
    CHECKER_PROTOCOL_VERSION, ColorBreakdown, ContentHash, SignatureStatus,
    SourceCertificateRequest, SourceCertificateVerifier, Verdict, VerificationCapabilities,
    WaiverGrant, WaiverVerifier, check, check_against_root, check_for_release,
    check_for_release_with_capabilities, check_json, check_json_for_release,
    check_json_for_release_with_capabilities, check_json_with_capabilities,
    check_with_capabilities,
};
use fs_evidence::{Color, ValidityDomain};
use fs_package::{Claim, EvidencePackage, FalsifierRecord, Provenance};

const ARTIFACT_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A deliberately corrupted content root (one byte flipped): the v5
/// 32-byte replacement for the old `root ^ 0xdead` tamper idiom.
fn flip(root: ContentHash) -> ContentHash {
    let mut bytes = *root.as_bytes();
    bytes[0] ^= 0xde;
    ContentHash(bytes)
}

fn prov() -> Provenance {
    Provenance::new("commit-abc", "lock-def")
}
fn verified(id: &str) -> Claim {
    Claim::from_certificate(id, "ok", -1.0, 1.0, "test-solver/cert", ARTIFACT_HASH)
}
fn estimated(id: &str) -> Claim {
    Claim::estimated(id, "maybe", "surrogate", 2.0)
}
fn validated(id: &str, regime: ValidityDomain) -> Claim {
    Claim::anchored(id, "matches", regime, "wt-2026", ARTIFACT_HASH)
}
fn good_regime() -> ValidityDomain {
    ValidityDomain::unconstrained().with("Re", 1e5, 3e5)
}

struct ExactSourceVerifier<'a> {
    claim_id: &'a str,
}

impl SourceCertificateVerifier for ExactSourceVerifier<'_> {
    fn verify(&self, request: &SourceCertificateRequest<'_>) -> bool {
        request.package_provenance == &prov()
            && request.claim_index == 0
            && request.claim_id == self.claim_id
            && request.statement == "ok"
            && request.lo.to_bits() == (-1.0f64).to_bits()
            && request.hi.to_bits() == 1.0f64.to_bits()
            && request.producer == "test-solver/cert"
            && request.certificate_hash.to_hex() == ARTIFACT_HASH
    }
}

#[test]
fn a_valid_package_passes_with_no_findings() {
    let pkg = EvidencePackage::new(prov())
        .with_claim(verified("c1"))
        .with_claim(validated("c2", good_regime()))
        .with_claim(estimated("c3"));
    let source_verifier = ExactSourceVerifier { claim_id: "c1" };
    let capabilities =
        VerificationCapabilities::deny_all().with_source_certificates(&source_verifier);
    let report = check_with_capabilities(&pkg, None, None, &capabilities);
    assert!(report.passed());
    assert_eq!(report.verdict, Verdict::Pass);
    assert!(report.findings.is_empty());
    assert_eq!(report.merkle_root, pkg.merkle_root());
    assert_eq!(report.breakdown.verified, 1);
    assert_eq!(report.breakdown.validated, 1);
    assert_eq!(report.breakdown.estimated, 1);
}

#[test]
fn an_incomplete_validated_claim_fails_the_check() {
    // unconstrained regime = missing regime tag.
    let pkg =
        EvidencePackage::new(prov()).with_claim(validated("v", ValidityDomain::unconstrained()));
    let report = check(&pkg);
    assert!(!report.passed());
    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].kind, "incomplete-validated-claim");
}

#[test]
fn a_semantically_empty_falsifier_record_fails_the_check() {
    let pkg =
        EvidencePackage::new(prov()).with_claim(verified("v").with_falsifier(FalsifierRecord {
            name: " ".to_string(),
            attempts: 0,
            refuted: false,
            detail: " ".to_string(),
        }));
    let report = check(&pkg);
    assert!(!report.passed());
    assert_eq!(report.findings[0].kind, "invalid-falsifier-record");
    assert_eq!(report.breakdown, ColorBreakdown::default());
    assert_eq!(report.render_pie(), "budget pie: no claims");
}

#[test]
fn placeholder_claim_and_falsifier_text_fail_the_check() {
    let placeholder_statement = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "claim",
        "TODO",
        0.0,
        1.0,
        "test-solver/cert",
        ARTIFACT_HASH,
    ));
    let report = check(&placeholder_statement);
    assert!(!report.passed());
    assert_eq!(report.findings[0].kind, "invalid-claim-statement");

    let placeholder_falsifier = EvidencePackage::new(prov()).with_claim(
        verified("claim").with_falsifier(FalsifierRecord {
            name: "independent-probe".to_string(),
            attempts: 1,
            refuted: false,
            detail: "placeholder".to_string(),
        }),
    );
    let report = check(&placeholder_falsifier);
    assert!(!report.passed());
    assert_eq!(report.findings[0].kind, "invalid-falsifier-record");
}

#[test]
fn content_address_mismatch_is_caught() {
    let pkg = EvidencePackage::new(prov()).with_claim(estimated("c1"));
    let real_root = pkg.merkle_root();
    // the right root passes.
    assert!(check_against_root(&pkg, real_root).passed());
    // a wrong expected root (a tampered/substituted package) fails.
    let report = check_against_root(&pkg, flip(real_root));
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == "content-address-mismatch")
    );
}

#[test]
fn content_address_mismatch_catches_provenance_tamper() {
    let pkg = EvidencePackage::new(prov()).with_claim(estimated("c1"));
    let root = pkg.merkle_root();
    let tampered = EvidencePackage::new(Provenance::new("commit-evil", "lock-def"))
        .with_claim(estimated("c1"));

    let report = check_against_root(&tampered, root);
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == "content-address-mismatch")
    );
}

#[test]
fn signature_presence_is_reported() {
    let unsigned = EvidencePackage::new(prov()).with_claim(estimated("e1"));
    assert_eq!(check(&unsigned).signature, SignatureStatus::Unsigned);
    let signed = unsigned.signed("ed25519:cafe");
    assert_eq!(
        check(&signed).signature,
        SignatureStatus::Unverified("ed25519:cafe".to_string())
    );
}

#[test]
fn the_budget_pie_renders_deterministically() {
    let pkg = EvidencePackage::new(prov())
        .with_claim(verified("c1"))
        .with_claim(estimated("c2"))
        .with_claim(estimated("c3"));
    let source_verifier = ExactSourceVerifier { claim_id: "c1" };
    let capabilities =
        VerificationCapabilities::deny_all().with_source_certificates(&source_verifier);
    let pie = check_with_capabilities(&pkg, None, None, &capabilities).render_pie();
    assert_eq!(
        pie,
        check_with_capabilities(&pkg, None, None, &capabilities).render_pie()
    );
    assert!(pie.contains("budget pie (3 claims)"));
    assert!(pie.contains("verified") && pie.contains("estimated"));
    assert!(pie.contains('#') && pie.contains('.'));
}

#[test]
fn the_budget_pie_handles_an_empty_package() {
    let pkg = EvidencePackage::new(prov());
    // an empty package still verifies (vacuously) and renders a no-claims pie.
    let report = check(&pkg);
    assert!(report.passed());
    assert_eq!(report.render_pie(), "budget pie: no claims");
}

struct ReleaseVerifier;

impl fs_checker::SignatureVerifier for ReleaseVerifier {
    fn verify(&self, merkle_root: &ContentHash, signature: &str) -> bool {
        signature == format!("release-test:{merkle_root}")
    }
}

fn signed_for_release(pkg: EvidencePackage) -> EvidencePackage {
    let root = pkg.merkle_root();
    pkg.signed(format!("release-test:{root}"))
}

fn passed_falsifier() -> FalsifierRecord {
    FalsifierRecord {
        name: "independent-interval-probe".to_string(),
        attempts: 64,
        refuted: false,
        detail: "64 boundary-biased probes found no violation".to_string(),
    }
}

fn assert_capability_refusal(report: &fs_checker::CheckReport, kind: &str) {
    assert!(!report.passed(), "capability refusal unexpectedly passed");
    assert_eq!(
        report.breakdown,
        ColorBreakdown::default(),
        "refused origin retained a positive evidence breakdown"
    );
    assert!(
        report.findings.iter().any(|finding| finding.kind == kind),
        "missing {kind} finding: {:?}",
        report.findings
    );
}

fn fixture_waiver_mac(message: &[u8]) -> String {
    let mut state = 0xcbf2_9ce4_8422_2325u64;
    for byte in message {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("checker-fixture:{state:016x}")
}

struct ExactWaiverVerifier;

impl WaiverVerifier for ExactWaiverVerifier {
    fn verify(&self, mac: &str, message: &[u8]) -> bool {
        mac == fixture_waiver_mac(message)
    }
}

fn waived_fixture() -> EvidencePackage {
    let pending = EvidencePackage::new(prov()).with_claim(
        Claim::waived(
            "waived",
            "authorized interval",
            Color::Verified { lo: -1.0, hi: 1.0 },
            WaiverGrant {
                waiver_id: "checker-waiver-2026".to_string(),
                expiry_day: 300,
                mac: "pending-authenticator".to_string(),
            },
        )
        .with_falsifier(passed_falsifier()),
    );
    let message = pending.waiver_message(0).expect("waiver target");
    pending
        .with_waiver_mac(0, fixture_waiver_mac(&message))
        .expect("install exact fixture authenticator")
}

#[test]
fn source_certificates_are_capability_gated_across_every_entry_path() {
    let unsigned = EvidencePackage::new(prov())
        .with_claim(verified("source").with_falsifier(passed_falsifier()));
    let root = unsigned.merkle_root();
    let signed = signed_for_release(unsigned.clone());

    for report in [
        check(&unsigned),
        check_json(&unsigned.to_json(), Some(root), None),
        check_for_release(&signed, root, &ReleaseVerifier),
        check_json_for_release(&signed.to_json(), root, &ReleaseVerifier),
    ] {
        assert_capability_refusal(&report, "source-certificate-refused");
    }

    let exact = ExactSourceVerifier { claim_id: "source" };
    let capabilities = VerificationCapabilities::deny_all().with_source_certificates(&exact);
    for report in [
        check_with_capabilities(&unsigned, Some(root), None, &capabilities),
        check_json_with_capabilities(&unsigned.to_json(), Some(root), None, &capabilities),
    ] {
        assert!(report.passed(), "{:?}", report.findings);
        assert_eq!(report.signature, SignatureStatus::Unsigned);
        assert_eq!(report.breakdown.verified, 1);
    }
    for report in [
        check_for_release_with_capabilities(&signed, root, &ReleaseVerifier, &capabilities),
        check_json_for_release_with_capabilities(
            &signed.to_json(),
            root,
            &ReleaseVerifier,
            &capabilities,
        ),
    ] {
        assert!(report.passed(), "{:?}", report.findings);
        assert!(matches!(report.signature, SignatureStatus::Valid(_)));
    }

    let wrong_subject = ExactSourceVerifier {
        claim_id: "different-source",
    };
    let wrong_capabilities =
        VerificationCapabilities::deny_all().with_source_certificates(&wrong_subject);
    assert_capability_refusal(
        &check_with_capabilities(&unsigned, None, None, &wrong_capabilities),
        "source-certificate-refused",
    );
}

#[test]
fn waivers_are_capability_gated_across_every_entry_path() {
    let unsigned = waived_fixture();
    let root = unsigned.merkle_root();
    let signed = signed_for_release(unsigned.clone());

    for report in [
        check(&unsigned),
        check_json(&unsigned.to_json(), Some(root), None),
        check_for_release(&signed, root, &ReleaseVerifier),
        check_json_for_release(&signed.to_json(), root, &ReleaseVerifier),
    ] {
        assert_capability_refusal(&report, "waiver-refused");
    }

    let waiver_verifier = ExactWaiverVerifier;
    let capabilities = VerificationCapabilities::deny_all().with_waivers(&waiver_verifier, 250);
    for report in [
        check_with_capabilities(&unsigned, Some(root), None, &capabilities),
        check_json_with_capabilities(&unsigned.to_json(), Some(root), None, &capabilities),
    ] {
        assert!(report.passed(), "{:?}", report.findings);
        assert_eq!(report.signature, SignatureStatus::Unsigned);
        assert_eq!(report.breakdown.verified, 1);
    }
    for report in [
        check_for_release_with_capabilities(&signed, root, &ReleaseVerifier, &capabilities),
        check_json_for_release_with_capabilities(
            &signed.to_json(),
            root,
            &ReleaseVerifier,
            &capabilities,
        ),
    ] {
        assert!(report.passed(), "{:?}", report.findings);
        assert!(matches!(report.signature, SignatureStatus::Valid(_)));
    }

    let expired = VerificationCapabilities::deny_all().with_waivers(&waiver_verifier, 301);
    assert_capability_refusal(
        &check_with_capabilities(&unsigned, None, None, &expired),
        "waiver-refused",
    );
}

#[test]
fn release_gate_requires_certificate_obligations() {
    let pkg = signed_for_release(
        EvidencePackage::new(prov())
            .with_claim(verified("verified").with_falsifier(passed_falsifier()))
            .with_claim(validated("validated", good_regime()).with_falsifier(passed_falsifier()))
            .with_claim(estimated("honest-estimate")),
    );
    let source_verifier = ExactSourceVerifier {
        claim_id: "verified",
    };
    let capabilities =
        VerificationCapabilities::deny_all().with_source_certificates(&source_verifier);
    let root = pkg.merkle_root();
    let report = check_for_release_with_capabilities(&pkg, root, &ReleaseVerifier, &capabilities);
    assert!(report.passed(), "{:?}", report.findings);
    assert!(matches!(report.signature, SignatureStatus::Valid(_)));
    assert!(
        check_json_for_release_with_capabilities(
            &pkg.to_json(),
            root,
            &ReleaseVerifier,
            &capabilities,
        )
        .passed()
    );
}

#[test]
fn release_gate_refuses_vacuous_or_unpaired_packages() {
    let empty = signed_for_release(EvidencePackage::new(prov()));
    assert!(
        check(&empty).passed(),
        "ordinary integrity check stays vacuous"
    );
    let report = check_for_release(&empty, empty.merkle_root(), &ReleaseVerifier);
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.kind == "release-empty-package")
    );

    let unpaired = signed_for_release(EvidencePackage::new(prov()).with_claim(verified("v")));
    let source_verifier = ExactSourceVerifier { claim_id: "v" };
    let capabilities =
        VerificationCapabilities::deny_all().with_source_certificates(&source_verifier);
    let report = check_for_release_with_capabilities(
        &unpaired,
        unpaired.merkle_root(),
        &ReleaseVerifier,
        &capabilities,
    );
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.kind == "release-falsifier-required")
    );
    let report = check_json_for_release_with_capabilities(
        &unpaired.to_json(),
        unpaired.merkle_root(),
        &ReleaseVerifier,
        &capabilities,
    );
    assert!(!report.passed(), "JSON must not bypass release policy");
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.kind == "release-falsifier-required")
    );
}

#[test]
fn release_gate_requires_matching_anchor_signature_and_root() {
    // Schema v5: the sealed `anchored` constructor attaches the matching
    // anchor, so an in-memory validated-without-anchor package is
    // unconstructible. The release anchor gate is now exercised through
    // the PARSE path: strip the matching anchor from the transported
    // JSON and the recomputed root refuses before the gate is even
    // reached — the transported form cannot lose its anchor silently.
    let anchored_pkg = signed_for_release(
        EvidencePackage::new(prov())
            .with_claim(validated("v", good_regime()).with_falsifier(passed_falsifier())),
    );
    let json = anchored_pkg.to_json();
    let stripped = json.replacen(
        "{\"dataset_id\":\"wt-2026\",\"content_hash\"",
        "{\"dataset_id\":\"different-dataset\",\"content_hash\"",
        1,
    );
    assert!(
        fs_checker::EvidencePackage::from_json(&stripped).is_err(),
        "anchor tamper breaks the content address at parse"
    );
    let report = check_for_release(&anchored_pkg, anchored_pkg.merkle_root(), &ReleaseVerifier);
    assert!(report.passed(), "{:?}", report.findings);
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.kind == "release-anchor-required")
    );

    let unsigned = EvidencePackage::new(prov()).with_claim(estimated("v"));
    let report = check_for_release(&unsigned, unsigned.merkle_root(), &ReleaseVerifier);
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.kind == "release-signature-required")
    );

    let signed = signed_for_release(unsigned);
    let report = check_for_release(&signed, flip(signed.merkle_root()), &ReleaseVerifier);
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.kind == "content-address-mismatch")
    );
}

#[test]
fn the_checker_advertises_its_protocol_version() {
    assert_eq!(CHECKER_PROTOCOL_VERSION, 3);
    assert_eq!(fs_checker::CHECKER_SUPPORTED_PACKAGE_FORMAT, 5);
    assert_eq!(
        fs_checker::CHECKER_SUPPORTED_PACKAGE_FORMAT,
        fs_package::FORMAT_VERSION
    );
}

#[test]
fn checking_is_deterministic() {
    let pkg = EvidencePackage::new(prov())
        .with_claim(estimated("c1"))
        .with_claim(estimated("c2"));
    assert_eq!(check(&pkg), check(&pkg));
}

/// qmao.6.1 — the third-party JSON path: parse-refused inputs never
/// pass; signature validity is asserted only through a capability over
/// the recomputed root; tamper anywhere fails.
#[test]
fn checker_json_path_and_signature_capability() {
    use fs_checker::{NoSignatureVerifier, SignatureVerifier, check_json, check_with};
    struct MacVerifier;
    fn mac(root: &ContentHash) -> String {
        format!("test-key/{root}")
    }
    impl SignatureVerifier for MacVerifier {
        fn verify(&self, merkle_root: &ContentHash, signature: &str) -> bool {
            signature == mac(merkle_root)
        }
    }
    let base = EvidencePackage::new(Provenance::new("v1.0", "lock:abc"))
        .with_claim(Claim::estimated("c1", "bounded", "surrogate", 1.0));
    let root = base.merkle_root();
    let pkg = base.signed(mac(&root));
    // Valid signature via the capability.
    let report = check_with(&pkg, Some(root), &MacVerifier);
    assert!(report.passed(), "{:?}", report.findings);
    assert!(matches!(
        report.signature,
        fs_checker::SignatureStatus::Valid(_)
    ));
    // The no-crypto default cannot assert validity (fail closed: the
    // signature stays Unverified and a finding is raised).
    let report = check_with(&pkg, Some(root), &NoSignatureVerifier);
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == "signature-invalid")
    );
    // Full JSON path: round trip passes; tampered JSON is parse-refused
    // (never a Pass with quietly wrong content).
    let json = pkg.to_json();
    assert!(check_json(&json, Some(root), Some(&MacVerifier)).passed());
    let tampered = json.replace("bounded", "PROVEN");
    let report = check_json(&tampered, Some(root), Some(&MacVerifier));
    assert!(!report.passed());
    assert_eq!(report.findings[0].kind, "parse-refused");
}
