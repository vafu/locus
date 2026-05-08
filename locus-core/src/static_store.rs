use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use locus_schema::{GraphSchema, Retention};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::Link;
use crate::error::ServiceError;
use crate::state::RuntimeState;

#[derive(Debug, Default, Serialize, Deserialize)]
struct StaticSnapshot {
    #[serde(default)]
    links: Vec<Link>,
    #[serde(default)]
    properties: Vec<StaticProperty>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StaticProperty {
    subject: String,
    key: String,
    value: String,
}

pub fn read_static_state(schema: &GraphSchema, path: &Path) -> Result<RuntimeState, ServiceError> {
    if !path.exists() {
        return Ok(RuntimeState::default());
    }
    let text = fs::read_to_string(path)?;
    let snapshot = serde_json::from_str::<StaticSnapshot>(&text)?;
    Ok(snapshot.into_state(schema))
}

pub fn write_static_state(
    schema: &GraphSchema,
    state: &RuntimeState,
    path: &Path,
) -> Result<(), ServiceError> {
    StaticSnapshot::from_state(schema, state).write(path)
}

impl StaticSnapshot {
    fn into_state(self, schema: &GraphSchema) -> RuntimeState {
        let mut state = RuntimeState::default();
        for property in self.properties {
            state
                .properties
                .insert((property.subject, property.key), property.value);
        }
        for link in self.links {
            let Some(spec) = schema.relation(&link.relation) else {
                warn!(relation = link.relation, "skipping unknown static relation");
                continue;
            };
            if spec.retention != Retention::Static {
                warn!(
                    relation = link.relation,
                    "skipping non-static persisted link"
                );
                continue;
            }
            if let Err(error) = spec.validate(schema, &state, &link.source, &link.target) {
                warn!(%error, "skipping invalid static link");
                continue;
            }
            state.links.insert(link);
        }
        state
    }

    fn from_state(schema: &GraphSchema, state: &RuntimeState) -> Self {
        let links = state
            .links()
            .into_iter()
            .filter(|link| {
                schema
                    .relation(&link.relation)
                    .is_some_and(|spec| spec.retention == Retention::Static)
            })
            .collect::<Vec<_>>();
        let subjects = links
            .iter()
            .flat_map(|link| [link.source.clone(), link.target.clone()])
            .collect::<BTreeSet<_>>();
        let properties = state
            .properties
            .iter()
            .filter(|((subject, key), _)| {
                subjects.contains(subject) && should_persist_property(key)
            })
            .map(|((subject, key), value)| StaticProperty {
                subject: subject.clone(),
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        Self { links, properties }
    }

    fn write(&self, path: &Path) -> Result<(), ServiceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("{}\n", serde_json::to_string_pretty(self)?))?;
        Ok(())
    }
}

fn should_persist_property(key: &str) -> bool {
    !matches!(
        key,
        "active" | "focused" | "urgent" | "external-id" | "source"
    )
}
