use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zbus::proxy;
use zvariant::{OwnedObjectPath, Type};

pub const BUS_NAME: &str = "io.github.Locus";
pub const ROOT_PATH: &str = "/io/github/Locus";
pub const MANAGER_INTERFACE: &str = "io.github.Locus.Manager";
pub const PROJECT_INTERFACE: &str = "io.github.Locus.Project";
pub const NONE_STRING: &str = "";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("invalid object path: {0}")]
    InvalidObjectPath(#[from] zvariant::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub icon: String,
    pub metadata: HashMap<String, String>,
}

pub fn project_object_path(project_id: &str) -> Result<OwnedObjectPath, ApiError> {
    let segment = project_object_segment(project_id);
    Ok(OwnedObjectPath::try_from(format!(
        "{ROOT_PATH}/projects/{segment}"
    ))?)
}

pub fn project_object_segment(project_id: &str) -> String {
    let digest = Sha256::digest(project_id.as_bytes());
    let mut segment = String::from("p_");
    for byte in &digest[..16] {
        segment.push_str(&format!("{byte:02x}"));
    }
    segment
}

#[proxy(
    default_service = "io.github.Locus",
    default_path = "/io/github/Locus",
    interface = "io.github.Locus.Manager"
)]
pub trait Manager {
    fn register_project(&self, path: &str, name: &str, icon: &str) -> zbus::Result<String>;

    fn bind_workspace(&self, workspace_id: &str, path: &str) -> zbus::Result<String>;

    fn unbind_workspace(&self, workspace_id: &str) -> zbus::Result<()>;

    fn set_metadata(&self, path: &str, key: &str, value: &str) -> zbus::Result<()>;

    fn remove_metadata(&self, path: &str, key: &str) -> zbus::Result<()>;

    fn get_active_project(&self) -> zbus::Result<String>;

    fn list_projects(&self) -> zbus::Result<Vec<Project>>;

    #[zbus(property)]
    fn active_project_id(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn active_project_changed(&self, project_id: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn project_changed(&self, project_id: String) -> zbus::Result<()>;
}

pub struct LocusClient<'a> {
    proxy: ManagerProxy<'a>,
}

impl<'a> LocusClient<'a> {
    pub async fn new(connection: &'a zbus::Connection) -> zbus::Result<Self> {
        Ok(Self {
            proxy: ManagerProxy::new(connection).await?,
        })
    }

    pub async fn register_project(
        &self,
        path: &str,
        name: Option<&str>,
        icon: Option<&str>,
    ) -> zbus::Result<String> {
        self.proxy
            .register_project(
                path,
                name.unwrap_or(NONE_STRING),
                icon.unwrap_or(NONE_STRING),
            )
            .await
    }

    pub async fn bind_workspace(&self, workspace_id: &str, path: &str) -> zbus::Result<String> {
        self.proxy.bind_workspace(workspace_id, path).await
    }

    pub async fn unbind_workspace(&self, workspace_id: &str) -> zbus::Result<()> {
        self.proxy.unbind_workspace(workspace_id).await
    }

    pub async fn set_metadata(&self, path: &str, key: &str, value: &str) -> zbus::Result<()> {
        self.proxy.set_metadata(path, key, value).await
    }

    pub async fn remove_metadata(&self, path: &str, key: &str) -> zbus::Result<()> {
        self.proxy.remove_metadata(path, key).await
    }

    pub async fn active_project(&self) -> zbus::Result<Option<String>> {
        let value = self.proxy.get_active_project().await?;
        Ok((value != NONE_STRING).then_some(value))
    }

    pub async fn list_projects(&self) -> zbus::Result<Vec<Project>> {
        self.proxy.list_projects().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_stable_object_segments() {
        assert_eq!(
            project_object_segment("/home/v47/proj/locus"),
            "p_e2cb328d5c2a6f54aee89e64366b43cc"
        );
    }

    #[test]
    fn object_paths_are_valid() {
        assert_eq!(
            project_object_path("/home/v47/proj/locus")
                .unwrap()
                .as_str(),
            "/io/github/Locus/projects/p_e2cb328d5c2a6f54aee89e64366b43cc"
        );
    }
}
