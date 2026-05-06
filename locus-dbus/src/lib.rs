use std::collections::HashMap;
use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::SignalEmitter;

use locus_api::{Graph, GraphError, Link, LinkSetChange, PropertyChange, Resolution};

pub const BUS_NAME: &str = "io.github.Locus";
pub const ROOT_PATH: &str = "/io/github/Locus";
pub const GRAPH_INTERFACE: &str = "io.github.Locus.Graph";
pub const NONE_STRING: &str = "";

pub type LinkTuple = (String, String, String);

#[derive(Debug, Clone)]
pub struct GraphIface<B> {
    backend: B,
}

impl<B> GraphIface<B>
where
    B: Graph,
{
    pub fn new(backend: B) -> Self {
        Self { backend }
    }
}

#[zbus::interface(
    name = "io.github.Locus.Graph",
    proxy(
        default_service = "io.github.Locus",
        default_path = "/io/github/Locus",
        gen_blocking = false,
        async_name = "GraphProxy",
        visibility = "pub"
    )
)]
impl<B> GraphIface<B>
where
    B: Graph + 'static,
{
    async fn set_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        let change = self
            .backend
            .set_link(source, relation, target)
            .map_err(to_fdo)?;
        if let LinkSetChange::Changed { removed, added } = change {
            emit_link_replacement::<B>(&emitter, removed, added, true).await?;
            emit_resolve_changes::<B>(
                &emitter,
                self.backend.refresh_resolutions().map_err(to_fdo)?,
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
        let link = self
            .backend
            .remove_link(source, relation, target)
            .map_err(to_fdo)?;
        Self::link_removed(&emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
        emit_resolve_changes::<B>(
            &emitter,
            self.backend.refresh_resolutions().map_err(to_fdo)?,
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
        let links = self
            .backend
            .remove_links(source, relation)
            .map_err(to_fdo)?;
        let changed = !links.is_empty();
        for link in links {
            Self::link_removed(&emitter, link.source, link.relation, link.target)
                .await
                .map_err(to_fdo_display)?;
        }
        if changed {
            emit_resolve_changes::<B>(
                &emitter,
                self.backend.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
        }
        Ok(())
    }

    async fn get_targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.backend.targets(source, relation).map_err(to_fdo)
    }

    async fn get_sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.backend.sources(target, relation).map_err(to_fdo)
    }

    async fn get_links(&self, subject: &str) -> zbus::fdo::Result<Vec<LinkTuple>> {
        Ok(self
            .backend
            .links(subject)
            .map_err(to_fdo)?
            .into_iter()
            .map(|link| link.to_tuple())
            .collect())
    }

    async fn get_all_links(&self) -> zbus::fdo::Result<Vec<LinkTuple>> {
        Ok(self
            .backend
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
        let change = self
            .backend
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
            .map_err(to_fdo_display)?;
            emit_resolve_changes::<B>(
                &emitter,
                self.backend.refresh_resolutions().map_err(to_fdo)?,
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
        self.backend.remove_property(subject, key).map_err(to_fdo)?;
        Self::property_removed(&emitter, subject.to_string(), key.to_string())
            .await
            .map_err(to_fdo_display)?;
        emit_resolve_changes::<B>(
            &emitter,
            self.backend.refresh_resolutions().map_err(to_fdo)?,
        )
        .await?;
        Ok(())
    }

    async fn get_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<String> {
        Ok(self
            .backend
            .property(subject, key)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn get_properties(&self, subject: &str) -> zbus::fdo::Result<HashMap<String, String>> {
        self.backend.properties(subject).map_err(to_fdo)
    }

    async fn get_subjects(&self) -> zbus::fdo::Result<Vec<String>> {
        self.backend.subjects().map_err(to_fdo)
    }

    async fn find_subjects(&self, key: &str, value: &str) -> zbus::fdo::Result<Vec<String>> {
        self.backend
            .subjects_with_property(key, wire_to_option(value))
            .map_err(to_fdo)
    }

    async fn resolve(&self, source: &str, path: Vec<String>) -> zbus::fdo::Result<String> {
        Ok(self
            .backend
            .resolve_path(source, &path)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn resolve_all(&self, source: &str, path: Vec<String>) -> zbus::fdo::Result<Vec<String>> {
        self.backend.resolve_all(source, &path).map_err(to_fdo)
    }

    async fn find_nearest(&self, source: &str, kind: &str) -> zbus::fdo::Result<String> {
        Ok(self
            .backend
            .resolve_kind(source, kind)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn subscribe_resolve(
        &self,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<String> {
        Ok(self
            .backend
            .subscribe_resolution(source, &path)
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
        path: Vec<String>,
        target: String,
    ) -> zbus::Result<()>;
}

pub type LocusClient<'a> = GraphProxy<'a>;
pub type Client<'a> = GraphProxy<'a>;

#[allow(async_fn_in_trait)]
pub trait ClientExt {
    async fn property_opt(&self, subject: &str, key: &str) -> zbus::fdo::Result<Option<String>>;

    async fn find_subjects_opt(
        &self,
        key: &str,
        value: Option<&str>,
    ) -> zbus::fdo::Result<Vec<String>>;

    async fn resolve_opt(
        &self,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<Option<String>>;

    async fn find_nearest_opt(&self, source: &str, kind: &str)
    -> zbus::fdo::Result<Option<String>>;

    async fn subscribe_resolve_opt(
        &self,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<Option<String>>;

    async fn set_context_link(
        &self,
        context: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()>;

    async fn context_targets(
        &self,
        context: &str,
        relation: &str,
    ) -> zbus::fdo::Result<Vec<String>>;
}

impl ClientExt for GraphProxy<'_> {
    async fn property_opt(&self, subject: &str, key: &str) -> zbus::fdo::Result<Option<String>> {
        wire_result_to_option(self.get_property(subject, key).await)
    }

    async fn find_subjects_opt(
        &self,
        key: &str,
        value: Option<&str>,
    ) -> zbus::fdo::Result<Vec<String>> {
        self.find_subjects(key, value.unwrap_or(NONE_STRING)).await
    }

    async fn resolve_opt(
        &self,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<Option<String>> {
        wire_result_to_option(self.resolve(source, path).await)
    }

    async fn find_nearest_opt(
        &self,
        source: &str,
        kind: &str,
    ) -> zbus::fdo::Result<Option<String>> {
        wire_result_to_option(self.find_nearest(source, kind).await)
    }

    async fn subscribe_resolve_opt(
        &self,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<Option<String>> {
        wire_result_to_option(self.subscribe_resolve(source, path).await)
    }

    async fn set_context_link(
        &self,
        context: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        self.set_link(&context_subject(context), relation, target)
            .await
    }

    async fn context_targets(
        &self,
        context: &str,
        relation: &str,
    ) -> zbus::fdo::Result<Vec<String>> {
        self.get_targets(&context_subject(context), relation).await
    }
}

async fn emit_link_replacement<B>(
    emitter: &SignalEmitter<'_>,
    removed: Vec<Link>,
    added: Link,
    emit_set: bool,
) -> zbus::fdo::Result<()>
where
    B: Graph + 'static,
{
    if emit_set {
        let old_targets = removed
            .iter()
            .map(|link| link.target.clone())
            .collect::<Vec<_>>();
        GraphIface::<B>::link_set(
            emitter,
            added.source.clone(),
            added.relation.clone(),
            old_targets,
            added.target.clone(),
        )
        .await
        .map_err(to_fdo_display)?;
    }

    for link in removed {
        GraphIface::<B>::link_removed(emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
    }
    GraphIface::<B>::link_added(emitter, added.source, added.relation, added.target)
        .await
        .map_err(to_fdo_display)?;
    Ok(())
}

async fn emit_resolve_changes<B>(
    emitter: &SignalEmitter<'_>,
    resolutions: Vec<Resolution>,
) -> zbus::fdo::Result<()>
where
    B: Graph + 'static,
{
    for resolution in resolutions {
        GraphIface::<B>::resolve_changed(
            emitter,
            resolution.source,
            resolution.path,
            resolution.target.unwrap_or_else(|| NONE_STRING.to_string()),
        )
        .await
        .map_err(to_fdo_display)?;
    }
    Ok(())
}

fn to_fdo(error: GraphError) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

fn to_fdo_display(error: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

pub fn wire_result_to_option(
    result: zbus::fdo::Result<String>,
) -> zbus::fdo::Result<Option<String>> {
    result.map(|value| (value != NONE_STRING).then_some(value))
}

fn wire_to_option(value: &str) -> Option<&str> {
    (value != NONE_STRING).then_some(value)
}

fn context_subject(context: &str) -> String {
    format!("context:{context}")
}

pub async fn serve<G>(graph: G) -> zbus::Result<Connection>
where
    G: Graph + 'static,
{
    Builder::session()?
        .name(BUS_NAME)?
        .serve_at(ROOT_PATH, GraphIface::new(graph))?
        .build()
        .await
}
