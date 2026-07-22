//! The versioned `.fsim` project schema (bead f85xj.6.1): the user-facing
//! contract for the ratified thermal-design-assurance vertical
//! (`frankensim-vertical-ratification-v1`).
//!
//! One semantic model, two spellings: a canonical s-expression grammar and an
//! isomorphic JSON rendering, both over `fs-ir`'s typed AST. Canonical bytes
//! are the checked s-expression render, hashed under
//! [`wire::FSIM_CANONICAL_DOMAIN`]; the JSON spelling reaches the same AST
//! and therefore the same canonical hash. The Five Explicits — units, seeds,
//! budgets, versions, capabilities — are mandatory sections; every omission
//! is a named [`fs_scenario::Violation`] with a fix, unknown fields are
//! refused, and the only defaults are receipted, never silent. Version bumps
//! travel through explicit [`migration`] receipts.

pub mod migration;
pub mod spec;
pub mod wire;

/// The current `.fsim` schema version. Readers admit exactly this version;
/// older envelopes must pass through [`migration::migrate_envelope`].
pub const FSIM_VERSION: u32 = 1;

pub use migration::{MigratedProject, MigrationRule, ProjectMigrationReceipt, migrate_envelope};
pub use spec::{
    Budgets, Cooling, DefaultReceipt, EntityDecl, Envelope, Fan, GeometryArtifact,
    InterfaceCardBinding, MaterialBinding, Metadata, OutputRequest, PowerDissipation, ProjectSpec,
    Seeds, SolverSettings, ThermalLimit, UnitsDoctrine, Vent, Versions,
};
pub use wire::{
    CanonicalizationReceipt, DecodedProject, ProjectError, canonical_hash, lower, parse_json,
    parse_sexpr, parse_sexpr_lenient, print_json, print_sexpr, recognize,
};
