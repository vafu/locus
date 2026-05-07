use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use zbus::Connection;
use zbus::names::InterfaceName;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::Value;

use crate::error::to_fdo_display;
use crate::{WATCH_INTERFACE, WATCH_ROOT_PATH};

pub(crate) type WatchRef = Arc<Mutex<WatchState>>;

#[derive(Debug, Default)]
pub(crate) struct WatchManager {
    next_id: u64,
    watches: BTreeMap<String, WatchRef>,
}

impl WatchManager {
    pub(crate) fn insert(
        &mut self,
        source: String,
        path: Vec<String>,
        target: String,
        properties: BTreeMap<String, String>,
    ) -> (String, WatchRef) {
        self.next_id += 1;
        let object_path = format!("{WATCH_ROOT_PATH}/{}", self.next_id);
        let state = Arc::new(Mutex::new(WatchState {
            source,
            path,
            target,
            properties,
        }));
        self.watches.insert(object_path.clone(), state.clone());
        (object_path, state)
    }

    fn remove(&mut self, object_path: &str) {
        self.watches.remove(object_path);
    }

    pub(crate) fn entries(&self) -> Vec<(String, WatchRef)> {
        self.watches
            .iter()
            .map(|(object_path, state)| (object_path.clone(), state.clone()))
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct WatchState {
    pub(crate) source: String,
    pub(crate) path: Vec<String>,
    target: String,
    properties: BTreeMap<String, String>,
}

impl WatchState {
    pub(crate) fn set_target_properties(
        &mut self,
        object_path: String,
        target: String,
        properties: BTreeMap<String, String>,
    ) -> WatchUpdate {
        let target_changed = (self.target != target).then(|| target.clone());
        let mut changed = BTreeMap::new();
        let mut removed = Vec::new();

        for (key, value) in &properties {
            if self.properties.get(key) != Some(value) {
                changed.insert(key.clone(), value.clone());
            }
        }
        for key in self.properties.keys() {
            if !properties.contains_key(key) {
                removed.push(key.clone());
            }
        }

        self.target = target;
        self.properties = properties;

        WatchUpdate {
            object_path,
            target_changed,
            properties_changed: !changed.is_empty() || !removed.is_empty(),
            properties: self.properties.clone(),
            changed,
            removed,
        }
    }
}

#[derive(Debug)]
pub(crate) struct WatchUpdate {
    object_path: String,
    target_changed: Option<String>,
    properties_changed: bool,
    properties: BTreeMap<String, String>,
    changed: BTreeMap<String, String>,
    removed: Vec<String>,
}

impl WatchUpdate {
    pub(crate) fn has_changes(&self) -> bool {
        self.target_changed.is_some() || self.properties_changed
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WatchIface {
    object_path: String,
    state: WatchRef,
    manager: Arc<Mutex<WatchManager>>,
}

impl WatchIface {
    pub(crate) fn new(
        object_path: String,
        state: WatchRef,
        manager: Arc<Mutex<WatchManager>>,
    ) -> Self {
        Self {
            object_path,
            state,
            manager,
        }
    }
}

#[zbus::interface(name = "io.github.Locus.Watch")]
impl WatchIface {
    #[zbus(property)]
    fn source(&self) -> zbus::fdo::Result<String> {
        Ok(lock_watch(&self.state)?.source.clone())
    }

    #[zbus(property, name = "Path")]
    fn path_property(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(lock_watch(&self.state)?.path.clone())
    }

    #[zbus(property)]
    fn target(&self) -> zbus::fdo::Result<String> {
        Ok(lock_watch(&self.state)?.target.clone())
    }

    #[zbus(property, name = "Properties")]
    fn properties_property(&self) -> zbus::fdo::Result<HashMap<String, String>> {
        Ok(lock_watch(&self.state)?
            .properties
            .clone()
            .into_iter()
            .collect())
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

pub(crate) async fn emit_watch_changed(
    conn: &Connection,
    update: WatchUpdate,
) -> zbus::fdo::Result<()> {
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

pub(crate) fn lock_watch(
    state: &Mutex<WatchState>,
) -> zbus::fdo::Result<MutexGuard<'_, WatchState>> {
    state
        .lock()
        .map_err(|_| zbus::fdo::Error::Failed("watch state poisoned".to_string()))
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

    fn watch_state(target: &str, properties: BTreeMap<String, String>) -> WatchState {
        WatchState {
            source: "context:selected".to_string(),
            path: vec!["window".to_string()],
            target: target.to_string(),
            properties,
        }
    }

    #[test]
    fn watch_update_reports_changed_and_removed_properties() {
        let mut watch = watch_state(
            "window:1",
            properties(&[("title", "Old"), ("app-id", "term")]),
        );

        let update = watch.set_target_properties(
            "/io/github/Locus/Watch/1".to_string(),
            "window:1".to_string(),
            properties(&[("title", "New"), ("urgent", "true")]),
        );

        assert_eq!(update.target_changed, None);
        assert_eq!(
            update.changed,
            properties(&[("title", "New"), ("urgent", "true")])
        );
        assert_eq!(update.removed, vec!["app-id".to_string()]);
    }

    #[test]
    fn watch_update_reports_target_and_property_changes_together() {
        let mut watch = watch_state("window:1", properties(&[("title", "Old")]));

        let update = watch.set_target_properties(
            "/io/github/Locus/Watch/1".to_string(),
            "window:2".to_string(),
            properties(&[("title", "New")]),
        );

        assert_eq!(update.target_changed, Some("window:2".to_string()));
        assert_eq!(update.changed, properties(&[("title", "New")]));
        assert_eq!(update.removed, Vec::<String>::new());
    }

    #[test]
    fn watch_update_ignores_unchanged_properties() {
        let mut watch = watch_state("window:1", properties(&[("title", "Same")]));

        let update = watch.set_target_properties(
            "/io/github/Locus/Watch/1".to_string(),
            "window:1".to_string(),
            properties(&[("title", "Same")]),
        );

        assert!(!update.has_changes());
    }
}
