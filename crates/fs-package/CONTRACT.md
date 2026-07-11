# CONTRACT: fs-package

Machine-checkable evidence packages (plan addendum, Proposal 12): a
content-addressed bundle of color-typed claims a standalone checker can
re-verify without solvers.

## Purpose and layer

Layer L6. Depends on `fs-evidence` (UTIL — `Color`, `ColorRank`,
`ValidityDomain`), `fs-crosswalk` (the static standards vocabulary used by
coverage reports), and dependency-free `fs-blake3` (the shared content-hash
owner). The structural core performs no I/O and has no solver dependency.
Injected verifier implementations are caller-owned synchronous capabilities.

## Public types and semantics

- `Claim` — a sealed claim plus its epistemic color and typed `ClaimOrigin`.
  Construction is through `from_certificate`, `anchored`, `estimated`,
  `derived`, or `waived`; callers cannot assemble an origin-free claim from a
  public `Color`. Claim ids are canonical, non-placeholder, and unique within a
  package; statements are non-blank and cannot be reserved placeholder text.
  `has_matching_validated_anchor()` exposes the exact dataset-anchor predicate
  used by release admission, while `requires_release_falsifier()` identifies
  Verified/Validated certificate-class claims and
  `requires_validated_anchor()` identifies the stricter validated-only
  obligation without exposing a second color algebra.
- `Provenance { code_version, constellation_lock }`.
- `VerificationCapabilities` — explicit source-certificate and waiver
  verification capabilities. `deny_all()` is the default. A
  `SourceCertificateVerifier` receives a typed `SourceCertificateRequest`
  containing the exact claim, package provenance, index, producer, and parsed
  artifact hash. A `WaiverVerification` pairs a `WaiverVerifier` with an
  explicit Unix-day clock context.
- `EvidencePackage { format_version, claims, provenance, signature }` —
  builder: `new(prov).with_claim(..).signed(..)`.
  - `merkle_root() -> ContentHash` — a 32-byte BLAKE3 Merkle root over the
    package identity: format version, claim count, provenance, and ordered
    claims. Header, claim, and internal-node hashes use standard BLAKE3
    derive-key domains. Detached signatures are excluded. Any reproducibility
    provenance or claim change changes the root.
  - `verify()` uses `VerificationCapabilities::deny_all()`: anchored,
    estimated, and independently re-derived claims can pass; source-certificate
    and waiver origins cannot.
  - `verify_with(&capabilities)` performs structural verification and then
    authenticates every capability-gated origin before returning a positive
    `PackageReport`.
  - `color_breakdown()` and `color_breakdown_with(..)` return a budget pie only
    through their corresponding verification path.
  - `waiver_message(index)` constructs package-owned, domain-separated
    authorization bytes; `with_waiver_mac` installs the final authenticator
    without changing those bytes.
  - `to_json()` is deterministic. `from_json()` strictly parses, checks
    structure, declared budgets, and the root while retaining unauthenticated
    origins; `from_json_with()` also authenticates them.
- `PackageReport { merkle_root, breakdown, claims }`.
- `PackageError` — structured refusals for incomplete provenance, invalid or
  duplicate claim ids, blank/placeholder claim statements, malformed color
  payloads, unsupported formats, receipt mismatches/parents, malformed
  falsifier/anchor records, and refuted claims.

## Invariants

- COMPLETENESS: reproducibility provenance fields, origin identities, and claim
  ids are canonical machine identities: no padding and no reserved placeholder
  tokens. Claim ids and waiver ids are unique within their namespaces, and
  claim statements are meaningful rather than blank
  or one of the reserved placeholders (`TODO`, `TBD`, `placeholder`, `N/A`/`NA`,
  `none`, `not run`, `pending`, `unknown`, `-`, or `?`, case-insensitive). A
  `Validated` claim must have a non-empty regime
  (`regime.bounds()` non-empty) whose axis names are non-blank and whose bounds
  are finite and ordered, plus a non-blank anchoring `dataset`. A `Verified`
  claim must carry a finite `[lo <= hi]` interval. An `Estimated` claim needs a
  non-blank estimator identity and a non-negative, non-NaN dispersion.
  Positive infinity is preserved as the lower-layer algebra's explicit
  no-quantitative-spread-claim sentinel; it is distinct from finite subtotal
  overflow, which verification rejects. An honest all-estimated package
  remains valid.
- FALSIFIER EVIDENCE: a record has a non-blank, non-placeholder stable
  falsifier identity, at least one executed attempt, and a non-blank,
  non-placeholder outcome detail. A refuted record still rejects its claim and
  package. This structural rule does not assert that the recorded work ran;
  source authentication remains a no-claim below.
- DATASET ANCHORS: every attached anchor has a non-blank stable dataset id and
  an exactly 64-character, lowercase hexadecimal content hash. Crosswalk
  anchoring coverage for an `AnchoredSource` requires an attached record whose
  dataset id AND content hash exactly equal the origin tuple, and whose dataset
  id equals the `Validated` color. An unrelated canonical hash does not count.
- SOURCE CERTIFICATES: a canonical certificate hash is only an artifact
  address. Positive verification requires an injected verifier to establish
  the exact typed claim request. Merely naming a producer and 64-hex hash never
  produces a report or coverage.
- WAIVERS: authorization requires an injected verifier and explicit date. The
  MAC message binds a domain tag, package provenance, ordered authorization
  context, target index, complete target claim, waiver id, and expiry. It
  excludes detached signatures and MAC fields so the message is stable before
  installing the authenticator. Expired, replayed, duplicated, or rejected
  grants fail closed.
- CONTENT-ADDRESSING: `merkle_root` is deterministic and tamper-evident across
  format version, claim count, provenance, and ordered claims. Derive-key modes
  separate package leaves/nodes from plain artifact hashes and each other; a
  detached signature does not change the root.
- Coverage is capability-aware. The plain coverage functions use deny-all
  verification and suppress every concept when a gated origin is unauthenticated;
  `_with` variants accept explicit capabilities. `ClaimOrigin` is present only
  after package origin verification succeeds; `WaiverAuthorization` additionally
  requires at least one authenticated, unexpired waiver.

## Error model

Structured `PackageError` values (refusals that teach), never panics. The JSON
parser maps structural refusals into `ParseError`; `from_json_with` additionally
maps capability refusals. Plain `from_json` is intentionally an integrity and
structure boundary, not an authentication verdict. The untrusted JSON
boundary is bounded before schema mapping: 64 MiB input, depth 64, one million
values, 100,000 members per container, 1 MiB decoded strings, and 128-byte
number tokens. In-memory verification enforces the corresponding transport
envelope before a package can pass, so a verified object remains serializable
and checkable under those bounds. Limit violations are structured refusals.

## Determinism class

The Merkle root, JSON, strict parsing, structural verification, and deny-all
verification are deterministic pure functions of the package (bit-exact on
float certificate payloads via `to_bits`). `verify_with` additionally depends
on caller-supplied capability decisions; reproducible deployments must provide
deterministic verifiers over pinned artifact stores and an explicit date.

## Cancellation behavior

No internal asynchronous work. Injected verifiers are synchronous callbacks;
their latency and cancellation behavior are outside this crate's control and
must be bounded by the caller's implementation.

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/package.rs` (Proposal 12): complete mixed-color package with injected
source verification;
all-estimated boundary (valid + round-trips); validated-missing-regime and
validated-missing-dataset completeness failures; verified bad-interval
failure; blank/placeholder statement and falsifier refusal; Merkle determinism
and claim/provenance tamper detection;
unsupported-format rejection; optional detached signature; deterministic JSON
carrying the root; in-memory/serialized semantic parity; exact full-width
falsifier attempt-count round trips with overflow refusal; deny-all source and
waiver behavior; exact typed source-subject checks; package-context waiver
replay resistance; duplicate waiver ids; capability-aware coverage; and origin
transport/identity limits. The crate unit battery additionally constructs a
private adversarial anchored claim to prove exact origin-hash matching.

## Schema v5: sealed origins and explicit capabilities (bead krym)

Every claim carries one content-addressed `ClaimOrigin`. `AnchoredSource` and
`EstimatedSource` must agree exactly with the color; `Derived` must be paired
with a backward-only composition receipt that re-derives the exact color.
`SourceCertificate` and `AuthenticatedWaiver` are capability-gated. Plain
verification, breakdown, and coverage are deny-all, so certificate-shaped bytes
cannot create a positive result. Waiver messages are package-owned and
domain-separated, and source verifiers receive typed requests rather than raw
strings. Origins and all their strings are included in the in-memory transport
envelope. The Merkle domains are `fs-package:v5:*`; v4 readers and transports
are refused by the one-version contract.

## Schema v4: mode-separated BLAKE3 roots (beads 7uq9, t7x3)

The content address is a 32-byte `ContentHash`. A header leaf binds the format,
ordered claim count, and reproducibility provenance; each claim is a separate
canonical leaf; internal nodes combine exact child bytes. Header, claim, and
node hashing use distinct standard BLAKE3 derive-key contexts, which also
separate package identities from plain ledger artifact hashes. Odd tree nodes
carry upward unchanged, while the header's claim count prevents tree-shape or
duplicate-tail ambiguity. Readers refuse v3's 16-hex FNV root by version and
root width; checker protocol v2 is the matching standalone ABI.

## Schema v3: receipts, falsifiers, anchors (bead xfxq)

Claims optionally carry a COMPOSITION RECEIPT (parent claim indices +
the ledger op): `verify` re-runs `fs_evidence::compose` over the
parents in order and requires EXACT color equality — a claim whose
color cannot be re-derived is `ReceiptMismatch`, so laundering a
Verified from Estimated parents is caught SEMANTICALLY, not just by
the content address; parents must point strictly backwards
(`BadReceiptParent` otherwise — a DAG by construction). FALSIFIER
RECORDS (name, attempts, refuted, detail) travel with the claim; any
`refuted: true` fails verification outright (`RefutedClaim`). ANCHOR
RECORDS give validated claims content-hashed dataset identities. All
three field families bind into the content address and round-trip
through the strict parser (booleans added to the closed grammar);
crosswalk coverage now reads validated falsifier logs and matching anchors from
the actual fields. Coverage fails closed for an invalid package, and raw
detached-signature presence never counts as authenticated sign-off. The
checker stays solver-free: `compose` lives in fs-evidence, already in its
dependency cone. Migration: v2 readers refuse v3 by version (the one-version
contract); in-tree constructors gained fields with builders, no call-site
changes.

## Schema v2: round trip and fail-closed parsing (bead qmao.6.1)

`to_json` emits the COMPLETE color payloads (floats as bit-exact hex
bits), provenance, signature, magnitude budget, and the content root;
`from_json` is a STRICT parser — unknown fields, missing fields, wrong
types, duplicate keys, bad hex, non-finite certificates, inverted
intervals, negative dispersions, and unknown color kinds each refuse
with a structured `ParseError`. The parser re-derives the magnitude
budget from the parsed claims and RECOMPUTES the content root from the
parsed fields: a package whose embedded root does not recompute is
tampered in transit and never loads. This is an integrity check, not the
schema-v5 source-origin proof named below. Decode-encode is both
semantically and textually stable (tested). The magnitude budget
attributes ERROR MAGNITUDES (verified interval widths, estimated
dispersions) and counts validated claims as unquantified regional
trust — never numerified. JSON number tokens are retained as decimal text until
they are converted into their target integer type; full-width `u64` falsifier
attempt counts therefore round-trip exactly, and overflow refuses instead of
rounding through `f64`.

## No-claim boundaries

- The Merkle hash is the shared in-house BLAKE3 implementation, with standard
  derive-key mode separation for typed package roots. This establishes content
  integrity, not authorship or scientific truth. A cryptographic SIGNATURE is
  detached and OPTIONAL; wiring a default Franken signature primitive is later
  work.
- The crate does not fetch artifacts or choose trust roots. Callers provide a
  `SourceCertificateVerifier`; that capability may retrieve and re-check the
  addressed proof artifact, or refuse. `fs-package` supplies only the typed,
  exact request and a deny-all default.
- The certificate payloads live in `fs-evidence::Color`; this crate bundles
  and content-addresses them, it does not produce them.
- A validated dataset hash proves content identity, not experimental quality or
  custodial authenticity. Those stronger properties require an external
  evidence policy. Likewise, successful waiver authentication proves only that
  the configured authority authorized the exact context through the stated
  date; it does not convert the waived scientific claim into an independently
  reproduced result.
