use std::collections::BTreeMap;

use crate::api::{NONE_STRING, Project};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: Option<String>,
    pub path: String,
    pub icon: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

impl ProjectRecord {
    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.path
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or(&self.path)
                .to_string()
        })
    }

    pub fn to_dto(&self) -> Project {
        Project {
            id: self.id.clone(),
            name: self.display_name(),
            path: self.path.clone(),
            icon: self.icon.clone().unwrap_or_else(|| NONE_STRING.to_string()),
            metadata: self.metadata.clone().into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    ActiveProjectChanged(Option<String>),
    ProjectChanged(String),
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeState {
    pub projects: BTreeMap<String, ProjectRecord>,
    pub workspace_bindings: BTreeMap<String, String>,
    pub active_workspace_id: Option<String>,
}

impl RuntimeState {
    pub fn active_project_id(&self) -> Option<String> {
        self.active_workspace_id
            .as_ref()
            .and_then(|workspace_id| self.workspace_bindings.get(workspace_id))
            .cloned()
    }
}
