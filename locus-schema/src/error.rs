#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    #[error("read schema failed: {0}")]
    Io(String),
    #[error("parse schema failed: {0}")]
    Parse(String),
    #[error("unknown relation {0:?}")]
    UnknownRelation(String),
    #[error("invalid relation cardinality {relation:?}: {cardinality:?}")]
    InvalidCardinality {
        relation: String,
        cardinality: String,
    },
    #[error(
        "relation {relation:?} cannot use weak retention because multiple sources may share one target"
    )]
    UnsafeWeakRetention { relation: String },
    #[error("invalid relation retention {relation:?}: {retention:?}")]
    InvalidRetention { relation: String, retention: String },
    #[error(
        "{role} {subject:?} does not match relation {relation:?}: expected exact subject {expected:?}"
    )]
    ExactMismatch {
        relation: String,
        role: &'static str,
        subject: String,
        expected: String,
    },
    #[error(
        "{role} {subject:?} does not match relation {relation:?}: expected kind {expected:?}, got {actual:?}"
    )]
    KindMismatch {
        relation: String,
        role: &'static str,
        subject: String,
        expected: String,
        actual: Option<String>,
    },
    #[error(
        "{role} {subject:?} does not match relation {relation:?}: kind {kind:?} requires property {property:?}"
    )]
    MissingRequiredProperty {
        relation: String,
        role: &'static str,
        subject: String,
        kind: String,
        property: String,
    },
}

impl From<std::io::Error> for SchemaError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_yaml::Error> for SchemaError {
    fn from(error: serde_yaml::Error) -> Self {
        Self::Parse(error.to_string())
    }
}
