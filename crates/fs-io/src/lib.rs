//! fs-io (plan patch Rev J): import/export with QUARANTINE — the world
//! boundary. Real workflows ingest dirty geometry and emit useful
//! artifacts, but NO imported artifact becomes a trusted value without a
//! certification receipt: imports land as [`Quarantined`] and promote to
//! `Evidence` only after the repair suite + validity checks pass.
//! Parsers treat every byte as hostile: bounded resources, structured
//! rejection, never a panic.
//!
//! Layer: L2 (MORPH). Runtime deps: `std`, fs-rep-mesh (repair/validity),
//! fs-evidence, fs-geom, fs-obs, fs-math. PNG/EXR export is fs-img's job
//! (L5); ledger `imports` rows are written HELM-side from the receipts
//! this crate emits (L2 must not call L6).

pub mod catalog;
pub mod export;
pub mod obj;
pub mod ply;
pub mod quarantine;
pub mod stl;

pub use catalog::{Catalog, ColumnKind, ColumnSpec, Schema};
pub use export::{export_3mf, export_glb, export_vtk};
pub use quarantine::{ImportDefect, ImportReceipt, PromotionRefusal, Quarantined, promote};

use core::fmt;

/// Crate version (compile-time stamp).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Hard cap on elements a parser will allocate for (bounded resource
/// consumption on hostile input; ~100M vertices/faces).
pub const MAX_ELEMENTS: usize = 100_000_000;

/// Structured I/O failures (Decalogue P10) — hostile input is REFUSED,
/// never trusted or panicked on.
#[derive(Debug, Clone, PartialEq)]
pub enum IoError {
    /// Structurally invalid bytes for the declared format.
    Malformed {
        /// Byte or line position where parsing failed.
        at: usize,
        /// Diagnosis.
        what: String,
    },
    /// Valid-looking input outside the implemented subset.
    Unsupported {
        /// What was encountered.
        what: String,
    },
    /// Input exceeds declared resource bounds.
    ResourceBound {
        /// What bound.
        what: String,
    },
    /// Catalog value fails its schema.
    Schema {
        /// Row (1-based, excluding the header).
        row: usize,
        /// Column name.
        column: String,
        /// Diagnosis with the offending text.
        what: String,
    },
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::Malformed { at, what } => write!(f, "malformed input at {at}: {what}"),
            IoError::Unsupported { what } => write!(f, "unsupported: {what}"),
            IoError::ResourceBound { what } => write!(f, "resource bound exceeded: {what}"),
            IoError::Schema { row, column, what } => {
                write!(
                    f,
                    "schema violation at row {row}, column {column:?}: {what}"
                )
            }
        }
    }
}

impl std::error::Error for IoError {}
