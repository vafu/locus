//! Schema parsing and validation for Locus graphs.
//!
//! Locus itself is a generic property graph. The schema gives publishers and
//! clients a shared vocabulary for node kinds, relation cardinality, required
//! properties, and named relation paths.
//!
//! The daemon uses this crate to reject invalid writes. Code generators use the
//! same parsed schema to produce typed client helpers, so the YAML schema stays
//! the source of truth for both runtime validation and language bindings.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
struct RawSchema {
    #[serde(default)]
    nodes: BTreeMap<String, RawNode>,
    #[serde(default)]
    relations: BTreeMap<String, RawRelation>,
    #[serde(default)]
    paths: BTreeMap<String, RawPath>,
}

#[derive(Debug, Deserialize, Default)]
struct RawNode {
    #[serde(default)]
    properties: BTreeMap<String, RawProperty>,
}

#[derive(Debug, Deserialize, Default)]
struct RawProperty {
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Deserialize)]
struct RawRelation {
    #[serde(default)]
    from: RawNodeSelector,
    #[serde(default)]
    to: RawNodeSelector,
    cardinality: String,
    #[serde(default)]
    retention: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPath {
    from: String,
    path: Vec<String>,
    #[serde(default)]
    many: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
enum RawNodeSelector {
    Kind(String),
    Detailed {
        kind: Option<String>,
        exact: Option<String>,
    },
    #[default]
    Any,
}

impl TryFrom<RawSchema> for GraphSchema {
    type Error = SchemaError;

    fn try_from(raw: RawSchema) -> Result<Self, Self::Error> {
        let nodes = raw
            .nodes
            .into_iter()
            .map(|(kind, node)| (kind, node.into()))
            .collect();
        let mut relations = BTreeMap::new();
        for (name, raw_relation) in raw.relations {
            let (sources_per_target, targets_per_source) =
                parse_cardinality(&name, &raw_relation.cardinality)?;
            let retention = parse_retention(&name, raw_relation.retention.as_deref())?;
            if retention == Retention::Weak && sources_per_target == Cardinality::Many {
                return Err(SchemaError::UnsafeWeakRetention { relation: name });
            }
            relations.insert(
                name.clone(),
                RelationSpec {
                    relation: name,
                    source: raw_relation.from.into(),
                    target: raw_relation.to.into(),
                    sources_per_target,
                    targets_per_source,
                    retention,
                },
            );
        }
        let paths = raw
            .paths
            .into_iter()
            .map(|(name, path)| {
                (
                    name.clone(),
                    PathSpec {
                        name,
                        source: path.from,
                        path: path.path,
                        many: path.many,
                    },
                )
            })
            .collect();
        Ok(Self {
            nodes,
            relations,
            paths,
        })
    }
}

impl From<RawNode> for NodeSpec {
    fn from(raw: RawNode) -> Self {
        Self {
            properties: raw
                .properties
                .into_iter()
                .map(|(key, property)| (key, property.into()))
                .collect(),
        }
    }
}

impl From<RawProperty> for PropertySpec {
    fn from(raw: RawProperty) -> Self {
        Self {
            required: raw.required,
        }
    }
}

impl From<RawNodeSelector> for NodeSelector {
    fn from(raw: RawNodeSelector) -> Self {
        match raw {
            RawNodeSelector::Kind(kind) if kind == "any" => Self::Any,
            RawNodeSelector::Kind(kind) => Self::Kind(kind),
            RawNodeSelector::Detailed {
                exact: Some(exact), ..
            } => Self::Exact(exact),
            RawNodeSelector::Detailed {
                kind: Some(kind), ..
            } => Self::Kind(kind),
            RawNodeSelector::Detailed { .. } | RawNodeSelector::Any => Self::Any,
        }
    }
}

fn parse_cardinality(
    relation: &str,
    cardinality: &str,
) -> Result<(Cardinality, Cardinality), SchemaError> {
    match cardinality {
        "one-to-one" | "1:1" => Ok((Cardinality::One, Cardinality::One)),
        "many-to-one" | "*:1" => Ok((Cardinality::Many, Cardinality::One)),
        "one-to-many" | "1:*" => Ok((Cardinality::One, Cardinality::Many)),
        "many-to-many" | "*:*" => Ok((Cardinality::Many, Cardinality::Many)),
        other => Err(SchemaError::InvalidCardinality {
            relation: relation.to_string(),
            cardinality: other.to_string(),
        }),
    }
}

fn parse_retention(relation: &str, retention: Option<&str>) -> Result<Retention, SchemaError> {
    match retention.unwrap_or("strong") {
        "strong" => Ok(Retention::Strong),
        "weak" => Ok(Retention::Weak),
        other => Err(SchemaError::InvalidRetention {
            relation: relation.to_string(),
            retention: other.to_string(),
        }),
    }
}

fn validate_selector(
    schema: &GraphSchema,
    properties: &impl PropertySource,
    selector: &NodeSelector,
    subject: &str,
    role: &'static str,
    relation: &str,
) -> Result<(), SchemaError> {
    match selector {
        NodeSelector::Any => {
            validate_required_properties(schema, properties, subject, role, relation)
        }
        NodeSelector::Exact(expected) if subject == expected => Ok(()),
        NodeSelector::Exact(expected) => Err(SchemaError::ExactMismatch {
            relation: relation.to_string(),
            role,
            subject: subject.to_string(),
            expected: expected.clone(),
        }),
        NodeSelector::Kind(expected) => {
            let actual = properties.property(subject, "kind");
            if actual.as_deref() == Some(expected) {
                validate_required_properties_for_kind(
                    schema, properties, subject, expected, role, relation,
                )
            } else {
                Err(SchemaError::KindMismatch {
                    relation: relation.to_string(),
                    role,
                    subject: subject.to_string(),
                    expected: expected.clone(),
                    actual,
                })
            }
        }
    }
}

fn validate_required_properties(
    schema: &GraphSchema,
    properties: &impl PropertySource,
    subject: &str,
    role: &'static str,
    relation: &str,
) -> Result<(), SchemaError> {
    let Some(kind) = properties.property(subject, "kind") else {
        return Ok(());
    };
    validate_required_properties_for_kind(schema, properties, subject, &kind, role, relation)
}

fn validate_required_properties_for_kind(
    schema: &GraphSchema,
    properties: &impl PropertySource,
    subject: &str,
    kind: &str,
    role: &'static str,
    relation: &str,
) -> Result<(), SchemaError> {
    let Some(node) = schema.node(kind) else {
        return Ok(());
    };
    for (property, spec) in &node.properties {
        if spec.required && properties.property(subject, property).is_none() {
            return Err(SchemaError::MissingRequiredProperty {
                relation: relation.to_string(),
                role,
                subject: subject.to_string(),
                kind: kind.to_string(),
                property: property.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nodes_relations_and_paths() {
        let schema = GraphSchema::parse_yaml(
            r#"
nodes:
  workspace: {}
  project:
    properties:
      path:
        required: true
      name: {}

relations:
  project:
    from: workspace
    to: project
    cardinality: one-to-one
    retention: weak

paths:
  selected-project:
    from: context:selected
    path: [window, workspace, project]
  workspace-projects:
    from: workspace
    path: [project]
    many: true
"#,
        )
        .unwrap();

        assert!(schema.node("project").is_some());
        assert!(schema.node("project").unwrap().properties["path"].required);
        assert_eq!(
            schema.relation("project").unwrap().source,
            NodeSelector::Kind("workspace".to_string())
        );
        assert_eq!(
            schema.relation("project").unwrap().retention,
            Retention::Weak
        );
        assert_eq!(
            schema.path("selected-project").unwrap().path,
            vec!["window", "workspace", "project"]
        );
        assert!(schema.path("workspace-projects").unwrap().many);
    }

    #[test]
    fn rejects_invalid_cardinality() {
        let error = GraphSchema::parse_yaml(
            r#"
relations:
  project:
    from: workspace
    to: project
    cardinality: sometimes
"#,
        )
        .unwrap_err();

        assert!(matches!(error, SchemaError::InvalidCardinality { .. }));
    }

    #[test]
    fn rejects_weak_retention_when_target_can_be_shared() {
        let error = GraphSchema::parse_yaml(
            r#"
relations:
  project:
    from: workspace
    to: project
    cardinality: many-to-one
    retention: weak
"#,
        )
        .unwrap_err();

        assert!(matches!(error, SchemaError::UnsafeWeakRetention { .. }));
    }

    #[test]
    fn rejects_invalid_retention() {
        let error = GraphSchema::parse_yaml(
            r#"
relations:
  project:
    from: workspace
    to: project
    cardinality: one-to-one
    retention: sticky
"#,
        )
        .unwrap_err();

        assert!(matches!(error, SchemaError::InvalidRetention { .. }));
    }
}
