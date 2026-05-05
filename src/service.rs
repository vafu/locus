use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::paths::{PathError, canonical_project_path};
use crate::state::{Link, RuntimeState};
use crate::storage::{SqliteStore, StorageError};

pub const PROJECT_PREFIX: &str = "project:";
pub const CONTEXT_PREFIX: &str = "context:";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Storage(#[from] StorageError),
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

    pub fn ensure_project(
        &self,
        path: &str,
        name: Option<&str>,
        icon: Option<&str>,
        durable: bool,
    ) -> Result<String, ServiceError> {
        let canonical = canonical_project_path(path)?;
        let subject = project_subject(&canonical);
        self.set_property(&subject, "kind", "project", durable)?;
        self.set_property(&subject, "path", &canonical, durable)?;
        if let Some(name) = name {
            self.set_property(&subject, "name", name, durable)?;
        }
        if let Some(icon) = icon {
            self.set_property(&subject, "icon", icon, durable)?;
        }
        Ok(subject)
    }

    pub fn projects(&self) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let mut projects = inner
            .state
            .durable_properties
            .keys()
            .chain(inner.state.ephemeral_properties.keys())
            .filter_map(|(subject, key)| {
                (key == "kind" && subject.starts_with(PROJECT_PREFIX)).then_some(subject.clone())
            })
            .collect::<Vec<_>>();
        projects.sort();
        projects.dedup();
        Ok(projects)
    }

    pub fn set_context_link(
        &self,
        context: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> Result<(Vec<Link>, Link), ServiceError> {
        let subject = context_subject(context);
        let removed = self.remove_links(&subject, relation)?;
        let added = self.add_link(&subject, relation, target, durable)?;
        Ok((removed, added))
    }

    pub fn context_targets(
        &self,
        context: &str,
        relation: &str,
    ) -> Result<Vec<String>, ServiceError> {
        self.targets(&context_subject(context), relation)
    }
}

pub fn project_subject(canonical_path: &str) -> String {
    format!("{PROJECT_PREFIX}{canonical_path}")
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
    fn ensure_project_sets_project_properties() {
        let (_tmp, service) = service();
        let subject = service
            .ensure_project(".", Some("Locus"), Some("code"), false)
            .unwrap();
        let properties = service.properties(&subject).unwrap();

        assert!(subject.starts_with("project:/"));
        assert_eq!(properties.get("kind").map(String::as_str), Some("project"));
        assert_eq!(properties.get("name").map(String::as_str), Some("Locus"));
        assert_eq!(properties.get("icon").map(String::as_str), Some("code"));
    }

    #[test]
    fn context_link_replaces_previous_relation() {
        let (_tmp, service) = service();
        service
            .set_context_link("active", "project", "project:a", false)
            .unwrap();
        service
            .set_context_link("active", "project", "project:b", false)
            .unwrap();

        assert_eq!(
            service.context_targets("active", "project").unwrap(),
            vec!["project:b"]
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
