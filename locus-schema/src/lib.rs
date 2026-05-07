//! Schema parsing and validation for Locus graphs.
//!
//! Locus itself is a generic property graph. The schema gives publishers and
//! clients a shared vocabulary for node kinds, relation cardinality, required
//! properties, and named relation paths.
//!
//! The daemon uses this crate to reject invalid writes. Code generators use the
//! same parsed schema to produce typed client helpers, so the YAML schema stays
//! the source of truth for both runtime validation and language bindings.

mod error;
mod model;
mod raw;
mod validation;

pub use error::SchemaError;
pub use model::{
    Cardinality, GraphSchema, NodeSelector, NodeSpec, PathSpec, PropertySource, PropertySpec,
    RelationSpec, Retention,
};

#[cfg(test)]
mod tests;
