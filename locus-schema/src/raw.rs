use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::SchemaError;
use crate::model::{
    Cardinality, GraphSchema, NodeSelector, NodeSpec, PathSpec, PropertySpec, RelationSpec,
    Retention,
};

#[derive(Debug, Deserialize)]
pub(crate) struct RawSchema {
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
        Ok(GraphSchema::new(nodes, relations, paths))
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
        "static" => Ok(Retention::Static),
        other => Err(SchemaError::InvalidRetention {
            relation: relation.to_string(),
            retention: other.to_string(),
        }),
    }
}
