use crate::api::{NONE_STRING, Project, ROOT_PATH, project_object_path};
use zbus::connection::Builder;
use zbus::fdo::ObjectManager;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, ObjectServer};

use crate::service::{LocusService, ServiceError};
use crate::state::{ProjectRecord, ServiceEvent};

fn to_fdo(error: ServiceError) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

fn api_to_fdo(error: crate::api::ApiError) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

fn zbus_to_fdo(error: zbus::Error) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

#[derive(Debug, Clone)]
pub struct ManagerIface {
    service: LocusService,
}

impl ManagerIface {
    pub fn new(service: LocusService) -> Self {
        Self { service }
    }

    async fn emit_events(
        emitter: &SignalEmitter<'_>,
        events: Vec<ServiceEvent>,
    ) -> zbus::Result<()> {
        for event in events {
            match event {
                ServiceEvent::ActiveProjectChanged(project_id) => {
                    Self::active_project_changed(emitter, option_to_wire(project_id)).await?;
                }
                ServiceEvent::ProjectChanged(project_id) => {
                    Self::project_changed(emitter, project_id).await?;
                }
            }
        }
        Ok(())
    }

    async fn ensure_project_object(
        &self,
        server: &ObjectServer,
        project_id: &str,
    ) -> zbus::fdo::Result<()> {
        server
            .at(
                project_object_path(project_id).map_err(api_to_fdo)?,
                ProjectIface::new(self.service.clone(), project_id.to_string()),
            )
            .await?;
        Ok(())
    }
}

#[zbus::interface(name = "io.github.Locus.Manager")]
impl ManagerIface {
    async fn register_project(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        path: &str,
        name: &str,
        icon: &str,
    ) -> zbus::fdo::Result<String> {
        let project_id = self
            .service
            .register_project(path, wire_to_option(name), wire_to_option(icon))
            .map_err(to_fdo)?;
        self.ensure_project_object(server, &project_id).await?;
        Ok(project_id)
    }

    async fn bind_workspace(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        workspace_id: &str,
        path: &str,
    ) -> zbus::fdo::Result<String> {
        let (project_id, events) = self
            .service
            .bind_workspace(workspace_id, path)
            .map_err(to_fdo)?;
        self.ensure_project_object(server, &project_id).await?;
        Self::emit_events(&emitter, events)
            .await
            .map_err(zbus_to_fdo)?;
        Ok(project_id)
    }

    async fn unbind_workspace(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        workspace_id: &str,
    ) -> zbus::fdo::Result<()> {
        let events = self
            .service
            .unbind_workspace(workspace_id)
            .map_err(to_fdo)?;
        Self::emit_events(&emitter, events)
            .await
            .map_err(zbus_to_fdo)?;
        Ok(())
    }

    async fn set_metadata(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        path: &str,
        key: &str,
        value: &str,
    ) -> zbus::fdo::Result<()> {
        let project_id = self
            .service
            .set_metadata(path, key, value)
            .map_err(to_fdo)?;
        self.ensure_project_object(server, &project_id).await?;
        Self::project_changed(&emitter, project_id)
            .await
            .map_err(zbus_to_fdo)?;
        Ok(())
    }

    async fn remove_metadata(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        path: &str,
        key: &str,
    ) -> zbus::fdo::Result<()> {
        let project_id = self.service.remove_metadata(path, key).map_err(to_fdo)?;
        self.ensure_project_object(server, &project_id).await?;
        Self::project_changed(&emitter, project_id)
            .await
            .map_err(zbus_to_fdo)?;
        Ok(())
    }

    async fn get_active_project(&self) -> zbus::fdo::Result<String> {
        Ok(option_to_wire(
            self.service.active_project_id().map_err(to_fdo)?,
        ))
    }

    async fn list_projects(&self) -> zbus::fdo::Result<Vec<Project>> {
        Ok(self
            .service
            .projects()
            .map_err(to_fdo)?
            .into_iter()
            .map(|project| project.to_dto())
            .collect())
    }

    #[zbus(property(emits_changed_signal = "false"))]
    async fn active_project_id(&self) -> zbus::fdo::Result<String> {
        Ok(option_to_wire(
            self.service.active_project_id().map_err(to_fdo)?,
        ))
    }

    #[zbus(signal)]
    async fn active_project_changed(
        emitter: &SignalEmitter<'_>,
        project_id: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn project_changed(emitter: &SignalEmitter<'_>, project_id: String) -> zbus::Result<()>;
}

#[derive(Debug, Clone)]
pub struct ProjectIface {
    service: LocusService,
    project_id: String,
}

impl ProjectIface {
    pub fn new(service: LocusService, project_id: String) -> Self {
        Self {
            service,
            project_id,
        }
    }

    fn project(&self) -> zbus::fdo::Result<ProjectRecord> {
        self.service
            .projects()
            .map_err(to_fdo)?
            .into_iter()
            .find(|project| project.id == self.project_id)
            .ok_or_else(|| zbus::fdo::Error::UnknownObject(self.project_id.clone()))
    }
}

#[zbus::interface(name = "io.github.Locus.Project")]
impl ProjectIface {
    #[zbus(property(emits_changed_signal = "const"))]
    async fn id(&self) -> zbus::fdo::Result<String> {
        Ok(self.project()?.id)
    }

    #[zbus(property(emits_changed_signal = "false"))]
    async fn name(&self) -> zbus::fdo::Result<String> {
        Ok(self.project()?.display_name())
    }

    #[zbus(property(emits_changed_signal = "const"))]
    async fn path(&self) -> zbus::fdo::Result<String> {
        Ok(self.project()?.path)
    }

    #[zbus(property(emits_changed_signal = "false"))]
    async fn icon(&self) -> zbus::fdo::Result<String> {
        Ok(option_to_wire(self.project()?.icon))
    }

    #[zbus(property(emits_changed_signal = "false"))]
    async fn metadata(&self) -> zbus::fdo::Result<std::collections::HashMap<String, String>> {
        Ok(self.project()?.metadata.into_iter().collect())
    }
}

fn wire_to_option(value: &str) -> Option<&str> {
    (value != NONE_STRING).then_some(value)
}

fn option_to_wire(value: Option<String>) -> String {
    value.unwrap_or_default()
}

pub async fn serve(service: LocusService) -> zbus::Result<Connection> {
    let mut builder = Builder::session()?
        .name(crate::api::BUS_NAME)?
        .serve_at(ROOT_PATH, ObjectManager)?
        .serve_at(ROOT_PATH, ManagerIface::new(service.clone()))?;

    for project in service
        .projects()
        .map_err(|error| zbus::Error::Failure(error.to_string()))?
    {
        builder = builder.serve_at(
            project_object_path(&project.id)
                .map_err(|error| zbus::Error::Failure(error.to_string()))?,
            ProjectIface::new(service.clone(), project.id),
        )?;
    }

    builder.build().await
}
