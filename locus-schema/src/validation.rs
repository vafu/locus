use crate::error::SchemaError;
use crate::model::{GraphSchema, NodeSelector, PropertySource};

pub(crate) fn validate_selector(
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
