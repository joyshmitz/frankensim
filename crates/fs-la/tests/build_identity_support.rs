//! G5 canonical-input tests for the GEMM build fingerprint.

#[path = "../build_identity_support.rs"]
mod support;

use support::{
    ASUPERSYNC_NON_SRC_INPUTS, EXECUTABLE_CONTEXT, GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN,
    GEMM_BUILD_FINGERPRINT_IDENTITY_VERSION, GEMM_BUILD_PAYLOAD_SCHEMA, GemmBuildIdentityInput,
    append_executable_identity, append_external_identity, append_source_fields,
    full_lowercase_git_oid, gemm_build_fingerprint,
    gemm_build_fingerprint_identity_version_is_supported, gemm_build_fingerprint_with_domain,
    git_output_line, push_field, push_optional_field, symbolic_git_ref,
};

fn fingerprint(payload: &[u8]) -> fs_blake3::ContentHash {
    gemm_build_fingerprint(&GemmBuildIdentityInput {
        canonical_payload: payload,
    })
}

#[test]
fn gemm_build_identity_domain_moves_fingerprint() {
    let input = GemmBuildIdentityInput {
        canonical_payload: b"canonical-build-payload",
    };
    assert_ne!(
        gemm_build_fingerprint(&input),
        gemm_build_fingerprint_with_domain(
            "frankensim.fs-la.gemm-build-fingerprint.v2.alternate",
            &input,
        )
    );
}

#[test]
fn embedded_payload_schema_moves_gemm_build_fingerprint() {
    let mut current = Vec::new();
    push_field(&mut current, "schema", GEMM_BUILD_PAYLOAD_SCHEMA.as_bytes());
    let mut changed = Vec::new();
    push_field(&mut changed, "schema", b"fs-la-gemm-codegen-v4");
    assert_ne!(fingerprint(&current), fingerprint(&changed));
}

#[test]
fn depgraph_evidence_moves_gemm_build_fingerprint() {
    let mut current = Vec::new();
    push_field(
        &mut current,
        "depgraph-receipt",
        br#"{"schema":"fs-la-depgraph-receipt-v1","packages":["a"]}"#,
    );
    let mut changed = Vec::new();
    push_field(
        &mut changed,
        "depgraph-receipt",
        br#"{"schema":"fs-la-depgraph-receipt-v1","packages":["b"]}"#,
    );
    assert_ne!(fingerprint(&current), fingerprint(&changed));
}

#[test]
fn gemm_build_fingerprint_identity_version_fails_closed() {
    assert_eq!(GEMM_BUILD_FINGERPRINT_IDENTITY_VERSION, 2);
    assert_eq!(
        GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN,
        "frankensim.fs-la.gemm-build-fingerprint.v2"
    );
    assert_eq!(GEMM_BUILD_PAYLOAD_SCHEMA, "fs-la-gemm-codegen-v3");
    assert!(gemm_build_fingerprint_identity_version_is_supported(2));
    assert!(!gemm_build_fingerprint_identity_version_is_supported(0));
    assert!(!gemm_build_fingerprint_identity_version_is_supported(1));
    assert!(!gemm_build_fingerprint_identity_version_is_supported(3));
}

#[test]
fn source_fields_are_order_independent_and_content_sensitive() {
    let fields = vec![
        ("crates/fs-la/src/gemm.rs".to_string(), b"alpha".to_vec()),
        ("crates/fs-exec/src/pool.rs".to_string(), b"beta".to_vec()),
    ];
    let mut forward = Vec::new();
    append_source_fields(&mut forward, fields.clone());
    let mut reverse = Vec::new();
    append_source_fields(&mut reverse, fields.into_iter().rev().collect());
    assert_eq!(forward, reverse, "directory order must be irrelevant");
    assert_eq!(
        fingerprint(&forward),
        fingerprint(&reverse),
        "directory order must not move the outer build fingerprint"
    );

    let mut changed = Vec::new();
    append_source_fields(
        &mut changed,
        vec![
            ("crates/fs-la/src/gemm.rs".to_string(), b"alphb".to_vec()),
            ("crates/fs-exec/src/pool.rs".to_string(), b"beta".to_vec()),
        ],
    );
    assert_ne!(forward, changed, "one source byte must change the payload");
    assert_ne!(
        fingerprint(&forward),
        fingerprint(&changed),
        "one source byte must change the outer build fingerprint"
    );
}

#[test]
fn field_framing_separates_names_and_values() {
    assert!(!GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN.is_empty());
    assert!(!EXECUTABLE_CONTEXT.is_empty());
    let mut left = Vec::new();
    push_field(&mut left, "ab", b"c");
    let mut right = Vec::new();
    push_field(&mut right, "a", b"bc");
    assert_ne!(left, right);
}

#[test]
fn executable_identity_binds_resolved_path_and_content() {
    fn payload(path: &str, bytes: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        append_executable_identity(&mut payload, "RUSTC", path, bytes);
        payload
    }

    let baseline = payload("/toolchain/bin/rustc", b"compiler-a");
    assert_ne!(baseline, payload("/other/bin/rustc", b"compiler-a"));
    assert_ne!(baseline, payload("/toolchain/bin/rustc", b"compiler-b"));
    assert_eq!(baseline, payload("/toolchain/bin/rustc", b"compiler-a"));
    assert_ne!(
        fingerprint(&baseline),
        fingerprint(&payload("/toolchain/bin/rustc", b"compiler-b"))
    );
}

#[test]
fn optional_fields_separate_absent_empty_and_literal_sentinel() {
    fn payload(value: Option<&[u8]>) -> Vec<u8> {
        let mut payload = Vec::new();
        push_optional_field(&mut payload, "SALT", value);
        payload
    }

    assert_ne!(payload(None), payload(Some(b"")));
    assert_ne!(payload(None), payload(Some(b"<unset>")));
    assert_ne!(payload(Some(b"")), payload(Some(b"<unset>")));
    assert_ne!(
        fingerprint(&payload(None)),
        fingerprint(&payload(Some(b"")))
    );
}

#[test]
fn git_observation_parsers_fail_closed() {
    assert_eq!(git_output_line(b"/repo/.git").unwrap(), "/repo/.git");
    assert_eq!(git_output_line(b"/repo/.git\n").unwrap(), "/repo/.git");
    assert!(git_output_line(b"").is_err());
    assert!(git_output_line(b"line\n\n").is_err());
    assert!(git_output_line(b"line\r\n").is_err());
    assert!(git_output_line("line\u{0085}".as_bytes()).is_err());
    assert!(git_output_line(&[0xff]).is_err());

    let sha1 = "1".repeat(40);
    let sha256 = "a".repeat(64);
    assert_eq!(full_lowercase_git_oid(sha1.as_bytes()).unwrap(), sha1);
    assert_eq!(
        full_lowercase_git_oid(format!("{sha256}\n").as_bytes()).unwrap(),
        sha256
    );
    assert!(full_lowercase_git_oid("A".repeat(40).as_bytes()).is_err());
    assert!(full_lowercase_git_oid("g".repeat(40).as_bytes()).is_err());
    assert!(full_lowercase_git_oid("1".repeat(39).as_bytes()).is_err());

    assert_eq!(
        symbolic_git_ref(b"ref: refs/heads/main\n").unwrap(),
        Some("refs/heads/main")
    );
    assert_eq!(symbolic_git_ref(sha1.as_bytes()).unwrap(), None);
    assert!(symbolic_git_ref(b"garbage\n").is_err());
    assert!(symbolic_git_ref("A".repeat(40).as_bytes()).is_err());
    assert!(symbolic_git_ref(b"ref: refs/heads/../../escape\n").is_err());
    assert!(symbolic_git_ref(b"ref: refs\\heads\\main\n").is_err());
    assert!(symbolic_git_ref(b"ref: refs/heads/main\nignored").is_err());
}

#[test]
fn external_identity_binds_lock_observed_head_source_and_include_inputs() {
    fn payload(
        lock: &[u8],
        observed_head: Option<&str>,
        source: &[u8],
        dashboard: &[u8],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        append_external_identity(
            &mut payload,
            lock,
            observed_head,
            vec![
                (
                    "external/asupersync/src/lib.rs".to_string(),
                    source.to_vec(),
                ),
                (
                    "external/asupersync/assets/dashboard.html".to_string(),
                    dashboard.to_vec(),
                ),
            ],
        );
        payload
    }

    assert_eq!(ASUPERSYNC_NON_SRC_INPUTS, &["assets/dashboard.html"]);
    let baseline = payload(
        b"lock-a",
        Some("1111111111111111111111111111111111111111"),
        b"src-a",
        b"dashboard-a",
    );
    assert_ne!(
        baseline,
        payload(
            b"lock-b",
            Some("1111111111111111111111111111111111111111"),
            b"src-a",
            b"dashboard-a"
        )
    );
    assert_ne!(
        baseline,
        payload(
            b"lock-a",
            Some("2222222222222222222222222222222222222222"),
            b"src-a",
            b"dashboard-a"
        )
    );
    assert_ne!(
        baseline,
        payload(
            b"lock-a",
            Some("1111111111111111111111111111111111111111"),
            b"src-b",
            b"dashboard-a"
        )
    );
    assert_ne!(
        baseline,
        payload(
            b"lock-a",
            Some("1111111111111111111111111111111111111111"),
            b"src-a",
            b"dashboard-b"
        ),
        "an included non-source compiler input must change the payload"
    );
    assert_ne!(
        fingerprint(&baseline),
        fingerprint(&payload(
            b"lock-a",
            Some("1111111111111111111111111111111111111111"),
            b"src-a",
            b"dashboard-b"
        )),
        "an included external input must change the outer build fingerprint"
    );
    assert_ne!(
        baseline,
        payload(b"lock-a", None, b"src-a", b"dashboard-a"),
        "missing transported Git metadata must be explicit provenance"
    );
    assert_ne!(
        payload(b"lock-a", None, b"src-a", b"dashboard-a"),
        payload(b"lock-a", Some(""), b"src-a", b"dashboard-a"),
        "absent Git metadata must not alias a present empty observation"
    );
}
