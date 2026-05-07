use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::error::ServiceError;
use crate::resolve::{resolve_all, resolve_kind, resolve_one};
use crate::state::RuntimeState;
use crate::{DeleteNodeChange, Link, LinkSetChange, PropertyChange, Resolution};
use locus_schema::{Cardinality, GraphSchema, Retention, SchemaError};
use tracing::trace;

#[derive(Debug)]
struct Inner {
    state: RuntimeState,
    schema: GraphSchema,
    resolutions: BTreeMap<(String, Vec<String>), Option<String>>,
}

#[derive(Clone, Debug)]
pub struct LocusService {
    inner: Arc<Mutex<Inner>>,
}

impl LocusService {
    pub fn with_schema(schema: GraphSchema) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state: RuntimeState::default(),
                schema,
                resolutions: BTreeMap::new(),
            })),
        }
    }

    pub fn remove_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> Result<DeleteNodeChange, ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.state.links.remove(&link);
        let mut change = DeleteNodeChange {
            removed_links: vec![link.clone()],
            removed_properties: Vec::new(),
        };
        let roots = inner.orphaned_weak_targets(&[link]);
        change.extend(inner.delete_nodes(roots));
        Ok(change)
    }

    pub fn remove_links(
        &self,
        source: &str,
        relation: &str,
    ) -> Result<DeleteNodeChange, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let removed = inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == source && link.relation == relation)
            .collect::<Vec<_>>();
        inner
            .state
            .links
            .retain(|link| !(link.source == source && link.relation == relation));
        let mut change = DeleteNodeChange {
            removed_links: removed.clone(),
            removed_properties: Vec::new(),
        };
        let roots = inner.orphaned_weak_targets(&removed);
        change.extend(inner.delete_nodes(roots));
        Ok(change)
    }

    pub fn delete_node(&self, subject: &str) -> Result<DeleteNodeChange, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.delete_nodes([subject.to_string()]))
    }
}

impl Inner {
    fn orphaned_weak_targets(&self, removed: &[Link]) -> Vec<String> {
        removed
            .iter()
            .filter(|link| {
                self.schema
                    .relation(&link.relation)
                    .is_some_and(|spec| spec.retention == Retention::Weak)
                    && !self
                        .state
                        .links()
                        .into_iter()
                        .any(|existing| existing.target == link.target)
            })
            .map(|link| link.target.clone())
            .collect()
    }

    fn delete_nodes(&mut self, subjects: impl IntoIterator<Item = String>) -> DeleteNodeChange {
        let mut deleted = BTreeSet::new();
        let mut stack = subjects.into_iter().collect::<Vec<_>>();

        while let Some(current) = stack.pop() {
            if !deleted.insert(current.clone()) {
                continue;
            }

            for link in self.state.links() {
                let Some(spec) = self.schema.relation(&link.relation) else {
                    continue;
                };
                if spec.retention == Retention::Weak && link.source == current {
                    stack.push(link.target);
                }
            }
        }

        let removed_links = self
            .state
            .links()
            .into_iter()
            .filter(|link| deleted.contains(&link.source) || deleted.contains(&link.target))
            .collect::<Vec<_>>();
        let removed_link_set = removed_links.iter().cloned().collect::<BTreeSet<_>>();
        self.state
            .links
            .retain(|link| !removed_link_set.contains(link));

        let removed_properties = self
            .state
            .properties
            .keys()
            .filter(|(property_subject, _)| deleted.contains(property_subject))
            .cloned()
            .collect::<Vec<_>>();
        for property in &removed_properties {
            self.state.properties.remove(property);
        }

        DeleteNodeChange {
            removed_links,
            removed_properties,
        }
    }
}

impl LocusService {
    pub fn targets(&self, source: &str, relation: &str) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == source && link.relation == relation)
            .map(|link| link.target)
            .collect())
    }

    pub fn sources(&self, target: &str, relation: &str) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.target == target && link.relation == relation)
            .map(|link| link.source)
            .collect())
    }

    pub fn links(&self, subject: &str) -> Result<Vec<Link>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == subject || link.target == subject)
            .collect())
    }

    pub fn all_links(&self) -> Result<Vec<Link>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.links().into_iter().collect())
    }

    pub fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
    ) -> Result<PropertyChange, ServiceError> {
        let span = tracing::trace_span!("core.set_property", subject, key);
        let _guard = span.enter();
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let visible_before = inner.state.property(subject, key);
        inner
            .state
            .properties
            .insert((subject.to_string(), key.to_string()), value.to_string());
        let visible_after = inner.state.property(subject, key);
        if visible_before == visible_after {
            Ok(PropertyChange::Unchanged)
        } else {
            Ok(PropertyChange::Changed)
        }
    }

    pub fn remove_property(&self, subject: &str, key: &str) -> Result<(), ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner
            .state
            .properties
            .remove(&(subject.to_string(), key.to_string()));
        Ok(())
    }

    pub fn property(&self, subject: &str, key: &str) -> Result<Option<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.property(subject, key))
    }

    pub fn properties(&self, subject: &str) -> Result<BTreeMap<String, String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.properties_for(subject))
    }

    pub fn subjects(&self) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let mut subjects = BTreeSet::new();
        for link in inner.state.links() {
            subjects.insert(link.source);
            subjects.insert(link.target);
        }
        for (subject, _) in inner.state.properties.keys() {
            subjects.insert(subject.clone());
        }
        Ok(subjects.into_iter().collect())
    }

    pub fn subjects_with_property(
        &self,
        key: &str,
        value: Option<&str>,
    ) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let mut subjects = BTreeSet::new();
        for ((subject, property_key), property_value) in &inner.state.properties {
            if property_key == key && value.is_none_or(|value| property_value == value) {
                subjects.insert(subject.clone());
            }
        }
        Ok(subjects.into_iter().collect())
    }

    pub fn resolve_kind(&self, source: &str, kind: &str) -> Result<Option<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(resolve_kind(&inner.state, source, kind))
    }

    pub fn resolve_path(
        &self,
        source: &str,
        path: &[String],
    ) -> Result<Option<String>, ServiceError> {
        let span = tracing::trace_span!("core.resolve_path", source, path = ?path);
        let _guard = span.enter();
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        resolve_one(&inner.state, source, path)
    }

    pub fn resolve_all(&self, source: &str, path: &[String]) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(resolve_all(&inner.state, source, path))
    }

    pub fn subscribe_resolution(
        &self,
        source: &str,
        path: &[String],
    ) -> Result<Resolution, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let target = resolve_one(&inner.state, source, path)?;
        inner
            .resolutions
            .insert((source.to_string(), path.to_vec()), target.clone());
        Ok(Resolution {
            source: source.to_string(),
            path: path.to_vec(),
            target,
        })
    }

    pub fn refresh_resolutions(&self) -> Result<Vec<Resolution>, ServiceError> {
        let span = tracing::trace_span!("core.refresh_resolutions");
        let _guard = span.enter();
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let keys = inner.resolutions.keys().cloned().collect::<Vec<_>>();
        let mut changed = Vec::new();

        for (source, path) in keys {
            let previous = inner
                .resolutions
                .get(&(source.clone(), path.clone()))
                .cloned();
            let target = resolve_one(&inner.state, &source, &path)?;
            if previous != Some(target.clone()) {
                inner
                    .resolutions
                    .insert((source.clone(), path.clone()), target.clone());
                changed.push(Resolution {
                    source,
                    path,
                    target,
                });
            }
        }

        trace!(changed = changed.len(), "resolutions refreshed");
        Ok(changed)
    }

    pub fn set_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> Result<LinkSetChange, ServiceError> {
        let span = tracing::trace_span!("core.set_link", source, relation, target);
        let _guard = span.enter();
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let spec = inner
            .schema
            .relation(relation)
            .ok_or_else(|| SchemaError::UnknownRelation(relation.to_string()))?
            .clone();
        spec.validate(&inner.schema, &inner.state, source, target)?;

        let visible_before = inner.state.links.contains(&link);
        if visible_before
            && spec.targets_per_source == Cardinality::Many
            && spec.sources_per_target == Cardinality::Many
        {
            return Ok(LinkSetChange::Unchanged);
        }

        if source != target {
            if let Some(existing) = inner.state.links().into_iter().find(|existing| {
                existing.source == target
                    && existing.target == source
                    && !(existing.source == source
                        && existing.relation == relation
                        && existing.target == target)
            }) {
                return Err(ServiceError::ReciprocalLink {
                    link_source: link.source,
                    relation: link.relation,
                    link_target: link.target,
                    existing_relation: existing.relation,
                });
            }
        }

        let removed = inner
            .state
            .links()
            .into_iter()
            .filter(|existing| {
                existing.relation == relation
                    && ((spec.targets_per_source == Cardinality::One && existing.source == source)
                        || (spec.sources_per_target == Cardinality::One
                            && existing.target == target))
            })
            .collect::<Vec<_>>();

        let visible_unchanged = removed.len() == 1 && removed.first() == Some(&link);
        if visible_unchanged {
            return Ok(LinkSetChange::Unchanged);
        }

        let removed_set = removed.iter().cloned().collect::<BTreeSet<_>>();
        inner
            .state
            .links
            .retain(|existing| !removed_set.contains(existing));
        let mut deleted = DeleteNodeChange {
            removed_links: removed.clone(),
            removed_properties: Vec::new(),
        };
        let roots = inner.orphaned_weak_targets(&removed);
        deleted.extend(inner.delete_nodes(roots));
        inner.state.links.insert(link.clone());

        Ok(LinkSetChange::Changed {
            removed,
            deleted,
            added: link,
        })
    }
}
