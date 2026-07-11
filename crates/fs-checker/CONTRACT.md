# CONTRACT: fs-checker

The standalone evidence-package checker (plan addendum, Proposal 12): an
independently distributable verifier — "don't trust us; here is the checker."

## Purpose and layer

Layer L6. Its sole direct dependency is `fs-package`; that package's production
cone contains `fs-evidence`, dependency-free `fs-blake3`, and the static
`fs-crosswalk` vocabulary. A HARD
distribution constraint (Proposal 12): NO solver stack, geometry kernel, or
license gate anywhere in the graph. By construction the checker cannot run a
solve. It carries `CHECKER_PROTOCOL_VERSION = 3` for the schema-v5 sealed-origin
ABI (distributed independently). `CHECKER_SUPPORTED_PACKAGE_FORMAT = 5` is an
explicit protocol literal with a compile-time assertion against
`fs_package::FORMAT_VERSION`, so a package schema bump cannot silently retain
an incompatible checker ABI.

## Public types and semantics

- `check(&EvidencePackage) -> CheckReport` — re-verify a package with all
  external origin capabilities denied.
- `check_against_root(&EvidencePackage, expected_root) -> CheckReport` — also
  confirm the content address matches (tamper / substitution detection), with
  external origins still denied.
- `check_with_capabilities(package, expected_root, signature_verifier,
  capabilities)` — the in-memory entry point for explicitly authenticated
  source certificates and waivers. Detached-signature verification is a
  separate optional capability.
- `check_json(...)` and `check_json_with_capabilities(...)` — strict schema-v5
  transport counterparts. Plain `check_json` denies external origins; the
  capability-aware form authenticates them after structural parsing.
- `check_for_release(&EvidencePackage, expected_root, verifier) -> CheckReport`
  — the stronger no-falsifier-no-ship admission gate: requires a non-empty
  package, authenticated detached signature, falsifiers on every Verified or
  Validated claim, and a matching content-hash dataset anchor on every
  Validated claim. The plain form denies external origins.
- `check_for_release_with_capabilities(...)` — release admission with explicit
  source-certificate and waiver capabilities in addition to the mandatory,
  independent signature verifier.
- `check_json_for_release(text, expected_root, verifier) -> CheckReport` — the
  strict-parser release entry point; malformed transports fail before
  admission and external origins are denied.
- `check_json_for_release_with_capabilities(...)` — the strict-parser release
  entry point with explicit origin capabilities.
- `CheckReport { verdict, merkle_root, breakdown, signature, findings }` —
  `passed()` and `render_pie()` (a deterministic text budget pie).
- `Verdict { Pass, Fail }`; `SignatureStatus { Unsigned, Unverified, Valid }`;
  `Finding { kind, detail }`.
- Any package-verification or origin-capability refusal carries a zeroed
  breakdown, so unauthenticated evidence cannot retain a normal-looking
  positive pie alongside the failure finding.
- Re-exports `EvidencePackage`, `ContentHash`, `ColorBreakdown`,
  `MagnitudeBudget`, `PackageError`, `ParseError`,
  `VerificationCapabilities`, and the source-certificate and waiver verifier
  interfaces.

## What it re-verifies

1. Format support, per-claim completeness, sealed origin/color consistency, and
   receipt re-derivation (delegated to `EvidencePackage::verify_with` — no
   solver).
2. Source-certificate and waiver origins only through the exact injected
   `VerificationCapabilities`. Plain entry points use `deny_all()`.
3. The content address: the Merkle root, recomputed independently and
   (optionally) checked against an expected value.
4. Signature validity only through an injected `SignatureVerifier` over the
   recomputed root. Presence without a verifier remains `Unverified`.
5. For explicit release admission only: non-vacuity, authenticated signature,
   per-certificate falsifier pairing, and per-Validated-claim dataset anchors.

## Invariants

- No solver / license in the build graph (enforced by the dependency set).
- A package that fails `verify_with` (incomplete claim, unsupported format, or
  refused capability) yields `Verdict::Fail` with a matching finding and a
  zeroed breakdown; a content-address mismatch fails.
- `check`, `check_against_root`, `check_json`, `check_with`,
  `check_for_release`, and `check_json_for_release` use
  `VerificationCapabilities::deny_all()`. Certificate-shaped bytes and waiver
  fields never authorize themselves.
- Source-certificate verification receives the complete typed request: package
  provenance, claim index and id, statement, interval, producer, and artifact
  hash. Waiver verification receives the package-owned authorization message
  and an explicit date context.
- Signature verification is independent from scientific-origin verification.
  It is optional outside release admission and mandatory at release admission.
- An empty package verifies vacuously and renders a "no claims" pie.
- An empty package never passes `check_for_release`; ordinary integrity
  checking and release admission are deliberately distinct verdicts.
- Verified and Validated claims never pass release admission without attached,
  structurally valid falsifier records. Validated claims additionally require
  an exact matching canonical dataset anchor.
- `render_pie` and the report are deterministic.

## Error model

The checker does not error — it REPORTS: failures become `Finding`s in a
`CheckReport` with `Verdict::Fail`. No panics.

## Determinism class

The deny-all report and rendered pie are deterministic pure functions of the
package. Capability-aware reports additionally depend on the supplied verifier
decisions and explicit waiver date; reproducible deployments must use pinned,
deterministic verifier implementations.

## Cancellation behavior

None (synchronous pure functions).

## Unsafe boundary

None. `#![deny(unsafe_code)]` via the workspace lint.

## Feature flags

None.

## Conformance tests

`tests/checker.rs` (Proposal 12): clean pass with no findings;
incomplete-validated-claim failure; content-address (Merkle) tamper detection;
including provenance tamper; malformed falsifier refusal with fail-closed pie;
signature-presence and verifier-capability reporting; deterministic budget-pie
rendering; empty-package pie; protocol version; determinism; and release-gate
admission/refusal batteries for empty, unpaired, unanchored, unsigned, and
wrong-root packages through both in-memory and strict JSON entry points. The
battery also checks positive and negative source-certificate and waiver
authentication through in-memory, JSON, release, and JSON-release paths;
ordinary positive paths remain unsigned, capability refusals zero the
breakdown, source verifiers bind the exact typed claim, and waiver verifiers
bind the complete package-owned authorization message.

## Independent re-verification (bead qmao.6.1)

`check_json` is the deny-all third-party entry point: strict parse (root
recomputation, structural semantics, and budget re-derivation happen in the
parser), then semantic re-verification, optionally against an expected root and
a `SignatureVerifier` capability. `check_json_with_capabilities` adds explicit
source-certificate and waiver authentication without changing signature
policy. Signature VALIDITY is asserted only
when a supplied capability accepts the signature over the RECOMPUTED
root; the in-tree `NoSignatureVerifier` accepts nothing (the no-crypto
no-claim — presence is recorded as `Unverified`, and supplying a
capability that rejects raises a `signature-invalid` finding). The
magnitude budget must reconcile with its parts. The normal dependency graph is
`fs-package -> {fs-blake3, fs-crosswalk, fs-evidence -> fs-obs}`: it contains no
solver and the checker cannot run a solve by construction.

## No-claim boundaries

- This crate ships no cryptographic primitive. It can assert signature validity
  only when a caller injects a `SignatureVerifier`; the default authenticates
  nothing.
- Composition receipts are re-run, but the checker itself does not produce or
  fetch source certificates. An injected `SourceCertificateVerifier` may
  retrieve and independently validate the addressed artifact; the checker only
  supplies the exact typed subject and fails closed without that capability.
- Schema v5 seals every claim behind a typed origin. Content addressing proves
  package integrity, not scientific truth. Successful source verification
  means only that the caller's configured verifier accepted the exact artifact
  subject; successful waiver verification means only that the configured
  authority authorized that exact package context through the stated date.
- Release admission adds falsifier, anchor, and signature obligations but does
  not re-run the source solver or independently establish experimental quality.
