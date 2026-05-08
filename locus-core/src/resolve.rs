use std::collections::{BTreeSet, VecDeque};

use crate::Link;
use crate::error::ServiceError;
use crate::state::RuntimeState;
use locus_schema::{GraphSchema, NodeSelector};

fn outgoing_neighbors(links: &BTreeSet<Link>, subject: &str) -> Vec<String> {
    links
        .iter()
        .filter_map(|link| {
            if link.source == subject {
                Some(link.target.clone())
            } else {
                None
            }
        })
        .collect()
}

fn outgoing_related_by(links: &BTreeSet<Link>, subject: &str, relation: &str) -> Vec<String> {
    links
        .iter()
        .filter(|link| link.relation == relation)
        .filter(|link| link.source == subject)
        .map(|link| link.target.clone())
        .collect()
}

fn incoming_related_by(links: &BTreeSet<Link>, subject: &str, relation: &str) -> Vec<String> {
    links
        .iter()
        .filter(|link| link.relation == relation)
        .filter(|link| link.target == subject)
        .map(|link| link.source.clone())
        .collect()
}

fn selector_matches(state: &RuntimeState, selector: &NodeSelector, subject: &str) -> bool {
    match selector {
        NodeSelector::Any => true,
        NodeSelector::Exact(expected) => expected == subject,
        NodeSelector::Kind(expected) => {
            state.property(subject, "kind").as_deref() == Some(expected)
        }
    }
}

fn related_by(
    schema: &GraphSchema,
    state: &RuntimeState,
    subject: &str,
    relation: &str,
) -> Vec<String> {
    let Some(spec) = schema.relation(relation) else {
        return Vec::new();
    };

    let source_matches = selector_matches(state, &spec.source, subject);
    let target_matches = selector_matches(state, &spec.target, subject);

    if source_matches || !target_matches {
        outgoing_related_by(&state.links, subject, relation)
    } else {
        incoming_related_by(&state.links, subject, relation)
    }
}

pub fn resolve_all(
    schema: &GraphSchema,
    state: &RuntimeState,
    source: &str,
    path: &[String],
) -> Vec<String> {
    let mut subjects = BTreeSet::from([source.to_string()]);
    for relation in path {
        let mut next = BTreeSet::new();
        for subject in &subjects {
            next.extend(related_by(schema, state, subject, relation));
        }
        subjects = next;
        if subjects.is_empty() {
            break;
        }
    }
    subjects.into_iter().collect()
}

pub fn resolve_one(
    schema: &GraphSchema,
    state: &RuntimeState,
    source: &str,
    path: &[String],
) -> Result<Option<String>, ServiceError> {
    let targets = resolve_all(schema, state, source, path);
    match targets.as_slice() {
        [] => Ok(None),
        [target] => Ok(Some(target.clone())),
        _ => Err(ServiceError::AmbiguousResolve {
            start: source.to_string(),
            path: path.to_vec(),
            targets,
        }),
    }
}

pub fn resolve_kind(state: &RuntimeState, source: &str, kind: &str) -> Option<String> {
    if state.property(source, "kind").as_deref() == Some(kind) {
        return Some(source.to_string());
    }

    let mut visited = BTreeSet::from([source.to_string()]);
    let mut queue = VecDeque::from([source.to_string()]);
    while let Some(subject) = queue.pop_front() {
        for neighbor in outgoing_neighbors(&state.links, &subject) {
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
