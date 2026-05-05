use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::state::{Link, RuntimeState};
use crate::storage::{SqliteStore, StorageError};

pub const CONTEXT_PREFIX: &str = "context:";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(
        "reciprocal link rejected: {link_source} --{relation}--> {link_target} conflicts with {link_target} --{existing_relation}--> {link_source}"
    )]
    ReciprocalLink {
        link_source: String,
        relation: String,
        link_target: String,
        existing_relation: String,
    },
    #[error("service lock is poisoned")]
    Poisoned,
}

#[derive(Debug)]
struct Inner {
    state: RuntimeState,
    store: SqliteStore,
}

#[derive(Clone, Debug)]
pub struct LocusService {
    inner: Arc<Mutex<Inner>>,
}

impl LocusService {
    pub fn new(store: SqliteStore) -> Result<Self, ServiceError> {
        let state = RuntimeState {
            durable_links: store.load_links()?,
            durable_properties: store.load_properties()?,
            ephemeral_links: Default::default(),
            ephemeral_properties: Default::default(),
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner { state, store })),
        })
    }

    pub fn add_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> Result<Link, ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        if source != target {
            if let Some(existing) = inner
                .state
                .links()
                .into_iter()
                .find(|existing| existing.source == target && existing.target == source)
            {
                return Err(ServiceError::ReciprocalLink {
                    link_source: link.source,
                    relation: link.relation,
                    link_target: link.target,
                    existing_relation: existing.relation,
                });
            }
        }

        if durable {
            inner.store.add_link(&link)?;
            inner.state.durable_links.insert(link.clone());
        } else {
            inner.state.ephemeral_links.insert(link.clone());
        }
        Ok(link)
    }

    pub fn remove_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> Result<Link, ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.store.remove_link(&link)?;
        inner.state.durable_links.remove(&link);
        inner.state.ephemeral_links.remove(&link);
        Ok(link)
    }

    pub fn remove_links(&self, source: &str, relation: &str) -> Result<Vec<Link>, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let removed = inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == source && link.relation == relation)
            .collect::<Vec<_>>();
        inner.store.remove_links(source, relation)?;
        inner
            .state
            .durable_links
            .retain(|link| !(link.source == source && link.relation == relation));
        inner
            .state
            .ephemeral_links
            .retain(|link| !(link.source == source && link.relation == relation));
        Ok(removed)
    }

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
        durable: bool,
    ) -> Result<(), ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        if durable {
            inner.store.set_property(subject, key, value)?;
            inner
                .state
                .durable_properties
                .insert((subject.to_string(), key.to_string()), value.to_string());
        } else {
            inner
                .state
                .ephemeral_properties
                .insert((subject.to_string(), key.to_string()), value.to_string());
        }
        Ok(())
    }

    pub fn remove_property(&self, subject: &str, key: &str) -> Result<(), ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.store.remove_property(subject, key)?;
        inner
            .state
            .durable_properties
            .remove(&(subject.to_string(), key.to_string()));
        inner
            .state
            .ephemeral_properties
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
        for (subject, _) in inner
            .state
            .durable_properties
            .keys()
            .chain(inner.state.ephemeral_properties.keys())
        {
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
        for ((subject, property_key), property_value) in inner
            .state
            .durable_properties
            .iter()
            .chain(inner.state.ephemeral_properties.iter())
        {
            if property_key == key && value.is_none_or(|value| property_value == value) {
                subjects.insert(subject.clone());
            }
        }
        Ok(subjects.into_iter().collect())
    }

    pub fn set_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> Result<(Vec<Link>, Link), ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;

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
            .filter(|existing| existing.source == source && existing.relation == relation)
            .collect::<Vec<_>>();

        if durable {
            inner.store.set_link(&link)?;
            inner
                .state
                .durable_links
                .retain(|existing| !(existing.source == source && existing.relation == relation));
            inner
                .state
                .ephemeral_links
                .retain(|existing| !(existing.source == source && existing.relation == relation));
            inner.state.durable_links.insert(link.clone());
        } else {
            inner.store.remove_links(source, relation)?;
            inner
                .state
                .ephemeral_links
                .retain(|existing| !(existing.source == source && existing.relation == relation));
            inner
                .state
                .durable_links
                .retain(|existing| !(existing.source == source && existing.relation == relation));
            inner.state.ephemeral_links.insert(link.clone());
        }

        Ok((removed, link))
    }
}

pub fn context_subject(context: &str) -> String {
    format!("{CONTEXT_PREFIX}{context}")
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn service() -> (TempDir, LocusService) {
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(tmp.path().join("locus.db")).unwrap();
        (tmp, LocusService::new(store).unwrap())
    }

    fn reopen(tmp: &TempDir) -> LocusService {
        let store = SqliteStore::open(tmp.path().join("locus.db")).unwrap();
        LocusService::new(store).unwrap()
    }

    #[test]
    fn durable_links_survive_restart() {
        let (tmp, service) = service();
        service.add_link("a", "project", "b", true).unwrap();
        let service = reopen(&tmp);

        assert_eq!(service.targets("a", "project").unwrap(), vec!["b"]);
    }

    #[test]
    fn ephemeral_links_do_not_survive_restart() {
        let (tmp, service) = service();
        service.add_link("a", "project", "b", false).unwrap();
        let service = reopen(&tmp);

        assert!(service.targets("a", "project").unwrap().is_empty());
    }

    #[test]
    fn returns_reverse_sources_for_multi_target_relations() {
        let (_tmp, service) = service();
        service
            .add_link("session:1", "project", "project:a", false)
            .unwrap();
        service
            .add_link("session:2", "project", "project:a", false)
            .unwrap();

        assert_eq!(
            service.sources("project:a", "project").unwrap(),
            vec!["session:1", "session:2"]
        );
    }

    #[test]
    fn rejects_reciprocal_links() {
        let (_tmp, service) = service();
        service
            .add_link("niri:workspace:6", "window", "niri:window:57", false)
            .unwrap();

        let error = service
            .add_link("niri:window:57", "workspace", "niri:workspace:6", false)
            .unwrap_err();

        assert!(matches!(error, ServiceError::ReciprocalLink { .. }));
        assert_eq!(service.all_links().unwrap().len(), 1);
    }

    #[test]
    fn ephemeral_properties_override_durable_properties() {
        let (_tmp, service) = service();
        service
            .set_property("project:a", "name", "Durable", true)
            .unwrap();
        service
            .set_property("project:a", "name", "Ephemeral", false)
            .unwrap();

        assert_eq!(
            service.property("project:a", "name").unwrap().as_deref(),
            Some("Ephemeral")
        );
    }

    #[test]
    fn subjects_include_links_and_properties() {
        let (_tmp, service) = service();
        service.add_link("a", "rel", "b", false).unwrap();
        service.set_property("c", "kind", "thing", false).unwrap();

        assert_eq!(service.subjects().unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn finds_subjects_by_property() {
        let (_tmp, service) = service();
        service.set_property("a", "kind", "project", false).unwrap();
        service
            .set_property("b", "kind", "workspace", false)
            .unwrap();
        service.set_property("c", "kind", "project", false).unwrap();

        assert_eq!(
            service
                .subjects_with_property("kind", Some("project"))
                .unwrap(),
            vec!["a", "c"]
        );
        assert_eq!(
            service.subjects_with_property("kind", None).unwrap(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn set_link_replaces_previous_relation() {
        let (_tmp, service) = service();
        service.set_link("a", "current", "b", false).unwrap();
        service.set_link("a", "current", "c", false).unwrap();

        assert_eq!(service.targets("a", "current").unwrap(), vec!["c"]);
        assert_eq!(
            service
                .all_links()
                .unwrap()
                .into_iter()
                .map(|link| link.to_tuple())
                .collect::<Vec<_>>(),
            vec![("a".to_string(), "current".to_string(), "c".to_string())]
        );
    }

    #[test]
    fn all_links_returns_durable_and_ephemeral_links() {
        let (_tmp, service) = service();
        service.add_link("a", "durable", "b", true).unwrap();
        service.add_link("b", "ephemeral", "c", false).unwrap();

        let links = service
            .all_links()
            .unwrap()
            .into_iter()
            .map(|link| link.to_tuple())
            .collect::<Vec<_>>();

        assert_eq!(
            links,
            vec![
                ("a".to_string(), "durable".to_string(), "b".to_string()),
                ("b".to_string(), "ephemeral".to_string(), "c".to_string()),
            ]
        );
    }
}
