use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use locus_core::{DeleteNodeChange, Link, LinkSetChange, LocusService, PropertyChange, Resolution};
use tracing::trace;
use zbus::Connection;
use zbus::object_server::ObjectServer;
use zbus::zvariant::OwnedObjectPath;

use crate::error::{to_fdo, to_fdo_display};
use crate::watch::{WatchIface, WatchManager, emit_watch_changed, lock_watch};
use crate::{LinkTuple, NONE_STRING};

#[derive(Clone)]
pub(crate) struct GraphState {
    service: LocusService,
    watches: Arc<Mutex<WatchManager>>,
}

impl GraphState {
    pub(crate) fn new(service: LocusService) -> Self {
        Self {
            service,
            watches: Arc::new(Mutex::new(WatchManager::default())),
        }
    }

    pub(crate) fn targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.service.targets(source, relation).map_err(to_fdo)
    }

    pub(crate) fn sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.service.sources(target, relation).map_err(to_fdo)
    }

    pub(crate) fn links(&self, subject: &str) -> zbus::fdo::Result<Vec<LinkTuple>> {
        self.service.links(subject).map_err(to_fdo).map(link_tuples)
    }

    pub(crate) fn all_links(&self) -> zbus::fdo::Result<Vec<LinkTuple>> {
        self.service.all_links().map_err(to_fdo).map(link_tuples)
    }

    pub(crate) fn property(&self, subject: &str, key: &str) -> zbus::fdo::Result<String> {
        self.service
            .property(subject, key)
            .map_err(to_fdo)
            .map(option_to_wire)
    }

    pub(crate) fn properties(&self, subject: &str) -> zbus::fdo::Result<HashMap<String, String>> {
        self.service
            .properties(subject)
            .map(|properties| properties.into_iter().collect())
            .map_err(to_fdo)
    }

    pub(crate) fn subjects(&self) -> zbus::fdo::Result<Vec<String>> {
        self.service.subjects().map_err(to_fdo)
    }

    pub(crate) fn find_subjects(&self, key: &str, value: &str) -> zbus::fdo::Result<Vec<String>> {
        self.service
            .subjects_with_property(key, wire_to_option(value))
            .map_err(to_fdo)
    }

    pub(crate) fn resolve(&self, source: &str, path: &[String]) -> zbus::fdo::Result<String> {
        self.service
            .resolve_path(source, path)
            .map_err(to_fdo)
            .map(option_to_wire)
    }

    pub(crate) fn resolve_all(
        &self,
        source: &str,
        path: &[String],
    ) -> zbus::fdo::Result<Vec<String>> {
        self.service.resolve_all(source, path).map_err(to_fdo)
    }

    pub(crate) fn find_nearest(&self, source: &str, kind: &str) -> zbus::fdo::Result<String> {
        self.service
            .resolve_kind(source, kind)
            .map_err(to_fdo)
            .map(option_to_wire)
    }

    pub(crate) fn subscribe_resolve(
        &self,
        source: &str,
        path: &[String],
    ) -> zbus::fdo::Result<String> {
        self.service
            .subscribe_resolution(source, path)
            .map_err(to_fdo)
            .map(|resolution| option_to_wire(resolution.target))
    }

    pub(crate) fn refresh_resolutions(&self) -> zbus::fdo::Result<Vec<Resolution>> {
        self.service.refresh_resolutions().map_err(to_fdo)
    }

    pub(crate) fn set_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<LinkSetChange> {
        self.service
            .set_link(source, relation, target)
            .map_err(to_fdo)
    }

    pub(crate) fn remove_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<Link> {
        self.service
            .remove_link(source, relation, target)
            .map_err(to_fdo)
    }

    pub(crate) fn remove_links(
        &self,
        source: &str,
        relation: &str,
    ) -> zbus::fdo::Result<Vec<Link>> {
        self.service.remove_links(source, relation).map_err(to_fdo)
    }

    pub(crate) fn delete_node(&self, subject: &str) -> zbus::fdo::Result<DeleteNodeChange> {
        self.service.delete_node(subject).map_err(to_fdo)
    }

    pub(crate) fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
    ) -> zbus::fdo::Result<PropertyChange> {
        self.service
            .set_property(subject, key, value)
            .map_err(to_fdo)
    }

    pub(crate) fn remove_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<()> {
        self.service.remove_property(subject, key).map_err(to_fdo)
    }

    pub(crate) async fn watch_node(
        &self,
        server: &ObjectServer,
        source: &str,
        path: Vec<String>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let target = self.resolve(source, &path)?;
        let properties = if target == NONE_STRING {
            BTreeMap::new()
        } else {
            self.service.properties(&target).map_err(to_fdo)?
        };
        let (object_path, state) = self
            .watches
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch manager poisoned".to_string()))?
            .insert(source.to_string(), path, target, properties);
        let watch = WatchIface::new(object_path.clone(), state, self.watches.clone());
        server
            .at(object_path.as_str(), watch)
            .await
            .map_err(to_fdo_display)?;
        OwnedObjectPath::try_from(object_path).map_err(to_fdo_display)
    }

    pub(crate) async fn refresh_watches(&self, conn: &Connection) -> zbus::fdo::Result<()> {
        let span = tracing::trace_span!("dbus.refresh_watches");
        let _guard = span.enter();
        let watches = self
            .watches
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("watch manager poisoned".to_string()))?
            .entries();
        let mut updates = Vec::new();

        for (object_path, state) in watches {
            let (source, path) = {
                let watch = lock_watch(&state)?;
                (watch.source.clone(), watch.path.clone())
            };
            let target = self.resolve(&source, &path)?;
            let properties = if target == NONE_STRING {
                BTreeMap::new()
            } else {
                // TODO: Let watch clients declare property keys, either at watch creation
                // or through add/remove methods, so unobserved target properties are not
                // recomputed or signaled through the watch object.
                self.service.properties(&target).map_err(to_fdo)?
            };
            let update = lock_watch(&state)?.set_target_properties(object_path, target, properties);
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

fn link_tuples(links: Vec<Link>) -> Vec<LinkTuple> {
    links.into_iter().map(|link| link.to_tuple()).collect()
}

fn option_to_wire(value: Option<String>) -> String {
    value.unwrap_or_else(|| NONE_STRING.to_string())
}

fn wire_to_option(value: &str) -> Option<&str> {
    (value != NONE_STRING).then_some(value)
}
