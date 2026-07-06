//! Conformance suite for fs-substrate (placeholder).
//!
//! Cases registered here define the crate's cross-implementation contract
//! (plan §13.3). The shared conformance harness (contract-conformance-infra
//! bead) will supersede this hand-rolled runner; the case shape is designed
//! so that migration is additive.

#[test]
fn conformance_placeholder_smoke() {
    // Case id: fs-substrate/smoke-000. Verdict schema: JSON-lines on stdout when the
    // real harness lands; for now the test passing IS the verdict.
    assert_eq!(fs_substrate::VERSION, env!("CARGO_PKG_VERSION"));
}
