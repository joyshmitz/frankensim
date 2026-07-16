//! Pure canonical framing helpers shared by the build script and its tests.

/// Semantic version of the outer GEMM build fingerprint. Version 2 admits the
/// canonical payload's explicit metadata-present/absent v3 schema.
pub const GEMM_BUILD_FINGERPRINT_IDENTITY_VERSION: u32 = 2;

/// Domain separating the outer GEMM build fingerprint from all other hashes.
pub const GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN: &str =
    "frankensim.fs-la.gemm-build-fingerprint.v2";

/// Exact schema tag framed inside the canonical build-input payload.
pub const GEMM_BUILD_PAYLOAD_SCHEMA: &str = "fs-la-gemm-codegen-v3";

/// Domain for compiler and wrapper executable content identities.
pub const EXECUTABLE_CONTEXT: &str = "frankensim.fs-la.gemm-build-executable.v1";

/// Compiler inputs included by the normal asupersync graph from outside its
/// package source directories. Keep this explicit so each addition is audited
/// against a concrete `include_*` site rather than sweeping unrelated assets.
pub const ASUPERSYNC_NON_SRC_INPUTS: &[&str] = &["assets/dashboard.html"];

/// Owner-local declaration consumed by `xtask check-identities`.
#[allow(dead_code)] // consumed as source text by xtask, not by the build binary
pub const GEMM_BUILD_FINGERPRINT_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
    "frankensim-identity-schema-v1",
    "id=fs-la:gemm-build-fingerprint",
    "version_const=GEMM_BUILD_FINGERPRINT_IDENTITY_VERSION",
    "version=2",
    "domain=frankensim.fs-la.gemm-build-fingerprint.v2",
    "domain_const=GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN",
    "encoder=gemm_build_fingerprint",
    "encoder_helpers=gemm_build_fingerprint_with_domain",
    "schema_functions=push_field,push_optional_field,append_source_fields,append_external_identity,append_executable_identity,git_output_line,full_lowercase_git_oid,symbolic_git_ref,gemm_build_fingerprint_identity_version_is_supported,crates/fs-la/build.rs#main,crates/fs-la/build.rs#required_env,crates/fs-la/build.rs#optional_env,crates/fs-la/build.rs#read_identity_file,crates/fs-la/build.rs#read_required_file,crates/fs-la/build.rs#resolve_executable,crates/fs-la/build.rs#add_executable_identity,crates/fs-la/build.rs#collect_rust_sources,crates/fs-la/build.rs#collect_regular_files,crates/fs-la/build.rs#add_source_closure,crates/fs-la/build.rs#command_stdout,crates/fs-la/build.rs#git_command,crates/fs-la/build.rs#git_path,crates/fs-la/build.rs#watch_optional_git_file,crates/fs-la/build.rs#observed_git_head,crates/fs-la/build.rs#add_asupersync_identity,crates/fs-la/build.rs#add_depgraph_evidence,crates/fs-blake3/src/lib.rs#ContentHash::as_bytes,crates/fs-blake3/src/lib.rs#ContentHash::to_hex,crates/fs-blake3/src/lib.rs#hash_domain",
    "schema_constants=GEMM_BUILD_FINGERPRINT_IDENTITY_VERSION,GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN,GEMM_BUILD_PAYLOAD_SCHEMA,EXECUTABLE_CONTEXT,ASUPERSYNC_NON_SRC_INPUTS,crates/fs-la/build.rs#CARGO_PROFILES,crates/fs-la/build.rs#PROFILE_CODEGEN_KEYS",
    "schema_dependencies=fs-la:depgraph-receipt",
    "digest=fs-blake3",
    "encoding=typed-binary",
    "sources=GemmBuildIdentityInput",
    "source_fields=GemmBuildIdentityInput.canonical_payload:semantic",
    "source_bindings=GemmBuildIdentityInput.canonical_payload>embedded-payload-schema-v3+build-environment+dependency-graph-evidence+source-closure+external-constellation-inputs+compiler-wrapper-executable-identities",
    "external_semantic_fields=artifact-domain-v2",
    "semantic_fields=artifact-domain-v2,embedded-payload-schema-v3,build-environment,dependency-graph-evidence,source-closure,external-constellation-inputs,compiler-wrapper-executable-identities",
    "excluded_fields=source-discovery-order:canonical-path-sort-only",
    "consumers=FS_LA_GEMM_BUILD_FINGERPRINT,fs-exec:GemmTuneKey,fs-session:gemm-tune-row-receipt",
    "mutations=artifact-domain-v2:crates/fs-la/tests/build_identity_support.rs#gemm_build_identity_domain_moves_fingerprint,embedded-payload-schema-v3:crates/fs-la/tests/build_identity_support.rs#embedded_payload_schema_moves_gemm_build_fingerprint,build-environment:crates/fs-la/tests/build_identity_support.rs#optional_fields_separate_absent_empty_and_literal_sentinel,dependency-graph-evidence:crates/fs-la/tests/build_identity_support.rs#depgraph_evidence_moves_gemm_build_fingerprint,source-closure:crates/fs-la/tests/build_identity_support.rs#source_fields_are_order_independent_and_content_sensitive,external-constellation-inputs:crates/fs-la/tests/build_identity_support.rs#external_identity_binds_lock_observed_head_source_and_include_inputs,compiler-wrapper-executable-identities:crates/fs-la/tests/build_identity_support.rs#executable_identity_binds_resolved_path_and_content",
    "nonsemantic_mutations=source-discovery-order:crates/fs-la/tests/build_identity_support.rs#source_fields_are_order_independent_and_content_sensitive",
    "field_guard=classify_gemm_build_identity_fields",
    "transport_guard=gemm_build_fingerprint",
    "version_guard=crates/fs-la/tests/build_identity_support.rs#gemm_build_fingerprint_identity_version_fails_closed",
    "coupling_surface=fs-la:gemm-build-fingerprint",
];

/// Canonical payload produced by the build script before outer domain hashing.
pub struct GemmBuildIdentityInput<'a> {
    pub canonical_payload: &'a [u8],
}

/// Whether retained build fingerprints use the one outer semantic version
/// understood by this build.
#[must_use]
#[allow(dead_code)] // exercised by the integration identity-version guard
pub const fn gemm_build_fingerprint_identity_version_is_supported(declared: u32) -> bool {
    declared == GEMM_BUILD_FINGERPRINT_IDENTITY_VERSION
}

/// Hash one fully assembled canonical build-input payload.
#[must_use]
pub fn gemm_build_fingerprint(input: &GemmBuildIdentityInput<'_>) -> fs_blake3::ContentHash {
    gemm_build_fingerprint_with_domain(GEMM_BUILD_FINGERPRINT_IDENTITY_DOMAIN, input)
}

pub(crate) fn gemm_build_fingerprint_with_domain(
    domain: &str,
    input: &GemmBuildIdentityInput<'_>,
) -> fs_blake3::ContentHash {
    fs_blake3::hash_domain(domain, input.canonical_payload)
}

#[allow(dead_code)] // exhaustive source-shape guard consumed by xtask
fn classify_gemm_build_identity_fields(input: &GemmBuildIdentityInput<'_>) {
    let GemmBuildIdentityInput {
        canonical_payload: _,
    } = input;
}

pub fn push_field(payload: &mut Vec<u8>, name: &str, value: &[u8]) {
    payload.extend_from_slice(&(name.len() as u64).to_le_bytes());
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(&(value.len() as u64).to_le_bytes());
    payload.extend_from_slice(value);
}

pub fn push_optional_field(payload: &mut Vec<u8>, name: &str, value: Option<&[u8]>) {
    push_field(
        payload,
        &format!("{name}:presence"),
        if value.is_some() {
            b"present"
        } else {
            b"absent"
        },
    );
    if let Some(value) = value {
        push_field(payload, name, value);
    }
}

/// Decode one Git-produced line without accepting lossy text, embedded control
/// characters, or more than one optional trailing newline.
pub fn git_output_line(bytes: &[u8]) -> Result<&str, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Git output is not UTF-8")?;
    let line = text.strip_suffix('\n').unwrap_or(text);
    if line.is_empty() {
        return Err("Git output line is empty");
    }
    if line.chars().any(char::is_control) {
        return Err("Git output contains a control character or multiple lines");
    }
    Ok(line)
}

/// Parse a complete lowercase SHA-1 or SHA-256 Git object identifier.
pub fn full_lowercase_git_oid(bytes: &[u8]) -> Result<&str, &'static str> {
    let oid = git_output_line(bytes)?;
    if !matches!(oid.len(), 40 | 64)
        || !oid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Git object id is not 40 or 64 lowercase hexadecimal characters");
    }
    Ok(oid)
}

/// Extract a symbolic HEAD ref only when it is a safe repository-relative
/// `refs/...` path. A detached object id intentionally returns `None`.
pub fn symbolic_git_ref(head_file: &[u8]) -> Result<Option<&str>, &'static str> {
    let head = git_output_line(head_file)?;
    let Some(reference) = head.strip_prefix("ref: ") else {
        full_lowercase_git_oid(head_file)?;
        return Ok(None);
    };
    if !reference.starts_with("refs/")
        || reference
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        || reference.bytes().any(|byte| matches!(byte, b'\\' | b':'))
    {
        return Err("symbolic Git HEAD is not a safe refs path");
    }
    Ok(Some(reference))
}

pub fn append_source_fields(payload: &mut Vec<u8>, mut fields: Vec<(String, Vec<u8>)>) {
    fields.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    assert!(
        fields
            .windows(2)
            .all(|pair| pair[0].0.as_str() != pair[1].0.as_str()),
        "duplicate path in GEMM build-identity source closure"
    );
    for (relative, bytes) in fields {
        push_field(payload, &format!("source:{relative}"), &bytes);
    }
}

pub fn append_external_identity(
    payload: &mut Vec<u8>,
    constellation_lock: &[u8],
    observed_git_head: Option<&str>,
    fields: Vec<(String, Vec<u8>)>,
) {
    push_field(payload, "constellation.lock", constellation_lock);
    push_optional_field(
        payload,
        "asupersync-observed-git-head",
        observed_git_head.map(str::as_bytes),
    );
    append_source_fields(payload, fields);
}

pub fn append_executable_identity(
    payload: &mut Vec<u8>,
    label: &str,
    resolved_path: &str,
    bytes: &[u8],
) {
    push_field(
        payload,
        &format!("executable:{label}:path"),
        resolved_path.as_bytes(),
    );
    let digest = fs_blake3::hash_domain(EXECUTABLE_CONTEXT, bytes);
    push_field(
        payload,
        &format!("executable:{label}:content"),
        digest.as_bytes(),
    );
}
