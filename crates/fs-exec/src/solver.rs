//! Resumable solvers (plan §5.2 behavior 2): iterative solvers as EXPLICIT
//! state machines. The architectural target is typed
//! pause/serialize/migrate/resume/fork; this module currently contains the
//! legacy reference path and a code-first v2 identity/envelope tranche. The
//! session, migration, fork-lineage, and retained-proof boundaries still open
//! are stated in `CONTRACT.md`.
//!
//! Distribution-readiness target (plan §17): state payloads are self-contained
//! bytes — no pointers or shared-memory assumptions, with large artifacts
//! referenced by content hash. V2 does not yet provide the durable expected
//! context, ledger/session recovery, or remote admission protocol needed to
//! claim post-restart or cross-machine resume.
//!
//! Target determinism invariant (G4/G5): pause → serialize → deserialize →
//! resume reproduces the uninterrupted trajectory BIT-EXACTLY within the
//! declared execution contract. The legacy reference solver defines a local
//! equivalence test; the v2 no-mock and cross-ISA proof is still pending.

use crate::cx::Cx;

/// In-house, deterministic, little-endian state codec (P1: no serde).
/// Floats travel as raw bits (`to_bits`), so round-trips are bit-exact
/// including NaN payloads and signed zeros.
pub mod codec {
    use core::fmt;

    /// Structured decode failure (Decalogue P10).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CodecError {
        /// Byte offset where decoding failed.
        pub at: usize,
        /// What the decoder was reading.
        pub what: &'static str,
        /// Bytes it needed.
        pub needed: usize,
        /// Bytes that remained.
        pub remaining: usize,
    }

    impl fmt::Display for CodecError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "solver-state decode failed at byte {}: reading {} needs {} bytes but {} \
                 remain; the snapshot is truncated or from an incompatible encoder version",
                self.at, self.what, self.needed, self.remaining
            )
        }
    }

    impl core::error::Error for CodecError {}

    /// Append-only encoder.
    #[derive(Debug, Default)]
    pub struct Enc {
        buf: Vec<u8>,
    }

    impl Enc {
        /// Fresh encoder.
        #[must_use]
        pub fn new() -> Self {
            Enc::default()
        }

        /// Append a u32 (little-endian).
        pub fn put_u32(&mut self, v: u32) {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }

        /// Append a u64 (little-endian).
        pub fn put_u64(&mut self, v: u64) {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }

        /// Append an f64 as raw bits (bit-exact round-trip).
        pub fn put_f64(&mut self, v: f64) {
            self.put_u64(v.to_bits());
        }

        /// Append a length-prefixed f64 slice.
        pub fn put_f64_slice(&mut self, xs: &[f64]) {
            self.put_u64(xs.len() as u64);
            for &x in xs {
                self.put_f64(x);
            }
        }

        /// Finish, yielding the snapshot bytes.
        #[must_use]
        pub fn into_bytes(self) -> Vec<u8> {
            self.buf
        }
    }

    /// Cursor decoder over snapshot bytes.
    #[derive(Debug)]
    pub struct Dec<'a> {
        bytes: &'a [u8],
        at: usize,
    }

    impl<'a> Dec<'a> {
        /// Decode from `bytes`.
        #[must_use]
        pub fn new(bytes: &'a [u8]) -> Self {
            Dec { bytes, at: 0 }
        }

        fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], CodecError> {
            let remaining = self.bytes.len() - self.at;
            if remaining < n {
                return Err(CodecError {
                    at: self.at,
                    what,
                    needed: n,
                    remaining,
                });
            }
            let s = &self.bytes[self.at..self.at + n];
            self.at += n;
            Ok(s)
        }

        /// Read a u32.
        ///
        /// # Errors
        /// [`CodecError`] on truncation.
        pub fn get_u32(&mut self) -> Result<u32, CodecError> {
            Ok(u32::from_le_bytes(
                self.take(4, "u32")?.try_into().expect("length checked"),
            ))
        }

        /// Read a u64.
        ///
        /// # Errors
        /// [`CodecError`] on truncation.
        pub fn get_u64(&mut self) -> Result<u64, CodecError> {
            Ok(u64::from_le_bytes(
                self.take(8, "u64")?.try_into().expect("length checked"),
            ))
        }

        /// Read an f64 (from raw bits).
        ///
        /// # Errors
        /// [`CodecError`] on truncation.
        pub fn get_f64(&mut self) -> Result<f64, CodecError> {
            Ok(f64::from_bits(self.get_u64()?))
        }

        /// Read a length-prefixed f64 slice.
        ///
        /// # Errors
        /// [`CodecError`] on truncation (including an implausible length).
        pub fn get_f64_vec(&mut self) -> Result<Vec<f64>, CodecError> {
            let encoded_len = self.get_u64()?;
            let remaining = self.bytes.len() - self.at;
            let len = usize::try_from(encoded_len).map_err(|_| CodecError {
                at: self.at,
                what: "f64 slice length exceeds platform usize",
                needed: usize::MAX,
                remaining,
            })?;
            let needed = len.checked_mul(8).ok_or(CodecError {
                at: self.at,
                what: "f64 slice byte length overflow",
                needed: usize::MAX,
                remaining,
            })?;
            if remaining < needed {
                return Err(CodecError {
                    at: self.at,
                    what: "f64 slice body",
                    needed,
                    remaining,
                });
            }
            (0..len).map(|_| self.get_f64()).collect()
        }

        /// True when every byte was consumed (decoders should check this to
        /// reject trailing garbage).
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.at == self.bytes.len()
        }

        /// Current payload cursor for the enclosing schema validator.
        pub(super) fn position(&self) -> usize {
            self.at
        }

        /// Bytes not consumed by the payload decoder.
        pub(super) fn remaining(&self) -> usize {
            self.bytes.len() - self.at
        }
    }
}

/// The snapshot ENVELOPE (bead wf9.8.2): magic, versions, type
/// identity, length, checksum, and provenance — all validated BEFORE
/// the payload decoder runs, so same-length bytes from another solver,
/// another schema version, a bit flip, a truncation, or an append can
/// never decode into plausible-but-wrong state.
pub mod envelope {
    use core::fmt;

    /// Envelope magic (8 bytes).
    pub const MAGIC: [u8; 8] = *b"FSEXSNAP";
    /// Envelope layout version. Bump only with a recorded migration.
    pub const ENVELOPE_VERSION: u32 = 1;
    /// Header size: magic + env version + type id + schema version +
    /// provenance + payload len + payload hash.
    pub const HEADER_LEN: usize = 8 + 4 + 8 + 4 + 8 + 8 + 8;

    /// Structured envelope refusal — never a wrong-state decode.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum EnvelopeError {
        /// Not a snapshot envelope at all.
        BadMagic,
        /// Shorter than a header (or than the declared payload).
        Truncated {
            /// Bytes needed.
            needed: usize,
            /// Bytes present.
            have: usize,
        },
        /// Envelope layout from a different (unsupported) version.
        UnknownEnvelopeVersion {
            /// The version found.
            found: u32,
        },
        /// The snapshot belongs to a DIFFERENT state type.
        WrongTypeId {
            /// The expected stable type id.
            expected: u64,
            /// The id in the envelope.
            found: u64,
        },
        /// Same type, incompatible schema version: explicit refusal
        /// (the structured alternative to a silent wrong decode; write
        /// a migration when a version must remain readable).
        IncompatibleSchema {
            /// The reader's schema version.
            expected: u32,
            /// The snapshot's schema version.
            found: u32,
        },
        /// Declared payload length disagrees with the actual bytes
        /// (truncation past the header, or appended bytes).
        LengthMismatch {
            /// Length declared in the header.
            declared: u64,
            /// Bytes actually present after the header.
            actual: u64,
        },
        /// Payload bytes do not hash to the declared checksum.
        ChecksumMismatch {
            /// The declared hash.
            declared: u64,
            /// The computed hash.
            computed: u64,
        },
    }

    impl fmt::Display for EnvelopeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                EnvelopeError::BadMagic => write!(f, "not a solver snapshot (bad magic)"),
                EnvelopeError::Truncated { needed, have } => write!(
                    f,
                    "snapshot truncated: needs {needed} bytes, {have} present"
                ),
                EnvelopeError::UnknownEnvelopeVersion { found } => write!(
                    f,
                    "unknown snapshot envelope version {found} (this reader supports {ENVELOPE_VERSION})"
                ),
                EnvelopeError::WrongTypeId { expected, found } => write!(
                    f,
                    "snapshot is for state type {found:#018x}, not {expected:#018x} — refusing a cross-type decode"
                ),
                EnvelopeError::IncompatibleSchema { expected, found } => write!(
                    f,
                    "snapshot schema v{found} is incompatible with this reader (v{expected}); \
                     write an explicit migration or regenerate the snapshot"
                ),
                EnvelopeError::LengthMismatch { declared, actual } => write!(
                    f,
                    "snapshot payload length mismatch: header declares {declared}, {actual} bytes present"
                ),
                EnvelopeError::ChecksumMismatch { declared, computed } => write!(
                    f,
                    "snapshot payload checksum mismatch (declared {declared:#018x}, computed {computed:#018x}): corrupted bytes"
                ),
            }
        }
    }

    impl core::error::Error for EnvelopeError {}

    /// Type-independent metadata from a fully validated solver snapshot.
    ///
    /// This is the ledger bridge: it proves envelope version, exact payload
    /// length, and checksum before exposing the run provenance, without
    /// pretending the ledger knows the concrete solver state's type codec.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SnapshotEnvelopeInfo {
        type_id: u64,
        schema_version: u32,
        provenance: u64,
        payload_len: u64,
    }

    impl SnapshotEnvelopeInfo {
        /// Stable solver-state type identity carried by the envelope.
        #[must_use]
        pub const fn type_id(self) -> u64 {
            self.type_id
        }

        /// Concrete state codec version carried by the envelope.
        #[must_use]
        pub const fn schema_version(self) -> u32 {
            self.schema_version
        }

        /// Caller-ledgered logical run identity.
        #[must_use]
        pub const fn provenance(self) -> u64 {
            self.provenance
        }

        /// Exact validated payload byte length.
        #[must_use]
        pub const fn payload_len(self) -> u64 {
            self.payload_len
        }
    }

    /// Validate an envelope without decoding its solver-specific payload.
    ///
    /// # Errors
    /// Bad magic, an unsupported envelope version, truncation/append, or a
    /// checksum mismatch is refused before metadata is returned.
    pub fn inspect(bytes: &[u8]) -> Result<SnapshotEnvelopeInfo, EnvelopeError> {
        if bytes.len() < HEADER_LEN {
            if bytes.len() >= 8 && bytes[..8] != MAGIC {
                return Err(EnvelopeError::BadMagic);
            }
            return Err(EnvelopeError::Truncated {
                needed: HEADER_LEN,
                have: bytes.len(),
            });
        }
        if bytes[..8] != MAGIC {
            return Err(EnvelopeError::BadMagic);
        }
        let u32_at = |offset: usize| {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("header length"))
        };
        let u64_at = |offset: usize| {
            u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("header length"))
        };
        let envelope_version = u32_at(8);
        if envelope_version != ENVELOPE_VERSION {
            return Err(EnvelopeError::UnknownEnvelopeVersion {
                found: envelope_version,
            });
        }
        let payload_len = u64_at(32);
        let payload = &bytes[HEADER_LEN..];
        if payload_len != payload.len() as u64 {
            return Err(EnvelopeError::LengthMismatch {
                declared: payload_len,
                actual: payload.len() as u64,
            });
        }
        let declared_hash = u64_at(40);
        let computed = fs_obs::fnv1a64(payload);
        if computed != declared_hash {
            return Err(EnvelopeError::ChecksumMismatch {
                declared: declared_hash,
                computed,
            });
        }
        Ok(SnapshotEnvelopeInfo {
            type_id: u64_at(12),
            schema_version: u32_at(20),
            provenance: u64_at(24),
            payload_len,
        })
    }

    /// Seal a payload: canonical header + payload bytes.
    #[must_use]
    pub fn seal(type_id: u64, schema_version: u32, provenance: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&ENVELOPE_VERSION.to_le_bytes());
        out.extend_from_slice(&type_id.to_le_bytes());
        out.extend_from_slice(&schema_version.to_le_bytes());
        out.extend_from_slice(&provenance.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&fs_obs::fnv1a64(payload).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Validate an envelope for (`type_id`, `schema_version`) and return
    /// `(payload, provenance)`. Every header field is checked before a
    /// single payload byte is interpreted.
    ///
    /// # Errors
    /// [`EnvelopeError`], each naming the exact refusal.
    pub fn open(
        bytes: &[u8],
        type_id: u64,
        schema_version: u32,
    ) -> Result<(&[u8], u64), EnvelopeError> {
        if bytes.len() < HEADER_LEN {
            if bytes.len() >= 8 && bytes[..8] != MAGIC {
                return Err(EnvelopeError::BadMagic);
            }
            return Err(EnvelopeError::Truncated {
                needed: HEADER_LEN,
                have: bytes.len(),
            });
        }
        if bytes[..8] != MAGIC {
            return Err(EnvelopeError::BadMagic);
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().expect("len"));
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().expect("len"));
        let env_version = u32_at(8);
        if env_version != ENVELOPE_VERSION {
            return Err(EnvelopeError::UnknownEnvelopeVersion { found: env_version });
        }
        let found_type = u64_at(12);
        if found_type != type_id {
            return Err(EnvelopeError::WrongTypeId {
                expected: type_id,
                found: found_type,
            });
        }
        let found_schema = u32_at(20);
        if found_schema != schema_version {
            return Err(EnvelopeError::IncompatibleSchema {
                expected: schema_version,
                found: found_schema,
            });
        }
        let provenance = u64_at(24);
        let declared_len = u64_at(32);
        let payload = &bytes[HEADER_LEN..];
        if declared_len != payload.len() as u64 {
            return Err(EnvelopeError::LengthMismatch {
                declared: declared_len,
                actual: payload.len() as u64,
            });
        }
        let declared_hash = u64_at(40);
        let computed = fs_obs::fnv1a64(payload);
        if computed != declared_hash {
            return Err(EnvelopeError::ChecksumMismatch {
                declared: declared_hash,
                computed,
            });
        }
        Ok((payload, provenance))
    }
}

/// A structurally checked legacy v1 snapshot retained as exact bytes plus its
/// original 64-bit payload checksum. Header fields are parsed but not covered
/// by that checksum. The checksum remains legacy correlation data: it is never
/// widened or converted into a current semantic identity.
#[derive(Clone, Copy)]
pub struct LegacySnapshotV1<'a> {
    bytes: &'a [u8],
    info: envelope::SnapshotEnvelopeInfo,
    exact_bytes: fs_blake3::identity::ContentId,
    payload_checksum: u64,
}

impl core::fmt::Debug for LegacySnapshotV1<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("LegacySnapshotV1")
            .field("byte_len", &self.bytes.len())
            .field("info", &self.info)
            .field("exact_bytes", &self.exact_bytes)
            .field("payload_checksum", &self.payload_checksum)
            .finish()
    }
}

impl<'a> LegacySnapshotV1<'a> {
    /// Exact validated v1 envelope bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Validated legacy envelope metadata.
    #[must_use]
    pub const fn info(self) -> envelope::SnapshotEnvelopeInfo {
        self.info
    }

    /// Plain BLAKE3 identity of the exact retained v1 bytes.
    ///
    /// This is raw content identity only. It does not authenticate the
    /// producer and is not a v2 resume identity.
    #[must_use]
    pub const fn exact_bytes_id(self) -> fs_blake3::identity::ContentId {
        self.exact_bytes
    }

    /// The exact historical FNV-1a payload checksum carried by v1.
    #[must_use]
    pub const fn payload_checksum(self) -> u64 {
        self.payload_checksum
    }
}

/// Validate and quarantine an exact legacy v1 snapshot.
///
/// # Resource no-claim
/// This compatibility entry point performs the historical unbounded FNV pass
/// followed by an unbounded BLAKE3 pass and has no cancellation probe. Call it
/// only on bytes already bounded by a trusted outer transport. The explicit
/// capped/cancellable legacy migration boundary is successor work.
///
/// # Errors
/// The same structural refusals as [`envelope::inspect`].
pub fn inspect_legacy_snapshot_v1(
    bytes: &[u8],
) -> Result<LegacySnapshotV1<'_>, envelope::EnvelopeError> {
    let info = envelope::inspect(bytes)?;
    let payload_checksum = u64::from_le_bytes(
        bytes[40..48]
            .try_into()
            .expect("validated v1 header contains the checksum"),
    );
    Ok(LegacySnapshotV1 {
        bytes,
        info,
        exact_bytes: fs_blake3::identity::ContentId::of_bytes(bytes),
        payload_checksum,
    })
}

/// Strongly identified snapshot envelope v2.
///
/// V2 intentionally lives beside the v1 compatibility surface. An unkeyed
/// digest establishes content consistency, never producer authority. State
/// decoding is exposed only after either exact caller-held roots/context match
/// or an admitted external authority binds the recomputed exact-envelope plus
/// resume subject and an independent expected context also matches.
pub mod snapshot_v2 {
    use core::fmt;

    use fs_blake3::identity::{
        Admitted, AuthorityRef, CANONICAL_FRAME_VERSION, CancellationProbe, CanonicalEncoder,
        CanonicalError, CanonicalLimits, CanonicalSchema, ChildSpec, ContentId, Field, FieldSpec,
        IdentityAuditRecord, IdentityReceipt, SchemaId, SemanticId, StrongIdentity, WireType,
    };

    use super::{SolverStateV2, codec};
    use crate::cx::{
        DRAIN_FINALIZE_REPORT_IDENTITY_DOMAIN, DRAIN_FINALIZE_REPORT_IDENTITY_VERSION,
        DrainFinalizeReport,
    };

    /// Envelope layout version for the strong-identity format.
    pub const ENVELOPE_VERSION_V2: u32 = 2;
    /// Distinct artifact magic. V1 and v2 deliberately cannot be parsed as one
    /// another after a version-byte rewrite.
    pub const MAGIC_V2: [u8; 8] = *b"FSEXSNV2";
    /// Semantic resume-identity version, intentionally independent of the
    /// transport-envelope version.
    pub const SNAPSHOT_RESUME_IDENTITY_VERSION_V2: u32 = 2;
    /// Domain of the v2 semantic resume identity.
    pub const SNAPSHOT_RESUME_IDENTITY_DOMAIN_V2: &str = "org.frankensim.fs-exec.solver-resume.v2";
    /// Composite authorization-subject schema version.
    pub const SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_VERSION_V2: u32 = 2;
    /// Domain of the composite v2 policy-authority subject.
    pub const SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_DOMAIN_V2: &str =
        "org.frankensim.fs-exec.solver-snapshot-authority-subject.v2";
    /// Stable domain for the exact drain-report encoding-era discriminator.
    pub const SNAPSHOT_DRAIN_REPORT_ERA_DOMAIN_V2: &str =
        "org.frankensim.fs-exec.snapshot-drain-report-era.v2";
    /// Fixed v2 header length. Variable-length values belong in the payload or
    /// are represented by exact 32-byte identities.
    pub const HEADER_LEN_V2: usize = 588;

    const OFFSET_VERSION: usize = 8;
    const OFFSET_HEADER_LEN: usize = 12;
    const OFFSET_STATE_TYPE: usize = 16;
    const OFFSET_STATE_SCHEMA: usize = 48;
    const OFFSET_STATE_CODEC: usize = 80;
    const OFFSET_STATE_CODEC_VERSION: usize = 112;
    const OFFSET_ALGORITHM: usize = 116;
    const OFFSET_ALGORITHM_VERSION: usize = 148;
    const OFFSET_PROBLEM: usize = 156;
    const OFFSET_RNG_COUNTER: usize = 188;
    const OFFSET_DETERMINISM: usize = 220;
    const OFFSET_LIFECYCLE: usize = 221;
    const OFFSET_RESERVED: usize = 222;
    const RESERVED_LEN: usize = 2;
    const OFFSET_CANONICAL_FRAME_VERSION: usize = 224;
    const OFFSET_RESUME_SCHEMA_ID: usize = 228;
    const OFFSET_AUTHORITY_SCHEMA_ID: usize = 260;
    const OFFSET_DRAIN_REPORT_ERA: usize = 292;
    const OFFSET_EXECUTION_FINGERPRINT: usize = 324;
    const OFFSET_BUDGET: usize = 356;
    const OFFSET_PROVENANCE: usize = 388;
    const OFFSET_PAUSE_REQUEST: usize = 420;
    const OFFSET_GATE_GENERATION: usize = 452;
    const OFFSET_DRAIN_RUN: usize = 460;
    const OFFSET_DRAIN_REGISTERED: usize = 468;
    const OFFSET_DRAINED_WORKERS: usize = 476;
    const OFFSET_DRAIN_REPORT: usize = 484;
    const OFFSET_PAYLOAD_LEN: usize = 516;
    const OFFSET_PAYLOAD_CONTENT: usize = 524;
    const OFFSET_RESUME_ID: usize = 556;
    const LIFECYCLE_PAUSED_AND_DRAINED: u8 = 1;

    const _: () = {
        assert!(OFFSET_VERSION == MAGIC_V2.len());
        assert!(OFFSET_HEADER_LEN == OFFSET_VERSION + 4);
        assert!(OFFSET_STATE_TYPE == OFFSET_HEADER_LEN + 4);
        assert!(OFFSET_STATE_SCHEMA == OFFSET_STATE_TYPE + 32);
        assert!(OFFSET_STATE_CODEC == OFFSET_STATE_SCHEMA + 32);
        assert!(OFFSET_STATE_CODEC_VERSION == OFFSET_STATE_CODEC + 32);
        assert!(OFFSET_ALGORITHM == OFFSET_STATE_CODEC_VERSION + 4);
        assert!(OFFSET_ALGORITHM_VERSION == OFFSET_ALGORITHM + 32);
        assert!(OFFSET_PROBLEM == OFFSET_ALGORITHM_VERSION + 8);
        assert!(OFFSET_RNG_COUNTER == OFFSET_PROBLEM + 32);
        assert!(OFFSET_DETERMINISM == OFFSET_RNG_COUNTER + 32);
        assert!(OFFSET_LIFECYCLE == OFFSET_DETERMINISM + 1);
        assert!(OFFSET_RESERVED == OFFSET_LIFECYCLE + 1);
        assert!(OFFSET_CANONICAL_FRAME_VERSION == OFFSET_RESERVED + RESERVED_LEN);
        assert!(OFFSET_RESUME_SCHEMA_ID == OFFSET_CANONICAL_FRAME_VERSION + 4);
        assert!(OFFSET_AUTHORITY_SCHEMA_ID == OFFSET_RESUME_SCHEMA_ID + 32);
        assert!(OFFSET_DRAIN_REPORT_ERA == OFFSET_AUTHORITY_SCHEMA_ID + 32);
        assert!(OFFSET_EXECUTION_FINGERPRINT == OFFSET_DRAIN_REPORT_ERA + 32);
        assert!(OFFSET_BUDGET == OFFSET_EXECUTION_FINGERPRINT + 32);
        assert!(OFFSET_PROVENANCE == OFFSET_BUDGET + 32);
        assert!(OFFSET_PAUSE_REQUEST == OFFSET_PROVENANCE + 32);
        assert!(OFFSET_GATE_GENERATION == OFFSET_PAUSE_REQUEST + 32);
        assert!(OFFSET_DRAIN_RUN == OFFSET_GATE_GENERATION + 8);
        assert!(OFFSET_DRAIN_REGISTERED == OFFSET_DRAIN_RUN + 8);
        assert!(OFFSET_DRAINED_WORKERS == OFFSET_DRAIN_REGISTERED + 8);
        assert!(OFFSET_DRAIN_REPORT == OFFSET_DRAINED_WORKERS + 8);
        assert!(OFFSET_PAYLOAD_LEN == OFFSET_DRAIN_REPORT + 32);
        assert!(OFFSET_PAYLOAD_CONTENT == OFFSET_PAYLOAD_LEN + 8);
        assert!(OFFSET_RESUME_ID == OFFSET_PAYLOAD_CONTENT + 32);
        assert!(HEADER_LEN_V2 == OFFSET_RESUME_ID + 32);
    };

    macro_rules! snapshot_binding_id {
        ($(#[$meta:meta])* $name:ident) => {
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub struct $name([u8; 32]);

            impl $name {
                /// Declare an exact identity supplied by the owning layer.
                /// Presence is not authentication; authority remains separate.
                #[must_use]
                pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                    Self(bytes)
                }

                /// Exact identity bytes.
                #[must_use]
                pub const fn as_bytes(&self) -> &[u8; 32] {
                    &self.0
                }
            }
        };
    }

    snapshot_binding_id!(
        /// Full-width nominal state-type identity declared by the state owner.
        SnapshotStateTypeIdV2
    );
    snapshot_binding_id!(
        /// Full-width state-schema identity declared by the state owner.
        SnapshotStateSchemaIdV2
    );
    snapshot_binding_id!(
        /// Full-width identity of the exact payload codec grammar.
        SnapshotStateCodecIdV2
    );
    snapshot_binding_id!(
        /// Full-width algorithm-family identity declared by the solver owner.
        SnapshotAlgorithmIdV2
    );
    snapshot_binding_id!(
        /// Full-width semantic problem identity supplied by the owning layer.
        SnapshotProblemIdV2
    );
    snapshot_binding_id!(
        /// Identity of exact RNG streams, counters, and stochastic cursor state.
        SnapshotRngCounterIdV2
    );
    snapshot_binding_id!(
        /// Identity of ISA, numeric, dispatch, and execution assumptions needed
        /// by the deterministic replay contract.
        SnapshotExecutionFingerprintIdV2
    );
    snapshot_binding_id!(
        /// Identity of remaining/consumed budget state at the pause boundary.
        SnapshotBudgetStateIdV2
    );
    snapshot_binding_id!(
        /// Identity of the complete run/ledger provenance context.
        SnapshotProvenanceIdV2
    );
    snapshot_binding_id!(
        /// Identity binding for the caller-declared pause request. This value
        /// is not proof of admission; the session layer must retain evidence.
        SnapshotPauseRequestIdV2
    );
    snapshot_binding_id!(
        /// Exact domain/version/wire-grammar era of executor drain reports.
        SnapshotDrainReportEraIdV2
    );

    /// Exact whole-envelope byte identity. This proves no origin or authority.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct SnapshotContentIdV2(ContentId);

    impl SnapshotContentIdV2 {
        fn from_content_id(content: ContentId) -> Self {
            Self(content)
        }

        /// Parse a raw retained digest for diagnostics or registry transport.
        /// Parsing does not verify bytes and cannot mint a
        /// [`SnapshotExpectationV2`].
        #[must_use]
        pub fn parse_slice(bytes: &[u8]) -> Option<Self> {
            ContentId::parse_slice(bytes).map(Self)
        }

        /// Exact digest bytes.
        #[must_use]
        pub fn as_bytes(&self) -> &[u8; 32] {
            self.0.as_bytes()
        }

        /// Lowercase hexadecimal rendering.
        #[must_use]
        pub fn to_hex(self) -> String {
            self.0.to_hex()
        }
    }

    impl fmt::Display for SnapshotContentIdV2 {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.0, formatter)
        }
    }

    /// Exact payload-byte identity stored inside the v2 header. It is distinct
    /// from the identity of the complete envelope.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct SnapshotPayloadContentIdV2(ContentId);

    impl SnapshotPayloadContentIdV2 {
        fn from_content_id(content: ContentId) -> Self {
            Self(content)
        }

        fn parse_slice(bytes: &[u8]) -> Option<Self> {
            ContentId::parse_slice(bytes).map(Self)
        }

        /// Exact digest bytes.
        #[must_use]
        pub fn as_bytes(&self) -> &[u8; 32] {
            self.0.as_bytes()
        }
    }

    /// Execution determinism contract bound into the resume identity.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum SnapshotDeterminismV2 {
        /// Fixed logical scheduling/reduction semantics.
        Deterministic,
        /// Explicitly ledgered relaxed/fast semantics.
        Fast,
    }

    impl SnapshotDeterminismV2 {
        const fn tag(self) -> u8 {
            match self {
                Self::Deterministic => 1,
                Self::Fast => 2,
            }
        }

        fn from_tag(tag: u8) -> Option<Self> {
            match tag {
                1 => Some(Self::Deterministic),
                2 => Some(Self::Fast),
                _ => None,
            }
        }
    }

    /// Header-safe declaration of a pause boundary.
    ///
    /// This value is deliberately observational: parsing candidate bytes can
    /// construct it, but no public conversion can turn it into the
    /// typed [`PausedSnapshotBoundaryV2`] input required to mint an
    /// [`ExpectedResumeContextV2`]. Keeping those types distinct prevents the
    /// parser itself from laundering candidate fields into caller expectation;
    /// neither type is producer authentication or an atomic state-freeze proof.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DeclaredPausedSnapshotBoundaryV2 {
        pause_request: SnapshotPauseRequestIdV2,
        gate_generation: u64,
        run: u64,
        registered_workers: u64,
        drained_workers: u64,
        drain_report: [u8; 32],
    }

    impl DeclaredPausedSnapshotBoundaryV2 {
        fn from_header(
            pause_request: SnapshotPauseRequestIdV2,
            gate_generation: u64,
            run: u64,
            registered_workers: u64,
            drained_workers: u64,
            drain_report: [u8; 32],
        ) -> Result<Self, SnapshotV2Error> {
            if registered_workers == 0 || registered_workers != drained_workers {
                return Err(SnapshotV2Error::InvalidDrainBoundary {
                    registered: registered_workers,
                    drained: drained_workers,
                });
            }
            let computed = drain_report_content(run, registered_workers, drained_workers);
            if drain_report != computed {
                return Err(SnapshotV2Error::DrainReportMismatch {
                    declared: drain_report,
                    computed,
                });
            }
            Ok(Self {
                pause_request,
                gate_generation,
                run,
                registered_workers,
                drained_workers,
                drain_report,
            })
        }

        /// Exact pause-request binding supplied by the session owner.
        #[must_use]
        pub const fn pause_request(self) -> SnapshotPauseRequestIdV2 {
            self.pause_request
        }

        /// Exact session gate generation at which the old run stopped.
        #[must_use]
        pub const fn gate_generation(self) -> u64 {
            self.gate_generation
        }

        /// Logical executor run that drained.
        #[must_use]
        pub const fn run(self) -> u64 {
            self.run
        }

        /// Total workers admitted by the executor drain tracker.
        #[must_use]
        pub const fn registered_workers(self) -> u64 {
            self.registered_workers
        }

        /// Total worker guards released before finalization.
        #[must_use]
        pub const fn drained_workers(self) -> u64 {
            self.drained_workers
        }

        /// Exact domain-separated executor report content identity.
        #[must_use]
        pub const fn drain_report(self) -> [u8; 32] {
            self.drain_report
        }
    }

    /// Typed drain-report binding required to construct caller-relative resume
    /// expectations.
    ///
    /// The only public constructor consumes [`DrainFinalizeReport`], whose
    /// fields are private and which only [`crate::cx::DrainTracker::finalize`]
    /// can mint after cancellation and complete worker drain. Candidate header
    /// parsing never constructs or returns this type. The report is reproducible
    /// for the same caller-selected run/counts and does not prove association
    /// with the supplied request, generation, session, or frozen solver state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PausedSnapshotBoundaryV2 {
        declaration: DeclaredPausedSnapshotBoundaryV2,
    }

    impl PausedSnapshotBoundaryV2 {
        /// Bind caller-declared pause-request and gate-generation labels to a
        /// typed request -> drain -> finalize report.
        #[must_use]
        pub fn from_drain_report(
            report: DrainFinalizeReport,
            pause_request: SnapshotPauseRequestIdV2,
            gate_generation: u64,
        ) -> Self {
            debug_assert_eq!(report.registered_workers(), report.drained_workers());
            debug_assert_ne!(report.registered_workers(), 0);
            Self {
                declaration: DeclaredPausedSnapshotBoundaryV2 {
                    pause_request,
                    gate_generation,
                    run: report.run().0,
                    registered_workers: report.registered_workers(),
                    drained_workers: report.drained_workers(),
                    drain_report: *report.content_hash().as_bytes(),
                },
            }
        }

        /// One-way observational declaration of this typed report binding.
        /// No inverse conversion from declarations exists.
        #[must_use]
        pub const fn declaration(self) -> DeclaredPausedSnapshotBoundaryV2 {
            self.declaration
        }
    }

    fn drain_report_content(run: u64, registered_workers: u64, drained_workers: u64) -> [u8; 32] {
        let mut preimage = [0_u8; 28];
        preimage[..4].copy_from_slice(&DRAIN_FINALIZE_REPORT_IDENTITY_VERSION.to_le_bytes());
        preimage[4..12].copy_from_slice(&run.to_le_bytes());
        preimage[12..20].copy_from_slice(&registered_workers.to_le_bytes());
        preimage[20..28].copy_from_slice(&drained_workers.to_le_bytes());
        *fs_blake3::hash_domain(DRAIN_FINALIZE_REPORT_IDENTITY_DOMAIN, &preimage).as_bytes()
    }

    pub(super) fn current_drain_report_era() -> SnapshotDrainReportEraIdV2 {
        let domain = ContentId::of_bytes(DRAIN_FINALIZE_REPORT_IDENTITY_DOMAIN.as_bytes());
        let mut preimage = [0_u8; 45];
        preimage[..4].copy_from_slice(&DRAIN_FINALIZE_REPORT_IDENTITY_VERSION.to_le_bytes());
        preimage[4..36].copy_from_slice(domain.as_bytes());
        // Descriptor of the current report preimage grammar: little-endian u32
        // version followed by little-endian u64 run, registered-worker, and
        // drained-worker fields in that order. This literal pins the current
        // era but is not mechanically derived from the report encoder; the
        // registry/coupling successor must make synchronized rotation and
        // independent retained vectors executable obligations.
        preimage[36..45].copy_from_slice(&[1, 4, 8, 8, 8, 0, 1, 2, 3]);
        SnapshotDrainReportEraIdV2::from_bytes(
            *fs_blake3::hash_domain(SNAPSHOT_DRAIN_REPORT_ERA_DOMAIN_V2, &preimage).as_bytes(),
        )
    }

    /// Exact resume fields declared by the current v2 envelope tranche.
    /// Session-owned solver configuration and atomic freeze evidence remain
    /// separate admission inputs and are not inferred from this data bundle.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct SnapshotResumeContextV2 {
        state_type: SnapshotStateTypeIdV2,
        state_schema: SnapshotStateSchemaIdV2,
        state_codec: SnapshotStateCodecIdV2,
        state_codec_version: u32,
        algorithm: SnapshotAlgorithmIdV2,
        algorithm_version: u64,
        problem: SnapshotProblemIdV2,
        rng_counter: SnapshotRngCounterIdV2,
        determinism: SnapshotDeterminismV2,
        execution_fingerprint: SnapshotExecutionFingerprintIdV2,
        budget: SnapshotBudgetStateIdV2,
        provenance: SnapshotProvenanceIdV2,
        pause_boundary: DeclaredPausedSnapshotBoundaryV2,
    }

    impl SnapshotResumeContextV2 {
        /// Build a resumable context from state-owned nominal identities and
        /// a typed drain-report boundary. No legacy u64 ID is widened.
        #[allow(clippy::too_many_arguments)]
        #[must_use]
        fn for_paused_state<S: SolverStateV2>(
            algorithm: SnapshotAlgorithmIdV2,
            algorithm_version: u64,
            problem: SnapshotProblemIdV2,
            rng_counter: SnapshotRngCounterIdV2,
            determinism: SnapshotDeterminismV2,
            execution_fingerprint: SnapshotExecutionFingerprintIdV2,
            budget: SnapshotBudgetStateIdV2,
            provenance: SnapshotProvenanceIdV2,
            pause_boundary: PausedSnapshotBoundaryV2,
        ) -> Self {
            Self {
                state_type: S::STATE_TYPE_ID_V2,
                state_schema: S::STATE_SCHEMA_ID_V2,
                state_codec: S::STATE_CODEC_ID_V2,
                state_codec_version: S::STATE_CODEC_VERSION_V2,
                algorithm,
                algorithm_version,
                problem,
                rng_counter,
                determinism,
                execution_fingerprint,
                budget,
                provenance,
                pause_boundary: pause_boundary.declaration(),
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn from_header(
            state_type: SnapshotStateTypeIdV2,
            state_schema: SnapshotStateSchemaIdV2,
            state_codec: SnapshotStateCodecIdV2,
            state_codec_version: u32,
            algorithm: SnapshotAlgorithmIdV2,
            algorithm_version: u64,
            problem: SnapshotProblemIdV2,
            rng_counter: SnapshotRngCounterIdV2,
            determinism: SnapshotDeterminismV2,
            execution_fingerprint: SnapshotExecutionFingerprintIdV2,
            budget: SnapshotBudgetStateIdV2,
            provenance: SnapshotProvenanceIdV2,
            pause_boundary: DeclaredPausedSnapshotBoundaryV2,
        ) -> Self {
            Self {
                state_type,
                state_schema,
                state_codec,
                state_codec_version,
                algorithm,
                algorithm_version,
                problem,
                rng_counter,
                determinism,
                execution_fingerprint,
                budget,
                provenance,
                pause_boundary,
            }
        }

        /// Whether this context names exactly state type `S` and its v2 codec.
        #[must_use]
        pub fn matches_state<S: SolverStateV2>(&self) -> bool {
            self.state_type == S::STATE_TYPE_ID_V2
                && self.state_schema == S::STATE_SCHEMA_ID_V2
                && self.state_codec == S::STATE_CODEC_ID_V2
                && self.state_codec_version == S::STATE_CODEC_VERSION_V2
        }

        /// Full-width nominal state-type identity.
        #[must_use]
        pub const fn state_type(&self) -> SnapshotStateTypeIdV2 {
            self.state_type
        }

        /// Full-width state-schema identity.
        #[must_use]
        pub const fn state_schema(&self) -> SnapshotStateSchemaIdV2 {
            self.state_schema
        }

        /// Full-width payload-codec identity.
        #[must_use]
        pub const fn state_codec(&self) -> SnapshotStateCodecIdV2 {
            self.state_codec
        }

        /// State codec version within the full-width schema domain.
        #[must_use]
        pub const fn state_codec_version(&self) -> u32 {
            self.state_codec_version
        }

        /// Algorithm family identity.
        #[must_use]
        pub const fn algorithm(&self) -> SnapshotAlgorithmIdV2 {
            self.algorithm
        }

        /// Algorithm implementation/semantic version.
        #[must_use]
        pub const fn algorithm_version(&self) -> u64 {
            self.algorithm_version
        }

        /// Semantic problem identity.
        #[must_use]
        pub const fn problem(&self) -> SnapshotProblemIdV2 {
            self.problem
        }

        /// RNG/counter state identity.
        #[must_use]
        pub const fn rng_counter(&self) -> SnapshotRngCounterIdV2 {
            self.rng_counter
        }

        /// Determinism contract.
        #[must_use]
        pub const fn determinism(&self) -> SnapshotDeterminismV2 {
            self.determinism
        }

        /// Exact execution/numeric fingerprint expected for replay.
        #[must_use]
        pub const fn execution_fingerprint(&self) -> SnapshotExecutionFingerprintIdV2 {
            self.execution_fingerprint
        }

        /// Budget-state identity.
        #[must_use]
        pub const fn budget(&self) -> SnapshotBudgetStateIdV2 {
            self.budget
        }

        /// Provenance-context identity.
        #[must_use]
        pub const fn provenance(&self) -> SnapshotProvenanceIdV2 {
            self.provenance
        }

        /// Header-safe declaration of the pause/drain/finalize boundary.
        /// This observational value has no conversion into an expected-context
        /// token.
        #[must_use]
        pub const fn pause_boundary(&self) -> DeclaredPausedSnapshotBoundaryV2 {
            self.pause_boundary
        }
    }

    /// First exact semantic field that differs between a caller expectation
    /// and a candidate declaration. Keeping diagnostics compact avoids placing
    /// two complete resume contexts in every [`SnapshotV2Error`] value.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum SnapshotContextFieldV2 {
        /// Nominal Rust state-family identity.
        StateType,
        /// State semantic-schema identity.
        StateSchema,
        /// Payload codec grammar identity.
        StateCodec,
        /// Payload codec semantic version.
        StateCodecVersion,
        /// Solver algorithm-family identity.
        Algorithm,
        /// Solver algorithm semantic version.
        AlgorithmVersion,
        /// Semantic problem identity.
        Problem,
        /// RNG stream/counter cursor identity.
        RngCounter,
        /// Deterministic or explicitly relaxed execution mode.
        Determinism,
        /// ISA/numeric/dispatch execution fingerprint.
        ExecutionFingerprint,
        /// Remaining and consumed budget-state identity.
        Budget,
        /// Run and ledger provenance identity.
        Provenance,
        /// Caller-declared pause-request identity.
        PauseRequest,
        /// Caller-declared session gate generation.
        GateGeneration,
        /// Logical run named by the drain report.
        DrainRun,
        /// Workers registered by the drain tracker.
        DrainRegistered,
        /// Worker guards drained before tracker finalization.
        DrainDrained,
        /// Domain-separated drain-report content identity.
        DrainReport,
    }

    impl SnapshotContextFieldV2 {
        /// Stable diagnostic field name.
        #[must_use]
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::StateType => "state-type",
                Self::StateSchema => "state-schema",
                Self::StateCodec => "state-codec",
                Self::StateCodecVersion => "state-codec-version",
                Self::Algorithm => "algorithm",
                Self::AlgorithmVersion => "algorithm-version",
                Self::Problem => "problem",
                Self::RngCounter => "rng-counter",
                Self::Determinism => "determinism",
                Self::ExecutionFingerprint => "execution-fingerprint",
                Self::Budget => "budget",
                Self::Provenance => "provenance",
                Self::PauseRequest => "pause-request",
                Self::GateGeneration => "gate-generation",
                Self::DrainRun => "drain-run",
                Self::DrainRegistered => "drain-registered",
                Self::DrainDrained => "drain-drained",
                Self::DrainReport => "drain-report",
            }
        }
    }

    fn first_context_mismatch(
        expected: &SnapshotResumeContextV2,
        found: &SnapshotResumeContextV2,
    ) -> Option<SnapshotContextFieldV2> {
        let fields = [
            (
                expected.state_type != found.state_type,
                SnapshotContextFieldV2::StateType,
            ),
            (
                expected.state_schema != found.state_schema,
                SnapshotContextFieldV2::StateSchema,
            ),
            (
                expected.state_codec != found.state_codec,
                SnapshotContextFieldV2::StateCodec,
            ),
            (
                expected.state_codec_version != found.state_codec_version,
                SnapshotContextFieldV2::StateCodecVersion,
            ),
            (
                expected.algorithm != found.algorithm,
                SnapshotContextFieldV2::Algorithm,
            ),
            (
                expected.algorithm_version != found.algorithm_version,
                SnapshotContextFieldV2::AlgorithmVersion,
            ),
            (
                expected.problem != found.problem,
                SnapshotContextFieldV2::Problem,
            ),
            (
                expected.rng_counter != found.rng_counter,
                SnapshotContextFieldV2::RngCounter,
            ),
            (
                expected.determinism != found.determinism,
                SnapshotContextFieldV2::Determinism,
            ),
            (
                expected.execution_fingerprint != found.execution_fingerprint,
                SnapshotContextFieldV2::ExecutionFingerprint,
            ),
            (
                expected.budget != found.budget,
                SnapshotContextFieldV2::Budget,
            ),
            (
                expected.provenance != found.provenance,
                SnapshotContextFieldV2::Provenance,
            ),
            (
                expected.pause_boundary.pause_request != found.pause_boundary.pause_request,
                SnapshotContextFieldV2::PauseRequest,
            ),
            (
                expected.pause_boundary.gate_generation != found.pause_boundary.gate_generation,
                SnapshotContextFieldV2::GateGeneration,
            ),
            (
                expected.pause_boundary.run != found.pause_boundary.run,
                SnapshotContextFieldV2::DrainRun,
            ),
            (
                expected.pause_boundary.registered_workers
                    != found.pause_boundary.registered_workers,
                SnapshotContextFieldV2::DrainRegistered,
            ),
            (
                expected.pause_boundary.drained_workers != found.pause_boundary.drained_workers,
                SnapshotContextFieldV2::DrainDrained,
            ),
            (
                expected.pause_boundary.drain_report != found.pause_boundary.drain_report,
                SnapshotContextFieldV2::DrainReport,
            ),
        ];
        fields
            .into_iter()
            .find_map(|(differs, field)| differs.then_some(field))
    }

    /// Caller-constructed expected context required by every producer and
    /// decode-admission path.
    ///
    /// This nominal wrapper cannot be made from [`SnapshotInspectionV2`]'s
    /// header-declared context. Its only public constructor requires an actual
    /// typed [`PausedSnapshotBoundaryV2`], preventing unanchored inspection from
    /// directly converting candidate header fields into an expectation. It is
    /// not global authority or proof of an atomic solver-state freeze.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ExpectedResumeContextV2(SnapshotResumeContextV2);

    impl ExpectedResumeContextV2 {
        /// Construct the exact context expected by the current solver/session.
        #[allow(clippy::too_many_arguments)]
        #[must_use]
        pub fn for_paused_state<S: SolverStateV2>(
            algorithm: SnapshotAlgorithmIdV2,
            algorithm_version: u64,
            problem: SnapshotProblemIdV2,
            rng_counter: SnapshotRngCounterIdV2,
            determinism: SnapshotDeterminismV2,
            execution_fingerprint: SnapshotExecutionFingerprintIdV2,
            budget: SnapshotBudgetStateIdV2,
            provenance: SnapshotProvenanceIdV2,
            pause_boundary: PausedSnapshotBoundaryV2,
        ) -> Self {
            Self(SnapshotResumeContextV2::for_paused_state::<S>(
                algorithm,
                algorithm_version,
                problem,
                rng_counter,
                determinism,
                execution_fingerprint,
                budget,
                provenance,
                pause_boundary,
            ))
        }

        /// Exact semantic context supplied independently of candidate parsing.
        #[must_use]
        pub const fn context(&self) -> &SnapshotResumeContextV2 {
            &self.0
        }
    }

    /// Static semantic schema for the v2 resume identity.
    pub enum SnapshotResumeIdentitySchemaV2 {}

    impl CanonicalSchema for SnapshotResumeIdentitySchemaV2 {
        const DOMAIN: &'static str = SNAPSHOT_RESUME_IDENTITY_DOMAIN_V2;
        const NAME: &'static str = "solver-resume";
        const VERSION: u32 = SNAPSHOT_RESUME_IDENTITY_VERSION_V2;
        const CONTEXT: &'static str = "nominal state type/schema/codec, algorithm, semantic problem, stochastic cursor, determinism and execution fingerprint, budget, provenance, caller-declared pause/drain-report fields, and exact payload identity; structural consistency only, with no producer authentication or atomic-freeze provenance";
        const FIELDS: &'static [FieldSpec] = &[
            FieldSpec::required("state-type", WireType::Bytes),
            FieldSpec::required("state-schema", WireType::Bytes),
            FieldSpec::required("state-codec", WireType::Bytes),
            FieldSpec::required("state-codec-version", WireType::U64),
            FieldSpec::required("algorithm", WireType::Bytes),
            FieldSpec::required("algorithm-version", WireType::U64),
            FieldSpec::required("problem", WireType::Bytes),
            FieldSpec::required("rng-counter", WireType::Bytes),
            FieldSpec::required("determinism", WireType::Variant),
            FieldSpec::required("execution-fingerprint", WireType::Bytes),
            FieldSpec::required("budget", WireType::Bytes),
            FieldSpec::required("provenance", WireType::Bytes),
            FieldSpec::required("pause-request", WireType::Bytes),
            FieldSpec::required("gate-generation", WireType::U64),
            FieldSpec::required("drain-report-version", WireType::U64),
            FieldSpec::required("drain-report-era", WireType::Bytes),
            FieldSpec::required("drain-run", WireType::U64),
            FieldSpec::required("drain-registered", WireType::U64),
            FieldSpec::required("drain-drained", WireType::U64),
            FieldSpec::required("drain-report", WireType::Bytes),
            FieldSpec::required("payload-content", WireType::Bytes),
            FieldSpec::required("payload-length", WireType::U64),
        ];
    }

    /// Typed semantic identity required to resume one exact state payload.
    pub type SnapshotResumeIdV2 = SemanticId<SnapshotResumeIdentitySchemaV2>;

    static SNAPSHOT_RESUME_CHILD_V2: ChildSpec = ChildSpec::for_identity::<SnapshotResumeIdV2>();

    /// Canonical schema for policy authorization of one exact envelope and its
    /// exact resume semantics. Neither component may be substituted.
    pub enum SnapshotAuthoritySubjectSchemaV2 {}

    impl CanonicalSchema for SnapshotAuthoritySubjectSchemaV2 {
        const DOMAIN: &'static str = SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_DOMAIN_V2;
        const NAME: &'static str = "solver-snapshot-authority-subject";
        const VERSION: u32 = SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_VERSION_V2;
        const CONTEXT: &'static str = "policy-relative authorization subject binding exact complete envelope bytes and exact typed resume semantics";
        const FIELDS: &'static [FieldSpec] = &[
            FieldSpec::required("envelope-content", WireType::Bytes),
            FieldSpec::child_of("resume", &SNAPSHOT_RESUME_CHILD_V2),
        ];
    }

    /// Typed subject presented to an injected verifier/admitter. Admission is
    /// policy-relative and does not by itself imply a cryptographic signature.
    pub type SnapshotAuthoritySubjectIdV2 = SemanticId<SnapshotAuthoritySubjectSchemaV2>;

    #[allow(dead_code)]
    struct SnapshotResumeIdentityComponentsV2 {
        state_type: SnapshotStateTypeIdV2,
        state_schema: SnapshotStateSchemaIdV2,
        state_codec: SnapshotStateCodecIdV2,
        state_codec_version: u32,
        algorithm: SnapshotAlgorithmIdV2,
        algorithm_version: u64,
        problem: SnapshotProblemIdV2,
        rng_counter: SnapshotRngCounterIdV2,
        determinism: SnapshotDeterminismV2,
        execution_fingerprint: SnapshotExecutionFingerprintIdV2,
        budget: SnapshotBudgetStateIdV2,
        provenance: SnapshotProvenanceIdV2,
        pause_request: SnapshotPauseRequestIdV2,
        gate_generation: u64,
        drain_report_version: u32,
        drain_report_era: SnapshotDrainReportEraIdV2,
        drain_run: u64,
        drain_registered: u64,
        drain_drained: u64,
        drain_report: [u8; 32],
        payload_content: SnapshotPayloadContentIdV2,
        payload_len: u64,
    }

    /// Owner-local declaration consumed by the identity schema checker.
    pub const SNAPSHOT_RESUME_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
        "frankensim-identity-schema-v1",
        "id=fs-exec:solver-resume",
        "version_const=SNAPSHOT_RESUME_IDENTITY_VERSION_V2",
        "version=2",
        "domain=org.frankensim.fs-exec.solver-resume.v2",
        "domain_const=SNAPSHOT_RESUME_IDENTITY_DOMAIN_V2",
        "encoder=resume_receipt",
        "encoder_helpers=resume_identity_components,encode_resume_receipt,ExpectedResumeContextV2::for_paused_state,SnapshotResumeContextV2::for_paused_state,SnapshotResumeContextV2::from_header,PausedSnapshotBoundaryV2::from_drain_report,PausedSnapshotBoundaryV2::declaration,DeclaredPausedSnapshotBoundaryV2::from_header,current_drain_report_era",
        "schema_constants=SNAPSHOT_RESUME_IDENTITY_VERSION_V2,SNAPSHOT_DRAIN_REPORT_ERA_DOMAIN_V2,crates/fs-exec/src/cx.rs#DRAIN_FINALIZE_REPORT_IDENTITY_VERSION,crates/fs-exec/src/cx.rs#DRAIN_FINALIZE_REPORT_IDENTITY_DOMAIN",
        "schema_functions=resume_receipt,resume_identity_components,encode_resume_receipt,current_drain_report_era,crates/fs-blake3/src/identity.rs#CanonicalEncoder::new,crates/fs-blake3/src/identity.rs#CanonicalEncoder::bytes,crates/fs-blake3/src/identity.rs#CanonicalEncoder::u64,crates/fs-blake3/src/identity.rs#CanonicalEncoder::variant,crates/fs-blake3/src/identity.rs#CanonicalEncoder::finish,crates/fs-blake3/src/lib.rs#hash_domain,crates/fs-blake3/src/identity.rs#ContentId::of_bytes",
        "schema_dependencies=fs-blake3:canonical-identity-frame",
        "digest=fs-blake3",
        "encoding=typed-binary",
        "sources=SnapshotResumeIdentityComponentsV2",
        "source_fields=SnapshotResumeIdentityComponentsV2.state_type:semantic,SnapshotResumeIdentityComponentsV2.state_schema:semantic,SnapshotResumeIdentityComponentsV2.state_codec:semantic,SnapshotResumeIdentityComponentsV2.state_codec_version:semantic,SnapshotResumeIdentityComponentsV2.algorithm:semantic,SnapshotResumeIdentityComponentsV2.algorithm_version:semantic,SnapshotResumeIdentityComponentsV2.problem:semantic,SnapshotResumeIdentityComponentsV2.rng_counter:semantic,SnapshotResumeIdentityComponentsV2.determinism:semantic,SnapshotResumeIdentityComponentsV2.execution_fingerprint:semantic,SnapshotResumeIdentityComponentsV2.budget:semantic,SnapshotResumeIdentityComponentsV2.provenance:semantic,SnapshotResumeIdentityComponentsV2.pause_request:semantic,SnapshotResumeIdentityComponentsV2.gate_generation:semantic,SnapshotResumeIdentityComponentsV2.drain_report_version:semantic,SnapshotResumeIdentityComponentsV2.drain_report_era:semantic,SnapshotResumeIdentityComponentsV2.drain_run:semantic,SnapshotResumeIdentityComponentsV2.drain_registered:semantic,SnapshotResumeIdentityComponentsV2.drain_drained:semantic,SnapshotResumeIdentityComponentsV2.drain_report:semantic,SnapshotResumeIdentityComponentsV2.payload_content:semantic,SnapshotResumeIdentityComponentsV2.payload_len:semantic",
        "source_bindings=SnapshotResumeIdentityComponentsV2.state_type>state-type,SnapshotResumeIdentityComponentsV2.state_schema>state-schema,SnapshotResumeIdentityComponentsV2.state_codec>state-codec,SnapshotResumeIdentityComponentsV2.state_codec_version>state-codec-version,SnapshotResumeIdentityComponentsV2.algorithm>algorithm,SnapshotResumeIdentityComponentsV2.algorithm_version>algorithm-version,SnapshotResumeIdentityComponentsV2.problem>problem,SnapshotResumeIdentityComponentsV2.rng_counter>rng-counter,SnapshotResumeIdentityComponentsV2.determinism>determinism,SnapshotResumeIdentityComponentsV2.execution_fingerprint>execution-fingerprint,SnapshotResumeIdentityComponentsV2.budget>budget,SnapshotResumeIdentityComponentsV2.provenance>provenance,SnapshotResumeIdentityComponentsV2.pause_request>pause-request,SnapshotResumeIdentityComponentsV2.gate_generation>gate-generation,SnapshotResumeIdentityComponentsV2.drain_report_version>drain-report-version,SnapshotResumeIdentityComponentsV2.drain_report_era>drain-report-era,SnapshotResumeIdentityComponentsV2.drain_run>drain-run,SnapshotResumeIdentityComponentsV2.drain_registered>drain-registered,SnapshotResumeIdentityComponentsV2.drain_drained>drain-drained,SnapshotResumeIdentityComponentsV2.drain_report>drain-report,SnapshotResumeIdentityComponentsV2.payload_content>payload-content,SnapshotResumeIdentityComponentsV2.payload_len>payload-length",
        "external_semantic_fields=canonical-frame-schema",
        "semantic_fields=canonical-frame-schema,state-type,state-schema,state-codec,state-codec-version,algorithm,algorithm-version,problem,rng-counter,determinism,execution-fingerprint,budget,provenance,pause-request,gate-generation,drain-report-version,drain-report-era,drain-run,drain-registered,drain-drained,drain-report,payload-content,payload-length",
        "excluded_fields=envelope-content-id:authority-subject-only,caller-expected-root:admission-only,authority-anchor:authority-only,allocation-limit:budget-only,cancellation-schedule:execution-only",
        "consumers=seal_encoded_payload,inspect,inspect_expected,inspect_authorized,SolverStateV2",
        "mutations=canonical-frame-schema:crates/fs-exec/src/solver.rs#v2_canonical_frame_and_identity_eras_fail_closed_before_payload,state-type:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,state-schema:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,state-codec:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,state-codec-version:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,algorithm:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,algorithm-version:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,problem:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,rng-counter:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,determinism:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,execution-fingerprint:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,budget:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,provenance:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,pause-request:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,gate-generation:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,drain-report-version:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,drain-report-era:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,drain-run:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,drain-registered:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,drain-drained:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,drain-report:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,payload-content:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently,payload-length:crates/fs-exec/src/solver.rs#v2_each_resume_source_field_moves_identity_independently",
        "nonsemantic_mutations=envelope-content-id:crates/fs-exec/src/solver.rs#v2_authority_subject_binds_content_and_resume_axes,caller-expected-root:crates/fs-exec/src/solver.rs#v2_authority_subject_binds_content_and_resume_axes,authority-anchor:crates/fs-exec/src/solver.rs#v2_authority_metadata_does_not_move_subject_identity,allocation-limit:crates/fs-exec/src/solver.rs#v2_nonsemantic_limits_do_not_move_roots,cancellation-schedule:crates/fs-exec/src/solver.rs#v2_nonsemantic_limits_do_not_move_roots",
        "field_guard=classify_snapshot_resume_fields",
        "transport_guard=inspect",
        "version_guard=crates/fs-exec/src/solver.rs#v2_refuses_corruption_downgrade_and_hostile_lengths_before_decode",
        "coupling_surface=fs-exec:solver-resume",
    ];

    /// Owner-local declaration for the composite policy-authority subject.
    pub const SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_SCHEMA_DECLARATION: &[&str] = &[
        "frankensim-identity-schema-v1",
        "id=fs-exec:solver-snapshot-authority-subject",
        "version_const=SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_VERSION_V2",
        "version=2",
        "domain=org.frankensim.fs-exec.solver-snapshot-authority-subject.v2",
        "domain_const=SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_DOMAIN_V2",
        "encoder=authority_subject_receipt",
        "encoder_helpers=none",
        "schema_constants=SNAPSHOT_AUTHORITY_SUBJECT_IDENTITY_VERSION_V2",
        "schema_functions=authority_subject_receipt,crates/fs-blake3/src/identity.rs#CanonicalEncoder::new,crates/fs-blake3/src/identity.rs#CanonicalEncoder::bytes,crates/fs-blake3/src/identity.rs#CanonicalEncoder::child,crates/fs-blake3/src/identity.rs#CanonicalEncoder::finish",
        "schema_dependencies=fs-blake3:canonical-identity-frame,fs-exec:solver-resume",
        "digest=fs-blake3",
        "encoding=typed-binary",
        "sources=SnapshotAuthoritySubjectComponentsV2",
        "source_fields=SnapshotAuthoritySubjectComponentsV2.content_id:semantic,SnapshotAuthoritySubjectComponentsV2.resume_id:semantic",
        "source_bindings=SnapshotAuthoritySubjectComponentsV2.content_id>envelope-content,SnapshotAuthoritySubjectComponentsV2.resume_id>resume",
        "external_semantic_fields=canonical-frame-schema",
        "semantic_fields=canonical-frame-schema,envelope-content,resume",
        "excluded_fields=authority-anchor:authority-only,verifier-id:authority-only,key-policy-id:authority-only,admission-state:authority-only,allocation-limit:budget-only,cancellation-schedule:execution-only",
        "consumers=seal_encoded_payload,inspect,inspect_authorized,SolverStateV2",
        "mutations=canonical-frame-schema:crates/fs-exec/src/solver.rs#v2_canonical_frame_and_identity_eras_fail_closed_before_payload,envelope-content:crates/fs-exec/src/solver.rs#v2_authority_subject_binds_content_and_resume_axes,resume:crates/fs-exec/src/solver.rs#v2_authority_subject_binds_content_and_resume_axes",
        "nonsemantic_mutations=authority-anchor:crates/fs-exec/src/solver.rs#v2_authority_metadata_does_not_move_subject_identity,verifier-id:crates/fs-exec/src/solver.rs#v2_authority_metadata_does_not_move_subject_identity,key-policy-id:crates/fs-exec/src/solver.rs#v2_authority_metadata_does_not_move_subject_identity,admission-state:crates/fs-exec/src/solver.rs#v2_authority_metadata_does_not_move_subject_identity,allocation-limit:crates/fs-exec/src/solver.rs#v2_nonsemantic_limits_do_not_move_roots,cancellation-schedule:crates/fs-exec/src/solver.rs#v2_nonsemantic_limits_do_not_move_roots",
        "field_guard=classify_snapshot_authority_subject_fields",
        "transport_guard=inspect_authorized",
        "version_guard=crates/fs-exec/src/solver.rs#v2_refuses_corruption_downgrade_and_hostile_lengths_before_decode",
        "coupling_surface=fs-exec:solver-snapshot-authority-subject",
    ];

    #[allow(dead_code)]
    struct SnapshotAuthoritySubjectComponentsV2 {
        content_id: SnapshotContentIdV2,
        resume_id: SnapshotResumeIdV2,
    }

    #[allow(dead_code)]
    fn classify_snapshot_authority_subject_fields(source: &SnapshotAuthoritySubjectComponentsV2) {
        let SnapshotAuthoritySubjectComponentsV2 {
            content_id: _,
            resume_id: _,
        } = source;
    }

    #[allow(dead_code)]
    fn classify_snapshot_resume_fields(source: &SnapshotResumeIdentityComponentsV2) {
        let SnapshotResumeIdentityComponentsV2 {
            state_type: _,
            state_schema: _,
            state_codec: _,
            state_codec_version: _,
            algorithm: _,
            algorithm_version: _,
            problem: _,
            rng_counter: _,
            determinism: _,
            execution_fingerprint: _,
            budget: _,
            provenance: _,
            pause_request: _,
            gate_generation: _,
            drain_report_version: _,
            drain_report_era: _,
            drain_run: _,
            drain_registered: _,
            drain_drained: _,
            drain_report: _,
            payload_content: _,
            payload_len: _,
        } = source;
    }

    /// Explicit resource and cancellation envelope for v2 production,
    /// inspection, and bounded state decoding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SnapshotLimitsV2 {
        max_payload_bytes: u64,
        hash_poll_bytes: u32,
        identity: CanonicalLimits,
        max_collection_items: u64,
        max_decoded_allocation_bytes: u64,
        codec_poll_bytes: u32,
    }

    impl SnapshotLimitsV2 {
        /// Construct explicit limits. Payload/hash/identity poll values must be
        /// positive and the codec poll interval must be at least eight bytes.
        /// Zero collection-item or decoded-allocation caps are valid deny-all
        /// policies for codecs that allocate no owned collections.
        #[must_use]
        pub const fn new(
            max_payload_bytes: u64,
            hash_poll_bytes: u32,
            identity: CanonicalLimits,
            max_collection_items: u64,
            max_decoded_allocation_bytes: u64,
            codec_poll_bytes: u32,
        ) -> Self {
            Self {
                max_payload_bytes,
                hash_poll_bytes,
                identity,
                max_collection_items,
                max_decoded_allocation_bytes,
                codec_poll_bytes,
            }
        }

        /// Maximum solver payload bytes.
        #[must_use]
        pub const fn max_payload_bytes(self) -> u64 {
            self.max_payload_bytes
        }

        /// Maximum exact bytes between hashing cancellation polls.
        #[must_use]
        pub const fn hash_poll_bytes(self) -> u32 {
            self.hash_poll_bytes
        }

        /// Canonical resume-identity limits.
        #[must_use]
        pub const fn identity(self) -> CanonicalLimits {
            self.identity
        }

        /// Maximum collection cardinality any v2 codec operation may admit.
        #[must_use]
        pub const fn max_collection_items(self) -> u64 {
            self.max_collection_items
        }

        /// Maximum logical bytes a v2 decoder may reserve for owned outputs.
        #[must_use]
        pub const fn max_decoded_allocation_bytes(self) -> u64 {
            self.max_decoded_allocation_bytes
        }

        /// Maximum payload-codec bytes between cancellation polls. Values
        /// below eight are invalid because eight bytes is the largest atomic
        /// primitive handled by the codec.
        #[must_use]
        pub const fn codec_poll_bytes(self) -> u32 {
            self.codec_poll_bytes
        }
    }

    /// Fallible, capped payload producer used by [`SolverStateV2`].
    ///
    /// Unlike the legacy [`codec::Enc`], every growth is preflighted against
    /// the payload cap and uses fallible reservation before bytes are appended.
    pub struct SnapshotEncoderV2<'a> {
        bytes: Vec<u8>,
        limits: SnapshotLimitsV2,
        bytes_since_poll: u64,
        poisoned: bool,
        cancellation: &'a mut dyn CancellationProbe,
    }

    impl fmt::Debug for SnapshotEncoderV2<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SnapshotEncoderV2")
                .field("encoded_bytes", &self.bytes.len())
                .field("limits", &self.limits)
                .finish_non_exhaustive()
        }
    }

    impl<'a> SnapshotEncoderV2<'a> {
        pub(super) fn new(
            limits: SnapshotLimitsV2,
            cancellation: &'a mut dyn CancellationProbe,
        ) -> Result<Self, SnapshotV2Error> {
            validate_limits(limits)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload encoding",
                    at: 0,
                });
            }
            Ok(Self {
                bytes: Vec::new(),
                limits,
                bytes_since_poll: 0,
                poisoned: false,
                cancellation,
            })
        }

        fn ensure_healthy(&self) -> Result<(), SnapshotV2Error> {
            if self.poisoned {
                return Err(SnapshotV2Error::CodecPoisoned {
                    phase: "payload encoding",
                    at: u64::try_from(self.bytes.len())
                        .map_err(|_| SnapshotV2Error::LengthOverflow)?,
                });
            }
            Ok(())
        }

        fn finish_operation<T>(
            &mut self,
            result: Result<T, SnapshotV2Error>,
        ) -> Result<T, SnapshotV2Error> {
            if result.is_err() {
                self.poisoned = true;
            }
            result
        }

        fn reserve_additional(&mut self, additional: usize) -> Result<(), SnapshotV2Error> {
            let current =
                u64::try_from(self.bytes.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            let additional_u64 =
                u64::try_from(additional).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            let requested = current
                .checked_add(additional_u64)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            if requested > self.limits.max_payload_bytes {
                return Err(SnapshotV2Error::PayloadLimitExceeded {
                    declared: requested,
                    limit: self.limits.max_payload_bytes,
                });
            }
            if self.cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload encoding allocation",
                    at: current,
                });
            }
            self.bytes.try_reserve_exact(additional).map_err(|_| {
                SnapshotV2Error::AllocationFailed {
                    phase: "payload encoding",
                    requested,
                }
            })?;
            if self.cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload encoding allocation",
                    at: current,
                });
            }
            Ok(())
        }

        fn poll_before_progress(&mut self, additional: u64) -> Result<(), SnapshotV2Error> {
            let projected = self
                .bytes_since_poll
                .checked_add(additional)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            if projected > u64::from(self.limits.codec_poll_bytes) {
                let at =
                    u64::try_from(self.bytes.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
                if self.cancellation.is_cancelled() {
                    return Err(SnapshotV2Error::Cancelled {
                        phase: "payload encoding",
                        at,
                    });
                }
                self.bytes_since_poll = 0;
            }
            Ok(())
        }

        fn append_reserved(&mut self, bytes: &[u8]) -> Result<(), SnapshotV2Error> {
            let additional =
                u64::try_from(bytes.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            self.poll_before_progress(additional)?;
            self.bytes.extend_from_slice(bytes);
            self.bytes_since_poll = self
                .bytes_since_poll
                .checked_add(additional)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            Ok(())
        }

        fn append(&mut self, bytes: &[u8]) -> Result<(), SnapshotV2Error> {
            self.reserve_additional(bytes.len())?;
            self.append_reserved(bytes)
        }

        /// Append one little-endian `u32`.
        pub fn put_u32(&mut self, value: u32) -> Result<(), SnapshotV2Error> {
            self.ensure_healthy()?;
            let result = self.append(&value.to_le_bytes());
            self.finish_operation(result)
        }

        /// Append one little-endian `u64`.
        pub fn put_u64(&mut self, value: u64) -> Result<(), SnapshotV2Error> {
            self.ensure_healthy()?;
            let result = self.append(&value.to_le_bytes());
            self.finish_operation(result)
        }

        /// Append exact IEEE-754 bits for one `f64`.
        pub fn put_f64(&mut self, value: f64) -> Result<(), SnapshotV2Error> {
            self.put_u64(value.to_bits())
        }

        /// Append one length-prefixed `f64` collection under the item cap.
        pub fn put_f64_slice(&mut self, values: &[f64]) -> Result<(), SnapshotV2Error> {
            self.ensure_healthy()?;
            let result = (|| {
                let count =
                    u64::try_from(values.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
                if count > self.limits.max_collection_items {
                    return Err(SnapshotV2Error::CodecResourceLimitExceeded {
                        resource: "collection items",
                        requested: count,
                        limit: self.limits.max_collection_items,
                        at: u64::try_from(self.bytes.len())
                            .map_err(|_| SnapshotV2Error::LengthOverflow)?,
                    });
                }
                let body_bytes = count
                    .checked_mul(8)
                    .ok_or(SnapshotV2Error::LengthOverflow)?;
                let additional = body_bytes
                    .checked_add(8)
                    .ok_or(SnapshotV2Error::LengthOverflow)?;
                self.reserve_additional(
                    usize::try_from(additional).map_err(|_| SnapshotV2Error::LengthOverflow)?,
                )?;
                self.append_reserved(&count.to_le_bytes())?;
                for &value in values {
                    self.append_reserved(&value.to_bits().to_le_bytes())?;
                }
                Ok(())
            })();
            self.finish_operation(result)
        }

        pub(super) fn finish(mut self) -> Result<Vec<u8>, SnapshotV2Error> {
            self.ensure_healthy()?;
            let at =
                u64::try_from(self.bytes.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if self.cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload encoding",
                    at,
                });
            }
            Ok(core::mem::take(&mut self.bytes))
        }
    }

    /// Capped, cancellation-aware state decoder. Collection allocations are
    /// admitted only after exact remaining wire extent and logical allocation
    /// charges have both been checked.
    pub struct SnapshotDecoderV2<'payload, 'probe> {
        decoder: codec::Dec<'payload>,
        limits: SnapshotLimitsV2,
        charged_allocation_bytes: u64,
        bytes_since_poll: u64,
        poisoned: bool,
        cancellation: &'probe mut dyn CancellationProbe,
    }

    impl fmt::Debug for SnapshotDecoderV2<'_, '_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SnapshotDecoderV2")
                .field("position", &self.decoder.position())
                .field("remaining", &self.decoder.remaining())
                .field("charged_allocation_bytes", &self.charged_allocation_bytes)
                .field("limits", &self.limits)
                .finish_non_exhaustive()
        }
    }

    impl<'payload, 'probe> SnapshotDecoderV2<'payload, 'probe> {
        fn new(
            payload: &'payload [u8],
            limits: SnapshotLimitsV2,
            cancellation: &'probe mut dyn CancellationProbe,
        ) -> Result<Self, SnapshotV2Error> {
            validate_limits(limits)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload decoding",
                    at: 0,
                });
            }
            Ok(Self {
                decoder: codec::Dec::new(payload),
                limits,
                charged_allocation_bytes: 0,
                bytes_since_poll: 0,
                poisoned: false,
                cancellation,
            })
        }

        fn ensure_healthy(&self) -> Result<(), SnapshotV2Error> {
            if self.poisoned {
                return Err(SnapshotV2Error::CodecPoisoned {
                    phase: "payload decoding",
                    at: u64::try_from(self.decoder.position())
                        .map_err(|_| SnapshotV2Error::LengthOverflow)?,
                });
            }
            Ok(())
        }

        fn finish_operation<T>(
            &mut self,
            result: Result<T, SnapshotV2Error>,
        ) -> Result<T, SnapshotV2Error> {
            if result.is_err() {
                self.poisoned = true;
            }
            result
        }

        fn poll_before_progress(&mut self, bytes: u64) -> Result<(), SnapshotV2Error> {
            let projected = self
                .bytes_since_poll
                .checked_add(bytes)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            if projected > u64::from(self.limits.codec_poll_bytes) {
                let at = u64::try_from(self.decoder.position())
                    .map_err(|_| SnapshotV2Error::LengthOverflow)?;
                if self.cancellation.is_cancelled() {
                    return Err(SnapshotV2Error::Cancelled {
                        phase: "payload decoding",
                        at,
                    });
                }
                self.bytes_since_poll = 0;
            }
            Ok(())
        }

        fn record_progress(&mut self, bytes: u64) -> Result<(), SnapshotV2Error> {
            self.bytes_since_poll = self
                .bytes_since_poll
                .checked_add(bytes)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            Ok(())
        }

        fn get_u32_inner(&mut self) -> Result<u32, SnapshotV2Error> {
            self.poll_before_progress(4)?;
            let value = self.decoder.get_u32().map_err(SnapshotV2Error::Payload)?;
            self.record_progress(4)?;
            Ok(value)
        }

        fn get_u64_inner(&mut self) -> Result<u64, SnapshotV2Error> {
            self.poll_before_progress(8)?;
            let value = self.decoder.get_u64().map_err(SnapshotV2Error::Payload)?;
            self.record_progress(8)?;
            Ok(value)
        }

        /// Decode one little-endian `u32`.
        pub fn get_u32(&mut self) -> Result<u32, SnapshotV2Error> {
            self.ensure_healthy()?;
            let result = self.get_u32_inner();
            self.finish_operation(result)
        }

        /// Decode one little-endian `u64`.
        pub fn get_u64(&mut self) -> Result<u64, SnapshotV2Error> {
            self.ensure_healthy()?;
            let result = self.get_u64_inner();
            self.finish_operation(result)
        }

        /// Decode one exact IEEE-754 `f64` bit pattern.
        pub fn get_f64(&mut self) -> Result<f64, SnapshotV2Error> {
            Ok(f64::from_bits(self.get_u64()?))
        }

        /// Decode one length-prefixed `f64` collection with pre-allocation
        /// extent, item, conversion, and allocation checks.
        pub fn get_f64_vec(&mut self) -> Result<Vec<f64>, SnapshotV2Error> {
            self.ensure_healthy()?;
            let result = self.get_f64_vec_inner();
            self.finish_operation(result)
        }

        fn get_f64_vec_inner(&mut self) -> Result<Vec<f64>, SnapshotV2Error> {
            let count = self.get_u64_inner()?;
            let at = u64::try_from(self.decoder.position())
                .map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if count > self.limits.max_collection_items {
                return Err(SnapshotV2Error::CodecResourceLimitExceeded {
                    resource: "collection items",
                    requested: count,
                    limit: self.limits.max_collection_items,
                    at,
                });
            }
            let wire_bytes = count
                .checked_mul(8)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            let wire_bytes_usize = usize::try_from(wire_bytes).map_err(|_| {
                SnapshotV2Error::CodecResourceLimitExceeded {
                    resource: "platform wire extent",
                    requested: wire_bytes,
                    limit: u64::try_from(usize::MAX).unwrap_or(u64::MAX),
                    at,
                }
            })?;
            if self.decoder.remaining() < wire_bytes_usize {
                return Err(SnapshotV2Error::Payload(codec::CodecError {
                    at: self.decoder.position(),
                    what: "f64 slice body",
                    needed: wire_bytes_usize,
                    remaining: self.decoder.remaining(),
                }));
            }
            let charged = self
                .charged_allocation_bytes
                .checked_add(wire_bytes)
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            if charged > self.limits.max_decoded_allocation_bytes {
                return Err(SnapshotV2Error::CodecResourceLimitExceeded {
                    resource: "decoded allocation bytes",
                    requested: charged,
                    limit: self.limits.max_decoded_allocation_bytes,
                    at,
                });
            }
            let count_usize = usize::try_from(count).map_err(|_| {
                SnapshotV2Error::CodecResourceLimitExceeded {
                    resource: "platform collection items",
                    requested: count,
                    limit: u64::try_from(usize::MAX).unwrap_or(u64::MAX),
                    at,
                }
            })?;
            let mut values = Vec::new();
            if self.cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload decoding allocation",
                    at,
                });
            }
            values.try_reserve_exact(count_usize).map_err(|_| {
                SnapshotV2Error::AllocationFailed {
                    phase: "payload decoding",
                    requested: wire_bytes,
                }
            })?;
            if self.cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload decoding allocation",
                    at,
                });
            }
            self.charged_allocation_bytes = charged;
            for _ in 0..count_usize {
                values.push(f64::from_bits(self.get_u64_inner()?));
            }
            Ok(values)
        }

        fn finish(&mut self) -> Result<(), SnapshotV2Error> {
            self.ensure_healthy()?;
            if !self.decoder.is_empty() {
                return Err(SnapshotV2Error::Payload(codec::CodecError {
                    at: self.decoder.position(),
                    what: "end of snapshot v2 payload",
                    needed: 0,
                    remaining: self.decoder.remaining(),
                }));
            }
            let at = u64::try_from(self.decoder.position())
                .map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if self.cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "payload decoding",
                    at,
                });
            }
            Ok(())
        }
    }

    /// Why a v2 envelope was refused before state publication.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SnapshotV2Error {
        /// Not a solver snapshot envelope.
        BadMagic,
        /// Fixed header or declared payload is truncated.
        Truncated {
            /// Minimum bytes required for the next admission stage.
            needed: usize,
            /// Exact bytes supplied by the caller.
            have: usize,
        },
        /// Only envelope v2 is accepted on this path.
        UnknownEnvelopeVersion {
            /// Exact unsupported version tag.
            found: u32,
        },
        /// The envelope names a canonical identity-frame era this reader does
        /// not implement.
        UnsupportedCanonicalFrameVersion {
            /// Era declared by the envelope.
            declared: u32,
            /// Era implemented by this reader.
            current: u32,
        },
        /// The resume semantic schema was encoded under another exact schema
        /// descriptor era and requires an explicit migrator.
        UnsupportedResumeSchemaEra {
            /// Exact schema descriptor identity retained in the envelope.
            declared: [u8; 32],
            /// Exact descriptor identity implemented by this reader.
            current: [u8; 32],
        },
        /// The composite policy-authority schema was encoded under another
        /// exact descriptor era and requires an explicit migrator.
        UnsupportedAuthoritySchemaEra {
            /// Exact schema descriptor identity retained in the envelope.
            declared: [u8; 32],
            /// Exact descriptor identity implemented by this reader.
            current: [u8; 32],
        },
        /// The drain-report domain/version/wire grammar differs from the
        /// reader's exact supported era.
        UnsupportedDrainReportEra {
            /// Era retained by the envelope.
            declared: SnapshotDrainReportEraIdV2,
            /// Era implemented by this reader.
            current: SnapshotDrainReportEraIdV2,
        },
        /// Header length is not the sole canonical v2 value.
        InvalidHeaderLength {
            /// Exact noncanonical header length.
            declared: u32,
        },
        /// An explicit limit is internally invalid.
        InvalidLimits(&'static str),
        /// Declared payload exceeds the caller's pre-decode cap.
        PayloadLimitExceeded {
            /// Exact payload bytes declared by the envelope or producer.
            declared: u64,
            /// Caller-supplied maximum payload bytes.
            limit: u64,
        },
        /// Checked wire/host length arithmetic overflowed.
        LengthOverflow,
        /// Declared and present payload extents differ exactly.
        LengthMismatch {
            /// Exact payload bytes declared in the header.
            declared: u64,
            /// Exact payload bytes physically present.
            actual: u64,
        },
        /// Unknown determinism mode tag.
        InvalidDeterminismTag {
            /// Exact unknown tag.
            found: u8,
        },
        /// Unknown drain/finalization tag.
        InvalidLifecycleTag {
            /// Exact unknown tag.
            found: u8,
        },
        /// A parsed pause boundary cannot have been produced by the executor
        /// drain tracker because its worker accounting is impossible.
        InvalidDrainBoundary {
            /// Workers ever registered for the old run.
            registered: u64,
            /// Worker guards released before finalization.
            drained: u64,
        },
        /// Header drain fields do not reproduce the retained executor report.
        DrainReportMismatch {
            /// Report root retained in the header.
            declared: [u8; 32],
            /// Root recomputed from the exact domain/version/run/count fields.
            computed: [u8; 32],
        },
        /// Reserved bytes must remain zero until a versioned migration.
        NonzeroReservedHeader,
        /// Payload bytes disagree with the header's BLAKE3 content root.
        PayloadContentMismatch {
            /// Payload identity retained in the header.
            declared: SnapshotPayloadContentIdV2,
            /// Payload identity recomputed from exact retained bytes.
            computed: SnapshotPayloadContentIdV2,
        },
        /// Canonical resume semantics disagree with the retained root.
        ResumeIdentityMismatch {
            /// Resume identity retained in the header.
            declared: SnapshotResumeIdV2,
            /// Resume identity independently reconstructed from header fields.
            computed: SnapshotResumeIdV2,
        },
        /// Caller-held exact whole-envelope root did not match.
        ExpectedContentMismatch {
            /// Caller-held exact root.
            expected: SnapshotContentIdV2,
            /// Root recomputed from the candidate envelope.
            computed: SnapshotContentIdV2,
        },
        /// Caller-held semantic resume root did not match.
        ExpectedResumeMismatch {
            /// Caller-held semantic root.
            expected: SnapshotResumeIdV2,
            /// Root reconstructed from the candidate semantics.
            computed: SnapshotResumeIdV2,
        },
        /// Candidate semantics do not match the caller-held expected context.
        ExpectedContextMismatch {
            /// First field in canonical resume order that differs.
            field: SnapshotContextFieldV2,
        },
        /// An internal admission path failed to attach the caller-supplied
        /// expected-context token required by bounded decode.
        MissingExpectedContext,
        /// Presented/admitted policy authority is for another exact envelope
        /// plus resume-identity subject.
        AuthoritySubjectMismatch,
        /// State type `S` does not own all retained type/schema/codec identities.
        WrongStateSchema,
        /// Payload codec requested resources outside its explicit envelope.
        CodecResourceLimitExceeded {
            /// Stable resource category.
            resource: &'static str,
            /// Exact logical amount requested.
            requested: u64,
            /// Exact caller-supplied cap.
            limit: u64,
            /// Payload byte cursor at refusal.
            at: u64,
        },
        /// A codec operation already refused; swallowing that error cannot
        /// resume the partially mutated encoder or decoder transaction.
        CodecPoisoned {
            /// Stable codec phase.
            phase: &'static str,
            /// Exact payload cursor when poison was observed again.
            at: u64,
        },
        /// Cancellation stopped a phase of production, inspection, identity
        /// construction, or state decoding.
        Cancelled {
            /// Stable operation phase.
            phase: &'static str,
            /// Phase-relative byte cursor or progress marker at observation.
            /// Its exact interpretation is named by `phase`; it is not a
            /// global count of bytes absorbed.
            at: u64,
        },
        /// Canonical semantic identity construction refused.
        Canonical(CanonicalError),
        /// Fallible output reservation refused.
        AllocationFailed {
            /// Stable allocation phase.
            phase: &'static str,
            /// Exact logical bytes requested by that phase.
            requested: u64,
        },
        /// The exact admitted payload failed its state codec.
        Payload(codec::CodecError),
    }

    #[allow(clippy::too_many_lines)]
    impl fmt::Display for SnapshotV2Error {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::BadMagic => formatter.write_str("not a solver snapshot (bad magic)"),
                Self::Truncated { needed, have } => {
                    write!(
                        formatter,
                        "snapshot truncated: needs {needed} bytes, {have} present"
                    )
                }
                Self::UnknownEnvelopeVersion { found } => write!(
                    formatter,
                    "snapshot envelope version {found} is not v{ENVELOPE_VERSION_V2}"
                ),
                Self::UnsupportedCanonicalFrameVersion { declared, current } => write!(
                    formatter,
                    "snapshot canonical identity frame v{declared} is unsupported by current frame v{current}; use an explicit identity-era migrator"
                ),
                Self::UnsupportedResumeSchemaEra { .. } => formatter.write_str(
                    "snapshot resume identity uses another canonical schema-descriptor era; use an explicit identity-era migrator",
                ),
                Self::UnsupportedAuthoritySchemaEra { .. } => formatter.write_str(
                    "snapshot authority subject uses another canonical schema-descriptor era; use an explicit identity-era migrator",
                ),
                Self::UnsupportedDrainReportEra { .. } => formatter.write_str(
                    "snapshot drain report uses another domain/version/wire era; use an explicit drain-report migrator",
                ),
                Self::InvalidHeaderLength { declared } => write!(
                    formatter,
                    "snapshot v2 header length {declared} is not canonical {HEADER_LEN_V2}"
                ),
                Self::InvalidLimits(reason) => {
                    write!(formatter, "invalid snapshot limits: {reason}")
                }
                Self::PayloadLimitExceeded { declared, limit } => write!(
                    formatter,
                    "snapshot payload declares {declared} bytes, above the {limit}-byte cap"
                ),
                Self::LengthOverflow => {
                    formatter.write_str("snapshot length arithmetic overflowed")
                }
                Self::LengthMismatch { declared, actual } => write!(
                    formatter,
                    "snapshot payload length mismatch: header declares {declared}, {actual} bytes present"
                ),
                Self::InvalidDeterminismTag { found } => {
                    write!(formatter, "unknown snapshot determinism tag {found}")
                }
                Self::InvalidLifecycleTag { found } => {
                    write!(formatter, "unknown snapshot lifecycle tag {found}")
                }
                Self::InvalidDrainBoundary {
                    registered,
                    drained,
                } => write!(
                    formatter,
                    "snapshot pause boundary is impossible: {registered} workers registered but {drained} drained"
                ),
                Self::DrainReportMismatch { .. } => formatter.write_str(
                    "snapshot pause boundary does not reproduce its executor drain-report identity",
                ),
                Self::NonzeroReservedHeader => {
                    formatter.write_str("snapshot v2 reserved header bytes are nonzero")
                }
                Self::PayloadContentMismatch { .. } => formatter.write_str(
                    "snapshot payload bytes do not match the retained BLAKE3 content identity",
                ),
                Self::ResumeIdentityMismatch { .. } => formatter.write_str(
                    "snapshot resume semantics do not match the retained typed identity",
                ),
                Self::ExpectedContentMismatch { .. } => {
                    formatter.write_str("snapshot exact bytes differ from the caller-held root")
                }
                Self::ExpectedResumeMismatch { .. } => formatter
                    .write_str("snapshot resume semantics differ from the caller-held root"),
                Self::ExpectedContextMismatch { field } => write!(
                    formatter,
                    "snapshot resume field {} differs from the caller-held expectation",
                    field.as_str()
                ),
                Self::MissingExpectedContext => formatter.write_str(
                    "snapshot admission did not retain its caller-supplied expected context",
                ),
                Self::AuthoritySubjectMismatch => formatter
                    .write_str("policy authority is bound to another exact snapshot subject"),
                Self::WrongStateSchema => formatter
                    .write_str("snapshot state type, schema, or codec does not belong to the requested Rust type"),
                Self::CodecResourceLimitExceeded {
                    resource,
                    requested,
                    limit,
                    at,
                } => write!(
                    formatter,
                    "snapshot codec requested {requested} {resource} at byte {at}, above the explicit limit {limit}"
                ),
                Self::CodecPoisoned { phase, at } => write!(
                    formatter,
                    "snapshot {phase} transaction was poisoned by an earlier refusal at byte {at}"
                ),
                Self::Cancelled { phase, at } => {
                    write!(
                        formatter,
                        "snapshot operation cancelled during {phase} at phase-relative byte {at}"
                    )
                }
                Self::Canonical(error) => write!(formatter, "snapshot identity refused: {error}"),
                Self::AllocationFailed { phase, requested } => write!(
                    formatter,
                    "snapshot allocation refused during {phase} for {requested} bytes"
                ),
                Self::Payload(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl core::error::Error for SnapshotV2Error {
        fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
            match self {
                Self::Canonical(error) => Some(error),
                Self::Payload(error) => Some(error),
                _ => None,
            }
        }
    }

    impl From<CanonicalError> for SnapshotV2Error {
        fn from(error: CanonicalError) -> Self {
            match error {
                CanonicalError::Cancelled { absorbed_bytes } => Self::Cancelled {
                    phase: "resume identity",
                    at: absorbed_bytes,
                },
                other => Self::Canonical(other),
            }
        }
    }

    /// Admission path attached by an inspection entry point. The names avoid
    /// turning caller expectations or policy admission into global trust.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum SnapshotAdmissionV2 {
        /// Internal roots are self-consistent, but no external expectation was
        /// supplied.
        UnanchoredConsistencyOnly,
        /// Exact caller-held roots and expected context all matched.
        MatchedCallerExpectation,
        /// An injected verifier/admitter accepted the exact composite subject;
        /// its semantics remain relative to that verifier and admission policy.
        PolicyRelativeAdmission,
    }

    /// Allocation-free compact rendering for identities whose enclosing field
    /// name already carries the nominal role.
    struct DebugDisplay<T>(T);

    impl<T: fmt::Display> fmt::Debug for DebugDisplay<T> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.0, formatter)
        }
    }

    /// Fixed-width lowercase hexadecimal rendering for raw 32-byte bindings.
    struct DebugHex32([u8; 32]);

    impl fmt::Debug for DebugHex32 {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            for byte in self.0 {
                write!(formatter, "{byte:02x}")?;
            }
            Ok(())
        }
    }

    /// Identity evidence discharged alongside owned canonical envelope bytes.
    ///
    /// This type deliberately has no whole-artifact clone or equality
    /// operation. Consuming a [`SealedSnapshotV2`] cannot silently return bare
    /// bytes while discarding the semantic and authority-subject receipts.
    #[must_use = "seal evidence must be retained, ledgered, or explicitly discharged"]
    pub struct SnapshotSealEvidenceV2 {
        content_id: SnapshotContentIdV2,
        resume: IdentityReceipt<SnapshotResumeIdV2>,
        authority_subject: IdentityReceipt<SnapshotAuthoritySubjectIdV2>,
        expected_context: ExpectedResumeContextV2,
    }

    impl fmt::Debug for SnapshotSealEvidenceV2 {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SnapshotSealEvidenceV2")
                .field("content_id", &DebugDisplay(self.content_id))
                .field("resume_id", &DebugDisplay(self.resume.id()))
                .field(
                    "authority_subject_id",
                    &DebugDisplay(self.authority_subject.id()),
                )
                .finish_non_exhaustive()
        }
    }

    impl SnapshotSealEvidenceV2 {
        /// Exact whole-envelope identity.
        #[must_use]
        pub const fn content_id(&self) -> SnapshotContentIdV2 {
            self.content_id
        }

        /// Typed semantic resume identity.
        #[must_use]
        pub const fn resume_id(&self) -> SnapshotResumeIdV2 {
            self.resume.id()
        }

        /// Complete unanchored semantic identity receipt.
        #[must_use]
        pub const fn resume_receipt(&self) -> IdentityReceipt<SnapshotResumeIdV2> {
            self.resume
        }

        /// Composite exact-byte plus resume-semantics subject.
        #[must_use]
        pub const fn authority_subject_id(&self) -> SnapshotAuthoritySubjectIdV2 {
            self.authority_subject.id()
        }

        /// Composite receipt suitable for a separate authority presentation.
        /// The receipt itself remains unanchored and proves no admission.
        #[must_use]
        pub const fn authority_subject_receipt(
            &self,
        ) -> IdentityReceipt<SnapshotAuthoritySubjectIdV2> {
            self.authority_subject
        }

        /// Caller-supplied expected context retained through sealing.
        #[must_use]
        pub const fn expected_context(&self) -> &ExpectedResumeContextV2 {
            &self.expected_context
        }

        /// Reconstruct the opaque in-process expectation from retained seal
        /// evidence without accepting caller-authored raw roots.
        #[must_use]
        pub fn expectation(&self) -> SnapshotExpectationV2 {
            SnapshotExpectationV2 {
                content_id: self.content_id,
                resume_id: self.resume.id(),
                expected_context: self.expected_context.clone(),
            }
        }
    }

    /// Opaque output of v2 sealing. Identity remains attached to exact bytes.
    pub struct SealedSnapshotV2 {
        bytes: Vec<u8>,
        content_id: SnapshotContentIdV2,
        resume: IdentityReceipt<SnapshotResumeIdV2>,
        authority_subject: IdentityReceipt<SnapshotAuthoritySubjectIdV2>,
        expected_context: ExpectedResumeContextV2,
    }

    impl fmt::Debug for SealedSnapshotV2 {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SealedSnapshotV2")
                .field("byte_len", &self.bytes.len())
                .field("content_id", &DebugDisplay(self.content_id))
                .field("resume_id", &DebugDisplay(self.resume.id()))
                .field(
                    "authority_subject_id",
                    &DebugDisplay(self.authority_subject.id()),
                )
                .finish_non_exhaustive()
        }
    }

    impl SealedSnapshotV2 {
        /// Exact canonical envelope bytes.
        #[must_use]
        pub fn bytes(&self) -> &[u8] {
            &self.bytes
        }

        /// Consume the attachment while returning exact canonical bytes
        /// together with their mandatory identity evidence. There is no silent
        /// bare-byte escape.
        #[must_use]
        pub fn into_parts(self) -> (Vec<u8>, SnapshotSealEvidenceV2) {
            (
                self.bytes,
                SnapshotSealEvidenceV2 {
                    content_id: self.content_id,
                    resume: self.resume,
                    authority_subject: self.authority_subject,
                    expected_context: self.expected_context,
                },
            )
        }

        /// Exact whole-envelope content identity.
        #[must_use]
        pub const fn content_id(&self) -> SnapshotContentIdV2 {
            self.content_id
        }

        /// Typed semantic resume identity.
        #[must_use]
        pub const fn resume_id(&self) -> SnapshotResumeIdV2 {
            self.resume.id()
        }

        /// Complete unanchored semantic identity receipt.
        #[must_use]
        pub const fn resume_receipt(&self) -> IdentityReceipt<SnapshotResumeIdV2> {
            self.resume
        }

        /// Composite receipt over exact complete bytes plus resume semantics.
        #[must_use]
        pub const fn authority_subject_receipt(
            &self,
        ) -> IdentityReceipt<SnapshotAuthoritySubjectIdV2> {
            self.authority_subject
        }

        /// Exact resume semantics bound into the envelope.
        #[must_use]
        pub const fn context(&self) -> &SnapshotResumeContextV2 {
            self.expected_context.context()
        }

        /// Caller-supplied expected context retained by typed sealing.
        #[must_use]
        pub const fn expected_context(&self) -> &ExpectedResumeContextV2 {
            &self.expected_context
        }

        /// Opaque exact-root plus expected-context token a caller may retain for
        /// in-process expected opening.
        #[must_use]
        pub fn expectation(&self) -> SnapshotExpectationV2 {
            SnapshotExpectationV2 {
                content_id: self.content_id,
                resume_id: self.resume.id(),
                expected_context: self.expected_context.clone(),
            }
        }
    }

    /// Caller-held exact roots required by the expected-root open path.
    #[derive(Debug, PartialEq, Eq)]
    pub struct SnapshotExpectationV2 {
        content_id: SnapshotContentIdV2,
        resume_id: SnapshotResumeIdV2,
        expected_context: ExpectedResumeContextV2,
    }

    impl SnapshotExpectationV2 {
        /// Expected exact whole-envelope content identity.
        #[must_use]
        pub const fn content_id(&self) -> SnapshotContentIdV2 {
            self.content_id
        }

        /// Expected semantic resume identity.
        #[must_use]
        pub const fn resume_id(&self) -> SnapshotResumeIdV2 {
            self.resume_id
        }

        /// Caller-retained expected resume context.
        #[must_use]
        pub const fn expected_context(&self) -> &ExpectedResumeContextV2 {
            &self.expected_context
        }
    }

    /// Fully checked v2 metadata and borrowed payload. Unanchored inspection
    /// never authorizes decoding through [`SolverStateV2`].
    pub struct SnapshotInspectionV2<'a> {
        payload: &'a [u8],
        payload_content: SnapshotPayloadContentIdV2,
        content_id: SnapshotContentIdV2,
        resume: IdentityReceipt<SnapshotResumeIdV2>,
        authority_subject: IdentityReceipt<SnapshotAuthoritySubjectIdV2>,
        context: SnapshotResumeContextV2,
        admission: SnapshotAdmissionV2,
        authority_evidence: Option<IdentityAuditRecord>,
        expected_context: Option<ExpectedResumeContextV2>,
    }

    impl fmt::Debug for SnapshotInspectionV2<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SnapshotInspectionV2")
                .field("payload_len", &self.payload.len())
                .field(
                    "payload_content_id",
                    &DebugHex32(*self.payload_content.as_bytes()),
                )
                .field("content_id", &DebugDisplay(self.content_id))
                .field("resume_id", &DebugDisplay(self.resume.id()))
                .field(
                    "authority_subject_id",
                    &DebugDisplay(self.authority_subject.id()),
                )
                .field("admission", &self.admission)
                .field(
                    "authority_evidence_present",
                    &self.authority_evidence.is_some(),
                )
                .finish_non_exhaustive()
        }
    }

    impl<'a> SnapshotInspectionV2<'a> {
        /// Exact borrowed payload bytes. Their admission state is separate.
        #[must_use]
        pub const fn payload(&self) -> &'a [u8] {
            self.payload
        }

        /// Exact payload-only identity.
        #[must_use]
        pub const fn payload_content_id(&self) -> SnapshotPayloadContentIdV2 {
            self.payload_content
        }

        /// Exact complete-envelope identity.
        #[must_use]
        pub const fn content_id(&self) -> SnapshotContentIdV2 {
            self.content_id
        }

        /// Typed semantic resume identity.
        #[must_use]
        pub const fn resume_id(&self) -> SnapshotResumeIdV2 {
            self.resume.id()
        }

        /// Complete recomputed, unanchored semantic identity receipt.
        #[must_use]
        pub const fn resume_receipt(&self) -> IdentityReceipt<SnapshotResumeIdV2> {
            self.resume
        }

        /// Composite exact-byte plus resume-semantics subject.
        #[must_use]
        pub const fn authority_subject_id(&self) -> SnapshotAuthoritySubjectIdV2 {
            self.authority_subject.id()
        }

        /// Composite recomputed receipt suitable for a separate authority
        /// presentation. This accessor does not change admission state.
        #[must_use]
        pub const fn authority_subject_receipt(
            &self,
        ) -> IdentityReceipt<SnapshotAuthoritySubjectIdV2> {
            self.authority_subject
        }

        /// Header-declared semantic context. This observation is not an
        /// caller-held expectation token.
        #[must_use]
        pub const fn context(&self) -> &SnapshotResumeContextV2 {
            &self.context
        }

        /// Admission path of this inspection.
        #[must_use]
        pub const fn admission(&self) -> SnapshotAdmissionV2 {
            self.admission
        }

        /// Retained verifier/anchor/key-policy audit evidence, present only for
        /// policy-relative authority admission.
        #[must_use]
        pub const fn authority_evidence(&self) -> Option<IdentityAuditRecord> {
            self.authority_evidence
        }
    }

    /// Identity and admission evidence discharged alongside an owned decoded
    /// state. It intentionally has no payload/state bytes and no whole-artifact
    /// clone/equality operation.
    #[must_use = "resume evidence must be retained, ledgered, or explicitly discharged"]
    pub struct SnapshotOpenEvidenceV2 {
        content_id: SnapshotContentIdV2,
        resume: IdentityReceipt<SnapshotResumeIdV2>,
        authority_subject: IdentityReceipt<SnapshotAuthoritySubjectIdV2>,
        expected_context: ExpectedResumeContextV2,
        admission: SnapshotAdmissionV2,
        authority_evidence: Option<IdentityAuditRecord>,
    }

    impl fmt::Debug for SnapshotOpenEvidenceV2 {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SnapshotOpenEvidenceV2")
                .field("content_id", &DebugDisplay(self.content_id))
                .field("resume_id", &DebugDisplay(self.resume.id()))
                .field(
                    "authority_subject_id",
                    &DebugDisplay(self.authority_subject.id()),
                )
                .field("admission", &self.admission)
                .field(
                    "authority_evidence_present",
                    &self.authority_evidence.is_some(),
                )
                .finish_non_exhaustive()
        }
    }

    impl SnapshotOpenEvidenceV2 {
        /// Exact whole-envelope identity.
        #[must_use]
        pub const fn content_id(&self) -> SnapshotContentIdV2 {
            self.content_id
        }

        /// Typed semantic resume identity.
        #[must_use]
        pub const fn resume_id(&self) -> SnapshotResumeIdV2 {
            self.resume.id()
        }

        /// Complete semantic identity receipt retained through decode.
        #[must_use]
        pub const fn resume_receipt(&self) -> IdentityReceipt<SnapshotResumeIdV2> {
            self.resume
        }

        /// Composite exact-byte plus resume-semantics subject.
        #[must_use]
        pub const fn authority_subject_id(&self) -> SnapshotAuthoritySubjectIdV2 {
            self.authority_subject.id()
        }

        /// Composite subject receipt retained through decode.
        #[must_use]
        pub const fn authority_subject_receipt(
            &self,
        ) -> IdentityReceipt<SnapshotAuthoritySubjectIdV2> {
            self.authority_subject
        }

        /// Mint an opaque expected-reopen token from retained admitted
        /// evidence. No caller-authored raw-root constructor is introduced.
        #[must_use]
        pub fn expectation(&self) -> SnapshotExpectationV2 {
            SnapshotExpectationV2 {
                content_id: self.content_id,
                resume_id: self.resume.id(),
                expected_context: self.expected_context.clone(),
            }
        }

        /// Caller-supplied expected context retained through decode.
        #[must_use]
        pub const fn expected_context(&self) -> &ExpectedResumeContextV2 {
            &self.expected_context
        }

        /// Which explicit admission path authorized decoding.
        #[must_use]
        pub const fn admission(&self) -> SnapshotAdmissionV2 {
            self.admission
        }

        /// Retained verifier/anchor/key-policy audit evidence, when present.
        #[must_use]
        pub const fn authority_evidence(&self) -> Option<IdentityAuditRecord> {
            self.authority_evidence
        }
    }

    /// Decoded state whose exact v2 identity and admission path remain attached.
    pub struct OpenedSnapshotV2<S> {
        state: S,
        content_id: SnapshotContentIdV2,
        resume: IdentityReceipt<SnapshotResumeIdV2>,
        authority_subject: IdentityReceipt<SnapshotAuthoritySubjectIdV2>,
        expected_context: ExpectedResumeContextV2,
        admission: SnapshotAdmissionV2,
        authority_evidence: Option<IdentityAuditRecord>,
    }

    impl<S> fmt::Debug for OpenedSnapshotV2<S> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            let state_type_id = self.expected_context.context().state_type();
            formatter
                .debug_struct("OpenedSnapshotV2")
                .field("state_type_id", &DebugHex32(*state_type_id.as_bytes()))
                .field("content_id", &DebugDisplay(self.content_id))
                .field("resume_id", &DebugDisplay(self.resume.id()))
                .field(
                    "authority_subject_id",
                    &DebugDisplay(self.authority_subject.id()),
                )
                .field("admission", &self.admission)
                .field(
                    "authority_evidence_present",
                    &self.authority_evidence.is_some(),
                )
                .finish_non_exhaustive()
        }
    }

    impl<S> OpenedSnapshotV2<S> {
        /// Borrow decoded state.
        #[must_use]
        pub const fn state(&self) -> &S {
            &self.state
        }

        /// Consume the attachment while returning the state together with its
        /// mandatory evidence object. There is no silent bare-state escape.
        #[must_use]
        pub fn into_parts(self) -> (S, SnapshotOpenEvidenceV2) {
            (
                self.state,
                SnapshotOpenEvidenceV2 {
                    content_id: self.content_id,
                    resume: self.resume,
                    authority_subject: self.authority_subject,
                    expected_context: self.expected_context,
                    admission: self.admission,
                    authority_evidence: self.authority_evidence,
                },
            )
        }

        /// Exact whole-envelope identity.
        #[must_use]
        pub const fn content_id(&self) -> SnapshotContentIdV2 {
            self.content_id
        }

        /// Typed semantic resume identity.
        #[must_use]
        pub const fn resume_id(&self) -> SnapshotResumeIdV2 {
            self.resume.id()
        }

        /// Complete semantic identity receipt attached to this state.
        #[must_use]
        pub const fn resume_receipt(&self) -> IdentityReceipt<SnapshotResumeIdV2> {
            self.resume
        }

        /// Composite exact-byte plus resume-semantics subject.
        #[must_use]
        pub const fn authority_subject_id(&self) -> SnapshotAuthoritySubjectIdV2 {
            self.authority_subject.id()
        }

        /// Composite exact-byte plus resume-semantics receipt attached to this
        /// state. The receipt preserves identity evidence, not authority.
        #[must_use]
        pub const fn authority_subject_receipt(
            &self,
        ) -> IdentityReceipt<SnapshotAuthoritySubjectIdV2> {
            self.authority_subject
        }

        /// Mint an opaque expected-reopen token while state and admitted
        /// evidence remain attached. No raw-root constructor is exposed.
        #[must_use]
        pub fn expectation(&self) -> SnapshotExpectationV2 {
            SnapshotExpectationV2 {
                content_id: self.content_id,
                resume_id: self.resume.id(),
                expected_context: self.expected_context.clone(),
            }
        }

        /// Exact semantic context.
        #[must_use]
        pub const fn context(&self) -> &SnapshotResumeContextV2 {
            self.expected_context.context()
        }

        /// Caller-supplied expectation retained through admission and bounded
        /// decode.
        #[must_use]
        pub const fn expected_context(&self) -> &ExpectedResumeContextV2 {
            &self.expected_context
        }

        /// Which explicit admission path authorized decoding.
        #[must_use]
        pub const fn admission(&self) -> SnapshotAdmissionV2 {
            self.admission
        }

        /// Retained verifier/anchor/key-policy audit evidence, if this state
        /// was admitted through the policy-relative authority path.
        #[must_use]
        pub const fn authority_evidence(&self) -> Option<IdentityAuditRecord> {
            self.authority_evidence
        }
    }

    fn validate_limits(limits: SnapshotLimitsV2) -> Result<(), SnapshotV2Error> {
        if limits.max_payload_bytes == 0 {
            return Err(SnapshotV2Error::InvalidLimits(
                "max_payload_bytes must be positive",
            ));
        }
        if limits.hash_poll_bytes == 0 {
            return Err(SnapshotV2Error::InvalidLimits(
                "hash_poll_bytes must be positive",
            ));
        }
        if limits.identity.cancellation_poll_bytes() == 0 {
            return Err(SnapshotV2Error::InvalidLimits(
                "identity cancellation_poll_bytes must be positive",
            ));
        }
        if limits.codec_poll_bytes < 8 {
            return Err(SnapshotV2Error::InvalidLimits(
                "codec_poll_bytes must be at least 8",
            ));
        }
        Ok(())
    }

    fn hash_content<C: CancellationProbe>(
        bytes: &[u8],
        poll_bytes: u32,
        phase: &'static str,
        cancellation: &mut C,
    ) -> Result<ContentId, SnapshotV2Error> {
        if cancellation.is_cancelled() {
            return Err(SnapshotV2Error::Cancelled { phase, at: 0 });
        }
        let chunk_len = usize::try_from(poll_bytes).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        let mut hasher = fs_blake3::Blake3::new();
        let mut absorbed = 0_u64;
        for chunk in bytes.chunks(chunk_len) {
            hasher.update(chunk);
            absorbed = absorbed
                .checked_add(
                    u64::try_from(chunk.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?,
                )
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase,
                    at: absorbed,
                });
            }
        }
        Ok(ContentId::parse_slice(hasher.finalize().as_bytes())
            .expect("a BLAKE3 root is exactly one ContentId"))
    }

    fn write_header_bytes<C: CancellationProbe>(
        header: &mut [u8],
        cursor: &mut usize,
        input: &[u8],
        poll_bytes: u32,
        cancellation: &mut C,
    ) -> Result<(), SnapshotV2Error> {
        let chunk_len = usize::try_from(poll_bytes).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        for chunk in input.chunks(chunk_len) {
            let at = u64::try_from(*cursor).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "envelope header encoding",
                    at,
                });
            }
            let end = (*cursor)
                .checked_add(chunk.len())
                .ok_or(SnapshotV2Error::LengthOverflow)?;
            let Some(destination) = header.get_mut(*cursor..end) else {
                return Err(SnapshotV2Error::LengthOverflow);
            };
            destination.copy_from_slice(chunk);
            *cursor = end;
            let at = u64::try_from(*cursor).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "envelope header encoding",
                    at,
                });
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn write_snapshot_header<C: CancellationProbe>(
        bytes: &mut [u8],
        context: &SnapshotResumeContextV2,
        payload_len: u64,
        payload_content: SnapshotPayloadContentIdV2,
        resume_id: SnapshotResumeIdV2,
        poll_bytes: u32,
        cancellation: &mut C,
    ) -> Result<(), SnapshotV2Error> {
        let Some(header) = bytes.get_mut(..HEADER_LEN_V2) else {
            return Err(SnapshotV2Error::LengthOverflow);
        };
        let mut cursor = 0_usize;
        let mut write =
            |input: &[u8]| write_header_bytes(header, &mut cursor, input, poll_bytes, cancellation);
        write(&MAGIC_V2)?;
        write(&ENVELOPE_VERSION_V2.to_le_bytes())?;
        write(
            &u32::try_from(HEADER_LEN_V2)
                .expect("fixed v2 header fits u32")
                .to_le_bytes(),
        )?;
        write(context.state_type.as_bytes())?;
        write(context.state_schema.as_bytes())?;
        write(context.state_codec.as_bytes())?;
        write(&context.state_codec_version.to_le_bytes())?;
        write(context.algorithm.as_bytes())?;
        write(&context.algorithm_version.to_le_bytes())?;
        write(context.problem.as_bytes())?;
        write(context.rng_counter.as_bytes())?;
        write(&[context.determinism.tag()])?;
        write(&[LIFECYCLE_PAUSED_AND_DRAINED])?;
        write(&[0; RESERVED_LEN])?;
        write(&CANONICAL_FRAME_VERSION.to_le_bytes())?;
        write(SchemaId::<SnapshotResumeIdentitySchemaV2>::for_schema().as_bytes())?;
        write(SchemaId::<SnapshotAuthoritySubjectSchemaV2>::for_schema().as_bytes())?;
        write(current_drain_report_era().as_bytes())?;
        write(context.execution_fingerprint.as_bytes())?;
        write(context.budget.as_bytes())?;
        write(context.provenance.as_bytes())?;
        write(context.pause_boundary.pause_request.as_bytes())?;
        write(&context.pause_boundary.gate_generation.to_le_bytes())?;
        write(&context.pause_boundary.run.to_le_bytes())?;
        write(&context.pause_boundary.registered_workers.to_le_bytes())?;
        write(&context.pause_boundary.drained_workers.to_le_bytes())?;
        write(&context.pause_boundary.drain_report)?;
        write(&payload_len.to_le_bytes())?;
        write(payload_content.as_bytes())?;
        write(resume_id.as_bytes())?;
        if cursor != HEADER_LEN_V2 {
            return Err(SnapshotV2Error::LengthOverflow);
        }
        Ok(())
    }

    fn prepend_header_space<C: CancellationProbe>(
        bytes: &mut Vec<u8>,
        poll_bytes: u32,
        cancellation: &mut C,
    ) -> Result<(), SnapshotV2Error> {
        let payload_len = bytes.len();
        let total_len = HEADER_LEN_V2
            .checked_add(payload_len)
            .ok_or(SnapshotV2Error::LengthOverflow)?;
        let requested_total =
            u64::try_from(total_len).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        let at = u64::try_from(payload_len).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        if cancellation.is_cancelled() {
            return Err(SnapshotV2Error::Cancelled {
                phase: "envelope reservation",
                at,
            });
        }
        bytes
            .try_reserve_exact(HEADER_LEN_V2)
            .map_err(|_| SnapshotV2Error::AllocationFailed {
                phase: "envelope sealing",
                requested: requested_total,
            })?;
        if cancellation.is_cancelled() {
            return Err(SnapshotV2Error::Cancelled {
                phase: "envelope reservation",
                at,
            });
        }
        bytes.resize(total_len, 0);
        let chunk_len = usize::try_from(poll_bytes).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        let mut end = payload_len;
        while end != 0 {
            let start = end.saturating_sub(chunk_len);
            let at =
                u64::try_from(payload_len - end).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "envelope payload shift",
                    at,
                });
            }
            bytes.copy_within(start..end, HEADER_LEN_V2 + start);
            end = start;
            let at =
                u64::try_from(payload_len - end).map_err(|_| SnapshotV2Error::LengthOverflow)?;
            if cancellation.is_cancelled() {
                return Err(SnapshotV2Error::Cancelled {
                    phase: "envelope payload shift",
                    at,
                });
            }
        }
        Ok(())
    }

    fn resume_receipt<C: CancellationProbe>(
        context: &SnapshotResumeContextV2,
        payload_content: SnapshotPayloadContentIdV2,
        payload_len: u64,
        limits: CanonicalLimits,
        cancellation: &mut C,
    ) -> Result<IdentityReceipt<SnapshotResumeIdV2>, SnapshotV2Error> {
        let source = resume_identity_components(
            context,
            payload_content,
            payload_len,
            DRAIN_FINALIZE_REPORT_IDENTITY_VERSION,
            current_drain_report_era(),
        );
        encode_resume_receipt(&source, limits, cancellation)
    }

    fn resume_identity_components(
        context: &SnapshotResumeContextV2,
        payload_content: SnapshotPayloadContentIdV2,
        payload_len: u64,
        drain_report_version: u32,
        drain_report_era: SnapshotDrainReportEraIdV2,
    ) -> SnapshotResumeIdentityComponentsV2 {
        SnapshotResumeIdentityComponentsV2 {
            state_type: context.state_type,
            state_schema: context.state_schema,
            state_codec: context.state_codec,
            state_codec_version: context.state_codec_version,
            algorithm: context.algorithm,
            algorithm_version: context.algorithm_version,
            problem: context.problem,
            rng_counter: context.rng_counter,
            determinism: context.determinism,
            execution_fingerprint: context.execution_fingerprint,
            budget: context.budget,
            provenance: context.provenance,
            pause_request: context.pause_boundary.pause_request,
            gate_generation: context.pause_boundary.gate_generation,
            drain_report_version,
            drain_report_era,
            drain_run: context.pause_boundary.run,
            drain_registered: context.pause_boundary.registered_workers,
            drain_drained: context.pause_boundary.drained_workers,
            drain_report: context.pause_boundary.drain_report,
            payload_content,
            payload_len,
        }
    }

    fn encode_resume_receipt<C: CancellationProbe>(
        source: &SnapshotResumeIdentityComponentsV2,
        limits: CanonicalLimits,
        cancellation: &mut C,
    ) -> Result<IdentityReceipt<SnapshotResumeIdV2>, SnapshotV2Error> {
        let mut build = || -> Result<IdentityReceipt<SnapshotResumeIdV2>, CanonicalError> {
            CanonicalEncoder::<SnapshotResumeIdV2, _>::new(limits, || cancellation.is_cancelled())?
                .bytes(Field::new(0, "state-type"), source.state_type.as_bytes())?
                .bytes(
                    Field::new(1, "state-schema"),
                    source.state_schema.as_bytes(),
                )?
                .bytes(Field::new(2, "state-codec"), source.state_codec.as_bytes())?
                .u64(
                    Field::new(3, "state-codec-version"),
                    u64::from(source.state_codec_version),
                )?
                .bytes(Field::new(4, "algorithm"), source.algorithm.as_bytes())?
                .u64(Field::new(5, "algorithm-version"), source.algorithm_version)?
                .bytes(Field::new(6, "problem"), source.problem.as_bytes())?
                .bytes(Field::new(7, "rng-counter"), source.rng_counter.as_bytes())?
                .variant(
                    Field::new(8, "determinism"),
                    u32::from(source.determinism.tag()),
                    &[],
                )?
                .bytes(
                    Field::new(9, "execution-fingerprint"),
                    source.execution_fingerprint.as_bytes(),
                )?
                .bytes(Field::new(10, "budget"), source.budget.as_bytes())?
                .bytes(Field::new(11, "provenance"), source.provenance.as_bytes())?
                .bytes(
                    Field::new(12, "pause-request"),
                    source.pause_request.as_bytes(),
                )?
                .u64(Field::new(13, "gate-generation"), source.gate_generation)?
                .u64(
                    Field::new(14, "drain-report-version"),
                    u64::from(source.drain_report_version),
                )?
                .bytes(
                    Field::new(15, "drain-report-era"),
                    source.drain_report_era.as_bytes(),
                )?
                .u64(Field::new(16, "drain-run"), source.drain_run)?
                .u64(Field::new(17, "drain-registered"), source.drain_registered)?
                .u64(Field::new(18, "drain-drained"), source.drain_drained)?
                .bytes(Field::new(19, "drain-report"), &source.drain_report)?
                .bytes(
                    Field::new(20, "payload-content"),
                    source.payload_content.as_bytes(),
                )?
                .u64(Field::new(21, "payload-length"), source.payload_len)?
                .finish()
        };
        build().map_err(|error| canonical_receipt_error(error, "resume identity"))
    }

    #[cfg(test)]
    #[derive(Debug, Clone, Copy)]
    pub(super) enum SnapshotResumeTestMutationV2 {
        StateType,
        StateSchema,
        StateCodec,
        StateCodecVersion,
        Algorithm,
        AlgorithmVersion,
        Problem,
        RngCounter,
        Determinism,
        ExecutionFingerprint,
        Budget,
        Provenance,
        PauseRequest,
        GateGeneration,
        DrainReportVersion,
        DrainReportEra,
        DrainRun,
        DrainRegistered,
        DrainDrained,
        DrainReport,
        PayloadContent,
        PayloadLength,
    }

    #[cfg(test)]
    pub(super) fn test_resume_receipt_with_mutation<C: CancellationProbe>(
        context: &SnapshotResumeContextV2,
        payload_content: SnapshotPayloadContentIdV2,
        payload_len: u64,
        mutation: SnapshotResumeTestMutationV2,
        limits: CanonicalLimits,
        cancellation: &mut C,
    ) -> Result<IdentityReceipt<SnapshotResumeIdV2>, SnapshotV2Error> {
        fn toggled(bytes: &[u8; 32]) -> [u8; 32] {
            let mut changed = *bytes;
            changed[0] ^= 1;
            changed
        }

        let mut source = resume_identity_components(
            context,
            payload_content,
            payload_len,
            DRAIN_FINALIZE_REPORT_IDENTITY_VERSION,
            current_drain_report_era(),
        );
        match mutation {
            SnapshotResumeTestMutationV2::StateType => {
                source.state_type =
                    SnapshotStateTypeIdV2::from_bytes(toggled(source.state_type.as_bytes()));
            }
            SnapshotResumeTestMutationV2::StateSchema => {
                source.state_schema =
                    SnapshotStateSchemaIdV2::from_bytes(toggled(source.state_schema.as_bytes()));
            }
            SnapshotResumeTestMutationV2::StateCodec => {
                source.state_codec =
                    SnapshotStateCodecIdV2::from_bytes(toggled(source.state_codec.as_bytes()));
            }
            SnapshotResumeTestMutationV2::StateCodecVersion => {
                source.state_codec_version = source.state_codec_version.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::Algorithm => {
                source.algorithm =
                    SnapshotAlgorithmIdV2::from_bytes(toggled(source.algorithm.as_bytes()));
            }
            SnapshotResumeTestMutationV2::AlgorithmVersion => {
                source.algorithm_version = source.algorithm_version.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::Problem => {
                source.problem =
                    SnapshotProblemIdV2::from_bytes(toggled(source.problem.as_bytes()));
            }
            SnapshotResumeTestMutationV2::RngCounter => {
                source.rng_counter =
                    SnapshotRngCounterIdV2::from_bytes(toggled(source.rng_counter.as_bytes()));
            }
            SnapshotResumeTestMutationV2::Determinism => {
                source.determinism = match source.determinism {
                    SnapshotDeterminismV2::Deterministic => SnapshotDeterminismV2::Fast,
                    SnapshotDeterminismV2::Fast => SnapshotDeterminismV2::Deterministic,
                };
            }
            SnapshotResumeTestMutationV2::ExecutionFingerprint => {
                source.execution_fingerprint = SnapshotExecutionFingerprintIdV2::from_bytes(
                    toggled(source.execution_fingerprint.as_bytes()),
                );
            }
            SnapshotResumeTestMutationV2::Budget => {
                source.budget =
                    SnapshotBudgetStateIdV2::from_bytes(toggled(source.budget.as_bytes()));
            }
            SnapshotResumeTestMutationV2::Provenance => {
                source.provenance =
                    SnapshotProvenanceIdV2::from_bytes(toggled(source.provenance.as_bytes()));
            }
            SnapshotResumeTestMutationV2::PauseRequest => {
                source.pause_request =
                    SnapshotPauseRequestIdV2::from_bytes(toggled(source.pause_request.as_bytes()));
            }
            SnapshotResumeTestMutationV2::GateGeneration => {
                source.gate_generation = source.gate_generation.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::DrainReportVersion => {
                source.drain_report_version = source.drain_report_version.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::DrainReportEra => {
                source.drain_report_era = SnapshotDrainReportEraIdV2::from_bytes(toggled(
                    source.drain_report_era.as_bytes(),
                ));
            }
            SnapshotResumeTestMutationV2::DrainRun => {
                source.drain_run = source.drain_run.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::DrainRegistered => {
                source.drain_registered = source.drain_registered.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::DrainDrained => {
                source.drain_drained = source.drain_drained.wrapping_add(1);
            }
            SnapshotResumeTestMutationV2::DrainReport => {
                source.drain_report = toggled(&source.drain_report);
            }
            SnapshotResumeTestMutationV2::PayloadContent => {
                source.payload_content = SnapshotPayloadContentIdV2::parse_slice(&toggled(
                    source.payload_content.as_bytes(),
                ))
                .expect("a toggled 32-byte payload digest parses");
            }
            SnapshotResumeTestMutationV2::PayloadLength => {
                source.payload_len = source.payload_len.wrapping_add(1);
            }
        }
        encode_resume_receipt(&source, limits, cancellation)
    }

    #[cfg(test)]
    pub(super) fn test_resume_receipt_with_drain_identity<C: CancellationProbe>(
        context: &SnapshotResumeContextV2,
        payload_content: SnapshotPayloadContentIdV2,
        payload_len: u64,
        drain_report_version: u32,
        drain_report_era: SnapshotDrainReportEraIdV2,
        limits: CanonicalLimits,
        cancellation: &mut C,
    ) -> Result<IdentityReceipt<SnapshotResumeIdV2>, SnapshotV2Error> {
        let source = resume_identity_components(
            context,
            payload_content,
            payload_len,
            drain_report_version,
            drain_report_era,
        );
        encode_resume_receipt(&source, limits, cancellation)
    }

    pub(super) fn authority_subject_receipt<C: CancellationProbe>(
        content_id: SnapshotContentIdV2,
        resume_id: SnapshotResumeIdV2,
        limits: CanonicalLimits,
        cancellation: &mut C,
    ) -> Result<IdentityReceipt<SnapshotAuthoritySubjectIdV2>, SnapshotV2Error> {
        let source = SnapshotAuthoritySubjectComponentsV2 {
            content_id,
            resume_id,
        };
        let mut build =
            || -> Result<IdentityReceipt<SnapshotAuthoritySubjectIdV2>, CanonicalError> {
                CanonicalEncoder::<SnapshotAuthoritySubjectIdV2, _>::new(limits, || {
                    cancellation.is_cancelled()
                })?
                .bytes(
                    Field::new(0, "envelope-content"),
                    source.content_id.as_bytes(),
                )?
                .child(Field::new(1, "resume"), source.resume_id)?
                .finish()
            };
        build().map_err(|error| canonical_receipt_error(error, "authority subject"))
    }

    fn canonical_receipt_error(error: CanonicalError, phase: &'static str) -> SnapshotV2Error {
        match error {
            CanonicalError::Cancelled { absorbed_bytes } => SnapshotV2Error::Cancelled {
                phase,
                at: absorbed_bytes,
            },
            other => SnapshotV2Error::Canonical(other),
        }
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("fixed v2 header was length-checked"),
        )
    }

    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("fixed v2 header was length-checked"),
        )
    }

    fn read_32(bytes: &[u8], offset: usize) -> [u8; 32] {
        bytes[offset..offset + 32]
            .try_into()
            .expect("fixed v2 header was length-checked")
    }

    /// Consume already encoded solver-state payload bytes and seal canonical v2
    /// without retaining a second full payload allocation.
    ///
    /// No value is published on resource, cancellation, or allocation refusal.
    /// The returned identities prove only exact content/semantic consistency.
    pub(super) fn seal_encoded_payload<C: CancellationProbe>(
        mut bytes: Vec<u8>,
        expected_context: &ExpectedResumeContextV2,
        limits: SnapshotLimitsV2,
        mut cancellation: C,
    ) -> Result<SealedSnapshotV2, SnapshotV2Error> {
        validate_limits(limits)?;
        let context = expected_context.context();
        let payload_len =
            u64::try_from(bytes.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        if payload_len > limits.max_payload_bytes {
            return Err(SnapshotV2Error::PayloadLimitExceeded {
                declared: payload_len,
                limit: limits.max_payload_bytes,
            });
        }
        let payload_content = SnapshotPayloadContentIdV2::from_content_id(hash_content(
            &bytes,
            limits.hash_poll_bytes,
            "payload hashing",
            &mut cancellation,
        )?);
        let resume = resume_receipt(
            context,
            payload_content,
            payload_len,
            limits.identity,
            &mut cancellation,
        )?;
        prepend_header_space(&mut bytes, limits.hash_poll_bytes, &mut cancellation)?;
        write_snapshot_header(
            &mut bytes,
            context,
            payload_len,
            payload_content,
            resume.id(),
            limits.hash_poll_bytes,
            &mut cancellation,
        )?;
        let content_id = SnapshotContentIdV2::from_content_id(hash_content(
            &bytes,
            limits.hash_poll_bytes,
            "envelope hashing",
            &mut cancellation,
        )?);
        let authority_subject =
            authority_subject_receipt(content_id, resume.id(), limits.identity, &mut cancellation)?;
        if cancellation.is_cancelled() {
            return Err(SnapshotV2Error::Cancelled {
                phase: "snapshot publication",
                at: u64::try_from(bytes.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?,
            });
        }
        Ok(SealedSnapshotV2 {
            bytes,
            content_id,
            resume,
            authority_subject,
            expected_context: expected_context.clone(),
        })
    }

    #[cfg(test)]
    pub(super) fn seal<C: CancellationProbe>(
        payload: &[u8],
        expected_context: &ExpectedResumeContextV2,
        limits: SnapshotLimitsV2,
        cancellation: C,
    ) -> Result<SealedSnapshotV2, SnapshotV2Error> {
        validate_limits(limits)?;
        let payload_len =
            u64::try_from(payload.len()).map_err(|_| SnapshotV2Error::LengthOverflow)?;
        if payload_len > limits.max_payload_bytes {
            return Err(SnapshotV2Error::PayloadLimitExceeded {
                declared: payload_len,
                limit: limits.max_payload_bytes,
            });
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(payload.len())
            .map_err(|_| SnapshotV2Error::AllocationFailed {
                phase: "test payload fixture",
                requested: payload_len,
            })?;
        owned.extend_from_slice(payload);
        seal_encoded_payload(owned, expected_context, limits, cancellation)
    }

    /// Fixed-header result used by streaming callers before they materialize
    /// the declared payload.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SnapshotPayloadPlanV2 {
        payload_len: u64,
        total_len: usize,
    }

    impl SnapshotPayloadPlanV2 {
        /// Exact payload bytes to fetch after the fixed header.
        #[must_use]
        pub const fn payload_len(self) -> u64 {
            self.payload_len
        }

        /// Exact complete artifact bytes after checked host conversion/addition.
        #[must_use]
        pub const fn total_len(self) -> usize {
            self.total_len
        }
    }

    /// Preflight one exact fixed header before allocating or fetching payload
    /// bytes. This checks the distinct format, canonical header shape, caps,
    /// lifecycle tag, and self-consistency of the declared drain report.
    pub fn preflight_header(
        header: &[u8; HEADER_LEN_V2],
        limits: SnapshotLimitsV2,
    ) -> Result<SnapshotPayloadPlanV2, SnapshotV2Error> {
        validate_limits(limits)?;
        if header[..8] != MAGIC_V2 {
            return Err(SnapshotV2Error::BadMagic);
        }
        let version = read_u32(header, OFFSET_VERSION);
        if version != ENVELOPE_VERSION_V2 {
            return Err(SnapshotV2Error::UnknownEnvelopeVersion { found: version });
        }
        let declared_header = read_u32(header, OFFSET_HEADER_LEN);
        if usize::try_from(declared_header).ok() != Some(HEADER_LEN_V2) {
            return Err(SnapshotV2Error::InvalidHeaderLength {
                declared: declared_header,
            });
        }
        let declared_frame = read_u32(header, OFFSET_CANONICAL_FRAME_VERSION);
        if declared_frame != CANONICAL_FRAME_VERSION {
            return Err(SnapshotV2Error::UnsupportedCanonicalFrameVersion {
                declared: declared_frame,
                current: CANONICAL_FRAME_VERSION,
            });
        }
        let declared_resume_schema = read_32(header, OFFSET_RESUME_SCHEMA_ID);
        let current_resume_schema = SchemaId::<SnapshotResumeIdentitySchemaV2>::for_schema();
        if &declared_resume_schema != current_resume_schema.as_bytes() {
            return Err(SnapshotV2Error::UnsupportedResumeSchemaEra {
                declared: declared_resume_schema,
                current: *current_resume_schema.as_bytes(),
            });
        }
        let declared_authority_schema = read_32(header, OFFSET_AUTHORITY_SCHEMA_ID);
        let current_authority_schema = SchemaId::<SnapshotAuthoritySubjectSchemaV2>::for_schema();
        if &declared_authority_schema != current_authority_schema.as_bytes() {
            return Err(SnapshotV2Error::UnsupportedAuthoritySchemaEra {
                declared: declared_authority_schema,
                current: *current_authority_schema.as_bytes(),
            });
        }
        let declared_drain_era =
            SnapshotDrainReportEraIdV2::from_bytes(read_32(header, OFFSET_DRAIN_REPORT_ERA));
        let current_drain_era = current_drain_report_era();
        if declared_drain_era != current_drain_era {
            return Err(SnapshotV2Error::UnsupportedDrainReportEra {
                declared: declared_drain_era,
                current: current_drain_era,
            });
        }
        if header[OFFSET_RESERVED..OFFSET_RESERVED + RESERVED_LEN]
            .iter()
            .any(|&byte| byte != 0)
        {
            return Err(SnapshotV2Error::NonzeroReservedHeader);
        }
        let determinism_tag = header[OFFSET_DETERMINISM];
        SnapshotDeterminismV2::from_tag(determinism_tag).ok_or(
            SnapshotV2Error::InvalidDeterminismTag {
                found: determinism_tag,
            },
        )?;
        let lifecycle_tag = header[OFFSET_LIFECYCLE];
        if lifecycle_tag != LIFECYCLE_PAUSED_AND_DRAINED {
            return Err(SnapshotV2Error::InvalidLifecycleTag {
                found: lifecycle_tag,
            });
        }
        DeclaredPausedSnapshotBoundaryV2::from_header(
            SnapshotPauseRequestIdV2::from_bytes(read_32(header, OFFSET_PAUSE_REQUEST)),
            read_u64(header, OFFSET_GATE_GENERATION),
            read_u64(header, OFFSET_DRAIN_RUN),
            read_u64(header, OFFSET_DRAIN_REGISTERED),
            read_u64(header, OFFSET_DRAINED_WORKERS),
            read_32(header, OFFSET_DRAIN_REPORT),
        )?;
        let payload_len = read_u64(header, OFFSET_PAYLOAD_LEN);
        if payload_len > limits.max_payload_bytes {
            return Err(SnapshotV2Error::PayloadLimitExceeded {
                declared: payload_len,
                limit: limits.max_payload_bytes,
            });
        }
        let payload_len_usize = usize::try_from(payload_len).map_err(|_| {
            SnapshotV2Error::CodecResourceLimitExceeded {
                resource: "platform payload bytes",
                requested: payload_len,
                limit: u64::try_from(usize::MAX).unwrap_or(u64::MAX),
                at: 0,
            }
        })?;
        let total_len = HEADER_LEN_V2
            .checked_add(payload_len_usize)
            .ok_or(SnapshotV2Error::LengthOverflow)?;
        Ok(SnapshotPayloadPlanV2 {
            payload_len,
            total_len,
        })
    }

    /// Validate v2 structure, limits, payload content, and semantic resume
    /// identity without granting authority to decode state.
    #[allow(clippy::too_many_lines)]
    pub fn inspect<C: CancellationProbe>(
        bytes: &[u8],
        limits: SnapshotLimitsV2,
        mut cancellation: C,
    ) -> Result<SnapshotInspectionV2<'_>, SnapshotV2Error> {
        validate_limits(limits)?;
        if bytes.len() < 8 {
            return Err(SnapshotV2Error::Truncated {
                needed: 8,
                have: bytes.len(),
            });
        }
        if bytes[..8] != MAGIC_V2 {
            return Err(SnapshotV2Error::BadMagic);
        }
        if bytes.len() < HEADER_LEN_V2 {
            return Err(SnapshotV2Error::Truncated {
                needed: HEADER_LEN_V2,
                have: bytes.len(),
            });
        }
        let header: &[u8; HEADER_LEN_V2] = bytes[..HEADER_LEN_V2]
            .try_into()
            .expect("fixed header slice has the exact checked length");
        let plan = preflight_header(header, limits)?;
        if bytes.len() != plan.total_len {
            let actual = u64::try_from(bytes.len() - HEADER_LEN_V2)
                .map_err(|_| SnapshotV2Error::LengthOverflow)?;
            return Err(SnapshotV2Error::LengthMismatch {
                declared: plan.payload_len,
                actual,
            });
        }
        let determinism = SnapshotDeterminismV2::from_tag(bytes[OFFSET_DETERMINISM])
            .expect("preflight validated determinism tag");
        let pause_boundary = DeclaredPausedSnapshotBoundaryV2::from_header(
            SnapshotPauseRequestIdV2::from_bytes(read_32(bytes, OFFSET_PAUSE_REQUEST)),
            read_u64(bytes, OFFSET_GATE_GENERATION),
            read_u64(bytes, OFFSET_DRAIN_RUN),
            read_u64(bytes, OFFSET_DRAIN_REGISTERED),
            read_u64(bytes, OFFSET_DRAINED_WORKERS),
            read_32(bytes, OFFSET_DRAIN_REPORT),
        )?;
        let context = SnapshotResumeContextV2::from_header(
            SnapshotStateTypeIdV2::from_bytes(read_32(bytes, OFFSET_STATE_TYPE)),
            SnapshotStateSchemaIdV2::from_bytes(read_32(bytes, OFFSET_STATE_SCHEMA)),
            SnapshotStateCodecIdV2::from_bytes(read_32(bytes, OFFSET_STATE_CODEC)),
            read_u32(bytes, OFFSET_STATE_CODEC_VERSION),
            SnapshotAlgorithmIdV2::from_bytes(read_32(bytes, OFFSET_ALGORITHM)),
            read_u64(bytes, OFFSET_ALGORITHM_VERSION),
            SnapshotProblemIdV2::from_bytes(read_32(bytes, OFFSET_PROBLEM)),
            SnapshotRngCounterIdV2::from_bytes(read_32(bytes, OFFSET_RNG_COUNTER)),
            determinism,
            SnapshotExecutionFingerprintIdV2::from_bytes(read_32(
                bytes,
                OFFSET_EXECUTION_FINGERPRINT,
            )),
            SnapshotBudgetStateIdV2::from_bytes(read_32(bytes, OFFSET_BUDGET)),
            SnapshotProvenanceIdV2::from_bytes(read_32(bytes, OFFSET_PROVENANCE)),
            pause_boundary,
        );
        let payload = &bytes[HEADER_LEN_V2..];
        let declared_payload =
            SnapshotPayloadContentIdV2::parse_slice(&read_32(bytes, OFFSET_PAYLOAD_CONTENT))
                .expect("32-byte v2 payload root always parses structurally");
        let computed_payload = SnapshotPayloadContentIdV2::from_content_id(hash_content(
            payload,
            limits.hash_poll_bytes,
            "payload hashing",
            &mut cancellation,
        )?);
        if declared_payload != computed_payload {
            return Err(SnapshotV2Error::PayloadContentMismatch {
                declared: declared_payload,
                computed: computed_payload,
            });
        }
        let resume = resume_receipt(
            &context,
            computed_payload,
            plan.payload_len,
            limits.identity,
            &mut cancellation,
        )?;
        let declared_resume = SnapshotResumeIdV2::parse_slice(&read_32(bytes, OFFSET_RESUME_ID))
            .expect("32-byte v2 resume root always parses structurally");
        if declared_resume != resume.id() {
            return Err(SnapshotV2Error::ResumeIdentityMismatch {
                declared: declared_resume,
                computed: resume.id(),
            });
        }
        let content_id = SnapshotContentIdV2::from_content_id(hash_content(
            bytes,
            limits.hash_poll_bytes,
            "envelope hashing",
            &mut cancellation,
        )?);
        let authority_subject =
            authority_subject_receipt(content_id, resume.id(), limits.identity, &mut cancellation)?;
        Ok(SnapshotInspectionV2 {
            payload,
            payload_content: computed_payload,
            content_id,
            resume,
            authority_subject,
            context,
            admission: SnapshotAdmissionV2::UnanchoredConsistencyOnly,
            authority_evidence: None,
            expected_context: None,
        })
    }

    /// Inspect only when both caller-retained exact roots agree.
    ///
    /// The returned payload view borrows only `bytes`; expectation context is
    /// cloned into the inspection so the caller-held token may be short-lived.
    pub fn inspect_expected<'a, C: CancellationProbe>(
        bytes: &'a [u8],
        expected: &SnapshotExpectationV2,
        limits: SnapshotLimitsV2,
        cancellation: C,
    ) -> Result<SnapshotInspectionV2<'a>, SnapshotV2Error> {
        let mut inspection = inspect(bytes, limits, cancellation)?;
        if inspection.content_id != expected.content_id {
            return Err(SnapshotV2Error::ExpectedContentMismatch {
                expected: expected.content_id,
                computed: inspection.content_id,
            });
        }
        if inspection.resume.id() != expected.resume_id {
            return Err(SnapshotV2Error::ExpectedResumeMismatch {
                expected: expected.resume_id,
                computed: inspection.resume.id(),
            });
        }
        if let Some(field) =
            first_context_mismatch(expected.expected_context.context(), &inspection.context)
        {
            return Err(SnapshotV2Error::ExpectedContextMismatch { field });
        }
        inspection.admission = SnapshotAdmissionV2::MatchedCallerExpectation;
        inspection.expected_context = Some(expected.expected_context.clone());
        Ok(inspection)
    }

    /// Inspect only when an admitted verifier/policy capability binds the exact
    /// recomputed complete-envelope plus resume subject and the candidate also
    /// matches the independently supplied caller/session context.
    /// Cryptographic signature semantics, when required, belong to the injected
    /// verifier capability and are not inferred from `Admitted` alone.
    pub fn inspect_authorized<'a, V, P, C>(
        bytes: &'a [u8],
        authority: &AuthorityRef<SnapshotAuthoritySubjectIdV2, V, P, Admitted>,
        expected_context: &ExpectedResumeContextV2,
        limits: SnapshotLimitsV2,
        cancellation: C,
    ) -> Result<SnapshotInspectionV2<'a>, SnapshotV2Error>
    where
        V: CanonicalSchema,
        P: CanonicalSchema,
        C: CancellationProbe,
    {
        let mut inspection = inspect(bytes, limits, cancellation)?;
        let authorized = authority.receipt();
        if authorized.id() != inspection.authority_subject.id()
            || authorized.canonical_preimage() != inspection.authority_subject.canonical_preimage()
            || authorized.canonical_bytes() != inspection.authority_subject.canonical_bytes()
        {
            return Err(SnapshotV2Error::AuthoritySubjectMismatch);
        }
        if let Some(field) = first_context_mismatch(expected_context.context(), &inspection.context)
        {
            return Err(SnapshotV2Error::ExpectedContextMismatch { field });
        }
        inspection.admission = SnapshotAdmissionV2::PolicyRelativeAdmission;
        inspection.authority_evidence = Some(authority.audit_record());
        inspection.expected_context = Some(expected_context.clone());
        Ok(inspection)
    }

    pub(super) fn decode_admitted<S: SolverStateV2, C: CancellationProbe>(
        inspection: SnapshotInspectionV2<'_>,
        limits: SnapshotLimitsV2,
        mut cancellation: C,
    ) -> Result<OpenedSnapshotV2<S>, SnapshotV2Error> {
        if !inspection.context.matches_state::<S>() {
            return Err(SnapshotV2Error::WrongStateSchema);
        }
        if inspection.admission == SnapshotAdmissionV2::UnanchoredConsistencyOnly {
            return Err(SnapshotV2Error::AuthoritySubjectMismatch);
        }
        let Some(expected_context) = inspection.expected_context else {
            return Err(SnapshotV2Error::MissingExpectedContext);
        };
        let mut decoder = SnapshotDecoderV2::new(inspection.payload, limits, &mut cancellation)?;
        let state = S::decode_v2(&mut decoder)?;
        decoder.finish()?;
        Ok(OpenedSnapshotV2 {
            state,
            content_id: inspection.content_id,
            resume: inspection.resume,
            authority_subject: inspection.authority_subject,
            expected_context,
            admission: inspection.admission,
            authority_evidence: inspection.authority_evidence,
        })
    }
}

/// A snapshot failure: envelope refusal or payload decode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// The envelope refused (wrong type/version/corruption) — the
    /// payload decoder never ran.
    Envelope(envelope::EnvelopeError),
    /// The envelope validated but the payload decoder failed (an
    /// encode/decode bug within one schema version).
    Payload(codec::CodecError),
}

impl core::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SnapshotError::Envelope(e) => write!(f, "{e}"),
            SnapshotError::Payload(e) => write!(f, "{e}"),
        }
    }
}

impl core::error::Error for SnapshotError {}

/// Legacy v1 solver-state codec declaration.
///
/// This compatibility trait deliberately exposes only the historical u64
/// identities and infallible payload codec. All envelope operations live on
/// [`LegacySnapshotV1Adapter`], so a call site must name the weak identity era
/// explicitly. Implementing [`SolverStateV2`] neither requires nor implies
/// this trait.
pub trait LegacySolverStateV1: Sized {
    /// Historical stable type identity. Never widen or reinterpret this u64 as
    /// a v2 state identity.
    const TYPE_ID_V1: u64;
    /// Historical payload schema version.
    const SCHEMA_VERSION_V1: u32;

    /// Write the legacy v1 snapshot payload.
    fn encode_v1(&self, encoder: &mut codec::Enc);

    /// Read a legacy v1 snapshot payload.
    ///
    /// # Errors
    /// [`codec::CodecError`] on truncated or incompatible bytes.
    fn decode_v1(decoder: &mut codec::Dec<'_>) -> Result<Self, codec::CodecError>;
}

/// A decoded legacy v1 state with the exact source envelope still attached.
///
/// This is quarantine evidence, not a migration receipt or producer
/// authentication. Consuming it returns both the decoded state and the exact
/// borrowed legacy source metadata.
#[must_use = "legacy source bytes and u64 metadata must remain explicit"]
pub struct OpenedLegacySnapshotV1<'a, S> {
    state: S,
    source: LegacySnapshotV1<'a>,
}

impl<S> core::fmt::Debug for OpenedLegacySnapshotV1<'_, S> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("OpenedLegacySnapshotV1")
            .field("source", &self.source)
            .field("state_type", &core::any::type_name::<S>())
            .finish_non_exhaustive()
    }
}

impl<'a, S> OpenedLegacySnapshotV1<'a, S> {
    /// Borrow the decoded legacy state.
    #[must_use]
    pub const fn state(&self) -> &S {
        &self.state
    }

    /// Exact checked legacy source bytes and historical u64 metadata.
    #[must_use]
    pub const fn source(&self) -> LegacySnapshotV1<'a> {
        self.source
    }

    /// Consume the attachment without silently discarding the legacy source.
    #[must_use]
    pub fn into_parts(self) -> (S, LegacySnapshotV1<'a>) {
        (self.state, self.source)
    }
}

/// Explicit compatibility namespace for the legacy v1 solver-state format.
///
/// The adapter preserves exact v1 bytes and u64 fields; it does not widen them
/// into v2 identities, authenticate their producer, or mint migration/fork
/// authority.
pub struct LegacySnapshotV1Adapter<S>(core::marker::PhantomData<fn() -> S>);

impl<S: LegacySolverStateV1> LegacySnapshotV1Adapter<S> {
    /// Seal a legacy v1 envelope with historical u64 provenance.
    #[must_use]
    pub fn seal(state: &S, provenance: u64) -> Vec<u8> {
        let mut encoder = codec::Enc::new();
        state.encode_v1(&mut encoder);
        envelope::seal(
            S::TYPE_ID_V1,
            S::SCHEMA_VERSION_V1,
            provenance,
            &encoder.into_bytes(),
        )
    }

    /// Validate and decode a legacy v1 envelope while retaining its exact
    /// source bytes and original payload checksum.
    ///
    /// # Errors
    /// [`SnapshotError`] — envelope refusals never reach the decoder.
    pub fn open(bytes: &[u8]) -> Result<OpenedLegacySnapshotV1<'_, S>, SnapshotError> {
        let source = inspect_legacy_snapshot_v1(bytes).map_err(SnapshotError::Envelope)?;
        let (payload, _) = envelope::open(bytes, S::TYPE_ID_V1, S::SCHEMA_VERSION_V1)
            .map_err(SnapshotError::Envelope)?;
        let mut decoder = codec::Dec::new(payload);
        let state = S::decode_v1(&mut decoder).map_err(SnapshotError::Payload)?;
        if !decoder.is_empty() {
            return Err(SnapshotError::Payload(codec::CodecError {
                at: decoder.position(),
                what: "end of legacy v1 snapshot payload",
                needed: 0,
                remaining: decoder.remaining(),
            }));
        }
        Ok(OpenedLegacySnapshotV1 { state, source })
    }

    /// Seal with unattributed historical provenance zero.
    #[must_use]
    pub fn to_bytes(state: &S) -> Vec<u8> {
        Self::seal(state, 0)
    }

    /// Decode an unattributed or attributed legacy envelope, explicitly
    /// discharging the attached source metadata.
    ///
    /// # Errors
    /// [`SnapshotError`] on any v1 envelope or codec refusal.
    pub fn from_bytes(bytes: &[u8]) -> Result<S, SnapshotError> {
        Self::open(bytes).map(|opened| opened.into_parts().0)
    }

    /// Historical FNV-1a correlation hash of the complete unattributed v1
    /// envelope. This u64 is not a v2 content or semantic identity.
    #[must_use]
    pub fn historical_content_hash(state: &S) -> u64 {
        fs_obs::fnv1a64(&Self::to_bytes(state))
    }

    /// Legacy in-memory round trip through exact v1 envelope bytes.
    ///
    /// This proves only v1 codec self-consistency. It is deliberately not
    /// called a semantic fork and carries no v2 lineage claim.
    ///
    /// # Errors
    /// [`SnapshotError`] when the legacy encoder and decoder disagree.
    pub fn round_trip(state: &S) -> Result<S, SnapshotError> {
        Self::from_bytes(&Self::to_bytes(state))
    }
}

/// Opt-in strong-identity snapshot contract.
///
/// V2 state owners must declare a full-width schema identity directly. The
/// legacy [`LegacySolverStateV1::TYPE_ID_V1`] is deliberately not re-hashed or
/// widened into this value. Existing v1 implementations therefore remain compatible
/// but gain no v2 resume authority until they make this explicit declaration.
/// These byte constants are nominal declarations, not owner authentication;
/// uniqueness is the implementer's responsibility until the owner-charter
/// registry lands. A malicious or mistaken implementation can reuse another
/// type's values, allocate or perform side effects outside the codec helpers,
/// or panic. This trait alone does not certify whole-implementation purity,
/// memory bounds, cancellation latency, or nominal Rust-type ownership.
///
/// A v2-only state cannot be routed into a legacy adapter accidentally:
///
/// ```compile_fail
/// use fs_exec::solver::{LegacySnapshotV1Adapter, SolverStateV2, snapshot_v2};
///
/// struct StrongOnly;
/// impl SolverStateV2 for StrongOnly {
///     const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 =
///         snapshot_v2::SnapshotStateTypeIdV2::from_bytes([1; 32]);
///     const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
///         snapshot_v2::SnapshotStateSchemaIdV2::from_bytes([2; 32]);
///     const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
///         snapshot_v2::SnapshotStateCodecIdV2::from_bytes([3; 32]);
///     const STATE_CODEC_VERSION_V2: u32 = 1;
///     fn encode_v2(
///         &self,
///         _: &mut snapshot_v2::SnapshotEncoderV2<'_>,
///     ) -> Result<(), snapshot_v2::SnapshotV2Error> { Ok(()) }
///     fn decode_v2(
///         _: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
///     ) -> Result<Self, snapshot_v2::SnapshotV2Error> { Ok(Self) }
/// }
///
/// let _legacy = LegacySnapshotV1Adapter::<StrongOnly>::to_bytes(&StrongOnly);
/// ```
pub trait SolverStateV2: Sized {
    /// Full-width nominal identity of this exact Rust state type.
    const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2;
    /// Full-width state schema identity owned by this exact Rust state type.
    const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2;
    /// Full-width identity of the exact v2 payload codec grammar.
    const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2;
    /// Codec version within [`Self::STATE_SCHEMA_ID_V2`].
    const STATE_CODEC_VERSION_V2: u32;

    /// Encode through the fallible, capped, cancellation-aware v2 producer.
    /// Implementations must not delegate to
    /// [`LegacySolverStateV1::encode_v1`].
    /// They must propagate every refusal, avoid side effects, use only
    /// budget-admitted storage, and treat the encoder as a transaction; helper
    /// poisoning prevents swallowed helper errors from publishing but cannot
    /// police arbitrary code outside the helper.
    fn encode_v2(
        &self,
        encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
    ) -> Result<(), snapshot_v2::SnapshotV2Error>;

    /// Decode through the capped, cancellation-aware v2 decoder.
    /// Implementations must not delegate to
    /// [`LegacySolverStateV1::decode_v1`].
    /// They must propagate every refusal and construct state only through
    /// budget-admitted resources. Direct allocations or side effects are
    /// outside the current enforcement boundary.
    fn decode_v2(
        decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
    ) -> Result<Self, snapshot_v2::SnapshotV2Error>;

    /// Encode and seal one v2 snapshot.
    ///
    /// `context` must carry the nominal declarations for `Self`; a context with
    /// different bytes is refused before publication. Equality of caller-chosen
    /// constants is not yet proof that two Rust types share an owner.
    ///
    /// # Errors
    /// Resource, cancellation, allocation, or state-schema refusal. A caller
    /// should pass a fresh finalization-scope probe: the old compute context is
    /// already cancelled by definition at a valid pause boundary.
    fn seal_v2<C>(
        &self,
        expected_context: &snapshot_v2::ExpectedResumeContextV2,
        limits: snapshot_v2::SnapshotLimitsV2,
        mut cancellation: C,
    ) -> Result<snapshot_v2::SealedSnapshotV2, snapshot_v2::SnapshotV2Error>
    where
        C: fs_blake3::identity::CancellationProbe,
    {
        if !expected_context.context().matches_state::<Self>() {
            return Err(snapshot_v2::SnapshotV2Error::WrongStateSchema);
        }
        let mut encoder = snapshot_v2::SnapshotEncoderV2::new(limits, &mut cancellation)?;
        self.encode_v2(&mut encoder)?;
        let payload = encoder.finish()?;
        snapshot_v2::seal_encoded_payload(payload, expected_context, limits, cancellation)
    }

    /// Decode only after exact caller-retained content and resume roots match.
    ///
    /// # Errors
    /// Any structural, identity, lifecycle, state-schema, cancellation, or
    /// payload-codec refusal.
    fn unseal_v2_expected<C>(
        bytes: &[u8],
        expected: &snapshot_v2::SnapshotExpectationV2,
        limits: snapshot_v2::SnapshotLimitsV2,
        mut cancellation: C,
    ) -> Result<snapshot_v2::OpenedSnapshotV2<Self>, snapshot_v2::SnapshotV2Error>
    where
        C: fs_blake3::identity::CancellationProbe,
    {
        let inspection =
            snapshot_v2::inspect_expected(bytes, expected, limits, || cancellation.is_cancelled())?;
        snapshot_v2::decode_admitted::<Self, _>(inspection, limits, cancellation)
    }

    /// Decode only after an injected verifier/admitter has authorized the
    /// exact recomputed complete-envelope plus semantic-resume subject.
    ///
    /// # Errors
    /// Any structural, authority, lifecycle, state-schema, cancellation, or
    /// payload-codec refusal.
    fn unseal_v2_authorized<V, P, C>(
        bytes: &[u8],
        authority: &fs_blake3::identity::AuthorityRef<
            snapshot_v2::SnapshotAuthoritySubjectIdV2,
            V,
            P,
            fs_blake3::identity::Admitted,
        >,
        expected_context: &snapshot_v2::ExpectedResumeContextV2,
        limits: snapshot_v2::SnapshotLimitsV2,
        mut cancellation: C,
    ) -> Result<snapshot_v2::OpenedSnapshotV2<Self>, snapshot_v2::SnapshotV2Error>
    where
        V: fs_blake3::identity::CanonicalSchema,
        P: fs_blake3::identity::CanonicalSchema,
        C: fs_blake3::identity::CancellationProbe,
    {
        let inspection =
            snapshot_v2::inspect_authorized(bytes, authority, expected_context, limits, || {
                cancellation.is_cancelled()
            })?;
        snapshot_v2::decode_admitted::<Self, _>(inspection, limits, cancellation)
    }
}

/// One bounded step's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepVerdict<T> {
    /// More steps remain.
    Continue,
    /// Converged/finished with a result.
    Done(T),
}

/// A legacy v1 iterative solver state machine.
///
/// `step_v1` advances one bounded unit of work. The v1 name prevents method
/// ambiguity for a solver that deliberately supports both identity eras.
pub trait ResumableSolverV1 {
    /// The explicitly legacy serializable snapshot type.
    type State: LegacySolverStateV1;
    /// The final result type.
    type Out;

    /// Advance one bounded step. Implementations may poll `cx` internally
    /// for finer-grained cancellation inside expensive steps.
    fn step_v1(&self, state: &mut Self::State, cx: &Cx<'_>) -> StepVerdict<Self::Out>;
}

/// A strong-identity v2 iterative solver state machine.
///
/// This trait has no legacy supertrait and no implicit v1/FNV path. A solver
/// that deliberately supports both eras implements the separately named
/// `step_v1` and `step_v2` methods, so method selection is never ambiguous.
pub trait ResumableSolverV2 {
    /// The independent strong-identity snapshot type.
    type State: SolverStateV2;
    /// The final result type.
    type Out;

    /// Advance one bounded v2 step. Implementations may poll `cx` internally
    /// for finer-grained cancellation inside expensive steps.
    fn step_v2(&self, state: &mut Self::State, cx: &Cx<'_>) -> StepVerdict<Self::Out>;
}

/// The outcome of [`drive_v1`] or [`drive_v2`]: finished, or paused holding the
/// resumable state. The caller may serialize it for a later resume. Establishing
/// semantic fork lineage is deliberately outside this driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverProgress<S, T> {
    /// Ran to completion.
    Done(T),
    /// Cancellation/pause was requested; `state` resumes bit-exactly.
    Paused(S),
}

/// Drive a legacy v1 solver until completion or pause.
pub fn drive_v1<R: ResumableSolverV1>(
    solver: &R,
    mut state: R::State,
    cx: &Cx<'_>,
) -> SolverProgress<R::State, R::Out> {
    loop {
        if cx.is_cancel_requested() {
            return SolverProgress::Paused(state);
        }
        match solver.step_v1(&mut state, cx) {
            StepVerdict::Continue => {}
            StepVerdict::Done(out) => return SolverProgress::Done(out),
        }
    }
}

/// Drive a strong-identity v2 solver until completion or pause.
///
/// A paused value remains statically bound by [`SolverStateV2`]. Sealing,
/// admitted opening, and evidence discharge remain explicit operations; this
/// driver does not manufacture snapshot evidence for an in-memory value.
pub fn drive_v2<R: ResumableSolverV2>(
    solver: &R,
    mut state: R::State,
    cx: &Cx<'_>,
) -> SolverProgress<R::State, R::Out> {
    loop {
        if cx.is_cancel_requested() {
            return SolverProgress::Paused(state);
        }
        match solver.step_v2(&mut state, cx) {
            StepVerdict::Continue => {}
            StepVerdict::Done(out) => return SolverProgress::Done(out),
        }
    }
}

/// Explicit legacy v1 round trip retained for compatibility callers.
///
/// This proves only historical codec self-consistency. It does not preserve or
/// mint v2 identity, budget, RNG, authority, or semantic fork lineage.
///
/// # Errors
/// [`SnapshotError`] when the legacy encoder and decoder disagree.
pub fn round_trip_legacy_v1<S: LegacySolverStateV1>(state: &S) -> Result<S, SnapshotError> {
    LegacySnapshotV1Adapter::<S>::round_trip(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cx::{CancelGate, DrainTracker, ExecMode, RunId, StreamKey};
    use asupersync::types::Budget;

    /// wf9.8.2 acceptance: the envelope refuses every corruption and
    /// misbinding class BEFORE the payload decoder runs.
    #[test]
    fn envelope_refuses_every_misbinding_class() {
        // A twin state with the IDENTICAL payload layout but its own
        // type id: same-length bytes must not cross-decode.
        #[derive(Debug, PartialEq)]
        struct TwinState {
            x: Vec<f64>,
            iter: u64,
        }
        impl LegacySolverStateV1 for TwinState {
            const TYPE_ID_V1: u64 = 0x5457_494e_0000_0001;
            const SCHEMA_VERSION_V1: u32 = 1;
            fn encode_v1(&self, enc: &mut codec::Enc) {
                enc.put_u64(self.iter);
                enc.put_f64_slice(&self.x);
            }
            fn decode_v1(dec: &mut codec::Dec<'_>) -> Result<Self, codec::CodecError> {
                Ok(TwinState {
                    iter: dec.get_u64()?,
                    x: dec.get_f64_vec()?,
                })
            }
        }
        let state = JacobiState {
            x: vec![1.5, -2.25, 0.0],
            iter: 42,
        };
        let sealed = LegacySnapshotV1Adapter::<JacobiState>::seal(&state, 0xABCD);
        let inspected = envelope::inspect(&sealed).expect("generic envelope inspection");
        assert_eq!(inspected.type_id(), JacobiState::TYPE_ID_V1);
        assert_eq!(inspected.schema_version(), JacobiState::SCHEMA_VERSION_V1);
        assert_eq!(inspected.provenance(), 0xABCD);
        assert_eq!(
            inspected.payload_len(),
            u64::try_from(sealed.len() - envelope::HEADER_LEN).expect("bounded fixture")
        );
        // The happy path round-trips bit-exactly WITH provenance.
        let opened = LegacySnapshotV1Adapter::<JacobiState>::open(&sealed).expect("valid seal");
        let (back, source) = opened.into_parts();
        let prov = source.info().provenance();
        assert_eq!(back, state);
        assert_eq!(prov, 0xABCD);
        assert_eq!(source.bytes(), sealed);
        assert_eq!(source.info(), inspected);
        assert_eq!(
            source.payload_checksum(),
            u64::from_le_bytes(sealed[40..48].try_into().unwrap())
        );
        // Cross-type: identical payload layout, refused by TYPE ID.
        assert!(matches!(
            LegacySnapshotV1Adapter::<TwinState>::open(&sealed),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::WrongTypeId { .. }
            ))
        ));
        // Bit flip in the payload: checksum refuses.
        let mut flipped = sealed.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0x40;
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&flipped),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::ChecksumMismatch { .. }
            ))
        ));
        // Bit flip in the magic: not a snapshot.
        let mut bad_magic = sealed.clone();
        bad_magic[0] ^= 0x01;
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&bad_magic),
            Err(SnapshotError::Envelope(envelope::EnvelopeError::BadMagic))
        ));
        // Truncation: header-level and payload-level both refuse.
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&sealed[..10]),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::Truncated { .. }
            ))
        ));
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&sealed[..sealed.len() - 3]),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::LengthMismatch { .. }
            ))
        ));
        // Appended bytes: refused by the declared length.
        let mut appended = sealed.clone();
        appended.extend_from_slice(&[0u8; 5]);
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&appended),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::LengthMismatch { .. }
            ))
        ));
        // Unknown envelope version.
        let mut future = sealed.clone();
        future[8..12].copy_from_slice(&9u32.to_le_bytes());
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&future),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::UnknownEnvelopeVersion { found: 9 }
            ))
        ));
        // Stale schema version: structured refusal, not a wrong decode.
        let mut stale = sealed;
        stale[20..24].copy_from_slice(&7u32.to_le_bytes());
        assert!(matches!(
            LegacySnapshotV1Adapter::<JacobiState>::open(&stale),
            Err(SnapshotError::Envelope(
                envelope::EnvelopeError::IncompatibleSchema {
                    expected: 1,
                    found: 7
                }
            ))
        ));
    }

    /// Reference solver: damped Jacobi on a fixed diagonally-dominant
    /// system (deterministic, non-trivial float trajectory).
    struct Jacobi {
        rhs: Vec<f64>,
        tol: f64,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct JacobiState {
        x: Vec<f64>,
        iter: u64,
    }

    impl LegacySolverStateV1 for JacobiState {
        const TYPE_ID_V1: u64 = 0x4a41_434f_4249_0001;
        const SCHEMA_VERSION_V1: u32 = 1;

        fn encode_v1(&self, enc: &mut codec::Enc) {
            enc.put_u64(self.iter);
            enc.put_f64_slice(&self.x);
        }

        fn decode_v1(dec: &mut codec::Dec<'_>) -> Result<Self, codec::CodecError> {
            Ok(JacobiState {
                iter: dec.get_u64()?,
                x: dec.get_f64_vec()?,
            })
        }
    }

    impl SolverStateV2 for JacobiState {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 =
            snapshot_v2::SnapshotStateTypeIdV2::from_bytes([0x41; 32]);
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            snapshot_v2::SnapshotStateSchemaIdV2::from_bytes([0x4a; 32]);
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            snapshot_v2::SnapshotStateCodecIdV2::from_bytes([0xc1; 32]);
        const STATE_CODEC_VERSION_V2: u32 = 1;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            encoder.put_u64(self.iter)?;
            encoder.put_f64_slice(&self.x)
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            Ok(Self {
                iter: decoder.get_u64()?,
                x: decoder.get_f64_vec()?,
            })
        }
    }

    /// Identical payload layout and schema/codec declarations, deliberately
    /// different only in its full-width v2 state-type identity.
    #[derive(Debug, Clone, PartialEq)]
    struct TwinV2State(JacobiState);

    impl SolverStateV2 for TwinV2State {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 =
            snapshot_v2::SnapshotStateTypeIdV2::from_bytes([0x54; 32]);
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            JacobiState::STATE_SCHEMA_ID_V2;
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            JacobiState::STATE_CODEC_ID_V2;
        const STATE_CODEC_VERSION_V2: u32 = 1;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            self.0.encode_v2(encoder)
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            JacobiState::decode_v2(decoder).map(Self)
        }
    }

    /// Identical v2 type/schema/codec declarations, deliberately different
    /// only in its codec version.
    #[derive(Debug, Clone, PartialEq)]
    struct CodecBumpV2State(JacobiState);

    impl SolverStateV2 for CodecBumpV2State {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 = JacobiState::STATE_TYPE_ID_V2;
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            JacobiState::STATE_SCHEMA_ID_V2;
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            JacobiState::STATE_CODEC_ID_V2;
        const STATE_CODEC_VERSION_V2: u32 = 2;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            self.0.encode_v2(encoder)
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            JacobiState::decode_v2(decoder).map(Self)
        }
    }

    /// Identical v2 type/codec declarations and payload layout, deliberately
    /// different only in its state-schema identity.
    #[derive(Debug, Clone, PartialEq)]
    struct SchemaOnlyV2State(JacobiState);

    impl SolverStateV2 for SchemaOnlyV2State {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 = JacobiState::STATE_TYPE_ID_V2;
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            snapshot_v2::SnapshotStateSchemaIdV2::from_bytes([0x5c; 32]);
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            JacobiState::STATE_CODEC_ID_V2;
        const STATE_CODEC_VERSION_V2: u32 = JacobiState::STATE_CODEC_VERSION_V2;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            self.0.encode_v2(encoder)
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            JacobiState::decode_v2(decoder).map(Self)
        }
    }

    /// Identical v2 type/schema declarations and payload layout, deliberately
    /// different only in its codec identity.
    #[derive(Debug, Clone, PartialEq)]
    struct CodecOnlyV2State(JacobiState);

    impl SolverStateV2 for CodecOnlyV2State {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 = JacobiState::STATE_TYPE_ID_V2;
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            JacobiState::STATE_SCHEMA_ID_V2;
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            snapshot_v2::SnapshotStateCodecIdV2::from_bytes([0xc2; 32]);
        const STATE_CODEC_VERSION_V2: u32 = JacobiState::STATE_CODEC_VERSION_V2;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            self.0.encode_v2(encoder)
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            JacobiState::decode_v2(decoder).map(Self)
        }
    }

    /// Adversarial codec implementation that deliberately swallows failures.
    /// The v2 transaction wrapper must remain poisoned and refuse publication.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SwallowingCodecV2State;

    impl SolverStateV2 for SwallowingCodecV2State {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 =
            snapshot_v2::SnapshotStateTypeIdV2::from_bytes([0x71; 32]);
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            snapshot_v2::SnapshotStateSchemaIdV2::from_bytes([0x72; 32]);
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            snapshot_v2::SnapshotStateCodecIdV2::from_bytes([0x73; 32]);
        const STATE_CODEC_VERSION_V2: u32 = 1;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            encoder.put_u64(1)?;
            let _ignored = encoder.put_u64(2);
            Ok(())
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            let _ignored = decoder.get_f64_vec();
            Ok(Self)
        }
    }

    /// A v2-only compile-pass fixture: there is intentionally no
    /// `LegacySolverStateV1` implementation for this state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct StrongOnlyCounterState {
        step: u64,
    }

    impl SolverStateV2 for StrongOnlyCounterState {
        const STATE_TYPE_ID_V2: snapshot_v2::SnapshotStateTypeIdV2 =
            snapshot_v2::SnapshotStateTypeIdV2::from_bytes([0x81; 32]);
        const STATE_SCHEMA_ID_V2: snapshot_v2::SnapshotStateSchemaIdV2 =
            snapshot_v2::SnapshotStateSchemaIdV2::from_bytes([0x82; 32]);
        const STATE_CODEC_ID_V2: snapshot_v2::SnapshotStateCodecIdV2 =
            snapshot_v2::SnapshotStateCodecIdV2::from_bytes([0x83; 32]);
        const STATE_CODEC_VERSION_V2: u32 = 1;

        fn encode_v2(
            &self,
            encoder: &mut snapshot_v2::SnapshotEncoderV2<'_>,
        ) -> Result<(), snapshot_v2::SnapshotV2Error> {
            encoder.put_u64(self.step)
        }

        fn decode_v2(
            decoder: &mut snapshot_v2::SnapshotDecoderV2<'_, '_>,
        ) -> Result<Self, snapshot_v2::SnapshotV2Error> {
            Ok(Self {
                step: decoder.get_u64()?,
            })
        }
    }

    struct StrongOnlyCounter {
        target: u64,
    }

    impl ResumableSolverV2 for StrongOnlyCounter {
        type State = StrongOnlyCounterState;
        type Out = u64;

        fn step_v2(&self, state: &mut Self::State, _cx: &Cx<'_>) -> StepVerdict<Self::Out> {
            state.step += 1;
            if state.step == self.target {
                StepVerdict::Done(state.step)
            } else {
                StepVerdict::Continue
            }
        }
    }

    enum SnapshotVerifierSchema {}

    impl fs_blake3::identity::CanonicalSchema for SnapshotVerifierSchema {
        const DOMAIN: &'static str = "org.frankensim.tests.snapshot-verifier.v1";
        const NAME: &'static str = "snapshot-verifier";
        const VERSION: u32 = 1;
        const CONTEXT: &'static str = "test verifier capability identity";
        const FIELDS: &'static [fs_blake3::identity::FieldSpec] =
            &[fs_blake3::identity::FieldSpec::required(
                "configuration",
                fs_blake3::identity::WireType::Bytes,
            )];
    }

    enum SnapshotPolicySchema {}

    impl fs_blake3::identity::CanonicalSchema for SnapshotPolicySchema {
        const DOMAIN: &'static str = "org.frankensim.tests.snapshot-policy.v1";
        const NAME: &'static str = "snapshot-policy";
        const VERSION: u32 = 1;
        const CONTEXT: &'static str = "test admission policy identity";
        const FIELDS: &'static [fs_blake3::identity::FieldSpec] =
            &[fs_blake3::identity::FieldSpec::required(
                "configuration",
                fs_blake3::identity::WireType::Bytes,
            )];
    }

    /// Deliberately permissive fixture proving that `Admitted` is only
    /// policy-relative. Production callers must inject a real verifier.
    struct PermitAllSnapshotPolicyFixture;

    impl
        fs_blake3::identity::AuthorityVerifier<
            snapshot_v2::SnapshotAuthoritySubjectIdV2,
            SnapshotVerifierSchema,
            SnapshotPolicySchema,
        > for PermitAllSnapshotPolicyFixture
    {
        type Error = core::convert::Infallible;

        fn verify(
            &self,
            _presented: &fs_blake3::identity::AuthorityRef<
                snapshot_v2::SnapshotAuthoritySubjectIdV2,
                SnapshotVerifierSchema,
                SnapshotPolicySchema,
                fs_blake3::identity::Presented,
            >,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl
        fs_blake3::identity::AuthorityAdmitter<
            snapshot_v2::SnapshotAuthoritySubjectIdV2,
            SnapshotVerifierSchema,
            SnapshotPolicySchema,
        > for PermitAllSnapshotPolicyFixture
    {
        type Error = core::convert::Infallible;

        fn admit(
            &self,
            _verified: &fs_blake3::identity::AuthorityRef<
                snapshot_v2::SnapshotAuthoritySubjectIdV2,
                SnapshotVerifierSchema,
                SnapshotPolicySchema,
                fs_blake3::identity::Verified,
            >,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn v2_limits(hash_poll_bytes: u32, identity_poll_bytes: u32) -> snapshot_v2::SnapshotLimitsV2 {
        snapshot_v2::SnapshotLimitsV2::new(
            1 << 20,
            hash_poll_bytes,
            fs_blake3::identity::CanonicalLimits::new(16_384, 4_096, 32, 32, identity_poll_bytes),
            4_096,
            1 << 20,
            hash_poll_bytes,
        )
    }

    fn paused_boundary(
        pause_request: u8,
        gate_generation: u64,
        run: u64,
        worker_count: u64,
    ) -> snapshot_v2::PausedSnapshotBoundaryV2 {
        let gate = CancelGate::new();
        let tracker = DrainTracker::new(RunId(run), &gate);
        let mut workers = Vec::new();
        for _ in 0..worker_count {
            workers.push(tracker.register_worker().expect("fixture worker registers"));
        }
        gate.request();
        drop(workers);
        let report = tracker.finalize().expect("fixture run fully drained");
        snapshot_v2::PausedSnapshotBoundaryV2::from_drain_report(
            report,
            snapshot_v2::SnapshotPauseRequestIdV2::from_bytes([pause_request; 32]),
            gate_generation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn v2_context<S: SolverStateV2>(
        algorithm: u8,
        algorithm_version: u64,
        problem: u8,
        rng_counter: u8,
        determinism: snapshot_v2::SnapshotDeterminismV2,
        execution_fingerprint: u8,
        budget: u8,
        provenance: u8,
        pause_boundary: snapshot_v2::PausedSnapshotBoundaryV2,
    ) -> snapshot_v2::ExpectedResumeContextV2 {
        snapshot_v2::ExpectedResumeContextV2::for_paused_state::<S>(
            snapshot_v2::SnapshotAlgorithmIdV2::from_bytes([algorithm; 32]),
            algorithm_version,
            snapshot_v2::SnapshotProblemIdV2::from_bytes([problem; 32]),
            snapshot_v2::SnapshotRngCounterIdV2::from_bytes([rng_counter; 32]),
            determinism,
            snapshot_v2::SnapshotExecutionFingerprintIdV2::from_bytes([execution_fingerprint; 32]),
            snapshot_v2::SnapshotBudgetStateIdV2::from_bytes([budget; 32]),
            snapshot_v2::SnapshotProvenanceIdV2::from_bytes([provenance; 32]),
            pause_boundary,
        )
    }

    fn base_v2_context<S: SolverStateV2>() -> snapshot_v2::ExpectedResumeContextV2 {
        v2_context::<S>(
            0x11,
            7,
            0x22,
            0x33,
            snapshot_v2::SnapshotDeterminismV2::Deterministic,
            0x3f,
            0x44,
            0x55,
            paused_boundary(0x66, 9, 17, 2),
        )
    }

    fn admitted_snapshot_authority(
        sealed: &snapshot_v2::SealedSnapshotV2,
    ) -> fs_blake3::identity::AuthorityRef<
        snapshot_v2::SnapshotAuthoritySubjectIdV2,
        SnapshotVerifierSchema,
        SnapshotPolicySchema,
        fs_blake3::identity::Admitted,
    > {
        admitted_snapshot_authority_with(
            sealed,
            b"independent snapshot authority fixture",
            0xa1,
            0xb1,
        )
    }

    fn admitted_snapshot_authority_with(
        sealed: &snapshot_v2::SealedSnapshotV2,
        anchor: &[u8],
        verifier_tag: u8,
        policy_tag: u8,
    ) -> fs_blake3::identity::AuthorityRef<
        snapshot_v2::SnapshotAuthoritySubjectIdV2,
        SnapshotVerifierSchema,
        SnapshotPolicySchema,
        fs_blake3::identity::Admitted,
    > {
        presented_snapshot_authority_with(sealed, anchor, verifier_tag, policy_tag)
            .verify(&PermitAllSnapshotPolicyFixture)
            .expect("fixture verifier accepts")
            .admit(&PermitAllSnapshotPolicyFixture)
            .expect("fixture policy admits")
    }

    fn presented_snapshot_authority_with(
        sealed: &snapshot_v2::SealedSnapshotV2,
        anchor: &[u8],
        verifier_tag: u8,
        policy_tag: u8,
    ) -> fs_blake3::identity::AuthorityRef<
        snapshot_v2::SnapshotAuthoritySubjectIdV2,
        SnapshotVerifierSchema,
        SnapshotPolicySchema,
        fs_blake3::identity::Presented,
    > {
        use fs_blake3::identity::{
            AuthorityRef, CanonicalEncoder, ExternalAnchorRef, Field, KeyPolicyId, Presented,
            VerifierId,
        };

        let limits = v2_limits(64, 64).identity();
        let verifier =
            CanonicalEncoder::<VerifierId<SnapshotVerifierSchema>, _>::new(limits, || false)
                .expect("verifier schema")
                .bytes(Field::new(0, "configuration"), &[verifier_tag; 32])
                .expect("verifier configuration")
                .finish()
                .expect("verifier identity")
                .id();
        let policy =
            CanonicalEncoder::<KeyPolicyId<SnapshotPolicySchema>, _>::new(limits, || false)
                .expect("policy schema")
                .bytes(Field::new(0, "configuration"), &[policy_tag; 32])
                .expect("policy configuration")
                .finish()
                .expect("policy identity")
                .id();
        AuthorityRef::<_, SnapshotVerifierSchema, SnapshotPolicySchema, Presented>::present(
            sealed.authority_subject_receipt(),
            ExternalAnchorRef::presented(fs_blake3::identity::ContentId::of_bytes(anchor)),
            verifier,
            policy,
        )
    }

    impl ResumableSolverV1 for Jacobi {
        type State = JacobiState;
        type Out = (Vec<f64>, u64);

        fn step_v1(&self, state: &mut JacobiState, _cx: &Cx<'_>) -> StepVerdict<(Vec<f64>, u64)> {
            let n = state.x.len();
            let mut next = vec![0.0f64; n];
            let mut residual = 0.0f64;
            for (i, slot) in next.iter_mut().enumerate() {
                let left = if i > 0 { state.x[i - 1] } else { 0.0 };
                let right = if i + 1 < n { state.x[i + 1] } else { 0.0 };
                *slot = state.x[i] + 0.6 * ((self.rhs[i] - left - right) / 4.0 - state.x[i]);
                residual = residual.max((*slot - state.x[i]).abs());
            }
            state.x = next;
            state.iter += 1;
            if residual < self.tol {
                StepVerdict::Done((state.x.clone(), state.iter))
            } else {
                StepVerdict::Continue
            }
        }
    }

    fn jacobi() -> (Jacobi, JacobiState) {
        let rhs: Vec<f64> = (0..32).map(|i| 1.0 + 0.25 * f64::from(i % 5)).collect();
        (
            Jacobi { rhs, tol: 1e-12 },
            JacobiState {
                x: vec![0.0; 32],
                iter: 0,
            },
        )
    }

    fn with_cx<R>(gate: &CancelGate, f: impl FnOnce(&Cx<'_>) -> R) -> R {
        let pool = fs_alloc::ArenaPool::new(fs_alloc::ArenaConfig::default());
        pool.scope(|arena| {
            let cx = Cx::new(
                gate,
                arena,
                StreamKey {
                    seed: 1,
                    kernel_id: 1,
                    tile: 0,
                    iteration: 0,
                },
                Budget::INFINITE,
                ExecMode::Deterministic,
            );
            f(&cx)
        })
    }

    #[test]
    fn codec_round_trips_are_bit_exact_and_reject_garbage() {
        let mut enc = codec::Enc::new();
        enc.put_u64(42);
        enc.put_f64(f64::NAN);
        enc.put_f64(-0.0);
        enc.put_f64_slice(&[1.5, f64::INFINITY, f64::MIN_POSITIVE]);
        let bytes = enc.into_bytes();
        let mut dec = codec::Dec::new(&bytes);
        assert_eq!(dec.get_u64().expect("u64"), 42);
        assert_eq!(
            dec.get_f64().expect("nan").to_bits(),
            f64::NAN.to_bits(),
            "NaN payload preserved"
        );
        assert_eq!(
            dec.get_f64().expect("neg zero").to_bits(),
            (-0.0f64).to_bits()
        );
        let v = dec.get_f64_vec().expect("slice");
        assert_eq!(v.len(), 3);
        assert!(dec.is_empty());
        // Truncation is a structured, teaching error.
        let err = codec::Dec::new(&bytes[..5])
            .get_u64()
            .expect_err("truncated");
        assert!(err.to_string().contains("truncated"), "{err}");
        let impossible_len = u64::MAX.to_le_bytes();
        let err = codec::Dec::new(&impossible_len)
            .get_f64_vec()
            .expect_err("wire lengths must fit usize and their byte extent");
        assert!(err.what.starts_with("f64 slice"), "{err}");
        #[cfg(target_pointer_width = "32")]
        assert_eq!(
            err.what, "f64 slice length exceeds platform usize",
            "32-bit readers must not truncate the u64 wire length"
        );
        // Trailing garbage is rejected by from_bytes.
        let (_, s0) = jacobi();
        let mut noisy = LegacySnapshotV1Adapter::<JacobiState>::to_bytes(&s0);
        noisy.push(0xFF);
        assert!(LegacySnapshotV1Adapter::<JacobiState>::from_bytes(&noisy).is_err());

        let mut encoder = codec::Enc::new();
        s0.encode_v1(&mut encoder);
        let mut payload = encoder.into_bytes();
        let decoded_len = payload.len();
        payload.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        let sealed_with_schema_tail = envelope::seal(
            JacobiState::TYPE_ID_V1,
            JacobiState::SCHEMA_VERSION_V1,
            0,
            &payload,
        );
        let Err(SnapshotError::Payload(tail)) =
            LegacySnapshotV1Adapter::<JacobiState>::from_bytes(&sealed_with_schema_tail)
        else {
            panic!("checksummed trailing schema bytes must reach the payload refusal");
        };
        assert_eq!(tail.at, decoded_len);
        assert_eq!(tail.remaining, 3);
    }

    #[test]
    fn legacy_v1_receipt_quarantines_exact_bytes_without_u64_widening() {
        let (_, state) = jacobi();
        let bytes = LegacySnapshotV1Adapter::<JacobiState>::seal(&state, 0xCAFE);
        let opened =
            LegacySnapshotV1Adapter::<JacobiState>::open(&bytes).expect("valid legacy adapter");
        let (decoded, legacy) = opened.into_parts();
        assert_eq!(decoded, state);
        assert_eq!(legacy.bytes(), bytes);
        assert_eq!(legacy.info().provenance(), 0xCAFE);
        assert_eq!(
            legacy.exact_bytes_id(),
            fs_blake3::identity::ContentId::of_bytes(&bytes)
        );
        assert_eq!(
            legacy.payload_checksum(),
            u64::from_le_bytes(bytes[40..48].try_into().expect("v1 checksum"))
        );
        assert_eq!(
            legacy.payload_checksum(),
            fs_obs::fnv1a64(&bytes[envelope::HEADER_LEN..])
        );
    }

    #[test]
    fn v2_drain_binding_is_reproducible_structure_not_session_or_freeze_authority() {
        let first = paused_boundary(0x66, 9, 17, 2);
        let independently_reproduced = paused_boundary(0x66, 9, 17, 2);
        assert_eq!(first.declaration(), independently_reproduced.declaration());
        assert_eq!(first, independently_reproduced);
    }

    #[test]
    fn v2_expected_and_policy_authority_paths_keep_identity_and_evidence_attached() {
        let (_, state) = jacobi();
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(64, 64);
        let sealed = state.seal_v2(&context, limits, || false).expect("v2 seal");

        let unanchored = snapshot_v2::inspect(sealed.bytes(), limits, || false)
            .expect("self-consistent unanchored inspection");
        assert_eq!(
            unanchored.admission(),
            snapshot_v2::SnapshotAdmissionV2::UnanchoredConsistencyOnly
        );
        assert_eq!(unanchored.content_id(), sealed.content_id());
        assert_eq!(unanchored.resume_id(), sealed.resume_id());
        assert_eq!(unanchored.resume_receipt().id(), sealed.resume_id());
        assert_eq!(
            unanchored.authority_subject_receipt().id(),
            sealed.authority_subject_receipt().id()
        );
        assert_eq!(unanchored.context(), context.context());

        let expected = sealed.expectation();
        let opened = JacobiState::unseal_v2_expected(sealed.bytes(), &expected, limits, || false)
            .expect("exact retained roots authorize decoding");
        assert_eq!(opened.state(), &state);
        assert_eq!(opened.content_id(), sealed.content_id());
        assert_eq!(opened.resume_id(), sealed.resume_id());
        assert_eq!(opened.context(), context.context());
        assert_eq!(opened.expected_context(), &context);
        assert_eq!(
            opened.admission(),
            snapshot_v2::SnapshotAdmissionV2::MatchedCallerExpectation
        );
        assert!(opened.authority_evidence().is_none());

        let authority = admitted_snapshot_authority(&sealed);
        let authorized =
            JacobiState::unseal_v2_authorized(sealed.bytes(), &authority, &context, limits, || {
                false
            })
            .expect("policy-admitted exact composite subject authorizes decoding");
        assert_eq!(authorized.state(), &state);
        assert_eq!(
            authorized.admission(),
            snapshot_v2::SnapshotAdmissionV2::PolicyRelativeAdmission
        );
        let evidence = authorized
            .authority_evidence()
            .expect("authority audit evidence survives decode");
        assert_eq!(
            evidence.no_claim(),
            fs_blake3::identity::NoClaimState::ScientificCorrectnessNotProven
        );

        assert!(matches!(
            TwinV2State::unseal_v2_expected(sealed.bytes(), &expected, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::WrongStateSchema)
        ));

        let wrong_context = v2_context::<JacobiState>(
            0x11,
            7,
            0x23,
            0x33,
            snapshot_v2::SnapshotDeterminismV2::Deterministic,
            0x3f,
            0x44,
            0x55,
            paused_boundary(0x66, 9, 17, 2),
        );
        assert!(matches!(
            JacobiState::unseal_v2_authorized(
                sealed.bytes(),
                &authority,
                &wrong_context,
                limits,
                || false,
            ),
            Err(snapshot_v2::SnapshotV2Error::ExpectedContextMismatch { .. })
        ));

        let unrelated = snapshot_v2::seal(
            b"unrelated authority subject",
            &wrong_context,
            limits,
            || false,
        )
        .expect("unrelated subject seals");
        let unrelated_authority = admitted_snapshot_authority(&unrelated);
        assert!(matches!(
            JacobiState::unseal_v2_authorized(
                sealed.bytes(),
                &unrelated_authority,
                &wrong_context,
                limits,
                || false,
            ),
            Err(snapshot_v2::SnapshotV2Error::AuthoritySubjectMismatch)
        ));
    }

    #[test]
    fn v2_authority_subject_binds_content_and_resume_axes() {
        let limits = v2_limits(64, 64);
        let baseline_context = base_v2_context::<JacobiState>();
        let changed_context = v2_context::<JacobiState>(
            0x12,
            7,
            0x22,
            0x33,
            snapshot_v2::SnapshotDeterminismV2::Deterministic,
            0x3f,
            0x44,
            0x55,
            paused_boundary(0x66, 9, 17, 2),
        );
        let baseline =
            snapshot_v2::seal(b"authority-axis payload", &baseline_context, limits, || {
                false
            })
            .expect("baseline seals");
        let changed_resume =
            snapshot_v2::seal(b"authority-axis payload", &changed_context, limits, || {
                false
            })
            .expect("changed resume semantics seal");

        let content_a = snapshot_v2::SnapshotContentIdV2::parse_slice(&[0xa1; 32])
            .expect("fixed-width test content root");
        let content_b = snapshot_v2::SnapshotContentIdV2::parse_slice(&[0xb2; 32])
            .expect("fixed-width test content root");
        let mut never_cancel = || false;
        let baseline_subject = snapshot_v2::authority_subject_receipt(
            content_a,
            baseline.resume_id(),
            limits.identity(),
            &mut never_cancel,
        )
        .expect("baseline authority subject");
        let mut never_cancel = || false;
        let content_changed_subject = snapshot_v2::authority_subject_receipt(
            content_b,
            baseline.resume_id(),
            limits.identity(),
            &mut never_cancel,
        )
        .expect("content-axis authority subject");
        let mut never_cancel = || false;
        let resume_changed_subject = snapshot_v2::authority_subject_receipt(
            content_a,
            changed_resume.resume_id(),
            limits.identity(),
            &mut never_cancel,
        )
        .expect("resume-axis authority subject");

        assert_ne!(baseline_subject.id(), content_changed_subject.id());
        assert_ne!(baseline_subject.id(), resume_changed_subject.id());
        assert_ne!(baseline.resume_id(), changed_resume.resume_id());

        // Minting the opaque caller expectation is an admission operation; it
        // does not feed back into either canonical identity.
        let expectation = baseline.expectation();
        assert_eq!(expectation.content_id(), baseline.content_id());
        assert_eq!(expectation.resume_id(), baseline.resume_id());
        assert_eq!(
            baseline.authority_subject_receipt().id(),
            snapshot_v2::inspect(baseline.bytes(), limits, || false)
                .expect("inspection recomputes the same subject")
                .authority_subject_id()
        );
    }

    #[test]
    fn v2_authority_metadata_does_not_move_subject_identity() {
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(64, 64);
        let sealed = snapshot_v2::seal(b"metadata-invariant payload", &context, limits, || false)
            .expect("snapshot seals");

        let presented =
            presented_snapshot_authority_with(&sealed, b"first independent anchor", 0x11, 0x21);
        assert_eq!(
            presented.receipt().id(),
            sealed.authority_subject_receipt().id()
        );
        let presented_audit = presented.audit_record();
        let verified = presented
            .verify(&PermitAllSnapshotPolicyFixture)
            .expect("fixture verifies");
        assert_eq!(
            verified.receipt().id(),
            sealed.authority_subject_receipt().id()
        );
        let admitted = verified
            .admit(&PermitAllSnapshotPolicyFixture)
            .expect("fixture admits");
        assert_eq!(
            admitted.receipt().id(),
            sealed.authority_subject_receipt().id()
        );
        assert_ne!(presented_audit.trust(), admitted.audit_record().trust());

        let other =
            admitted_snapshot_authority_with(&sealed, b"second independent anchor", 0x12, 0x22);
        assert_eq!(admitted.receipt().id(), other.receipt().id());
        let admitted_audit = admitted.audit_record();
        let other_audit = other.audit_record();
        assert_ne!(admitted_audit.anchor(), other_audit.anchor());
        assert_ne!(admitted_audit.verifier(), other_audit.verifier());
        assert_ne!(admitted_audit.key_policy(), other_audit.key_policy());
    }

    #[test]
    fn v2_drain_report_domain_version_and_wire_era_are_pinned() {
        fn era_for(version: u32, domain_name: &str, wire_grammar: [u8; 9]) -> [u8; 32] {
            let domain = fs_blake3::identity::ContentId::of_bytes(domain_name.as_bytes());
            let mut preimage = [0_u8; 45];
            preimage[..4].copy_from_slice(&version.to_le_bytes());
            preimage[4..36].copy_from_slice(domain.as_bytes());
            preimage[36..45].copy_from_slice(&wire_grammar);
            *fs_blake3::hash_domain(snapshot_v2::SNAPSHOT_DRAIN_REPORT_ERA_DOMAIN_V2, &preimage)
                .as_bytes()
        }

        let version = crate::cx::DRAIN_FINALIZE_REPORT_IDENTITY_VERSION;
        let domain = crate::cx::DRAIN_FINALIZE_REPORT_IDENTITY_DOMAIN;
        let grammar = [1, 4, 8, 8, 8, 0, 1, 2, 3];
        let current = snapshot_v2::current_drain_report_era();
        assert_eq!(*current.as_bytes(), era_for(version, domain, grammar));
        let version_changed_era = era_for(version.wrapping_add(1), domain, grammar);
        let domain_changed_era =
            era_for(version, "org.frankensim.tests.stale-drain-domain", grammar);
        let grammar_changed_era = era_for(version, domain, [1, 4, 8, 8, 8, 0, 1, 3, 2]);
        assert_ne!(*current.as_bytes(), version_changed_era);
        assert_ne!(*current.as_bytes(), domain_changed_era);
        assert_ne!(*current.as_bytes(), grammar_changed_era);

        let payload = b"drain-era identity payload";
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(64, 64);
        let sealed = snapshot_v2::seal(payload, &context, limits, || false)
            .expect("current drain era seals");
        let inspection = snapshot_v2::inspect(sealed.bytes(), limits, || false)
            .expect("current drain era inspects");
        let payload_len = u64::try_from(payload.len()).expect("small fixture length");

        let mut never_cancel = || false;
        let reconstructed = snapshot_v2::test_resume_receipt_with_drain_identity(
            context.context(),
            inspection.payload_content_id(),
            payload_len,
            version,
            current,
            limits.identity(),
            &mut never_cancel,
        )
        .expect("current components reconstruct the sealed resume receipt");
        assert_eq!(reconstructed.id(), sealed.resume_id());

        let mut never_cancel = || false;
        let version_changed = snapshot_v2::test_resume_receipt_with_drain_identity(
            context.context(),
            inspection.payload_content_id(),
            payload_len,
            version.wrapping_add(1),
            current,
            limits.identity(),
            &mut never_cancel,
        )
        .expect("alternate report version has a well-formed distinct identity");
        assert_ne!(version_changed.id(), sealed.resume_id());

        let mut never_cancel = || false;
        let era_changed = snapshot_v2::test_resume_receipt_with_drain_identity(
            context.context(),
            inspection.payload_content_id(),
            payload_len,
            version,
            snapshot_v2::SnapshotDrainReportEraIdV2::from_bytes(grammar_changed_era),
            limits.identity(),
            &mut never_cancel,
        )
        .expect("alternate report era has a well-formed distinct identity");
        assert_ne!(era_changed.id(), sealed.resume_id());
    }

    #[test]
    fn v2_canonical_frame_and_identity_eras_fail_closed_before_payload() {
        const OFFSET_CANONICAL_FRAME_VERSION: usize = 224;
        const OFFSET_RESUME_SCHEMA_ID: usize = 228;
        const OFFSET_AUTHORITY_SCHEMA_ID: usize = 260;
        const OFFSET_DRAIN_REPORT_ERA: usize = 292;

        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(64, 64);
        let sealed = snapshot_v2::seal(b"era-preflight payload", &context, limits, || false)
            .expect("current eras seal");

        let mut stale_frame = sealed.bytes().to_vec();
        stale_frame[OFFSET_CANONICAL_FRAME_VERSION..OFFSET_CANONICAL_FRAME_VERSION + 4]
            .copy_from_slice(
                &fs_blake3::identity::CANONICAL_FRAME_VERSION
                    .wrapping_add(1)
                    .to_le_bytes(),
            );
        assert!(matches!(
            snapshot_v2::inspect(&stale_frame, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedCanonicalFrameVersion { .. })
        ));

        let mut stale_resume_schema = sealed.bytes().to_vec();
        stale_resume_schema[OFFSET_RESUME_SCHEMA_ID] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&stale_resume_schema, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedResumeSchemaEra { .. })
        ));

        let mut stale_authority_schema = sealed.bytes().to_vec();
        stale_authority_schema[OFFSET_AUTHORITY_SCHEMA_ID] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&stale_authority_schema, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedAuthoritySchemaEra { .. })
        ));

        let mut stale_drain_era = sealed.bytes().to_vec();
        stale_drain_era[OFFSET_DRAIN_REPORT_ERA] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&stale_drain_era, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedDrainReportEra { .. })
        ));
    }

    #[test]
    fn v2_owned_artifact_debug_and_consumption_keep_evidence_bounded() {
        struct DeliberatelyNoDebugState;
        fn assert_debug<T: core::fmt::Debug>() {}
        assert_debug::<snapshot_v2::OpenedSnapshotV2<DeliberatelyNoDebugState>>();

        let assert_bounded_debug = |label: &str, rendered: &str| {
            assert!(
                rendered.len() < 512,
                "{label} Debug output is {} bytes, expected fewer than 512: {rendered}",
                rendered.len()
            );
            assert!(
                !rendered.contains("x:"),
                "{label} Debug output leaked decoded state fields: {rendered}"
            );
        };
        let (_, state) = jacobi();
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(64, 64);
        let sealed = state
            .seal_v2(&context, limits, || false)
            .expect("state seals");
        let sealed_debug = format!("{sealed:?}");
        assert_bounded_debug("sealed snapshot", &sealed_debug);
        let expected_content = sealed.content_id();
        let expected_resume = sealed.resume_id();
        let expected_subject = sealed.authority_subject_receipt().id();
        let authority = admitted_snapshot_authority(&sealed);

        let (bytes, seal_evidence) = sealed.into_parts();
        assert_eq!(seal_evidence.content_id(), expected_content);
        assert_eq!(seal_evidence.resume_id(), expected_resume);
        assert_eq!(seal_evidence.resume_receipt().id(), expected_resume);
        assert_eq!(seal_evidence.authority_subject_id(), expected_subject);
        assert_eq!(
            seal_evidence.authority_subject_receipt().id(),
            expected_subject
        );
        assert_eq!(seal_evidence.expected_context(), &context);
        let seal_evidence_debug = format!("{seal_evidence:?}");
        assert_bounded_debug("discharged seal evidence", &seal_evidence_debug);
        let expectation = seal_evidence.expectation();
        let inspection = snapshot_v2::inspect_expected(&bytes, &expectation, limits, || false)
            .expect("retained seal evidence inspects exact bytes");
        let inspection_debug = format!("{inspection:?}");
        assert_bounded_debug("expected-root inspection", &inspection_debug);
        assert!(inspection_debug.contains("authority_evidence_present: false"));
        let opened = JacobiState::unseal_v2_expected(&bytes, &expectation, limits, || false)
            .expect("retained seal evidence opens exact bytes");
        let opened_debug = format!("{opened:?}");
        assert_bounded_debug("expected-root opened snapshot", &opened_debug);
        assert!(opened_debug.contains("authority_evidence_present: false"));
        assert!(!opened_debug.contains(core::any::type_name::<JacobiState>()));
        let authorized =
            JacobiState::unseal_v2_authorized(&bytes, &authority, &context, limits, || false)
                .expect("policy-relative authority opens exact bytes");
        let authorized_debug = format!("{authorized:?}");
        assert_bounded_debug("policy-authorized opened snapshot", &authorized_debug);
        assert!(authorized_debug.contains("authority_evidence_present: true"));
        assert!(!authorized_debug.contains(authority.audit_record().context()));
        let (authorized_state, authorized_evidence) = authorized.into_parts();
        assert_eq!(authorized_state, state);
        let authorized_evidence_debug = format!("{authorized_evidence:?}");
        assert_bounded_debug(
            "policy-authorized discharged evidence",
            &authorized_evidence_debug,
        );
        assert!(authorized_evidence_debug.contains("authority_evidence_present: true"));
        let opened_expectation = opened.expectation();
        let (decoded, open_evidence) = opened.into_parts();
        assert_eq!(decoded, state);
        let open_evidence_debug = format!("{open_evidence:?}");
        assert_bounded_debug("expected-root discharged evidence", &open_evidence_debug);
        assert!(open_evidence_debug.contains("authority_evidence_present: false"));
        assert_eq!(open_evidence.content_id(), expected_content);
        assert_eq!(open_evidence.resume_id(), expected_resume);
        assert_eq!(open_evidence.resume_receipt().id(), expected_resume);
        assert_eq!(open_evidence.authority_subject_id(), expected_subject);
        assert_eq!(
            open_evidence.authority_subject_receipt().id(),
            expected_subject
        );
        let evidence_expectation = open_evidence.expectation();
        let reopened_from_attached =
            JacobiState::unseal_v2_expected(&bytes, &opened_expectation, limits, || false)
                .expect("attached opened evidence supports exact expected reopen");
        assert_eq!(reopened_from_attached.state(), &state);
        let reopened_from_discharged =
            JacobiState::unseal_v2_expected(&bytes, &evidence_expectation, limits, || false)
                .expect("discharged open evidence supports exact expected reopen");
        assert_eq!(reopened_from_discharged.state(), &state);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn v2_every_resume_semantic_moves_identity() {
        let payload = b"same exact solver payload";
        let limits = v2_limits(64, 64);
        let baseline_context = base_v2_context::<JacobiState>();
        let baseline = snapshot_v2::seal(payload, &baseline_context, limits, || false)
            .expect("baseline v2 envelope");

        let contexts = [
            base_v2_context::<TwinV2State>(),
            base_v2_context::<SchemaOnlyV2State>(),
            base_v2_context::<CodecOnlyV2State>(),
            base_v2_context::<CodecBumpV2State>(),
            v2_context::<JacobiState>(
                0x12,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                8,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x23,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x34,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Fast,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x40,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x45,
                0x55,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x56,
                paused_boundary(0x66, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x67, 9, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 10, 17, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 18, 2),
            ),
            v2_context::<JacobiState>(
                0x11,
                7,
                0x22,
                0x33,
                snapshot_v2::SnapshotDeterminismV2::Deterministic,
                0x3f,
                0x44,
                0x55,
                paused_boundary(0x66, 9, 17, 3),
            ),
        ];
        for (index, context) in contexts.into_iter().enumerate() {
            let changed = snapshot_v2::seal(payload, &context, limits, || false)
                .expect("semantic variant seals");
            assert_ne!(
                changed.resume_id(),
                baseline.resume_id(),
                "semantic context field {index} did not move resume identity"
            );
            assert_ne!(
                changed.content_id(),
                baseline.content_id(),
                "semantic context field {index} did not move exact envelope identity"
            );
            assert_ne!(
                changed.authority_subject_receipt().id(),
                baseline.authority_subject_receipt().id(),
                "semantic context field {index} did not move authority subject identity"
            );
        }

        let same_length_payload_change = snapshot_v2::seal(
            b"same exact solver payloae",
            &baseline_context,
            limits,
            || false,
        )
        .expect("same-length payload mutation");
        assert_ne!(same_length_payload_change.resume_id(), baseline.resume_id());
        assert_ne!(
            same_length_payload_change.authority_subject_receipt().id(),
            baseline.authority_subject_receipt().id()
        );
        let length_change = snapshot_v2::seal(
            b"same exact solver payload+",
            &baseline_context,
            limits,
            || false,
        )
        .expect("payload-length mutation");
        assert_ne!(length_change.resume_id(), baseline.resume_id());
        assert_ne!(
            length_change.authority_subject_receipt().id(),
            baseline.authority_subject_receipt().id()
        );
    }

    #[test]
    fn v2_each_resume_source_field_moves_identity_independently() {
        use snapshot_v2::SnapshotResumeTestMutationV2 as Mutation;

        let payload = b"independent resume-source sensitivity";
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(64, 64);
        let sealed = snapshot_v2::seal(payload, &context, limits, || false)
            .expect("baseline source components seal");
        let inspection = snapshot_v2::inspect(sealed.bytes(), limits, || false)
            .expect("baseline source components inspect");
        let payload_len = u64::try_from(payload.len()).expect("small fixture length");
        let mutations = [
            Mutation::StateType,
            Mutation::StateSchema,
            Mutation::StateCodec,
            Mutation::StateCodecVersion,
            Mutation::Algorithm,
            Mutation::AlgorithmVersion,
            Mutation::Problem,
            Mutation::RngCounter,
            Mutation::Determinism,
            Mutation::ExecutionFingerprint,
            Mutation::Budget,
            Mutation::Provenance,
            Mutation::PauseRequest,
            Mutation::GateGeneration,
            Mutation::DrainReportVersion,
            Mutation::DrainReportEra,
            Mutation::DrainRun,
            Mutation::DrainRegistered,
            Mutation::DrainDrained,
            Mutation::DrainReport,
            Mutation::PayloadContent,
            Mutation::PayloadLength,
        ];

        for mutation in mutations {
            let mut never_cancel = || false;
            let changed = snapshot_v2::test_resume_receipt_with_mutation(
                context.context(),
                inspection.payload_content_id(),
                payload_len,
                mutation,
                limits.identity(),
                &mut never_cancel,
            )
            .expect("single-field canonical source mutation remains encodable");
            assert_ne!(
                changed.id(),
                sealed.resume_id(),
                "{mutation:?} did not move the typed resume identity"
            );
        }
    }

    #[test]
    fn v2_nonsemantic_limits_do_not_move_roots() {
        let payload = b"limit-invariant payload";
        let context = base_v2_context::<JacobiState>();
        let first = snapshot_v2::seal(payload, &context, v2_limits(8, 8), || false)
            .expect("fine-grained schedule");
        let looser = snapshot_v2::SnapshotLimitsV2::new(
            2 << 20,
            4096,
            fs_blake3::identity::CanonicalLimits::new(32_768, 8_192, 64, 64, 4096),
            8_192,
            2 << 20,
            4096,
        );
        let second = snapshot_v2::seal(payload, &context, looser, || false)
            .expect("coarse-grained looser budget");
        assert_eq!(first.bytes(), second.bytes());
        assert_eq!(first.content_id(), second.content_id());
        assert_eq!(first.resume_id(), second.resume_id());
        assert_eq!(
            first.authority_subject_receipt().id(),
            second.authority_subject_receipt().id()
        );
    }

    #[test]
    fn v2_expected_inspection_borrows_bytes_not_expectation() {
        let payload = b"expectation-lifetime-independent payload";
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(8, 8);
        let sealed =
            snapshot_v2::seal(payload, &context, limits, || false).expect("lifetime fixture seals");

        let inspection = {
            let expectation = sealed.expectation();
            snapshot_v2::inspect_expected(sealed.bytes(), &expectation, limits, || false)
                .expect("block-local expectation admits the snapshot")
        };

        assert_eq!(inspection.payload(), payload);
        assert_eq!(
            inspection.admission(),
            snapshot_v2::SnapshotAdmissionV2::MatchedCallerExpectation
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn v2_refuses_corruption_downgrade_and_hostile_lengths_before_decode() {
        const OFFSET_HEADER_LEN: usize = 12;
        const OFFSET_PROBLEM: usize = 156;
        const OFFSET_DETERMINISM: usize = 220;
        const OFFSET_LIFECYCLE: usize = 221;
        const OFFSET_RESERVED: usize = 222;
        const OFFSET_CANONICAL_FRAME_VERSION: usize = 224;
        const OFFSET_RESUME_SCHEMA_ID: usize = 228;
        const OFFSET_AUTHORITY_SCHEMA_ID: usize = 260;
        const OFFSET_DRAIN_REPORT_ERA: usize = 292;
        const OFFSET_DRAIN_REPORT: usize = 484;
        const OFFSET_PAYLOAD_LEN: usize = 516;

        let (_, state) = jacobi();
        let context = base_v2_context::<JacobiState>();
        let limits = v2_limits(8, 8);
        let sealed = state
            .seal_v2(&context, limits, || false)
            .expect("baseline v2");
        let expectation = sealed.expectation();
        let header: &[u8; snapshot_v2::HEADER_LEN_V2] = sealed.bytes()
            [..snapshot_v2::HEADER_LEN_V2]
            .try_into()
            .expect("exact fixed header");
        let plan = snapshot_v2::preflight_header(header, limits).expect("header preflight");
        assert_eq!(
            plan.payload_len(),
            u64::try_from(sealed.bytes().len() - snapshot_v2::HEADER_LEN_V2)
                .expect("fixture extent")
        );
        assert_eq!(plan.total_len(), sealed.bytes().len());

        let mut flipped_payload = sealed.bytes().to_vec();
        let last = flipped_payload.len() - 1;
        flipped_payload[last] ^= 0x80;
        assert!(matches!(
            snapshot_v2::inspect(&flipped_payload, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::PayloadContentMismatch { .. })
        ));

        let mut changed_problem = sealed.bytes().to_vec();
        changed_problem[OFFSET_PROBLEM] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&changed_problem, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::ResumeIdentityMismatch { .. })
        ));

        let mut bad_header_len = sealed.bytes().to_vec();
        bad_header_len[OFFSET_HEADER_LEN..OFFSET_HEADER_LEN + 4]
            .copy_from_slice(&487_u32.to_le_bytes());
        assert!(matches!(
            snapshot_v2::inspect(&bad_header_len, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::InvalidHeaderLength { declared: 487 })
        ));

        let mut bad_mode = sealed.bytes().to_vec();
        bad_mode[OFFSET_DETERMINISM] = 0xff;
        assert!(matches!(
            snapshot_v2::inspect(&bad_mode, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::InvalidDeterminismTag { found: 0xff })
        ));

        let mut stale_frame = sealed.bytes().to_vec();
        stale_frame[OFFSET_CANONICAL_FRAME_VERSION..OFFSET_CANONICAL_FRAME_VERSION + 4]
            .copy_from_slice(&(fs_blake3::identity::CANONICAL_FRAME_VERSION + 1).to_le_bytes());
        assert!(matches!(
            snapshot_v2::inspect(&stale_frame, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedCanonicalFrameVersion { .. })
        ));

        let mut stale_resume_schema = sealed.bytes().to_vec();
        stale_resume_schema[OFFSET_RESUME_SCHEMA_ID] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&stale_resume_schema, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedResumeSchemaEra { .. })
        ));

        let mut stale_authority_schema = sealed.bytes().to_vec();
        stale_authority_schema[OFFSET_AUTHORITY_SCHEMA_ID] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&stale_authority_schema, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedAuthoritySchemaEra { .. })
        ));

        let mut stale_drain_era = sealed.bytes().to_vec();
        stale_drain_era[OFFSET_DRAIN_REPORT_ERA] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&stale_drain_era, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::UnsupportedDrainReportEra { .. })
        ));

        let mut bad_lifecycle = sealed.bytes().to_vec();
        bad_lifecycle[OFFSET_LIFECYCLE] = 2;
        assert!(matches!(
            snapshot_v2::inspect(&bad_lifecycle, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::InvalidLifecycleTag { found: 2 })
        ));

        let mut forged_drain_report = sealed.bytes().to_vec();
        forged_drain_report[OFFSET_DRAIN_REPORT] ^= 1;
        assert!(matches!(
            snapshot_v2::inspect(&forged_drain_report, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::DrainReportMismatch { .. })
        ));

        let mut future_header = sealed.bytes().to_vec();
        future_header[OFFSET_RESERVED] = 1;
        assert!(matches!(
            snapshot_v2::inspect(&future_header, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::NonzeroReservedHeader)
        ));

        let mut hostile_len = sealed.bytes().to_vec();
        hostile_len[OFFSET_PAYLOAD_LEN..OFFSET_PAYLOAD_LEN + 8]
            .copy_from_slice(&(limits.max_payload_bytes() + 1).to_le_bytes());
        assert!(matches!(
            snapshot_v2::inspect(&hostile_len, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::PayloadLimitExceeded { .. })
        ));

        assert!(matches!(
            snapshot_v2::inspect(
                &sealed.bytes()[..snapshot_v2::HEADER_LEN_V2 - 1],
                limits,
                || false,
            ),
            Err(snapshot_v2::SnapshotV2Error::Truncated { .. })
        ));
        let mut appended = sealed.bytes().to_vec();
        appended.push(0);
        assert!(matches!(
            snapshot_v2::inspect(&appended, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::LengthMismatch { .. })
        ));

        let legacy = LegacySnapshotV1Adapter::<JacobiState>::seal(&state, 7);
        assert!(matches!(
            snapshot_v2::inspect(&legacy, limits, || false),
            Err(snapshot_v2::SnapshotV2Error::BadMagic)
        ));
        assert!(matches!(
            envelope::inspect(sealed.bytes()),
            Err(envelope::EnvelopeError::BadMagic)
        ));
        let mut version_only_downgrade = sealed.bytes().to_vec();
        version_only_downgrade[8..12].copy_from_slice(&1_u32.to_le_bytes());
        assert!(matches!(
            envelope::inspect(&version_only_downgrade),
            Err(envelope::EnvelopeError::BadMagic)
        ));

        let mut changed_state = state.clone();
        changed_state.iter += 1;
        let recomputed = changed_state
            .seal_v2(&context, limits, || false)
            .expect("attacker can recompute unkeyed roots for different bytes");
        assert_eq!(
            snapshot_v2::inspect(recomputed.bytes(), limits, || false)
                .expect("recomputed bytes are internally consistent")
                .admission(),
            snapshot_v2::SnapshotAdmissionV2::UnanchoredConsistencyOnly
        );
        assert!(matches!(
            JacobiState::unseal_v2_expected(recomputed.bytes(), &expectation, limits, || false,),
            Err(snapshot_v2::SnapshotV2Error::ExpectedContentMismatch { .. })
        ));

        let wrong_authority = admitted_snapshot_authority(&recomputed);
        assert!(matches!(
            JacobiState::unseal_v2_authorized(
                sealed.bytes(),
                &wrong_authority,
                &context,
                limits,
                || false,
            ),
            Err(snapshot_v2::SnapshotV2Error::AuthoritySubjectMismatch)
        ));

        let mut hostile_payload = Vec::new();
        hostile_payload.extend_from_slice(&0_u64.to_le_bytes());
        hostile_payload.extend_from_slice(&u64::MAX.to_le_bytes());
        let hostile_internal = snapshot_v2::seal(&hostile_payload, &context, limits, || false)
            .expect("bounded hostile payload can be enveloped without decoding");
        assert!(matches!(
            JacobiState::unseal_v2_expected(
                hostile_internal.bytes(),
                &hostile_internal.expectation(),
                limits,
                || false,
            ),
            Err(snapshot_v2::SnapshotV2Error::CodecResourceLimitExceeded {
                resource: "collection items",
                ..
            })
        ));

        #[derive(Debug)]
        struct CancelAfter {
            successful_polls: usize,
        }
        impl fs_blake3::identity::CancellationProbe for CancelAfter {
            fn is_cancelled(&mut self) -> bool {
                if self.successful_polls == 0 {
                    true
                } else {
                    self.successful_polls -= 1;
                    false
                }
            }
        }
        assert!(matches!(
            snapshot_v2::seal(
                &[0x5a; 128],
                &context,
                limits,
                CancelAfter {
                    successful_polls: 1,
                },
            ),
            Err(snapshot_v2::SnapshotV2Error::Cancelled {
                phase: "payload hashing",
                at: 8,
            })
        ));
        assert!(matches!(
            state.seal_v2(
                &context,
                limits,
                CancelAfter {
                    successful_polls: 1,
                },
            ),
            Err(snapshot_v2::SnapshotV2Error::Cancelled {
                phase: "payload encoding allocation",
                at: 0,
            })
        ));

        let mut inspect_polls = 0_usize;
        snapshot_v2::inspect_expected(sealed.bytes(), &expectation, limits, || {
            inspect_polls += 1;
            false
        })
        .expect("count non-cancelling inspection polls");
        assert!(matches!(
            JacobiState::unseal_v2_expected(
                sealed.bytes(),
                &expectation,
                limits,
                CancelAfter {
                    successful_polls: inspect_polls,
                },
            ),
            Err(snapshot_v2::SnapshotV2Error::Cancelled {
                phase: "payload decoding",
                at: 0,
            })
        ));

        let one_byte_cap = snapshot_v2::SnapshotLimitsV2::new(
            1,
            1,
            fs_blake3::identity::CanonicalLimits::new(16_384, 4_096, 32, 32, 1),
            4_096,
            1 << 20,
            8,
        );
        assert!(matches!(
            snapshot_v2::seal(b"ab", &context, one_byte_cap, || false),
            Err(snapshot_v2::SnapshotV2Error::PayloadLimitExceeded {
                declared: 2,
                limit: 1,
            })
        ));
        let zero_hash_poll = snapshot_v2::SnapshotLimitsV2::new(
            1 << 20,
            0,
            fs_blake3::identity::CanonicalLimits::new(16_384, 4_096, 32, 32, 1),
            4_096,
            1 << 20,
            8,
        );
        assert!(matches!(
            snapshot_v2::inspect(sealed.bytes(), zero_hash_poll, || false),
            Err(snapshot_v2::SnapshotV2Error::InvalidLimits(
                "hash_poll_bytes must be positive"
            ))
        ));
    }

    #[test]
    fn v2_codec_refusal_is_sticky_when_an_implementation_swallows_the_error() {
        #[derive(Debug)]
        struct CancelOnFourthPoll {
            polls: u8,
        }

        impl fs_blake3::identity::CancellationProbe for CancelOnFourthPoll {
            fn is_cancelled(&mut self) -> bool {
                self.polls = self.polls.saturating_add(1);
                self.polls >= 4
            }
        }

        let context = base_v2_context::<SwallowingCodecV2State>();
        let limits = v2_limits(8, 8);
        assert!(matches!(
            SwallowingCodecV2State.seal_v2(&context, limits, CancelOnFourthPoll { polls: 0 },),
            Err(snapshot_v2::SnapshotV2Error::CodecPoisoned {
                phase: "payload encoding",
                ..
            })
        ));

        let hostile_count = u64::MAX.to_le_bytes();
        let sealed = snapshot_v2::seal(&hostile_count, &context, limits, || false)
            .expect("outer envelope can carry a bounded hostile codec count");
        let expectation = sealed.expectation();
        assert!(matches!(
            SwallowingCodecV2State::unseal_v2_expected(
                sealed.bytes(),
                &expectation,
                limits,
                || false,
            ),
            Err(snapshot_v2::SnapshotV2Error::CodecPoisoned {
                phase: "payload decoding",
                ..
            })
        ));
    }

    #[test]
    fn v2_only_state_seals_opens_pauses_and_resumes_without_legacy_capability() {
        let state = StrongOnlyCounterState { step: 2 };
        let context = base_v2_context::<StrongOnlyCounterState>();
        let limits = v2_limits(64, 64);
        let sealed = state
            .seal_v2(&context, limits, || false)
            .expect("v2-only state seals");
        let expectation = sealed.expectation();
        let opened = StrongOnlyCounterState::unseal_v2_expected(
            sealed.bytes(),
            &expectation,
            limits,
            || false,
        )
        .expect("v2-only state opens with typed evidence");
        assert_eq!(opened.state(), &state);
        assert_eq!(opened.content_id(), sealed.content_id());
        assert_eq!(opened.resume_id(), sealed.resume_id());
        let (resumed_state, open_evidence) = opened.into_parts();
        assert_eq!(open_evidence.content_id(), sealed.content_id());

        let solver = StrongOnlyCounter { target: 4 };
        let cancelled_gate = CancelGate::new();
        cancelled_gate.request();
        let paused = with_cx(&cancelled_gate, |cx| drive_v2(&solver, resumed_state, cx));
        let SolverProgress::Paused(paused_state) = paused else {
            panic!("pre-requested v2 drive must pause");
        };
        assert_eq!(paused_state, state);

        let live_gate = CancelGate::new();
        let SolverProgress::Done(done) =
            with_cx(&live_gate, |cx| drive_v2(&solver, paused_state, cx))
        else {
            panic!("fresh v2 drive must resume to completion");
        };
        assert_eq!(done, 4);
    }

    #[test]
    fn pause_serialize_resume_is_bit_exact_versus_uninterrupted() {
        let (solver, s0) = jacobi();
        // Uninterrupted reference.
        let gate = CancelGate::new();
        let SolverProgress::Done((x_ref, iters_ref)) =
            with_cx(&gate, |cx| drive_v1(&solver, s0.clone(), cx))
        else {
            panic!("uninterrupted run must finish");
        };
        // Interrupted every step: advance ONE bounded step, then pause,
        // serialize, deserialize, resume — the maximally hostile schedule.
        let mut state = s0;
        let mut resumes = 0u64;
        let finished = loop {
            let g2 = CancelGate::new();
            let (st, verdict) = with_cx(&g2, |cx| {
                let mut st = state.clone();
                let verdict = solver.step_v1(&mut st, cx);
                (st, verdict)
            });
            match verdict {
                StepVerdict::Done(out) => break out,
                StepVerdict::Continue => {
                    let bytes = LegacySnapshotV1Adapter::<JacobiState>::to_bytes(&st);
                    state = LegacySnapshotV1Adapter::<JacobiState>::from_bytes(&bytes)
                        .expect("round trip");
                    resumes += 1;
                }
            }
        };
        assert_eq!(finished.1, iters_ref, "same iteration count");
        assert!(resumes > 10, "the trajectory must actually be interrupted");
        let bits_ref: Vec<u64> = x_ref.iter().map(|v| v.to_bits()).collect();
        let bits_paused: Vec<u64> = finished.0.iter().map(|v| v.to_bits()).collect();
        assert_eq!(bits_ref, bits_paused, "bit-exact continuation (G4 law)");
    }

    #[test]
    fn drive_pauses_on_cancel_and_resumes_to_the_same_answer() {
        let (solver, s0) = jacobi();
        let gate = CancelGate::new();
        let SolverProgress::Done((x_ref, _)) =
            with_cx(&gate, |cx| drive_v1(&solver, s0.clone(), cx))
        else {
            panic!("reference finishes");
        };
        // Cancel mid-flight: drive must return Paused with usable state.
        let paused_state = {
            let gate = CancelGate::new();
            gate.request();
            match with_cx(&gate, |cx| drive_v1(&solver, s0, cx)) {
                SolverProgress::Paused(s) => s,
                SolverProgress::Done(_) => panic!("pre-requested gate must pause"),
            }
        };
        let gate = CancelGate::new();
        let SolverProgress::Done((x_resumed, _)) =
            with_cx(&gate, |cx| drive_v1(&solver, paused_state, cx))
        else {
            panic!("resume finishes");
        };
        assert_eq!(
            x_ref.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            x_resumed.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn forks_are_independent_and_serialization_proven() {
        let (solver, s0) = jacobi();
        // Advance 10 steps.
        let gate = CancelGate::new();
        let mut warm = s0;
        with_cx(&gate, |cx| {
            for _ in 0..10 {
                let _ = solver.step_v1(&mut warm, cx);
            }
        });
        let fork_a = round_trip_legacy_v1(&warm).expect("v1 round trip proves serializability");
        let fork_b = round_trip_legacy_v1(&warm).expect("second v1 round trip");
        assert_eq!(
            LegacySnapshotV1Adapter::<JacobiState>::historical_content_hash(&fork_a),
            LegacySnapshotV1Adapter::<JacobiState>::historical_content_hash(&fork_b)
        );
        // Diverge: different subsequent inputs (different rhs) per fork.
        let solver_b = {
            let mut j = jacobi().0;
            j.rhs.iter_mut().for_each(|r| *r += 0.5);
            j
        };
        let SolverProgress::Done((xa, _)) = with_cx(&gate, |cx| drive_v1(&solver, fork_a, cx))
        else {
            panic!("fork A finishes");
        };
        let SolverProgress::Done((xb, _)) = with_cx(&gate, |cx| drive_v1(&solver_b, fork_b, cx))
        else {
            panic!("fork B finishes");
        };
        assert_ne!(
            xa[0].to_bits(),
            xb[0].to_bits(),
            "forks with different inputs stay independent"
        );
    }
}
