use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::SchemaError;
use crate::raw::RawSchema;
use crate::validation::validate_selector;

/// Cardinality for one side of a relation.
///
/// `RelationSpec` stores cardinality as two directional limits:
/// `sources_per_target` and `targets_per_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// At most one item is allowed on this side.
    One,
    /// Any number of items is allowed on this side.
    Many,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Retention {
    #[default]
    Strong,
    Weak,
}

/// A schema selector for the source or target node of a relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeSelector {
    /// Match one exact node id, for example `context:selected`.
    Exact(String),
    /// Match any node whose `kind` property equals this value.
    Kind(String),
    /// Match any node. If the node has a known `kind`, required properties for
    /// that kind are still enforced.
    Any,
}

/// Runtime validation rules for a named relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationSpec {
    /// Relation name as used in graph links.
    pub relation: String,
    /// Allowed source node selector.
    pub source: NodeSelector,
    /// Allowed target node selector.
    pub target: NodeSelector,
    /// How many sources may point at a single target.
    pub sources_per_target: Cardinality,
    /// How many targets may be attached to a single source.
    pub targets_per_source: Cardinality,
    /// Whether the target may outlive the source independently.
    pub retention: Retention,
}

/// Parsed graph schema.
///
/// Schema sections:
///
/// - `nodes`: node kind declarations and property metadata.
/// - `relations`: relation selectors and cardinality.
/// - `paths`: named relation paths for clients and codegen.
#[derive(Debug, Clone, Default)]
pub struct GraphSchema {
    nodes: BTreeMap<String, NodeSpec>,
    relations: BTreeMap<String, RelationSpec>,
    paths: BTreeMap<String, PathSpec>,
}

/// Properties known for one node kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeSpec {
    /// Property declarations keyed by property name.
    pub properties: BTreeMap<String, PropertySpec>,
}

/// Metadata for one property.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PropertySpec {
    /// Whether this property must exist before a node participates in a
    /// schema-validated relation.
    pub required: bool,
}

/// A named relation path.
///
/// Paths are not used to validate writes. They document and generate common
/// read-side traversals such as `context:selected -> window -> workspace ->
/// project`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSpec {
    /// Path name as declared in YAML.
    pub name: String,
    /// Source node id for this path.
    pub source: String,
    /// Ordered relation names to traverse.
    pub path: Vec<String>,
    /// Whether consumers should treat this path as a multi-target traversal.
    pub many: bool,
}

/// Minimal property lookup interface used during relation validation.
pub trait PropertySource {
    /// Return a property value for `subject` and `key`, if one exists.
    fn property(&self, subject: &str, key: &str) -> Option<String>;
}

impl GraphSchema {
    pub(crate) fn new(
        nodes: BTreeMap<String, NodeSpec>,
        relations: BTreeMap<String, RelationSpec>,
        paths: BTreeMap<String, PathSpec>,
    ) -> Self {
        Self {
            nodes,
            relations,
            paths,
        }
    }

    /// Load and parse a schema from a YAML file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let text = fs::read_to_string(path)?;
        Self::parse_yaml(&text)
    }

    /// Parse a YAML schema string.
    pub fn parse_yaml(text: &str) -> Result<Self, SchemaError> {
        let raw: RawSchema = serde_yaml::from_str(text)?;
        raw.try_into()
    }

    /// Return all declared node kinds.
    pub fn nodes(&self) -> &BTreeMap<String, NodeSpec> {
        &self.nodes
    }

    /// Return all declared relations.
    pub fn relations(&self) -> &BTreeMap<String, RelationSpec> {
        &self.relations
    }

    /// Return all declared named paths.
    pub fn paths(&self) -> &BTreeMap<String, PathSpec> {
        &self.paths
    }

    /// Look up a relation by name.
    pub fn relation(&self, relation: &str) -> Option<&RelationSpec> {
        self.relations.get(relation)
    }

    /// Look up a node kind by name.
    pub fn node(&self, kind: &str) -> Option<&NodeSpec> {
        self.nodes.get(kind)
    }

    /// Look up a named path by name.
    pub fn path(&self, name: &str) -> Option<&PathSpec> {
        self.paths.get(name)
    }
}

impl RelationSpec {
    /// Validate that `source --relation--> target` matches this relation spec.
    ///
    /// Cardinality replacement is handled by the graph runtime. This method
    /// only checks source/target selectors and required properties.
    pub fn validate(
        &self,
        schema: &GraphSchema,
        properties: &impl PropertySource,
        source: &str,
        target: &str,
    ) -> Result<(), SchemaError> {
        validate_selector(
            schema,
            properties,
            &self.source,
            source,
            "source",
            &self.relation,
        )?;
        validate_selector(
            schema,
            properties,
            &self.target,
            target,
            "target",
            &self.relation,
        )?;
        Ok(())
    }
}
