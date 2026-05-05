use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::state::{Link, RuntimeState};

pub const CONTEXT_PREFIX: &str = "context:";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(
        "reciprocal link rejected: {link_source} --{relation}--> {link_target} conflicts with {link_target} --{existing_relation}--> {link_source}"
    )]
    ReciprocalLink {
        link_source: String,
        relation: String,
        link_target: String,
        existing_relation: String,
    },
    #[error("service lock is poisoned")]
    Poisoned,
}

#[derive(Debug)]
struct Inner {
    state: RuntimeState,
    resolutions: BTreeMap<(String, String), Option<String>>,
}

#[derive(Clone, Debug)]
pub struct LocusService {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkChange {
    Unchanged,
    Changed(Link),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSetChange {
    Unchanged,
    Changed { removed: Vec<Link>, added: Link },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyChange {
    Unchanged,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub source: String,
    pub kind: String,
    pub target: Option<String>,
}

impl LocusService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state: RuntimeState::default(),
                resolutions: BTreeMap::new(),
            })),
        }
    }

    pub fn add_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> Result<LinkChange, ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let visible_before = inner.state.links().contains(&link);
        if visible_before {
            return Ok(LinkChange::Unchanged);
        }
        if !visible_before && source != target {
            if let Some(existing) = inner
                .state
                .links()
                .into_iter()
                .find(|existing| existing.source == target && existing.target == source)
            {
                return Err(ServiceError::ReciprocalLink {
                    link_source: link.source,
                    relation: link.relation,
                    link_target: link.target,
                    existing_relation: existing.relation,
                });
            }
        }

        inner.state.links.insert(link.clone());
        Ok(LinkChange::Changed(link))
    }

    pub fn remove_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> Result<Link, ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner.state.links.remove(&link);
        Ok(link)
    }

    pub fn remove_links(&self, source: &str, relation: &str) -> Result<Vec<Link>, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let removed = inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == source && link.relation == relation)
            .collect::<Vec<_>>();
        inner
            .state
            .links
            .retain(|link| !(link.source == source && link.relation == relation));
        Ok(removed)
    }

    pub fn targets(&self, source: &str, relation: &str) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == source && link.relation == relation)
            .map(|link| link.target)
            .collect())
    }

    pub fn sources(&self, target: &str, relation: &str) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.target == target && link.relation == relation)
            .map(|link| link.source)
            .collect())
    }

    pub fn links(&self, subject: &str) -> Result<Vec<Link>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner
            .state
            .links()
            .into_iter()
            .filter(|link| link.source == subject || link.target == subject)
            .collect())
    }

    pub fn all_links(&self) -> Result<Vec<Link>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.links().into_iter().collect())
    }

    pub fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
    ) -> Result<PropertyChange, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let visible_before = inner.state.property(subject, key);
        inner
            .state
            .properties
            .insert((subject.to_string(), key.to_string()), value.to_string());
        let visible_after = inner.state.property(subject, key);
        if visible_before == visible_after {
            Ok(PropertyChange::Unchanged)
        } else {
            Ok(PropertyChange::Changed)
        }
    }

    pub fn remove_property(&self, subject: &str, key: &str) -> Result<(), ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        inner
            .state
            .properties
            .remove(&(subject.to_string(), key.to_string()));
        Ok(())
    }

    pub fn property(&self, subject: &str, key: &str) -> Result<Option<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.property(subject, key))
    }

    pub fn properties(&self, subject: &str) -> Result<BTreeMap<String, String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(inner.state.properties_for(subject))
    }

    pub fn subjects(&self) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let mut subjects = BTreeSet::new();
        for link in inner.state.links() {
            subjects.insert(link.source);
            subjects.insert(link.target);
        }
        for (subject, _) in inner.state.properties.keys() {
            subjects.insert(subject.clone());
        }
        Ok(subjects.into_iter().collect())
    }

    pub fn subjects_with_property(
        &self,
        key: &str,
        value: Option<&str>,
    ) -> Result<Vec<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let mut subjects = BTreeSet::new();
        for ((subject, property_key), property_value) in &inner.state.properties {
            if property_key == key && value.is_none_or(|value| property_value == value) {
                subjects.insert(subject.clone());
            }
        }
        Ok(subjects.into_iter().collect())
    }

    pub fn resolve_kind(&self, source: &str, kind: &str) -> Result<Option<String>, ServiceError> {
        let inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        Ok(resolve_kind(&inner.state, source, kind))
    }

    pub fn subscribe_resolution(
        &self,
        source: &str,
        kind: &str,
    ) -> Result<Resolution, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let target = resolve_kind(&inner.state, source, kind);
        inner
            .resolutions
            .insert((source.to_string(), kind.to_string()), target.clone());
        Ok(Resolution {
            source: source.to_string(),
            kind: kind.to_string(),
            target,
        })
    }

    pub fn refresh_resolutions(&self) -> Result<Vec<Resolution>, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;
        let keys = inner.resolutions.keys().cloned().collect::<Vec<_>>();
        let mut changed = Vec::new();

        for (source, kind) in keys {
            let previous = inner
                .resolutions
                .get(&(source.clone(), kind.clone()))
                .cloned();
            let target = resolve_kind(&inner.state, &source, &kind);
            if previous != Some(target.clone()) {
                inner
                    .resolutions
                    .insert((source.clone(), kind.clone()), target.clone());
                changed.push(Resolution {
                    source,
                    kind,
                    target,
                });
            }
        }

        Ok(changed)
    }

    pub fn set_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> Result<LinkSetChange, ServiceError> {
        let link = Link::new(source, relation, target);
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Poisoned)?;

        if source != target {
            if let Some(existing) = inner.state.links().into_iter().find(|existing| {
                existing.source == target
                    && existing.target == source
                    && !(existing.source == source
                        && existing.relation == relation
                        && existing.target == target)
            }) {
                return Err(ServiceError::ReciprocalLink {
                    link_source: link.source,
                    relation: link.relation,
                    link_target: link.target,
                    existing_relation: existing.relation,
                });
            }
        }

        let removed = inner
            .state
            .links()
            .into_iter()
            .filter(|existing| existing.source == source && existing.relation == relation)
            .collect::<Vec<_>>();

        let visible_unchanged = removed.len() == 1 && removed.first() == Some(&link);
        if visible_unchanged {
            return Ok(LinkSetChange::Unchanged);
        }

        inner
            .state
            .links
            .retain(|existing| !(existing.source == source && existing.relation == relation));
        inner.state.links.insert(link.clone());

        Ok(LinkSetChange::Changed {
            removed,
            added: link,
        })
    }
}

pub fn context_subject(context: &str) -> String {
    format!("{CONTEXT_PREFIX}{context}")
}

fn neighbors(links: &BTreeSet<Link>, subject: &str) -> Vec<String> {
    links
        .iter()
        .filter_map(|link| {
            if link.source == subject {
                Some(link.target.clone())
            } else if link.target == subject {
                Some(link.source.clone())
            } else {
                None
            }
        })
        .collect()
}

fn resolve_kind(state: &RuntimeState, source: &str, kind: &str) -> Option<String> {
    if state.property(source, "kind").as_deref() == Some(kind) {
        return Some(source.to_string());
    }

    let mut visited = BTreeSet::from([source.to_string()]);
    let mut queue = VecDeque::from([source.to_string()]);
    while let Some(subject) = queue.pop_front() {
        for neighbor in neighbors(&state.links, &subject) {
            if !visited.insert(neighbor.clone()) {
                continue;
            }
            if state.property(&neighbor, "kind").as_deref() == Some(kind) {
                return Some(neighbor);
            }
            queue.push_back(neighbor);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> LocusService {
        LocusService::new()
    }

    #[test]
    fn returns_reverse_sources_for_multi_target_relations() {
        let service = service();
        service
            .add_link("session:1", "project", "project:a")
            .unwrap();
        service
            .add_link("session:2", "project", "project:a")
            .unwrap();

        assert_eq!(
            service.sources("project:a", "project").unwrap(),
            vec!["session:1", "session:2"]
        );
    }

    #[test]
    fn rejects_reciprocal_links() {
        let service = service();
        service
            .add_link("niri:workspace:6", "window", "niri:window:57")
            .unwrap();

        let error = service
            .add_link("niri:window:57", "workspace", "niri:workspace:6")
            .unwrap_err();

        assert!(matches!(error, ServiceError::ReciprocalLink { .. }));
        assert_eq!(service.all_links().unwrap().len(), 1);
    }

    #[test]
    fn set_property_replaces_existing_property() {
        let service = service();
        service.set_property("project:a", "name", "Old").unwrap();
        service.set_property("project:a", "name", "New").unwrap();

        assert_eq!(
            service.property("project:a", "name").unwrap().as_deref(),
            Some("New")
        );
    }

    #[test]
    fn subjects_include_links_and_properties() {
        let service = service();
        service.add_link("a", "rel", "b").unwrap();
        service.set_property("c", "kind", "thing").unwrap();

        assert_eq!(service.subjects().unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn finds_subjects_by_property() {
        let service = service();
        service.set_property("a", "kind", "project").unwrap();
        service.set_property("b", "kind", "workspace").unwrap();
        service.set_property("c", "kind", "project").unwrap();

        assert_eq!(
            service
                .subjects_with_property("kind", Some("project"))
                .unwrap(),
            vec!["a", "c"]
        );
        assert_eq!(
            service.subjects_with_property("kind", None).unwrap(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn resolves_shortest_path_to_kind() {
        let service = service();
        service
            .set_property("project:a", "kind", "project")
            .unwrap();
        service
            .set_property("project:b", "kind", "project")
            .unwrap();
        service
            .add_link("context:selected", "window", "window:1")
            .unwrap();
        service
            .add_link("window:1", "workspace", "workspace:1")
            .unwrap();
        service
            .add_link("workspace:1", "project", "project:a")
            .unwrap();
        service
            .add_link("window:1", "project", "project:b")
            .unwrap();

        assert_eq!(
            service.resolve_kind("context:selected", "project").unwrap(),
            Some("project:b".to_string())
        );
    }

    #[test]
    fn subscribed_resolution_only_reports_changed_target() {
        let service = service();
        assert_eq!(
            service
                .subscribe_resolution("context:selected", "project")
                .unwrap()
                .target,
            None
        );

        service
            .add_link("context:selected", "window", "window:1")
            .unwrap();
        assert!(service.refresh_resolutions().unwrap().is_empty());

        service
            .set_property("project:a", "kind", "project")
            .unwrap();
        service
            .add_link("window:1", "project", "project:a")
            .unwrap();
        assert_eq!(
            service.refresh_resolutions().unwrap(),
            vec![Resolution {
                source: "context:selected".to_string(),
                kind: "project".to_string(),
                target: Some("project:a".to_string()),
            }]
        );

        service.add_link("unrelated", "rel", "node").unwrap();
        assert!(service.refresh_resolutions().unwrap().is_empty());
    }

    #[test]
    fn set_link_replaces_previous_relation() {
        let service = service();
        assert!(matches!(
            service.set_link("a", "current", "b").unwrap(),
            LinkSetChange::Changed { .. }
        ));
        assert!(matches!(
            service.set_link("a", "current", "c").unwrap(),
            LinkSetChange::Changed { .. }
        ));

        assert_eq!(service.targets("a", "current").unwrap(), vec!["c"]);
        assert_eq!(
            service
                .all_links()
                .unwrap()
                .into_iter()
                .map(|link| link.to_tuple())
                .collect::<Vec<_>>(),
            vec![("a".to_string(), "current".to_string(), "c".to_string())]
        );
    }

    #[test]
    fn set_link_is_noop_when_visible_target_is_unchanged() {
        let service = service();
        assert!(matches!(
            service.set_link("a", "current", "b").unwrap(),
            LinkSetChange::Changed { .. }
        ));
        assert_eq!(
            service.set_link("a", "current", "b").unwrap(),
            LinkSetChange::Unchanged
        );
    }

    #[test]
    fn add_link_is_noop_when_link_already_exists() {
        let service = service();
        assert!(matches!(
            service.add_link("a", "rel", "b").unwrap(),
            LinkChange::Changed(_)
        ));
        assert_eq!(
            service.add_link("a", "rel", "b").unwrap(),
            LinkChange::Unchanged
        );
    }

    #[test]
    fn set_property_is_noop_when_visible_value_is_unchanged() {
        let service = service();
        assert_eq!(
            service.set_property("a", "name", "A").unwrap(),
            PropertyChange::Changed
        );
        assert_eq!(
            service.set_property("a", "name", "A").unwrap(),
            PropertyChange::Unchanged
        );
    }

    #[test]
    fn all_links_returns_links() {
        let service = service();
        service.add_link("a", "rel", "b").unwrap();
        service.add_link("b", "rel", "c").unwrap();

        let links = service
            .all_links()
            .unwrap()
            .into_iter()
            .map(|link| link.to_tuple())
            .collect::<Vec<_>>();

        assert_eq!(
            links,
            vec![
                ("a".to_string(), "rel".to_string(), "b".to_string()),
                ("b".to_string(), "rel".to_string(), "c".to_string()),
            ]
        );
    }
}
