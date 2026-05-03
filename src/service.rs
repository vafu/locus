use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::paths::{PathError, canonical_project_path};
use crate::state::{ProjectRecord, RuntimeState, ServiceEvent};
use crate::storage::{SqliteStore, StorageError};

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
            projects: store.load_projects()?,
            workspace_bindings: store.load_workspace_bindings()?,
            active_workspace_id: None,
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner { state, store })),
        })
    }

    pub fn register_project(
        &self,
        path: &str,
        name: Option<&str>,
        icon: Option<&str>,
    ) -> Result<String, ServiceError> {
        let project_id = canonical_project_path(path)?;
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.store.upsert_project(&project_id, name, icon)?;
        inner
            .state
            .projects
            .entry(project_id.clone())
            .and_modify(|project| {
                if let Some(name) = name {
                    project.name = Some(name.to_string());
                }
                if let Some(icon) = icon {
                    project.icon = Some(icon.to_string());
                }
            })
            .or_insert_with(|| ProjectRecord {
                id: project_id.clone(),
                path: project_id.clone(),
                name: name.map(ToOwned::to_owned),
                icon: icon.map(ToOwned::to_owned),
                metadata: Default::default(),
            });
        Ok(project_id)
    }

    pub fn bind_workspace(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<(String, Vec<ServiceEvent>), ServiceError> {
        let project_id = self.register_project(path, None, None)?;
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let active_before = inner.state.active_project_id();
        inner.store.bind_workspace(workspace_id, &project_id)?;
        inner
            .state
            .workspace_bindings
            .insert(workspace_id.to_string(), project_id.clone());
        let mut events = vec![ServiceEvent::ProjectChanged(project_id.clone())];
        let active_after = inner.state.active_project_id();
        if active_before != active_after {
            events.push(ServiceEvent::ActiveProjectChanged(active_after));
        }
        Ok((project_id, events))
    }

    pub fn unbind_workspace(&self, workspace_id: &str) -> Result<Vec<ServiceEvent>, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let active_before = inner.state.active_project_id();
        let previous = inner.store.unbind_workspace(workspace_id)?;
        inner.state.workspace_bindings.remove(workspace_id);
        let mut events = previous
            .into_iter()
            .map(ServiceEvent::ProjectChanged)
            .collect::<Vec<_>>();
        let active_after = inner.state.active_project_id();
        if active_before != active_after {
            events.push(ServiceEvent::ActiveProjectChanged(active_after));
        }
        Ok(events)
    }

    pub fn set_metadata(&self, path: &str, key: &str, value: &str) -> Result<String, ServiceError> {
        let project_id = self.register_project(path, None, None)?;
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.store.set_metadata(&project_id, key, value)?;
        if let Some(project) = inner.state.projects.get_mut(&project_id) {
            project.metadata.insert(key.to_string(), value.to_string());
        }
        Ok(project_id)
    }

    pub fn remove_metadata(&self, path: &str, key: &str) -> Result<String, ServiceError> {
        let project_id = self.register_project(path, None, None)?;
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.store.remove_metadata(&project_id, key)?;
        if let Some(project) = inner.state.projects.get_mut(&project_id) {
            project.metadata.remove(key);
        }
        Ok(project_id)
    }

    pub fn set_active_workspace(
        &self,
        workspace_id: Option<String>,
    ) -> Result<Vec<ServiceEvent>, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let active_before = inner.state.active_project_id();
        inner.state.active_workspace_id = workspace_id;
        let active_after = inner.state.active_project_id();
        if active_before == active_after {
            Ok(Vec::new())
        } else {
            Ok(vec![ServiceEvent::ActiveProjectChanged(active_after)])
        }
    }

    pub fn active_project_id(&self) -> Result<Option<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.active_project_id())
    }

    pub fn list_project_ids(&self) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.projects.keys().cloned().collect())
    }

    pub fn projects(&self) -> Result<Vec<ProjectRecord>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.projects.values().cloned().collect())
    }
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

    #[test]
    fn auto_registers_from_metadata() {
        let (_tmp, service) = service();
        service
            .set_metadata(".", "remarked.notebook", "locus")
            .unwrap();
        assert_eq!(service.list_project_ids().unwrap().len(), 1);
    }

    #[test]
    fn binding_active_workspace_emits_active_project_change() {
        let (_tmp, service) = service();
        service
            .set_active_workspace(Some("test:1".to_string()))
            .unwrap();
        let (_project_id, events) = service.bind_workspace("test:1", ".").unwrap();
        assert!(matches!(
            events.as_slice(),
            [
                ServiceEvent::ProjectChanged(_),
                ServiceEvent::ActiveProjectChanged(Some(_))
            ]
        ));
    }
}
