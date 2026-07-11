//! Battery for evidence packages (addendum Proposal 12). Covers a complete
//! mixed-color package, the all-estimated boundary (still valid, round-trips),
//! completeness failures (validated claim missing regime / dataset, verified
//! claim with a bad interval), Merkle content-addressing (determinism + tamper
//! detection), the format-version gate, optional signature, the color
//! breakdown, and deterministic JSON.

use fs_evidence::{Color, IntervalOp, ValidityDomain};
use fs_package::{
    Claim, EvidencePackage, FalsifierRecord, MAX_JSON_CONTAINER_ITEMS, MAX_JSON_DEPTH,
    MAX_JSON_NUMBER_BYTES, MAX_JSON_STRING_BYTES, PackageError, PackageReport, Provenance,
    SourceCertificateRequest, SourceCertificateVerifier, VerificationCapabilities, WaiverGrant,
    WaiverVerifier,
};

const CANONICAL_DATASET_HASH: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct FixtureSourceVerifier;

fn fixture_source_hash(
    provenance: &Provenance,
    claim_index: usize,
    claim_id: &str,
    statement: &str,
    lo: f64,
    hi: f64,
    producer: &str,
) -> String {
    use core::fmt::Write as _;

    fn push_atom(out: &mut String, atom: &str) {
        use core::fmt::Write as _;
        let _ = write!(out, "{}:{atom}|", atom.len());
    }

    let mut subject = String::from("fs-package:test:source-certificate-subject|");
    push_atom(&mut subject, &provenance.code_version);
    push_atom(&mut subject, &provenance.constellation_lock);
    let _ = write!(subject, "index:{claim_index}|");
    push_atom(&mut subject, claim_id);
    push_atom(&mut subject, statement);
    let _ = write!(subject, "lo:{}|hi:{}|", lo.to_bits(), hi.to_bits());
    push_atom(&mut subject, producer);
    fs_blake3::hash_domain("fs-package:test:source-certificate", subject.as_bytes()).to_hex()
}

impl SourceCertificateVerifier for FixtureSourceVerifier {
    fn verify(&self, request: &SourceCertificateRequest<'_>) -> bool {
        request.certificate_hash.to_hex()
            == fixture_source_hash(
                request.package_provenance,
                request.claim_index,
                request.claim_id,
                request.statement,
                request.lo,
                request.hi,
                request.producer,
            )
    }
}

static FIXTURE_SOURCE_VERIFIER: FixtureSourceVerifier = FixtureSourceVerifier;

fn source_capabilities() -> VerificationCapabilities<'static> {
    VerificationCapabilities::deny_all().with_source_certificates(&FIXTURE_SOURCE_VERIFIER)
}

fn verify_package(pkg: &EvidencePackage) -> Result<PackageReport, PackageError> {
    pkg.verify_with(&source_capabilities())
}

fn prov() -> Provenance {
    Provenance::new("commit-abc123", "lock-deadbeef")
}

fn source_claim(
    provenance: &Provenance,
    claim_index: usize,
    id: &str,
    statement: &str,
    lo: f64,
    hi: f64,
) -> Claim {
    let producer = "test-solver/cert";
    Claim::from_certificate(
        id,
        statement,
        lo,
        hi,
        producer,
        fixture_source_hash(provenance, claim_index, id, statement, lo, hi, producer),
    )
}

fn verified(id: &str) -> Claim {
    source_claim(
        &prov(),
        0,
        id,
        &format!("{id}: stress <= sigma*"),
        -1.0,
        1.0,
    )
}
fn estimated(id: &str) -> Claim {
    Claim::estimated(id, format!("{id}: surrogate says ok"), "surrogate", 2.0)
}
fn validated(id: &str, regime: ValidityDomain, dataset: &str) -> Claim {
    Claim::anchored(
        id,
        format!("{id}: matches data"),
        regime,
        dataset,
        CANONICAL_DATASET_HASH,
    )
}
fn good_regime() -> ValidityDomain {
    ValidityDomain::unconstrained().with("Re", 1e5, 3e5)
}

fn derived_package() -> EvidencePackage {
    let verified_color = |lo: f64, hi: f64| Color::Verified { lo, hi };
    let provenance = Provenance::new("v", "l");
    EvidencePackage::new(provenance.clone())
        .with_claim(source_claim(&provenance, 0, "a", "left", 1.0, 2.0))
        .with_claim(source_claim(&provenance, 1, "b", "right", 10.0, 20.0))
        .with_claim(
            Claim::derived(
                "c",
                "sum",
                fs_evidence::compose(
                    &verified_color(1.0, 2.0),
                    &verified_color(10.0, 20.0),
                    IntervalOp::Add,
                ),
                vec![0, 1],
                IntervalOp::Add,
            )
            .with_falsifier(FalsifierRecord {
                name: "interval-probe".to_string(),
                attempts: 512,
                refuted: false,
                detail: "no violation found".to_string(),
            })
            .with_anchor("wt-2026-run9", CANONICAL_DATASET_HASH),
        )
}

fn assert_serialized_refuses(pkg: &EvidencePackage) {
    assert!(
        EvidencePackage::from_json(&pkg.to_json()).is_err(),
        "serialized package must refuse the same semantics as verify(): {pkg:?}"
    );
}

#[test]
fn fixture_source_authority_binds_every_typed_request_field() {
    let provenance = prov();
    let other_provenance = Provenance::new("different-commit", "lock-deadbeef");
    let hash = fixture_source_hash(
        &provenance,
        0,
        "claim",
        "certified subject",
        1.0,
        2.0,
        "test-solver/cert",
    );
    let request = SourceCertificateRequest {
        package_provenance: &provenance,
        claim_index: 0,
        claim_id: "claim",
        statement: "certified subject",
        lo: 1.0,
        hi: 2.0,
        producer: "test-solver/cert",
        certificate_hash: fs_package::ContentHash::from_hex(&hash).expect("fixture hash"),
    };
    assert!(FIXTURE_SOURCE_VERIFIER.verify(&request));
    for altered in [
        SourceCertificateRequest {
            package_provenance: &other_provenance,
            ..request
        },
        SourceCertificateRequest {
            claim_index: 1,
            ..request
        },
        SourceCertificateRequest {
            claim_id: "other-claim",
            ..request
        },
        SourceCertificateRequest {
            statement: "different subject",
            ..request
        },
        SourceCertificateRequest { lo: 0.0, ..request },
        SourceCertificateRequest { hi: 3.0, ..request },
        SourceCertificateRequest {
            producer: "other-solver/cert",
            ..request
        },
    ] {
        assert!(
            !FIXTURE_SOURCE_VERIFIER.verify(&altered),
            "fixture authority accepted a modified typed request: {altered:?}"
        );
    }
}

#[test]
fn a_complete_mixed_color_package_verifies() {
    let pkg = EvidencePackage::new(prov())
        .with_claim(verified("c1"))
        .with_claim(validated("c2", good_regime(), "wind-tunnel-2026"))
        .with_claim(estimated("c3"));
    let report = verify_package(&pkg).expect("complete package verifies");
    assert_eq!(report.claims, 3);
    assert_eq!(report.breakdown.verified, 1);
    assert_eq!(report.breakdown.validated, 1);
    assert_eq!(report.breakdown.estimated, 1);
    assert_eq!(report.merkle_root, pkg.merkle_root());
    assert_ne!(report.merkle_root.as_bytes(), &[0u8; 32]);
}

#[test]
fn an_all_estimated_package_is_still_valid_and_round_trips() {
    let pkg = EvidencePackage::new(prov())
        .with_claim(estimated("e1"))
        .with_claim(estimated("e2"));
    let report = pkg.verify().expect("all-estimated is honest, not invalid");
    assert_eq!(report.breakdown.estimated, 2);
    assert_eq!(report.breakdown.verified, 0);
    let json = pkg.to_json();
    assert!(json.contains("\"estimated\""));
    assert!(json.contains("e1") && json.contains("e2"));
}

#[test]
fn a_validated_claim_missing_its_regime_fails_completeness() {
    // an unconstrained (empty) regime = no regime tag.
    let pkg = EvidencePackage::new(prov()).with_claim(validated(
        "v",
        ValidityDomain::unconstrained(),
        "some-data",
    ));
    assert!(matches!(
        pkg.verify(),
        Err(PackageError::IncompleteValidatedClaim {
            missing: "regime",
            ..
        })
    ));
}

#[test]
fn a_validated_claim_missing_its_dataset_fails_completeness() {
    let pkg = EvidencePackage::new(prov()).with_claim(validated("v", good_regime(), "   "));
    // Schema v5: the sealed constructor anchors the claim to its own
    // dataset, so a blank dataset now refuses at the anchor-record gate
    // (earlier, and just as closed) rather than at color completeness.
    assert!(matches!(
        pkg.verify(),
        Err(PackageError::InvalidAnchorRecord {
            field: "dataset_id",
            ..
        })
    ));
}

#[test]
fn a_verified_claim_with_a_bad_interval_fails() {
    let pkg = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "v",
        "backwards",
        5.0,
        1.0,
        "test-solver/cert",
        CANONICAL_DATASET_HASH,
    ));
    assert!(matches!(
        pkg.verify(),
        Err(PackageError::IncompleteVerifiedClaim { .. })
    ));
}

#[test]
fn in_memory_and_serialized_provenance_and_identity_gates_are_identical() {
    let bad_code = EvidencePackage::new(Provenance::new(" ", "lock")).with_claim(verified("c"));
    assert!(matches!(
        bad_code.verify(),
        Err(PackageError::IncompleteProvenance {
            missing: "code_version"
        })
    ));
    assert_serialized_refuses(&bad_code);

    let bad_lock = EvidencePackage::new(Provenance::new("commit", "\t")).with_claim(verified("c"));
    assert!(matches!(
        bad_lock.verify(),
        Err(PackageError::IncompleteProvenance {
            missing: "constellation_lock"
        })
    ));
    assert_serialized_refuses(&bad_lock);

    let blank_id = EvidencePackage::new(prov()).with_claim(verified("  "));
    assert!(matches!(
        blank_id.verify(),
        Err(PackageError::InvalidClaimId {
            index: 0,
            reason: "blank",
            ..
        })
    ));
    assert_serialized_refuses(&blank_id);

    let duplicate_id = EvidencePackage::new(prov())
        .with_claim(verified("same"))
        .with_claim(estimated("same"));
    assert!(matches!(
        duplicate_id.verify(),
        Err(PackageError::InvalidClaimId {
            index: 1,
            reason: "duplicate",
            ..
        })
    ));
    assert_serialized_refuses(&duplicate_id);
}

#[test]
fn in_memory_and_serialized_claim_statements_must_be_meaningful() {
    for (statement, reason) in [
        (" \t", "blank"),
        ("TODO", "placeholder"),
        (" n/A ", "placeholder"),
    ] {
        let pkg = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
            "claim",
            statement,
            0.0,
            1.0,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ));
        assert!(matches!(
            pkg.verify(),
            Err(PackageError::InvalidClaimStatement {
                claim,
                reason: found,
            }) if claim == "claim" && found == reason
        ));
        assert_serialized_refuses(&pkg);
    }
}

#[test]
fn in_memory_and_serialized_estimate_gates_are_identical() {
    let blank_estimator = EvidencePackage::new(prov()).with_claim(Claim::estimated(
        "e",
        "missing estimator identity",
        " ",
        1.0,
    ));
    assert!(matches!(
        blank_estimator.verify(),
        Err(PackageError::IncompleteEstimatedClaim {
            missing: "estimator",
            ..
        })
    ));
    assert_serialized_refuses(&blank_estimator);

    for dispersion in [-1.0, f64::NEG_INFINITY, f64::NAN] {
        let pkg = EvidencePackage::new(prov()).with_claim(Claim::estimated(
            "e",
            "invalid dispersion",
            "probe",
            dispersion,
        ));
        assert!(matches!(
            pkg.verify(),
            Err(PackageError::InvalidEstimatedDispersion { .. })
        ));
        assert_serialized_refuses(&pkg);
    }

    let explicitly_unbounded = EvidencePackage::new(prov()).with_claim(Claim::estimated(
        "unbounded",
        "honest no-spread-claim sentinel",
        "regime-exit",
        f64::INFINITY,
    ));
    assert!(explicitly_unbounded.verify().is_ok());
    assert!(
        explicitly_unbounded
            .magnitude_budget()
            .estimated_dispersion
            .is_infinite()
    );
    assert_eq!(
        EvidencePackage::from_json(&explicitly_unbounded.to_json()).unwrap(),
        explicitly_unbounded
    );
}

#[test]
fn in_memory_and_serialized_magnitude_gates_are_identical() {
    let width_overflow = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "wide",
        "finite endpoints whose width overflows",
        -f64::MAX,
        f64::MAX,
        "test-solver/cert",
        CANONICAL_DATASET_HASH,
    ));
    assert!(matches!(
        width_overflow.verify(),
        Err(PackageError::MagnitudeOverflow {
            component: "verified_width",
            ..
        })
    ));
    assert_serialized_refuses(&width_overflow);

    let dispersion_overflow = EvidencePackage::new(prov())
        .with_claim(Claim::estimated(
            "d1",
            "large finite dispersion",
            "probe-1",
            f64::MAX,
        ))
        .with_claim(Claim::estimated(
            "d2",
            "second large finite dispersion",
            "probe-2",
            f64::MAX,
        ));
    assert!(matches!(
        dispersion_overflow.verify(),
        Err(PackageError::MagnitudeOverflow {
            component: "estimated_dispersion",
            ..
        })
    ));
    assert_serialized_refuses(&dispersion_overflow);

    let large = f64::MAX * 0.75;
    let cross_component_overflow = EvidencePackage::new(prov())
        .with_claim(Claim::from_certificate(
            "wide-finite",
            "large but finite interval width",
            0.0,
            large,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(Claim::estimated(
            "spread-finite",
            "large but finite estimated spread",
            "probe",
            large,
        ));
    assert!(matches!(
        cross_component_overflow.verify(),
        Err(PackageError::MagnitudeOverflow {
            component: "quantified_total",
            ..
        })
    ));
    assert_serialized_refuses(&cross_component_overflow);
}

#[test]
fn in_memory_and_serialized_regime_gates_are_identical() {
    for regime in [
        ValidityDomain::unconstrained().with(" ", 1.0, 2.0),
        ValidityDomain::unconstrained().with("Re", 1.0, f64::INFINITY),
    ] {
        let pkg = EvidencePackage::new(prov()).with_claim(validated("v", regime, "dataset"));
        assert!(matches!(
            pkg.verify(),
            Err(PackageError::InvalidValidatedRegime { .. })
        ));
        assert_serialized_refuses(&pkg);
    }

    let ordered = EvidencePackage::new(prov()).with_claim(validated(
        "ordered",
        ValidityDomain::unconstrained().with("Re", 1.0, 2.0),
        "dataset",
    ));
    let ordered_pair = format!(
        "[\"{:016x}\",\"{:016x}\"]",
        1.0_f64.to_bits(),
        2.0_f64.to_bits()
    );
    let inverted_pair = format!(
        "[\"{:016x}\",\"{:016x}\"]",
        2.0_f64.to_bits(),
        1.0_f64.to_bits()
    );
    let inverted_json = ordered.to_json().replace(&ordered_pair, &inverted_pair);
    let err = EvidencePackage::from_json(&inverted_json).expect_err("inverted regime refuses");
    assert!(err.why.contains("inverted bounds"), "{err}");
}

#[test]
fn falsifier_attempt_counts_round_trip_without_f64_precision_loss() {
    let first_unrepresentable_integer = (1_u64 << 53) + 1;
    let pkg = EvidencePackage::new(prov()).with_claim(
        verified("count")
            .with_falsifier(FalsifierRecord {
                name: "precision-probe".to_string(),
                attempts: first_unrepresentable_integer,
                refuted: false,
                detail: "one above f64's exact-integer range".to_string(),
            })
            .with_falsifier(FalsifierRecord {
                name: "exhaustive-probe".to_string(),
                attempts: u64::MAX,
                refuted: false,
                detail: "full-width counter".to_string(),
            }),
    );
    let json = pkg.to_json();
    assert!(json.contains("\"attempts\":9007199254740993"));
    assert!(json.contains("\"attempts\":18446744073709551615"));
    let back = EvidencePackage::from_json(&json).expect("full-width u64 values parse exactly");
    assert_eq!(back, pkg);
    assert_eq!(
        back.claims[0].falsifiers()[0].attempts,
        first_unrepresentable_integer
    );
    assert_eq!(back.claims[0].falsifiers()[1].attempts, u64::MAX);

    let overflow = json.replace("18446744073709551615", "18446744073709551616");
    let err = EvidencePackage::from_json(&overflow).expect_err("u64 overflow must refuse");
    assert!(err.why.contains("out of range"), "{err}");
}

#[test]
fn falsifier_records_require_identity_work_and_outcome() {
    let assert_invalid = |record: FalsifierRecord, expected_field: &'static str| {
        let pkg = EvidencePackage::new(prov()).with_claim(verified("f").with_falsifier(record));
        assert!(matches!(
            pkg.verify(),
            Err(PackageError::InvalidFalsifierRecord {
                falsifier: 0,
                field,
                ..
            }) if field == expected_field
        ));
        assert_serialized_refuses(&pkg);
    };

    assert_invalid(
        FalsifierRecord {
            name: " \t".to_string(),
            attempts: 1,
            refuted: false,
            detail: "no violation".to_string(),
        },
        "name",
    );
    assert_invalid(
        FalsifierRecord {
            name: "interval-probe".to_string(),
            attempts: 0,
            refuted: false,
            detail: "no work ran".to_string(),
        },
        "attempts",
    );
    assert_invalid(
        FalsifierRecord {
            name: "interval-probe".to_string(),
            attempts: 1,
            refuted: false,
            detail: "  ".to_string(),
        },
        "detail",
    );
    for placeholder in ["TODO", " placeholder ", "N/A", "not run", "unknown"] {
        assert_invalid(
            FalsifierRecord {
                name: placeholder.to_string(),
                attempts: 1,
                refuted: false,
                detail: "no violation observed".to_string(),
            },
            "name",
        );
        assert_invalid(
            FalsifierRecord {
                name: "interval-probe".to_string(),
                attempts: 1,
                refuted: false,
                detail: placeholder.to_string(),
            },
            "detail",
        );
    }
}

#[test]
fn anchor_records_require_identity_and_canonical_content_hash() {
    let cases = [
        (" ", CANONICAL_DATASET_HASH, "dataset_id"),
        ("dataset", "deadbeef", "content_hash"),
        (
            "dataset",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
            "content_hash",
        ),
        (
            "dataset",
            "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
            "content_hash",
        ),
    ];
    for (dataset_id, content_hash, expected_field) in cases {
        let claim = verified("a").with_anchor(dataset_id.to_string(), content_hash.to_string());
        let pkg = EvidencePackage::new(prov()).with_claim(claim);
        assert!(matches!(
            pkg.verify(),
            Err(PackageError::InvalidAnchorRecord {
                anchor: 0,
                field,
                ..
            }) if field == expected_field
        ));
        assert_serialized_refuses(&pkg);
    }
}

#[test]
fn untrusted_json_resource_limits_fail_before_schema_mapping() {
    let nested = format!(
        "{}null{}",
        "[".repeat(MAX_JSON_DEPTH + 1),
        "]".repeat(MAX_JSON_DEPTH + 1)
    );
    let err = EvidencePackage::from_json(&nested).expect_err("deep nesting must refuse");
    assert!(err.why.contains("nesting depth"), "{err}");

    let oversized_string = format!("\"{}\"", "x".repeat(MAX_JSON_STRING_BYTES + 1));
    let err =
        EvidencePackage::from_json(&oversized_string).expect_err("oversized string must refuse");
    assert!(err.why.contains("decoded string"), "{err}");

    let oversized_number = "9".repeat(MAX_JSON_NUMBER_BYTES + 1);
    let err =
        EvidencePackage::from_json(&oversized_number).expect_err("long number token must refuse");
    assert!(err.why.contains("number token"), "{err}");

    let oversized_array = format!(
        "[{}]",
        std::iter::repeat_n("null", MAX_JSON_CONTAINER_ITEMS + 1)
            .collect::<Vec<_>>()
            .join(",")
    );
    let err =
        EvidencePackage::from_json(&oversized_array).expect_err("oversized array must refuse");
    assert!(err.why.contains("array element count"), "{err}");

    let oversized_in_memory = EvidencePackage::new(prov()).with_claim(Claim::estimated(
        "large",
        "x".repeat(MAX_JSON_STRING_BYTES + 1),
        "probe",
        1.0,
    ));
    assert!(matches!(
        oversized_in_memory.verify(),
        Err(PackageError::TransportLimit { .. })
    ));
    assert_serialized_refuses(&oversized_in_memory);
}

#[test]
fn the_merkle_root_is_deterministic_and_tamper_evident() {
    let build = || {
        EvidencePackage::new(prov())
            .with_claim(verified("c1"))
            .with_claim(estimated("c2"))
    };
    // identical packages -> identical content address.
    assert_eq!(build().merkle_root(), build().merkle_root());
    assert_eq!(
        build().merkle_root().to_hex(),
        "6b8d0c29e8b270d8ad523ec0aff139b03898f022330e3b972b6e3abd1048a1b7",
        "schema-v5 package-root fixture (re-pinned because the TEST certificate artifact address \
         now binds the full typed source request, including provenance and index); production \
         canonicalization did not change"
    );
    // tampering with a claim changes the root.
    let tampered = EvidencePackage::new(prov())
        .with_claim(Claim::from_certificate(
            "c1",
            "TAMPERED",
            -1.0,
            1.0,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(estimated("c2"));
    assert_ne!(build().merkle_root(), tampered.merkle_root());
}

#[test]
fn the_merkle_root_covers_reproducibility_provenance() {
    let pkg = EvidencePackage::new(prov()).with_claim(verified("c1"));
    let changed_code = EvidencePackage::new(Provenance::new("commit-other", "lock-deadbeef"))
        .with_claim(verified("c1"));
    let changed_lock = EvidencePackage::new(Provenance::new("commit-abc123", "lock-other"))
        .with_claim(verified("c1"));

    assert_ne!(pkg.merkle_root(), changed_code.merkle_root());
    assert_ne!(pkg.merkle_root(), changed_lock.merkle_root());
}

#[test]
fn an_unsupported_format_version_is_rejected() {
    let mut pkg = EvidencePackage::new(prov()).with_claim(estimated("e1"));
    pkg.format_version = 999;
    assert!(matches!(
        pkg.verify(),
        Err(PackageError::UnsupportedFormat { found: 999 })
    ));
}

#[test]
fn a_signature_is_optional_and_detached() {
    let unsigned = EvidencePackage::new(prov()).with_claim(estimated("e1"));
    assert!(
        unsigned.verify().is_ok(),
        "no signature is fine (content-addressed)"
    );
    assert!(unsigned.to_json().contains("\"signature\":null"));
    let signed = unsigned.clone().signed("ed25519:deadbeef");
    // signing does not change the content address (detached).
    assert_eq!(unsigned.merkle_root(), signed.merkle_root());
    assert!(signed.to_json().contains("ed25519:deadbeef"));
}

#[test]
fn json_is_deterministic_and_carries_the_root() {
    let pkg = EvidencePackage::new(prov())
        .with_claim(verified("c1"))
        .with_claim(validated("c2", good_regime(), "wt-2026"));
    let j = pkg.to_json();
    assert_eq!(j, pkg.to_json());
    assert!(j.starts_with('{') && j.ends_with('}'));
    assert!(j.contains(&pkg.merkle_root().to_hex()));
    assert!(j.contains("\"format_version\":5"), "schema v5 (krym)");
    // v3 carries COMPLETE payloads, not just rank labels.
    assert!(j.contains("\"lo_bits\":") && j.contains("\"dataset\":"));
}

/// qmao.6.1 — the schema-v3 round trip and its fail-closed walls: a
/// package decode-encodes stably (golden), floats travel bit-exactly,
/// and hostile/missing/unknown/non-finite/forged/tampered inputs each
/// refuse with a structured parse error.
#[test]
fn v3_round_trip_and_fail_closed_walls() {
    let pkg = EvidencePackage::new(Provenance::new("frankensim@abc123", "lock:deadbeef"))
        .with_claim(Claim::from_certificate(
            "c-verified",
            "tip deflection within bound",
            0.1875,
            0.25,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(Claim::anchored(
            "c-validated",
            "k-epsilon matched within regime",
            fs_evidence::ValidityDomain::unconstrained().with("reynolds", 1e3, 1e5),
            "tunnel-run-9",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(Claim::estimated(
            "c-estimated",
            "surrogate prediction",
            "pod-deim",
            0.02,
        ))
        .signed("test-key/1234abcd");
    // Golden decode-encode stability: parse(to_json) == pkg, and the
    // re-emission is byte-identical (semantic AND textual round trip).
    let json = pkg.to_json();
    let back = EvidencePackage::from_json(&json).expect("canonical JSON parses");
    assert_eq!(back, pkg, "semantic round trip");
    assert_eq!(back.to_json(), json, "textual round trip");
    let leading_zero = json.replacen("\"format_version\":5", "\"format_version\":05", 1);
    assert!(
        EvidencePackage::from_json(&leading_zero).is_err(),
        "non-JSON leading-zero integer refuses"
    );
    // Forged Verified claim: structurally shaped, but the embedded root
    // no longer recomputes from the tampered content — refused at parse.
    let forged = json.replace("tip deflection within bound", "tip deflection PROVEN SAFE");
    let err = EvidencePackage::from_json(&forged).expect_err("forgery refused");
    assert!(err.why.contains("does not recompute"), "{err}");
    // Widening the certified interval (payload tamper) also refused.
    let widened = json.replace(
        &format!("{:016x}", 0.25f64.to_bits()),
        &format!("{:016x}", 2.5f64.to_bits()),
    );
    assert!(
        EvidencePackage::from_json(&widened).is_err(),
        "payload tamper refused"
    );
    // Non-finite certificate bits: fail closed.
    let nan = json.replace(
        &format!("{:016x}", 0.1875f64.to_bits()),
        &format!("{:016x}", f64::NAN.to_bits()),
    );
    let err = EvidencePackage::from_json(&nan).expect_err("NaN certificate refused");
    assert!(err.why.contains("non-finite"), "{err}");
    // Unknown fields: closed schema.
    let unknown = json.replacen(
        "{\"format_version\"",
        "{\"vendor_extra\":1,\"format_version\"",
        1,
    );
    let err = EvidencePackage::from_json(&unknown).expect_err("unknown field refused");
    assert!(err.why.contains("unknown field"), "{err}");
    // Missing fields: fail closed.
    let missing = json.replacen("\"signature\":\"test-key/1234abcd\",", "", 1);
    assert!(
        EvidencePackage::from_json(&missing).is_err(),
        "missing field refused"
    );
    // Unknown color kind: fail closed.
    let bad_kind = json.replacen("\"kind\":\"verified\"", "\"kind\":\"blessed\"", 1);
    let err = EvidencePackage::from_json(&bad_kind).expect_err("unknown kind refused");
    assert!(err.why.contains("unknown color kind"), "{err}");
    // Drifted magnitude budget: refused (it must re-derive).
    let mb_tag = "\"validated_unquantified\":1";
    assert!(json.contains(mb_tag), "fixture sanity");
    let drifted = json.replacen(mb_tag, "\"validated_unquantified\":7", 1);
    let err = EvidencePackage::from_json(&drifted).expect_err("budget drift refused");
    assert!(err.why.contains("re-derive"), "{err}");
    // Hostile garbage and truncation: structured refusals, no panic.
    assert!(EvidencePackage::from_json("{\"format_version\":2").is_err());
    assert!(EvidencePackage::from_json(&json[..json.len() / 2]).is_err());
    assert!(EvidencePackage::from_json("").is_err());
    // Canonical fixed-width roots and JSON's control-character rule are
    // enforced at the parser boundary, before semantic verification.
    let root = pkg.merkle_root().to_hex();
    let short_root = json.replacen(&format!("\"{root}\""), &format!("\"{}\"", &root[1..]), 1);
    let err = EvidencePackage::from_json(&short_root).expect_err("short root refused");
    assert!(err.why.contains("64 hex chars"), "{err}");
    let raw_control = json.replacen("surrogate prediction", "surrogate\nprediction", 1);
    let err = EvidencePackage::from_json(&raw_control).expect_err("raw control refused");
    assert!(err.why.contains("control character"), "{err}");
}

/// qmao.6.1 — the magnitude budget attributes ERROR MAGNITUDES, not
/// claim counts, and reconciles with an independent recomputation.
#[test]
fn magnitude_budget_reconciles() {
    let pkg = EvidencePackage::new(Provenance::new("v", "l"))
        .with_claim(Claim::from_certificate(
            "a",
            "s",
            0.0,
            0.5,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(Claim::from_certificate(
            "b",
            "s",
            1.0,
            1.25,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(Claim::estimated("c", "s", "e", 0.125))
        .with_claim(Claim::anchored(
            "d",
            "s",
            fs_evidence::ValidityDomain::unconstrained().with("re", 1.0, 2.0),
            "ds",
            CANONICAL_DATASET_HASH,
        ));
    let mb = pkg.magnitude_budget();
    assert_eq!(mb.verified_width.to_bits(), 0.75f64.to_bits());
    assert_eq!(mb.estimated_dispersion.to_bits(), 0.125f64.to_bits());
    assert_eq!(
        mb.validated_unquantified, 1,
        "regional trust counted, never numerified"
    );
    assert_eq!(
        mb.quantified_total.to_bits(),
        (mb.verified_width + mb.estimated_dispersion).to_bits(),
        "total reconciles with its parts"
    );
}

/// qmao.6.1 — crosswalk coverage derives from fields ACTUALLY PRESENT
/// in a parsed package: a mapped concept with absent evidence is
/// "mapped but absent", never covered; no static mapping can report
/// evidence that the package does not carry.
#[test]
fn coverage_cannot_claim_absent_evidence() {
    use fs_crosswalk::{PackageConcept, Standard};
    use fs_package::{CoverageStatus, package_coverage_with, package_presence_with};
    // Unsigned, verified-only package: no validated claims, no regime
    // tags, no datasets, no signature.
    let provenance = Provenance::new("v", "lock");
    let pkg = EvidencePackage::new(provenance.clone()).with_claim(source_claim(
        &provenance,
        0,
        "c",
        "bounded",
        0.0,
        1.0,
    ));
    let capabilities = source_capabilities();
    let presence = package_presence_with(&pkg, &capabilities);
    let get = |c: PackageConcept| presence.iter().find(|p| p.concept == c).unwrap();
    assert!(get(PackageConcept::VerifiedColor).present);
    assert!(!get(PackageConcept::ValidatedColor).present);
    assert!(!get(PackageConcept::RegimeTag).present);
    assert!(!get(PackageConcept::AnchoringDataset).present);
    assert!(!get(PackageConcept::Signature).present);
    assert!(
        !get(PackageConcept::FalsifierLog).present,
        "absent falsifier records can never read as present"
    );
    for standard in Standard::ALL {
        for (concept, status, _why) in package_coverage_with(&pkg, standard, &capabilities) {
            if matches!(status, CoverageStatus::Covered) {
                let p = presence.iter().find(|p| p.concept == concept).unwrap();
                assert!(
                    p.present,
                    "{concept:?} covered without evidence for {standard:?}"
                );
            }
            if concept == PackageConcept::FalsifierLog {
                assert!(
                    !matches!(status, CoverageStatus::Covered),
                    "falsifier logs without records cannot be covered ({standard:?})"
                );
            }
        }
    }
}

#[test]
fn coverage_requires_a_valid_package_and_authenticated_evidence() {
    use fs_crosswalk::{PackageConcept, Standard};
    use fs_package::{CoverageStatus, package_coverage, package_presence};

    let refuted = EvidencePackage::new(prov())
        .with_claim(verified("refuted").with_falsifier(FalsifierRecord {
            name: "interval-probe".to_string(),
            attempts: 1,
            refuted: true,
            detail: "counterexample found".to_string(),
        }))
        .signed("raw-detached-signature");
    assert!(package_presence(&refuted).iter().all(|row| !row.present));
    for standard in Standard::ALL {
        assert!(
            package_coverage(&refuted, standard)
                .iter()
                .all(|(_, status, _)| !matches!(status, CoverageStatus::Covered)),
            "invalid package covered a concept for {standard:?}"
        );
    }

    let merely_signed = EvidencePackage::new(prov())
        .with_claim(verified("signed"))
        .signed("unauthenticated-bytes");
    let signature = package_presence(&merely_signed)
        .into_iter()
        .find(|row| row.concept == PackageConcept::Signature)
        .expect("signature concept judged");
    assert!(!signature.present, "{}", signature.why);
    for standard in Standard::ALL {
        let status = package_coverage(&merely_signed, standard)
            .into_iter()
            .find(|(concept, _, _)| *concept == PackageConcept::Signature)
            .expect("signature is mapped for every standard")
            .1;
        assert!(!matches!(status, CoverageStatus::Covered));
    }
}

#[test]
fn dataset_coverage_requires_a_matching_valid_anchor() {
    use fs_crosswalk::PackageConcept;
    use fs_package::package_presence;

    // Schema v5: a validated claim can only be CONSTRUCTED anchored to
    // its own dataset (the AnchoredSource origin attaches the matching
    // record), so "validated but unanchored" is unrepresentable in
    // memory; an EXTRA unrelated anchor neither helps nor hides it.
    let unrelated = EvidencePackage::new(prov()).with_claim(
        validated("v", good_regime(), "wind-tunnel-2026")
            .with_anchor("different-dataset", CANONICAL_DATASET_HASH),
    );
    unrelated
        .verify()
        .expect("an extra unrelated anchor is structurally valid");
    let unrelated_presence = package_presence(&unrelated);
    let anchor = unrelated_presence
        .iter()
        .find(|row| row.concept == PackageConcept::AnchoringDataset)
        .expect("anchor concept judged");
    assert!(anchor.present, "{}", anchor.why);

    let matching = EvidencePackage::new(prov()).with_claim(
        validated("v", good_regime(), "wind-tunnel-2026")
            .with_anchor("wind-tunnel-2026", CANONICAL_DATASET_HASH),
    );
    matching.verify().expect("matching anchor verifies");
    let matching_presence = package_presence(&matching);
    let anchor = matching_presence
        .iter()
        .find(|row| row.concept == PackageConcept::AnchoringDataset)
        .expect("anchor concept judged");
    assert!(anchor.present, "{}", anchor.why);
}

/// xfxq (schema v3) — composition receipts re-run solver-free, refuted
/// falsifiers fail, anchors and falsifier logs travel with claims, and
/// every new field round-trips through the strict parser bound into the
/// content address.
#[test]
fn v3_receipts_falsifiers_anchors() {
    use fs_package::PackageError;
    let ve = |lo: f64, hi: f64| Color::Verified { lo, hi };
    // A well-formed derived package: c = a + b with a valid receipt.
    let good = derived_package();
    verify_package(&good).expect("receipt re-derives");
    // Round trip: the v3 fields survive the strict parser bit-for-bit.
    let back = EvidencePackage::from_json(&good.to_json()).expect("v3 parses");
    assert_eq!(back, good);
    assert_eq!(back.claims[2].falsifiers()[0].attempts, 512);
    assert_eq!(back.claims[2].anchors()[0].dataset_id, "wt-2026-run9");
    // FORGED receipt: claiming Verified while a parent is Estimated —
    // the re-run composition cannot reproduce it (semantic catch, not
    // just the content-address catch).
    let forged = EvidencePackage::new(Provenance::new("v", "l"))
        .with_claim(Claim::estimated("a", "shaky", "guess", 0.5))
        .with_claim(Claim::from_certificate(
            "b",
            "solid",
            1.0,
            2.0,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        ))
        .with_claim(Claim::derived(
            "c",
            "laundered",
            ve(2.0, 4.0),
            vec![0, 1],
            IntervalOp::Add,
        ));
    assert!(matches!(
        forged.verify(),
        Err(PackageError::ReceiptMismatch { claim }) if claim == "c"
    ));
    // Forward/self parent references refuse.
    let cyclic = EvidencePackage::new(Provenance::new("v", "l")).with_claim(Claim::derived(
        "a",
        "s",
        ve(0.0, 1.0),
        vec![0],
        IntervalOp::Hull,
    ));
    assert!(matches!(
        cyclic.verify(),
        Err(PackageError::BadReceiptParent { parent: 0, .. })
    ));
    // A refuted falsifier fails the whole claim.
    let refuted = EvidencePackage::new(Provenance::new("v", "l")).with_claim(
        Claim::from_certificate(
            "a",
            "wrong",
            0.0,
            1.0,
            "test-solver/cert",
            CANONICAL_DATASET_HASH,
        )
        .with_falsifier(FalsifierRecord {
            name: "adversary".to_string(),
            attempts: 3,
            refuted: true,
            detail: "counterexample at x=0.7".to_string(),
        }),
    );
    assert!(matches!(
        refuted.verify(),
        Err(PackageError::RefutedClaim { falsifier, .. }) if falsifier == "adversary"
    ));
}

#[test]
fn v3_falsifier_coverage_and_content_binding() {
    let good = derived_package();
    let capabilities = source_capabilities();
    let presence = fs_package::package_presence_with(&good, &capabilities);
    let falsifier = presence
        .iter()
        .find(|row| row.concept == fs_crosswalk::PackageConcept::FalsifierLog)
        .expect("falsifier concept judged");
    assert!(falsifier.present, "{}", falsifier.why);

    let tampered = good
        .to_json()
        .replace("\"refuted\":false", "\"refuted\":true");
    assert!(
        EvidencePackage::from_json(&tampered).is_err(),
        "content root binds falsifier fields"
    );
}

/// 7uq9 (schema v4) — the content address is a domain-separated 32-byte
/// BLAKE3 root; legacy v3 transports and legacy 16-hex FNV roots are
/// refused with messages that NAME the incompatibility.
#[test]
fn v4_blake3_root_refuses_legacy_transports() {
    let pkg = EvidencePackage::new(prov()).with_claim(verified("c1"));
    let json = pkg.to_json();
    let root = pkg.merkle_root().to_hex();
    assert_eq!(root.len(), 64, "32-byte BLAKE3 root renders as 64 hex");
    assert!(json.contains(&format!("\"merkle_root\":\"{root}\"")));

    // A v3 transport is refused BY VERSION before any field is read.
    let v4 = json.replacen("\"format_version\":5", "\"format_version\":4", 1);
    let err = EvidencePackage::from_json(&v4).expect_err("v4 refused");
    assert!(err.why.contains("unsupported version 4"), "{err}");

    // A legacy 16-hex FNV root inside a v4 envelope is named as such.
    let legacy = json.replacen(&format!("\"{root}\""), "\"deadbeefcafe0123\"", 1);
    let err = EvidencePackage::from_json(&legacy).expect_err("legacy root refused");
    assert!(err.why.contains("legacy v3 FNV"), "{err}");

    // Right length, non-hex content: refused at the boundary.
    let garbled = json.replacen(
        &format!("\"{root}\""),
        &format!("\"{}\"", "z".repeat(64)),
        1,
    );
    let err = EvidencePackage::from_json(&garbled).expect_err("non-hex root refused");
    assert!(err.why.contains("non-hex"), "{err}");

    // A validly-formatted but WRONG root: recomputation refuses it.
    let mut flipped = root.clone().into_bytes();
    flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
    let wrong = json.replacen(&root, core::str::from_utf8(&flipped).unwrap(), 1);
    let err = EvidencePackage::from_json(&wrong).expect_err("wrong root refused");
    assert!(err.why.contains("does not recompute"), "{err}");
}

/// 7uq9 — domain separation: the root is not the bare BLAKE3 of any
/// undomained serialization. The package header binds claim count, so
/// zero/one and duplicate-tail shapes have distinct roots; the BLAKE3
/// owner separately locks cross-mode and cross-domain behavior.
#[test]
fn v4_root_is_domain_separated() {
    let pkg = EvidencePackage::new(prov()).with_claim(verified("c1"));
    let root = pkg.merkle_root();
    // Not the undomained hash of the canonical JSON or of the claim.
    assert_ne!(root, fs_blake3::hash_bytes(pkg.to_json().as_bytes()));
    assert_ne!(
        root,
        EvidencePackage::new(prov()).merkle_root(),
        "header claim count distinguishes zero and one claim"
    );
    assert_ne!(
        root,
        EvidencePackage::new(prov())
            .with_claim(verified("c1"))
            .with_claim(verified("c1"))
            .merkle_root(),
        "duplicating the final claim changes the tree identity"
    );
    // Stable across recomputation and bound to claim ORDER.
    let swapped = EvidencePackage::new(prov())
        .with_claim(verified("c2"))
        .with_claim(verified("c1"));
    let ordered = EvidencePackage::new(prov())
        .with_claim(verified("c1"))
        .with_claim(verified("c2"));
    assert_ne!(swapped.merkle_root(), ordered.merkle_root());
}

struct ExactSourceVerifier;

impl SourceCertificateVerifier for ExactSourceVerifier {
    fn verify(&self, request: &SourceCertificateRequest<'_>) -> bool {
        request.package_provenance == &prov()
            && request.claim_index == 0
            && request.claim_id == "source"
            && request.statement == "certified interval"
            && request.lo.to_bits() == 1.0f64.to_bits()
            && request.hi.to_bits() == 2.0f64.to_bits()
            && request.producer == "test-solver/cert"
            && request.certificate_hash.to_hex() == CANONICAL_DATASET_HASH
    }
}

static EXACT_SOURCE_VERIFIER: ExactSourceVerifier = ExactSourceVerifier;

#[test]
fn source_certificate_hashes_require_typed_external_verification() {
    use fs_crosswalk::{PackageConcept, Standard};
    use fs_package::CoverageStatus;

    let pkg = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "source",
        "certified interval",
        1.0,
        2.0,
        "test-solver/cert",
        CANONICAL_DATASET_HASH,
    ));
    assert!(matches!(
        pkg.verify(),
        Err(PackageError::SourceCertificateRefused {
            why: "source-certificate capability missing",
            ..
        })
    ));
    assert!(pkg.color_breakdown().is_err());
    assert!(
        fs_package::package_presence(&pkg)
            .iter()
            .all(|row| !row.present),
        "unverified source bytes must not produce positive coverage"
    );

    let capabilities =
        VerificationCapabilities::deny_all().with_source_certificates(&EXACT_SOURCE_VERIFIER);
    let report = pkg
        .verify_with(&capabilities)
        .expect("typed verifier recognizes the exact certificate subject");
    assert_eq!(report.breakdown.verified, 1);
    assert_eq!(
        pkg.color_breakdown_with(&capabilities)
            .expect("authenticated breakdown")
            .verified,
        1
    );
    let presence = fs_package::package_presence_with(&pkg, &capabilities);
    assert!(
        presence
            .iter()
            .find(|row| row.concept == PackageConcept::VerifiedColor)
            .expect("verified concept")
            .present
    );
    assert!(
        presence
            .iter()
            .find(|row| row.concept == PackageConcept::ClaimOrigin)
            .expect("origin concept")
            .present,
        "claim-origin presence requires the successful typed verifier above"
    );
    assert!(
        !presence
            .iter()
            .find(|row| row.concept == PackageConcept::WaiverAuthorization)
            .expect("waiver concept")
            .present
    );
    let origin_status = fs_package::package_coverage_with(&pkg, Standard::AsmeVvV40, &capabilities)
        .into_iter()
        .find(|(concept, _, _)| *concept == PackageConcept::ClaimOrigin)
        .expect("claim-origin crosswalk row")
        .1;
    assert_eq!(origin_status, CoverageStatus::Covered);
    let decoded = EvidencePackage::from_json_with(&pkg.to_json(), &capabilities)
        .expect("strict transport plus capability authentication");
    assert_eq!(decoded, pkg);

    let widened = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "source",
        "certified interval",
        1.0,
        3.0,
        "test-solver/cert",
        CANONICAL_DATASET_HASH,
    ));
    assert!(matches!(
        widened.verify_with(&capabilities),
        Err(PackageError::SourceCertificateRefused {
            why: "rejected by the injected verifier",
            ..
        })
    ));
}

struct HashWaiverVerifier;

fn waiver_mac(message: &[u8]) -> String {
    fs_blake3::hash_domain("fs-package:test:waiver-mac", message).to_hex()
}

impl WaiverVerifier for HashWaiverVerifier {
    fn verify(&self, mac: &str, message: &[u8]) -> bool {
        mac == waiver_mac(message)
    }
}

static HASH_WAIVER_VERIFIER: HashWaiverVerifier = HashWaiverVerifier;

fn waived_package(provenance: Provenance, waiver_id: &str, expiry_day: u64) -> EvidencePackage {
    let pending = EvidencePackage::new(provenance).with_claim(Claim::waived(
        "waived",
        "authorized interval",
        Color::Verified { lo: 0.0, hi: 1.0 },
        WaiverGrant {
            waiver_id: waiver_id.to_string(),
            expiry_day,
            mac: "pending-authenticator".to_string(),
        },
    ));
    let message = pending.waiver_message(0).expect("waiver target");
    let mac = waiver_mac(&message);
    let authorized = pending
        .with_waiver_mac(0, mac)
        .expect("install waiver authenticator");
    assert_eq!(
        authorized.waiver_message(0).as_deref(),
        Some(message.as_slice())
    );
    authorized
}

#[test]
fn waiver_authentication_enables_capability_aware_coverage() {
    use fs_crosswalk::{PackageConcept, Standard};
    use fs_package::CoverageStatus;

    let pkg = waived_package(prov(), "waiver-2026-01", 200);
    assert!(matches!(
        pkg.verify(),
        Err(PackageError::WaiverRefused {
            why: "waiver capability missing",
            ..
        })
    ));
    assert!(pkg.color_breakdown().is_err());
    assert!(
        fs_package::package_presence(&pkg)
            .iter()
            .all(|row| !row.present)
    );

    let capabilities =
        VerificationCapabilities::deny_all().with_waivers(&HASH_WAIVER_VERIFIER, 199);
    pkg.verify_with(&capabilities)
        .expect("unexpired, context-bound waiver verifies");
    let presence = fs_package::package_presence_with(&pkg, &capabilities);
    for concept in [
        PackageConcept::ClaimOrigin,
        PackageConcept::WaiverAuthorization,
    ] {
        assert!(
            presence
                .iter()
                .find(|row| row.concept == concept)
                .expect("schema-v5 authorization concept")
                .present,
            "{concept:?} must require and reflect successful waiver authentication"
        );
    }
    let waiver_status =
        fs_package::package_coverage_with(&pkg, Standard::FaaEasaCbA, &capabilities)
            .into_iter()
            .find(|(concept, _, _)| *concept == PackageConcept::WaiverAuthorization)
            .expect("waiver crosswalk row")
            .1;
    assert_eq!(waiver_status, CoverageStatus::Covered);
    let asme_status = fs_package::package_coverage_with(&pkg, Standard::AsmeVvV40, &capabilities)
        .into_iter()
        .find(|(concept, _, _)| *concept == PackageConcept::WaiverAuthorization)
        .expect("waiver no-counterpart row")
        .1;
    assert_eq!(asme_status, CoverageStatus::NoClaim);
    let decoded = EvidencePackage::from_json_with(&pkg.to_json(), &capabilities)
        .expect("waiver capability is available at the JSON boundary");
    assert_eq!(decoded, pkg);
}

#[test]
fn waiver_authentication_binds_package_context_id_expiry_and_claim() {
    let pkg = waived_package(prov(), "waiver-2026-01", 200);
    let capabilities =
        VerificationCapabilities::deny_all().with_waivers(&HASH_WAIVER_VERIFIER, 199);

    let old_mac = pkg
        .claims
        .iter()
        .find_map(|claim| match claim.origin() {
            fs_package::ClaimOrigin::AuthenticatedWaiver(grant) => Some(grant.mac.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(!old_mac.is_empty(), "waiver fixture changed origin");
    for replay in [
        EvidencePackage::new(Provenance::new("different-commit", "lock-deadbeef")).with_claim(
            Claim::waived(
                "waived",
                "authorized interval",
                Color::Verified { lo: 0.0, hi: 1.0 },
                WaiverGrant {
                    waiver_id: "waiver-2026-01".to_string(),
                    expiry_day: 200,
                    mac: old_mac.clone(),
                },
            ),
        ),
        EvidencePackage::new(prov()).with_claim(Claim::waived(
            "waived",
            "different assertion",
            Color::Verified { lo: 0.0, hi: 1.0 },
            WaiverGrant {
                waiver_id: "waiver-2026-01".to_string(),
                expiry_day: 200,
                mac: old_mac.clone(),
            },
        )),
        EvidencePackage::new(prov()).with_claim(Claim::waived(
            "waived",
            "authorized interval",
            Color::Verified { lo: 0.0, hi: 1.0 },
            WaiverGrant {
                waiver_id: "different-waiver".to_string(),
                expiry_day: 200,
                mac: old_mac.clone(),
            },
        )),
        EvidencePackage::new(prov()).with_claim(Claim::waived(
            "waived",
            "authorized interval",
            Color::Verified { lo: 0.0, hi: 1.0 },
            WaiverGrant {
                waiver_id: "waiver-2026-01".to_string(),
                expiry_day: 201,
                mac: old_mac.clone(),
            },
        )),
    ] {
        assert!(matches!(
            replay.verify_with(&capabilities),
            Err(PackageError::WaiverRefused {
                why: "rejected by the injected verifier",
                ..
            })
        ));
    }

    let expired = VerificationCapabilities::deny_all().with_waivers(&HASH_WAIVER_VERIFIER, 201);
    assert!(matches!(
        pkg.verify_with(&expired),
        Err(PackageError::WaiverRefused { why: "expired", .. })
    ));
}

#[test]
fn waiver_ids_are_unique_and_builder_targets_are_typed() {
    let duplicate = EvidencePackage::new(prov())
        .with_claim(Claim::waived(
            "first",
            "first waiver",
            Color::Estimated {
                estimator: "probe".to_string(),
                dispersion: 1.0,
            },
            WaiverGrant {
                waiver_id: "same-waiver".to_string(),
                expiry_day: 200,
                mac: "opaque-one".to_string(),
            },
        ))
        .with_claim(Claim::waived(
            "second",
            "second waiver",
            Color::Estimated {
                estimator: "probe".to_string(),
                dispersion: 1.0,
            },
            WaiverGrant {
                waiver_id: "same-waiver".to_string(),
                expiry_day: 200,
                mac: "opaque-two".to_string(),
            },
        ));
    assert!(matches!(
        duplicate.verify(),
        Err(PackageError::DuplicateWaiverId {
            first_claim,
            duplicate_claim,
            ..
        }) if first_claim == "first" && duplicate_claim == "second"
    ));

    let ordinary = EvidencePackage::new(prov()).with_claim(estimated("ordinary"));
    assert!(matches!(
        ordinary.with_waiver_mac(0, "mac"),
        Err(PackageError::InvalidWaiverTarget { index: 0 })
    ));
}

#[test]
fn multiple_waivers_share_one_mac_independent_authorization_context() {
    let pending = EvidencePackage::new(prov())
        .with_claim(Claim::waived(
            "first",
            "first authorization",
            Color::Verified { lo: 0.0, hi: 1.0 },
            WaiverGrant {
                waiver_id: "waiver-one".to_string(),
                expiry_day: 200,
                mac: "pending-one".to_string(),
            },
        ))
        .with_claim(Claim::waived(
            "second",
            "second authorization",
            Color::Verified { lo: 2.0, hi: 3.0 },
            WaiverGrant {
                waiver_id: "waiver-two".to_string(),
                expiry_day: 200,
                mac: "pending-two".to_string(),
            },
        ));
    let first_message = pending.waiver_message(0).expect("first waiver target");
    let second_message = pending.waiver_message(1).expect("second waiver target");
    let package = pending
        .with_waiver_mac(0, waiver_mac(&first_message))
        .expect("first authenticator")
        .with_waiver_mac(1, waiver_mac(&second_message))
        .expect("second authenticator");
    assert_eq!(
        package.waiver_message(0).as_deref(),
        Some(first_message.as_slice())
    );
    assert_eq!(
        package.waiver_message(1).as_deref(),
        Some(second_message.as_slice())
    );
    let capabilities =
        VerificationCapabilities::deny_all().with_waivers(&HASH_WAIVER_VERIFIER, 199);
    package
        .verify_with(&capabilities)
        .expect("both waivers authenticate against the shared context");
}

#[test]
fn origin_transport_and_machine_identity_boundaries_fail_closed() {
    let oversized_source = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "source",
        "bounded",
        0.0,
        1.0,
        "p".repeat(MAX_JSON_STRING_BYTES + 1),
        CANONICAL_DATASET_HASH,
    ));
    assert!(matches!(
        oversized_source.verify(),
        Err(PackageError::TransportLimit { what, .. }) if what == "origin.producer"
    ));

    let oversized_waiver = EvidencePackage::new(prov()).with_claim(Claim::waived(
        "waived",
        "bounded",
        Color::Verified { lo: 0.0, hi: 1.0 },
        WaiverGrant {
            waiver_id: "waiver-id".to_string(),
            expiry_day: 200,
            mac: "m".repeat(MAX_JSON_STRING_BYTES + 1),
        },
    ));
    assert!(matches!(
        oversized_waiver.verify(),
        Err(PackageError::TransportLimit { what, .. }) if what == "origin.mac"
    ));

    let placeholder_id = EvidencePackage::new(prov()).with_claim(estimated("TODO"));
    assert!(matches!(
        placeholder_id.verify(),
        Err(PackageError::InvalidClaimId {
            reason: "placeholder",
            ..
        })
    ));
    let padded_provenance =
        EvidencePackage::new(Provenance::new(" commit", "lock")).with_claim(estimated("e"));
    assert!(matches!(
        padded_provenance.verify(),
        Err(PackageError::InvalidIdentity {
            field: "provenance.code_version",
            reason: "surrounding-whitespace",
            ..
        })
    ));
    let placeholder_estimator =
        EvidencePackage::new(prov()).with_claim(Claim::estimated("e", "estimate", "unknown", 1.0));
    assert!(matches!(
        placeholder_estimator.verify(),
        Err(PackageError::InvalidIdentity {
            field: "color.estimator",
            reason: "placeholder",
            ..
        })
    ));
    let placeholder_producer = EvidencePackage::new(prov()).with_claim(Claim::from_certificate(
        "source",
        "bounded",
        0.0,
        1.0,
        "pending",
        CANONICAL_DATASET_HASH,
    ));
    assert!(matches!(
        placeholder_producer.verify(),
        Err(PackageError::InvalidOrigin { .. })
    ));
}
