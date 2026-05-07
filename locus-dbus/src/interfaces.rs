use std::collections::HashMap;

use locus_core::{DeleteNodeChange, Link, LinkSetChange, LocusService, PropertyChange, Resolution};
use tracing::debug;
use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::OwnedObjectPath;

use crate::error::to_fdo_display;
use crate::state::GraphState;
use crate::{
    BUS_NAME, LinkTuple, MUTATION_DELETE_NODE, MUTATION_REMOVE_LINK, MUTATION_REMOVE_LINKS,
    MUTATION_REMOVE_PROPERTY, MUTATION_SET_LINK, MUTATION_SET_PROPERTY, MutationTuple, NONE_STRING,
    ROOT_PATH,
};

#[derive(Clone)]
pub struct GraphReadIface {
    state: GraphState,
}

#[derive(Clone)]
pub struct GraphWriteIface {
    state: GraphState,
}

#[derive(Clone)]
pub struct GraphResolveIface {
    state: GraphState,
}

impl GraphWriteIface {
    async fn refresh_dependents(&self, emitter: &SignalEmitter<'_>) -> zbus::fdo::Result<()> {
        self.state.refresh_watches(emitter.connection()).await?;
        emit_resolve_changes(emitter, self.state.refresh_resolutions()?).await
    }

    async fn set_link_changed(
        &self,
        emitter: &SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<bool> {
        let change = self.state.set_link(source, relation, target)?;
        let LinkSetChange::Changed { removed, added } = change else {
            return Ok(false);
        };
        debug!(removed = removed.len(), "link changed");
        emit_link_replacement(emitter, removed, added, true).await?;
        Ok(true)
    }

    async fn remove_link_changed(
        &self,
        emitter: &SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<bool> {
        let link = self.state.remove_link(source, relation, target)?;
        Self::link_removed(emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
        Ok(true)
    }

    async fn remove_links_changed(
        &self,
        emitter: &SignalEmitter<'_>,
        source: &str,
        relation: &str,
    ) -> zbus::fdo::Result<bool> {
        let links = self.state.remove_links(source, relation)?;
        let changed = !links.is_empty();
        for link in links {
            Self::link_removed(emitter, link.source, link.relation, link.target)
                .await
                .map_err(to_fdo_display)?;
        }
        Ok(changed)
    }

    async fn delete_node_changed(
        &self,
        emitter: &SignalEmitter<'_>,
        subject: &str,
    ) -> zbus::fdo::Result<bool> {
        let change = self.state.delete_node(subject)?;
        let changed = !change.is_empty();
        emit_node_deleted(emitter, change).await?;
        Ok(changed)
    }

    async fn set_property_changed(
        &self,
        emitter: &SignalEmitter<'_>,
        subject: &str,
        key: &str,
        value: &str,
    ) -> zbus::fdo::Result<bool> {
        let change = self.state.set_property(subject, key, value)?;
        if change != PropertyChange::Changed {
            return Ok(false);
        }
        debug!(subject, key, "property changed");
        Self::property_changed(
            emitter,
            subject.to_string(),
            key.to_string(),
            value.to_string(),
        )
        .await
        .map_err(to_fdo_display)?;
        Ok(true)
    }

    async fn remove_property_changed(
        &self,
        emitter: &SignalEmitter<'_>,
        subject: &str,
        key: &str,
    ) -> zbus::fdo::Result<bool> {
        self.state.remove_property(subject, key)?;
        Self::property_removed(emitter, subject.to_string(), key.to_string())
            .await
            .map_err(to_fdo_display)?;
        Ok(true)
    }

    async fn apply_mutation(
        &self,
        emitter: &SignalEmitter<'_>,
        (operation, first, second, third): MutationTuple,
    ) -> zbus::fdo::Result<bool> {
        match operation.as_str() {
            MUTATION_SET_LINK => {
                self.set_link_changed(emitter, &first, &second, &third)
                    .await
            }
            MUTATION_REMOVE_LINK => {
                self.remove_link_changed(emitter, &first, &second, &third)
                    .await
            }
            MUTATION_REMOVE_LINKS => self.remove_links_changed(emitter, &first, &second).await,
            MUTATION_DELETE_NODE => self.delete_node_changed(emitter, &first).await,
            MUTATION_SET_PROPERTY => {
                self.set_property_changed(emitter, &first, &second, &third)
                    .await
            }
            MUTATION_REMOVE_PROPERTY => {
                self.remove_property_changed(emitter, &first, &second).await
            }
            _ => Err(zbus::fdo::Error::InvalidArgs(format!(
                "unknown mutation operation: {operation}"
            ))),
        }
    }
}

#[zbus::interface(
    name = "io.github.Locus.Graph.Read",
    proxy(
        default_service = "io.github.Locus",
        default_path = "/io/github/Locus",
        gen_blocking = false,
        async_name = "GraphReadProxy",
        visibility = "pub"
    )
)]
impl GraphReadIface {
    async fn get_targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.get_targets", source, relation);
        let _guard = span.enter();
        self.state.targets(source, relation)
    }

    async fn get_sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.get_sources", target, relation);
        let _guard = span.enter();
        self.state.sources(target, relation)
    }

    async fn get_links(&self, subject: &str) -> zbus::fdo::Result<Vec<LinkTuple>> {
        let span = tracing::trace_span!("dbus.get_links", subject);
        let _guard = span.enter();
        self.state.links(subject)
    }

    async fn get_all_links(&self) -> zbus::fdo::Result<Vec<LinkTuple>> {
        let span = tracing::trace_span!("dbus.get_all_links");
        let _guard = span.enter();
        self.state.all_links()
    }

    async fn get_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.get_property", subject, key);
        let _guard = span.enter();
        self.state.property(subject, key)
    }

    async fn get_properties(&self, subject: &str) -> zbus::fdo::Result<HashMap<String, String>> {
        let span = tracing::trace_span!("dbus.get_properties", subject);
        let _guard = span.enter();
        self.state.properties(subject)
    }

    async fn get_subjects(&self) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.get_subjects");
        let _guard = span.enter();
        self.state.subjects()
    }

    async fn find_subjects(&self, key: &str, value: &str) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.find_subjects", key, value);
        let _guard = span.enter();
        self.state.find_subjects(key, value)
    }
}

#[zbus::interface(
    name = "io.github.Locus.Graph.Write",
    proxy(
        default_service = "io.github.Locus",
        default_path = "/io/github/Locus",
        gen_blocking = false,
        async_name = "GraphWriteProxy",
        visibility = "pub"
    )
)]
impl GraphWriteIface {
    async fn set_link(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.set_link", source, relation, target);
        let _guard = span.enter();
        if self
            .set_link_changed(&emitter, source, relation, target)
            .await?
        {
            self.refresh_dependents(&emitter).await?;
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
        let span = tracing::trace_span!("dbus.remove_link", source, relation, target);
        let _guard = span.enter();
        if self
            .remove_link_changed(&emitter, source, relation, target)
            .await?
        {
            self.refresh_dependents(&emitter).await?;
        }
        Ok(())
    }

    async fn remove_links(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        source: &str,
        relation: &str,
    ) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.remove_links", source, relation);
        let _guard = span.enter();
        if self
            .remove_links_changed(&emitter, source, relation)
            .await?
        {
            self.refresh_dependents(&emitter).await?;
        }
        Ok(())
    }

    async fn delete_node(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        subject: &str,
    ) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.delete_node", subject);
        let _guard = span.enter();
        if self.delete_node_changed(&emitter, subject).await? {
            self.refresh_dependents(&emitter).await?;
        }
        Ok(())
    }

    async fn set_property(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        subject: &str,
        key: &str,
        value: &str,
    ) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.set_property", subject, key);
        let _guard = span.enter();
        if self
            .set_property_changed(&emitter, subject, key, value)
            .await?
        {
            self.refresh_dependents(&emitter).await?;
        }
        Ok(())
    }

    async fn remove_property(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        subject: &str,
        key: &str,
    ) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.remove_property", subject, key);
        let _guard = span.enter();
        if self.remove_property_changed(&emitter, subject, key).await? {
            self.refresh_dependents(&emitter).await?;
        }
        Ok(())
    }

    async fn apply_mutations(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        mutations: Vec<MutationTuple>,
    ) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.apply_mutations", count = mutations.len());
        let _guard = span.enter();
        let mut changed = false;

        for mutation in mutations {
            changed |= self.apply_mutation(&emitter, mutation).await?;
        }

        if changed {
            self.refresh_dependents(&emitter).await?;
        }
        Ok(())
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
}

#[zbus::interface(
    name = "io.github.Locus.Graph.Resolve",
    proxy(
        default_service = "io.github.Locus",
        default_path = "/io/github/Locus",
        gen_blocking = false,
        async_name = "GraphResolveProxy",
        visibility = "pub"
    )
)]
impl GraphResolveIface {
    async fn resolve(&self, source: &str, path: Vec<String>) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.resolve", source, path = ?path);
        let _guard = span.enter();
        self.state.resolve(source, &path)
    }

    async fn resolve_all(&self, source: &str, path: Vec<String>) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.resolve_all", source, path = ?path);
        let _guard = span.enter();
        self.state.resolve_all(source, &path)
    }

    async fn find_nearest(&self, source: &str, kind: &str) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.find_nearest", source, kind);
        let _guard = span.enter();
        self.state.find_nearest(source, kind)
    }

    async fn subscribe_resolve(
        &self,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.subscribe_resolve", source, path = ?path);
        let _guard = span.enter();
        self.state.subscribe_resolve(source, &path)
    }

    async fn watch_node(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let span = tracing::trace_span!("dbus.watch_node", source, path = ?path);
        let _guard = span.enter();
        self.state.watch_node(server, source, path).await
    }

    #[zbus(signal)]
    async fn resolve_changed(
        emitter: &SignalEmitter<'_>,
        source: String,
        path: Vec<String>,
        target: String,
    ) -> zbus::Result<()>;
}

async fn emit_link_replacement(
    emitter: &SignalEmitter<'_>,
    removed: Vec<Link>,
    added: Link,
    emit_set: bool,
) -> zbus::fdo::Result<()> {
    if emit_set {
        let old_targets = removed
            .iter()
            .map(|link| link.target.clone())
            .collect::<Vec<_>>();
        GraphWriteIface::link_set(
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
        GraphWriteIface::link_removed(emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
    }
    GraphWriteIface::link_added(emitter, added.source, added.relation, added.target)
        .await
        .map_err(to_fdo_display)?;
    Ok(())
}

async fn emit_node_deleted(
    emitter: &SignalEmitter<'_>,
    change: DeleteNodeChange,
) -> zbus::fdo::Result<()> {
    for link in change.removed_links {
        GraphWriteIface::link_removed(emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
    }
    for (subject, key) in change.removed_properties {
        GraphWriteIface::property_removed(emitter, subject, key)
            .await
            .map_err(to_fdo_display)?;
    }
    Ok(())
}

async fn emit_resolve_changes(
    emitter: &SignalEmitter<'_>,
    resolutions: Vec<Resolution>,
) -> zbus::fdo::Result<()> {
    for resolution in resolutions {
        GraphResolveIface::resolve_changed(
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

pub async fn serve(service: LocusService) -> zbus::Result<Connection> {
    let state = GraphState::new(service);
    Builder::session()?
        .name(BUS_NAME)?
        .serve_at(
            ROOT_PATH,
            GraphReadIface {
                state: state.clone(),
            },
        )?
        .serve_at(
            ROOT_PATH,
            GraphWriteIface {
                state: state.clone(),
            },
        )?
        .serve_at(ROOT_PATH, GraphResolveIface { state })?
        .build()
        .await
}
