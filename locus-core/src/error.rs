use thiserror::Error;

use locus_schema::SchemaError;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(
        "reciprocal link rejected: {link_source} --{relation}--> {link_target} conflicts with {link_target} --{existing_relation}--> {link_source}"
    )]
    ReciprocalLink {
        link_source: String,
        relation: String,
        link_target: String,
        existing_relation: String,
    },
    #[error("resolve path from {start:?} through {path:?} is ambiguous: {targets:?}")]
    AmbiguousResolve {
        start: String,
        path: Vec<String>,
        targets: Vec<String>,
    },
    #[error("service lock is poisoned")]
    Poisoned,
    #[error("static graph persistence failed: {0}")]
    Persistence(String),
}

impl From<std::io::Error> for ServiceError {
    fn from(error: std::io::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}

impl From<serde_json::Error> for ServiceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}
