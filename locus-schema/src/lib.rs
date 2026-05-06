use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    One,
    Many,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeSelector {
    Exact(String),
    Kind(String),
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationSpec {
    pub relation: String,
    pub source: NodeSelector,
    pub target: NodeSelector,
    pub sources_per_target: Cardinality,
    pub targets_per_source: Cardinality,
}

#[derive(Debug, Clone, Default)]
pub struct GraphSchema {
    nodes: BTreeMap<String, NodeSpec>,
    relations: BTreeMap<String, RelationSpec>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeSpec {
    pub properties: BTreeMap<String, PropertySpec>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PropertySpec {
    pub required: bool,
}

pub trait PropertySource {
    fn property(&self, subject: &str, key: &str) -> Option<String>;
}

impl GraphSchema {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let text = fs::read_to_string(path)?;
        Self::parse_yaml(&text)
    }

    pub fn parse_yaml(text: &str) -> Result<Self, SchemaError> {
        let raw: RawSchema = serde_yaml::from_str(text)?;
        raw.try_into()
    }

    pub fn relation(&self, relation: &str) -> Option<&RelationSpec> {
        self.relations.get(relation)
    }

    pub fn node(&self, kind: &str) -> Option<&NodeSpec> {
        self.nodes.get(kind)
    }
}

impl RelationSpec {
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
            relations.insert(
                name.clone(),
                RelationSpec {
                    relation: name,
                    source: raw_relation.from.into(),
                    target: raw_relation.to.into(),
                    sources_per_target,
                    targets_per_source,
                },
            );
        }
        Ok(Self { nodes, relations })
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
