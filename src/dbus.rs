use crate::api::{NONE_STRING, ROOT_PATH};
use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::SignalEmitter;

use crate::service::{LocusService, ServiceError};

fn to_fdo(error: ServiceError) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

#[derive(Debug, Clone)]
pub struct GraphIface {
    service: LocusService,
}

impl GraphIface {
    pub fn new(service: LocusService) -> Self {
        Self { service }
    }
}

#[zbus::interface(name = "io.github.Locus.Graph")]
impl GraphIface {
    async fn add_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()> {
        let link = self
            .service
            .add_link(source, relation, target, durable)
            .map_err(to_fdo)?;
        Self::link_added(&emitter, link.source, link.relation, link.target)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(())
    }

    async fn remove_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        let link = self
            .service
            .remove_link(source, relation, target)
            .map_err(to_fdo)?;
        Self::link_removed(&emitter, link.source, link.relation, link.target)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(())
    }

    async fn remove_links(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
    ) -> zbus::fdo::Result<()> {
        let links = self
            .service
            .remove_links(source, relation)
            .map_err(to_fdo)?;
        for link in links {
            Self::link_removed(&emitter, link.source, link.relation, link.target)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        Ok(())
    }

    async fn get_targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.service.targets(source, relation).map_err(to_fdo)
    }

    async fn get_sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.service.sources(target, relation).map_err(to_fdo)
    }

    async fn get_links(&self, subject: &str) -> zbus::fdo::Result<Vec<(String, String, String)>> {
        Ok(self
            .service
            .links(subject)
            .map_err(to_fdo)?
            .into_iter()
            .map(|link| link.to_tuple())
            .collect())
    }

    async fn set_property(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        subject: &str,
        key: &str,
        value: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()> {
        self.service
            .set_property(subject, key, value, durable)
            .map_err(to_fdo)?;
        Self::property_changed(
            &emitter,
            subject.to_string(),
            key.to_string(),
            value.to_string(),
        )
        .await
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(())
    }

    async fn remove_property(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        subject: &str,
        key: &str,
    ) -> zbus::fdo::Result<()> {
        self.service.remove_property(subject, key).map_err(to_fdo)?;
        Self::property_removed(&emitter, subject.to_string(), key.to_string())
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(())
    }

    async fn get_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<String> {
        Ok(self
            .service
            .property(subject, key)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn get_properties(
        &self,
        subject: &str,
    ) -> zbus::fdo::Result<std::collections::HashMap<String, String>> {
        Ok(self
            .service
            .properties(subject)
            .map_err(to_fdo)?
            .into_iter()
            .collect())
    }

    async fn ensure_project(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        path: &str,
        name: &str,
        icon: &str,
        durable: bool,
    ) -> zbus::fdo::Result<String> {
        let subject = self
            .service
            .ensure_project(path, wire_to_option(name), wire_to_option(icon), durable)
            .map_err(to_fdo)?;
        for (key, value) in self.service.properties(&subject).map_err(to_fdo)? {
            Self::property_changed(&emitter, subject.clone(), key, value)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        Ok(subject)
    }

    async fn list_projects(&self) -> zbus::fdo::Result<Vec<String>> {
        self.service.projects().map_err(to_fdo)
    }

    async fn set_context_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        context: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()> {
        let (removed, added) = self
            .service
            .set_context_link(context, relation, target, durable)
            .map_err(to_fdo)?;
        for link in removed {
            Self::link_removed(&emitter, link.source, link.relation, link.target)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        Self::link_added(&emitter, added.source, added.relation, added.target)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(())
    }

    async fn get_context_targets(
        &self,
        context: &str,
        relation: &str,
    ) -> zbus::fdo::Result<Vec<String>> {
        self.service
            .context_targets(context, relation)
            .map_err(to_fdo)
    }

    #[zbus(signal)]
    async fn link_added(
        emitter: &SignalEmitter<'_>,
        source: String,
        relation: String,
        target: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn link_removed(
        emitter: &SignalEmitter<'_>,
        source: String,
        relation: String,
        target: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn property_changed(
        emitter: &SignalEmitter<'_>,
        subject: String,
        key: String,
        value: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn property_removed(
        emitter: &SignalEmitter<'_>,
        subject: String,
        key: String,
    ) -> zbus::Result<()>;
}

fn wire_to_option(value: &str) -> Option<&str> {
    (value != NONE_STRING).then_some(value)
}

pub async fn serve(service: LocusService) -> zbus::Result<Connection> {
    Builder::session()?
        .name(crate::api::BUS_NAME)?
        .serve_at(ROOT_PATH, GraphIface::new(service))?
        .build()
        .await
}
