use std::collections::HashMap;
use std::error::Error;
use std::fmt;

pub type GraphResult<T> = Result<T, GraphError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphError {
    message: String,
}

impl GraphError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for GraphError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    pub source: String,
    pub relation: String,
    pub target: String,
}

impl Link {
    pub fn new(source: &str, relation: &str, target: &str) -> Self {
        Self {
            source: source.to_string(),
            relation: relation.to_string(),
            target: target.to_string(),
        }
    }

    pub fn to_tuple(&self) -> (String, String, String) {
        (
            self.source.clone(),
            self.relation.clone(),
            self.target.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSetChange {
    Unchanged,
    Changed { removed: Vec<Link>, added: Link },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyChange {
    Unchanged,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub source: String,
    pub path: Vec<String>,
    pub target: Option<String>,
}

pub trait Graph: Send + Sync {
    fn set_link(&self, source: &str, relation: &str, target: &str) -> GraphResult<LinkSetChange>;

    fn remove_link(&self, source: &str, relation: &str, target: &str) -> GraphResult<Link>;

    fn remove_links(&self, source: &str, relation: &str) -> GraphResult<Vec<Link>>;

    fn targets(&self, source: &str, relation: &str) -> GraphResult<Vec<String>>;

    fn sources(&self, target: &str, relation: &str) -> GraphResult<Vec<String>>;

    fn links(&self, subject: &str) -> GraphResult<Vec<Link>>;

    fn all_links(&self) -> GraphResult<Vec<Link>>;

    fn set_property(&self, subject: &str, key: &str, value: &str) -> GraphResult<PropertyChange>;

    fn remove_property(&self, subject: &str, key: &str) -> GraphResult<()>;

    fn property(&self, subject: &str, key: &str) -> GraphResult<Option<String>>;

    fn properties(&self, subject: &str) -> GraphResult<HashMap<String, String>>;

    fn subjects(&self) -> GraphResult<Vec<String>>;

    fn subjects_with_property(&self, key: &str, value: Option<&str>) -> GraphResult<Vec<String>>;

    fn resolve_kind(&self, source: &str, kind: &str) -> GraphResult<Option<String>>;

    fn resolve_path(&self, source: &str, path: &[String]) -> GraphResult<Option<String>>;

    fn resolve_all(&self, source: &str, path: &[String]) -> GraphResult<Vec<String>>;

    fn subscribe_resolution(&self, source: &str, path: &[String]) -> GraphResult<Resolution>;

    fn refresh_resolutions(&self) -> GraphResult<Vec<Resolution>>;
}
