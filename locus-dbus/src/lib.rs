use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use zbus::Connection;
use zbus::connection::Builder;
use zbus::names::InterfaceName;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::{OwnedObjectPath, Value};

use locus_api::{Graph, GraphError, Link, LinkSetChange, PropertyChange, Resolution};
use tracing::{debug, trace};

pub const BUS_NAME: &str = "io.github.Locus";
pub const ROOT_PATH: &str = "/io/github/Locus";
pub const GRAPH_INTERFACE: &str = "io.github.Locus.Graph";
pub const WATCH_INTERFACE: &str = "io.github.Locus.Watch";
pub const WATCH_ROOT_PATH: &str = "/io/github/Locus/Watch";
pub const NONE_STRING: &str = "";
pub const MUTATION_SET_LINK: &str = "set-link";
pub const MUTATION_REMOVE_LINK: &str = "remove-link";
pub const MUTATION_REMOVE_LINKS: &str = "remove-links";
pub const MUTATION_DELETE_NODE: &str = "delete-node";
pub const MUTATION_SET_PROPERTY: &str = "set-property";
pub const MUTATION_REMOVE_PROPERTY: &str = "remove-property";

pub type LinkTuple = (String, String, String);
pub type MutationTuple = (String, String, String, String);

#[derive(Debug, Clone)]
pub struct GraphIface<B> {
    backend: B,
    watches: Arc<Mutex<WatchManager>>,
}

impl<B> GraphIface<B>
where
    B: Graph,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            watches: Arc::new(Mutex::new(WatchManager::default())),
        }
    }

    async fn refresh_watches(&self, conn: &Connection) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.refresh_watches");
        let _guard = span.enter();
        let handles = self
            .watches
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch manager poisoned".to_string()))?
            .handles();
        let mut updates = Vec::new();

        for handle in handles {
            let source = handle.source()?;
            let path = handle.path()?;
            let target = self
                .backend
                .resolve_path(&source, &path)
                .map_err(to_fdo)?
                .unwrap_or_else(|| NONE_STRING.to_string());
            let properties = if target == NONE_STRING {
                BTreeMap::new()
            } else {
                // TODO: Let watch clients declare property keys, either at watch creation
                // or through add/remove methods, so unobserved target properties are not
                // recomputed or signaled through the watch object.
                self.backend
                    .properties(&target)
                    .map(|properties| properties.into_iter().collect())
                    .map_err(to_fdo)?
            };
            let update = handle.set_target_properties(target, properties)?;
            if update.has_changes() {
                updates.push(update);
            }
        }

        let changed_count = updates.len();
        for update in updates {
            emit_watch_changed(conn, update).await?;
        }

        trace!(changed = changed_count, "watch targets refreshed");
        Ok(())
    }
}

#[derive(Debug, Default)]
struct WatchManager {
    next_id: u64,
    watches: BTreeMap<String, WatchHandle>,
}

impl WatchManager {
    fn insert(
        &mut self,
        source: String,
        path: Vec<String>,
        target: String,
        properties: BTreeMap<String, String>,
    ) -> WatchHandle {
        self.next_id += 1;
        let object_path = format!("{WATCH_ROOT_PATH}/{}", self.next_id);
        let state = Arc::new(Mutex::new(WatchState {
            source,
            path,
            target,
            properties,
        }));
        let handle = WatchHandle {
            object_path: object_path.clone(),
            state,
        };
        self.watches.insert(object_path, handle.clone());
        handle
    }

    fn remove(&mut self, object_path: &str) {
        self.watches.remove(object_path);
    }

    fn handles(&self) -> Vec<WatchHandle> {
        self.watches.values().cloned().collect()
    }
}

#[derive(Debug, Clone)]
struct WatchHandle {
    object_path: String,
    state: Arc<Mutex<WatchState>>,
}

impl WatchHandle {
    fn source(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .state
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch state poisoned".to_string()))?
            .source
            .clone())
    }

    fn path(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch state poisoned".to_string()))?
            .path
            .clone())
    }

    fn target(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .state
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch state poisoned".to_string()))?
            .target
            .clone())
    }

    fn properties(&self) -> zbus::fdo::Result<HashMap<String, String>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch state poisoned".to_string()))?
            .properties
            .clone()
            .into_iter()
            .collect())
    }

    fn set_target_properties(
        &self,
        target: String,
        properties: BTreeMap<String, String>,
    ) -> zbus::fdo::Result<WatchUpdate> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch state poisoned".to_string()))?;
        let target_changed = (state.target != target).then(|| target.clone());
        let mut changed = BTreeMap::new();
        let mut removed = Vec::new();

        for (key, value) in &properties {
            if state.properties.get(key) != Some(value) {
                changed.insert(key.clone(), value.clone());
            }
        }
        for key in state.properties.keys() {
            if !properties.contains_key(key) {
                removed.push(key.clone());
            }
        }

        state.target = target;
        state.properties = properties;

        Ok(WatchUpdate {
            object_path: self.object_path.clone(),
            target_changed,
            properties_changed: !changed.is_empty() || !removed.is_empty(),
            properties: state.properties.clone(),
            changed,
            removed,
        })
    }
}

#[derive(Debug)]
struct WatchUpdate {
    object_path: String,
    target_changed: Option<String>,
    properties_changed: bool,
    properties: BTreeMap<String, String>,
    changed: BTreeMap<String, String>,
    removed: Vec<String>,
}

impl WatchUpdate {
    fn has_changes(&self) -> bool {
        self.target_changed.is_some() || self.properties_changed
    }
}

#[derive(Debug)]
struct WatchState {
    source: String,
    path: Vec<String>,
    target: String,
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct WatchIface {
    object_path: String,
    handle: WatchHandle,
    manager: Arc<Mutex<WatchManager>>,
}

#[zbus::interface(name = "io.github.Locus.Watch")]
impl WatchIface {
    #[zbus(property)]
    fn source(&self) -> zbus::fdo::Result<String> {
        self.handle.source()
    }

    #[zbus(property, name = "Path")]
    fn path_property(&self) -> zbus::fdo::Result<Vec<String>> {
        self.handle.path()
    }

    #[zbus(property)]
    fn target(&self) -> zbus::fdo::Result<String> {
        self.handle.target()
    }

    #[zbus(property, name = "Properties")]
    fn properties_property(&self) -> zbus::fdo::Result<HashMap<String, String>> {
        self.handle.properties()
    }

    async fn close(&self, #[zbus(object_server)] server: &ObjectServer) -> zbus::fdo::Result<()> {
        self.manager
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch manager poisoned".to_string()))?
            .remove(&self.object_path);
        server
            .remove::<WatchIface, _>(self.object_path.as_str())
            .await
            .map_err(to_fdo_display)?;
        Ok(())
    }

    #[zbus(signal)]
    async fn properties_updated(
        emitter: &SignalEmitter<'_>,
        changed: HashMap<String, String>,
        removed: Vec<String>,
    ) -> zbus::Result<()>;
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
        let span = tracing::trace_span!("dbus.set_link", source, relation, target);
        let _guard = span.enter();
        let change = self
            .backend
            .set_link(source, relation, target)
            .map_err(to_fdo)?;
        if let LinkSetChange::Changed { removed, added } = change {
            debug!(removed = removed.len(), "link changed");
            emit_link_replacement::<B>(&emitter, removed, added, true).await?;
            self.refresh_watches(emitter.connection()).await?;
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
        let span = tracing::trace_span!("dbus.remove_link", source, relation, target);
        let _guard = span.enter();
        let link = self
            .backend
            .remove_link(source, relation, target)
            .map_err(to_fdo)?;
        Self::link_removed(&emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
        self.refresh_watches(emitter.connection()).await?;
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
        let span = tracing::trace_span!("dbus.remove_links", source, relation);
        let _guard = span.enter();
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
            self.refresh_watches(emitter.connection()).await?;
            emit_resolve_changes::<B>(
                &emitter,
                self.backend.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
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
        let change = self.backend.delete_node(subject).map_err(to_fdo)?;
        let changed = !change.is_empty();
        emit_node_deleted::<B>(&emitter, change).await?;
        if changed {
            self.refresh_watches(emitter.connection()).await?;
            emit_resolve_changes::<B>(
                &emitter,
                self.backend.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
        }
        Ok(())
    }

    async fn get_targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.get_targets", source, relation);
        let _guard = span.enter();
        self.backend.targets(source, relation).map_err(to_fdo)
    }

    async fn get_sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.get_sources", target, relation);
        let _guard = span.enter();
        self.backend.sources(target, relation).map_err(to_fdo)
    }

    async fn get_links(&self, subject: &str) -> zbus::fdo::Result<Vec<LinkTuple>> {
        let span = tracing::trace_span!("dbus.get_links", subject);
        let _guard = span.enter();
        Ok(self
            .backend
            .links(subject)
            .map_err(to_fdo)?
            .into_iter()
            .map(|link| link.to_tuple())
            .collect())
    }

    async fn get_all_links(&self) -> zbus::fdo::Result<Vec<LinkTuple>> {
        let span = tracing::trace_span!("dbus.get_all_links");
        let _guard = span.enter();
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
        let span = tracing::trace_span!("dbus.set_property", subject, key);
        let _guard = span.enter();
        let change = self
            .backend
            .set_property(subject, key, value)
            .map_err(to_fdo)?;
        if change == PropertyChange::Changed {
            debug!(subject, key, "property changed");
            Self::property_changed(
                &emitter,
                subject.to_string(),
                key.to_string(),
                value.to_string(),
            )
            .await
            .map_err(to_fdo_display)?;
            self.refresh_watches(emitter.connection()).await?;
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
        let span = tracing::trace_span!("dbus.remove_property", subject, key);
        let _guard = span.enter();
        self.backend.remove_property(subject, key).map_err(to_fdo)?;
        Self::property_removed(&emitter, subject.to_string(), key.to_string())
            .await
            .map_err(to_fdo_display)?;
        self.refresh_watches(emitter.connection()).await?;
        emit_resolve_changes::<B>(
            &emitter,
            self.backend.refresh_resolutions().map_err(to_fdo)?,
        )
        .await?;
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

        for (operation, first, second, third) in mutations {
            match operation.as_str() {
                MUTATION_SET_LINK => {
                    let change = self
                        .backend
                        .set_link(&first, &second, &third)
                        .map_err(to_fdo)?;
                    if let LinkSetChange::Changed { removed, added } = change {
                        emit_link_replacement::<B>(&emitter, removed, added, true).await?;
                        changed = true;
                    }
                }
                MUTATION_REMOVE_LINK => {
                    let link = self
                        .backend
                        .remove_link(&first, &second, &third)
                        .map_err(to_fdo)?;
                    Self::link_removed(&emitter, link.source, link.relation, link.target)
                        .await
                        .map_err(to_fdo_display)?;
                    changed = true;
                }
                MUTATION_REMOVE_LINKS => {
                    let links = self.backend.remove_links(&first, &second).map_err(to_fdo)?;
                    changed |= !links.is_empty();
                    for link in links {
                        Self::link_removed(&emitter, link.source, link.relation, link.target)
                            .await
                            .map_err(to_fdo_display)?;
                    }
                }
                MUTATION_DELETE_NODE => {
                    let change = self.backend.delete_node(&first).map_err(to_fdo)?;
                    changed |= !change.is_empty();
                    emit_node_deleted::<B>(&emitter, change).await?;
                }
                MUTATION_SET_PROPERTY => {
                    let change = self
                        .backend
                        .set_property(&first, &second, &third)
                        .map_err(to_fdo)?;
                    if change == PropertyChange::Changed {
                        Self::property_changed(&emitter, first, second, third)
                            .await
                            .map_err(to_fdo_display)?;
                        changed = true;
                    }
                }
                MUTATION_REMOVE_PROPERTY => {
                    self.backend
                        .remove_property(&first, &second)
                        .map_err(to_fdo)?;
                    Self::property_removed(&emitter, first, second)
                        .await
                        .map_err(to_fdo_display)?;
                    changed = true;
                }
                _ => {
                    return Err(zbus::fdo::Error::InvalidArgs(format!(
                        "unknown mutation operation: {operation}"
                    )));
                }
            }
        }

        if changed {
            self.refresh_watches(emitter.connection()).await?;
            emit_resolve_changes::<B>(
                &emitter,
                self.backend.refresh_resolutions().map_err(to_fdo)?,
            )
            .await?;
        }
        Ok(())
    }

    async fn get_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.get_property", subject, key);
        let _guard = span.enter();
        Ok(self
            .backend
            .property(subject, key)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn get_properties(&self, subject: &str) -> zbus::fdo::Result<HashMap<String, String>> {
        let span = tracing::trace_span!("dbus.get_properties", subject);
        let _guard = span.enter();
        self.backend.properties(subject).map_err(to_fdo)
    }

    async fn get_subjects(&self) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.get_subjects");
        let _guard = span.enter();
        self.backend.subjects().map_err(to_fdo)
    }

    async fn find_subjects(&self, key: &str, value: &str) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.find_subjects", key, value);
        let _guard = span.enter();
        self.backend
            .subjects_with_property(key, wire_to_option(value))
            .map_err(to_fdo)
    }

    async fn resolve(&self, source: &str, path: Vec<String>) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.resolve", source, path = ?path);
        let _guard = span.enter();
        Ok(self
            .backend
            .resolve_path(source, &path)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn resolve_all(&self, source: &str, path: Vec<String>) -> zbus::fdo::Result<Vec<String>> {
        let span = tracing::trace_span!("dbus.resolve_all", source, path = ?path);
        let _guard = span.enter();
        self.backend.resolve_all(source, &path).map_err(to_fdo)
    }

    async fn find_nearest(&self, source: &str, kind: &str) -> zbus::fdo::Result<String> {
        let span = tracing::trace_span!("dbus.find_nearest", source, kind);
        let _guard = span.enter();
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
        let span = tracing::trace_span!("dbus.subscribe_resolve", source, path = ?path);
        let _guard = span.enter();
        Ok(self
            .backend
            .subscribe_resolution(source, &path)
            .map_err(to_fdo)?
            .target
            .unwrap_or_else(|| NONE_STRING.to_string()))
    }

    async fn watch_node(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let span = tracing::trace_span!("dbus.watch_node", source, path = ?path);
        let _guard = span.enter();
        let target = self
            .backend
            .resolve_path(source, &path)
            .map_err(to_fdo)?
            .unwrap_or_else(|| NONE_STRING.to_string());
        let properties = if target == NONE_STRING {
            BTreeMap::new()
        } else {
            self.backend
                .properties(&target)
                .map(|properties| properties.into_iter().collect())
                .map_err(to_fdo)?
        };
        let handle = self
            .watches
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch manager poisoned".to_string()))?
            .insert(source.to_string(), path, target, properties);
        let object_path = handle.object_path.clone();
        let watch = WatchIface {
            object_path: object_path.clone(),
            handle,
            manager: self.watches.clone(),
        };
        server
            .at(object_path.as_str(), watch)
            .await
            .map_err(to_fdo_display)?;
        OwnedObjectPath::try_from(object_path).map_err(to_fdo_display)
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

async fn emit_node_deleted<B>(
    emitter: &SignalEmitter<'_>,
    change: locus_api::DeleteNodeChange,
) -> zbus::fdo::Result<()>
where
    B: Graph + 'static,
{
    for link in change.removed_links {
        GraphIface::<B>::link_removed(emitter, link.source, link.relation, link.target)
            .await
            .map_err(to_fdo_display)?;
    }
    for (subject, key) in change.removed_properties {
        GraphIface::<B>::property_removed(emitter, subject, key)
            .await
            .map_err(to_fdo_display)?;
    }
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

async fn emit_watch_changed(conn: &Connection, update: WatchUpdate) -> zbus::fdo::Result<()> {
    let emitter = SignalEmitter::new(conn, update.object_path.as_str()).map_err(to_fdo_display)?;
    let interface = InterfaceName::try_from(WATCH_INTERFACE).map_err(to_fdo_display)?;
    let mut changed_properties = HashMap::new();

    if let Some(target) = update.target_changed {
        changed_properties.insert("Target", Value::from(target));
    }
    if update.properties_changed {
        changed_properties.insert(
            "Properties",
            Value::from(
                update
                    .properties
                    .clone()
                    .into_iter()
                    .collect::<HashMap<_, _>>(),
            ),
        );
    }
    if !changed_properties.is_empty() {
        zbus::fdo::Properties::properties_changed(
            &emitter,
            interface,
            changed_properties,
            (&[]).into(),
        )
        .await
        .map_err(to_fdo_display)?;
    }
    if update.properties_changed {
        WatchIface::properties_updated(
            &emitter,
            update.changed.into_iter().collect(),
            update.removed,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn watch_update_reports_changed_and_removed_properties() {
        let mut manager = WatchManager::default();
        let handle = manager.insert(
            "context:selected".to_string(),
            vec!["window".to_string()],
            "window:1".to_string(),
            properties(&[("title", "Old"), ("app-id", "term")]),
        );

        let update = handle
            .set_target_properties(
                "window:1".to_string(),
                properties(&[("title", "New"), ("urgent", "true")]),
            )
            .unwrap();

        assert_eq!(update.target_changed, None);
        assert_eq!(
            update.changed,
            properties(&[("title", "New"), ("urgent", "true")])
        );
        assert_eq!(update.removed, vec!["app-id".to_string()]);
    }

    #[test]
    fn watch_update_reports_target_and_property_changes_together() {
        let mut manager = WatchManager::default();
        let handle = manager.insert(
            "context:selected".to_string(),
            vec!["window".to_string()],
            "window:1".to_string(),
            properties(&[("title", "Old")]),
        );

        let update = handle
            .set_target_properties("window:2".to_string(), properties(&[("title", "New")]))
            .unwrap();

        assert_eq!(update.target_changed, Some("window:2".to_string()));
        assert_eq!(update.changed, properties(&[("title", "New")]));
        assert_eq!(update.removed, Vec::<String>::new());
    }

    #[test]
    fn watch_update_ignores_unchanged_properties() {
        let mut manager = WatchManager::default();
        let handle = manager.insert(
            "context:selected".to_string(),
            vec!["window".to_string()],
            "window:1".to_string(),
            properties(&[("title", "Same")]),
        );

        let update = handle
            .set_target_properties("window:1".to_string(), properties(&[("title", "Same")]))
            .unwrap();

        assert!(!update.has_changes());
    }
}
