use crate::api::{NONE_STRING, ROOT_PATH};
use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::SignalEmitter;

use crate::service::{LinkChange, LinkSetChange, LocusService, PropertyChange, ServiceError};

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

#[zbus::interface(
    name = "io.github.Locus.Graph",
    proxy(
        default_service = "io.github.Locus",
        default_path = "/io/github/Locus",
        gen_blocking = false,
        visibility = "pub"
    )
)]
impl GraphIface {
    async fn add_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        eprintln!("locusd: AddLink source={source:?} relation={relation:?} target={target:?}");
        let change = self
            .service
            .add_link(source, relation, target)
            .map_err(to_fdo)?;
        if let LinkChange::Changed(link) = change {
            Self::link_added(&emitter, link.source, link.relation, link.target)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
            emit_resolve_changes(
                &emitter,
                self.service.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
        }
        Ok(())
    }

    async fn set_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        eprintln!("locusd: SetLink source={source:?} relation={relation:?} target={target:?}");
        let change = self
            .service
            .set_link(source, relation, target)
            .map_err(to_fdo)?;
        if let LinkSetChange::Changed { removed, added } = change {
            emit_link_replacement(&emitter, removed, added, true).await?;
            emit_resolve_changes(
                &emitter,
                self.service.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
        }
        Ok(())
    }

    async fn remove_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        eprintln!("locusd: RemoveLink source={source:?} relation={relation:?} target={target:?}");
        let link = self
            .service
            .remove_link(source, relation, target)
            .map_err(to_fdo)?;
        Self::link_removed(&emitter, link.source, link.relation, link.target)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        emit_resolve_changes(
            &emitter,
            self.service.refresh_resolutions().map_err(to_fdo)?,
        )
        .await?;
        Ok(())
    }

    async fn remove_links(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
    ) -> zbus::fdo::Result<()> {
        eprintln!("locusd: RemoveLinks source={source:?} relation={relation:?}");
        let links = self
            .service
            .remove_links(source, relation)
            .map_err(to_fdo)?;
        let changed = !links.is_empty();
        for link in links {
            Self::link_removed(&emitter, link.source, link.relation, link.target)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        if changed {
            emit_resolve_changes(
                &emitter,
                self.service.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
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

    async fn get_all_links(&self) -> zbus::fdo::Result<Vec<(String, String, String)>> {
        Ok(self
            .service
            .all_links()
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
    ) -> zbus::fdo::Result<()> {
        eprintln!("locusd: SetProperty subject={subject:?} key={key:?} value={value:?}");
        let change = self
            .service
            .set_property(subject, key, value)
            .map_err(to_fdo)?;
        if change == PropertyChange::Changed {
            Self::property_changed(
                &emitter,
                subject.to_string(),
                key.to_string(),
                value.to_string(),
            )
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
            emit_resolve_changes(
                &emitter,
                self.service.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
        }
        Ok(())
    }

    async fn remove_property(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        subject: &str,
        key: &str,
    ) -> zbus::fdo::Result<()> {
        eprintln!("locusd: RemoveProperty subject={subject:?} key={key:?}");
        self.service.remove_property(subject, key).map_err(to_fdo)?;
        Self::property_removed(&emitter, subject.to_string(), key.to_string())
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        emit_resolve_changes(
            &emitter,
            self.service.refresh_resolutions().map_err(to_fdo)?,
        )
        .await?;
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

    async fn get_subjects(&self) -> zbus::fdo::Result<Vec<String>> {
        self.service.subjects().map_err(to_fdo)
    }

    async fn find_subjects(&self, key: &str, value: &str) -> zbus::fdo::Result<Vec<String>> {
        self.service
            .subjects_with_property(key, wire_to_option(value))
            .map_err(to_fdo)
    }

    async fn resolve(&self, source: &str, kind: &str) -> zbus::fdo::Result<String> {
        Ok(self
            .service
            .resolve_kind(source, kind)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn subscribe_resolve(&self, source: &str, kind: &str) -> zbus::fdo::Result<String> {
        Ok(self
            .service
            .subscribe_resolution(source, kind)
            .map_err(to_fdo)?
            .target
            .unwrap_or_else(|| NONE_STRING.to_string()))
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
    async fn link_set(
        emitter: &SignalEmitter<'_>,
        source: String,
        relation: String,
        old_targets: Vec<String>,
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

    #[zbus(signal)]
    async fn resolve_changed(
        emitter: &SignalEmitter<'_>,
        source: String,
        kind: String,
        target: String,
    ) -> zbus::Result<()>;
}

async fn emit_link_replacement(
    emitter: &SignalEmitter<'_>,
    removed: Vec<crate::state::Link>,
    added: crate::state::Link,
    emit_set: bool,
) -> zbus::fdo::Result<()> {
    if emit_set {
        let old_targets = removed
            .iter()
            .map(|link| link.target.clone())
            .collect::<Vec<_>>();
        GraphIface::link_set(
            emitter,
            added.source.clone(),
            added.relation.clone(),
            old_targets,
            added.target.clone(),
        )
        .await
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    }

    for link in removed {
        GraphIface::link_removed(emitter, link.source, link.relation, link.target)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    }
    GraphIface::link_added(emitter, added.source, added.relation, added.target)
        .await
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    Ok(())
}

async fn emit_resolve_changes(
    emitter: &SignalEmitter<'_>,
    resolutions: Vec<crate::service::Resolution>,
) -> zbus::fdo::Result<()> {
    for resolution in resolutions {
        GraphIface::resolve_changed(
            emitter,
            resolution.source,
            resolution.kind,
            resolution.target.unwrap_or_else(|| NONE_STRING.to_string()),
        )
        .await
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    }
    Ok(())
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
