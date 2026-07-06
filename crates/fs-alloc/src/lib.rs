//! fs-alloc — Scope arenas with O(1) cancel reclaim, 128-byte alignment, hugepages, pools.
//!
//! Layer: L0. See CONTRACT.md for invariants, error model, determinism
//! class, cancellation behavior, and no-claim boundaries. This crate is part
//! of the FrankenSim workspace; the layer dependency direction is enforced by
//! `cargo run -p xtask -- check-all`.

/// Crate version, re-exported for provenance stamping (the Five Explicits'
/// "versions" pillar reaches down to individual crates).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_stamped() {
        assert!(!super::VERSION.is_empty());
    }
}
