use std::collections::{BTreeSet, VecDeque};

use crate::error::ServiceError;
use crate::state::RuntimeState;
use locus_api::Link;

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

fn related_by(links: &BTreeSet<Link>, subject: &str, relation: &str) -> Vec<String> {
    links
        .iter()
        .filter(|link| link.relation == relation)
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

pub fn resolve_all(state: &RuntimeState, source: &str, path: &[String]) -> Vec<String> {
    let mut subjects = BTreeSet::from([source.to_string()]);
    for relation in path {
        let mut next = BTreeSet::new();
        for subject in &subjects {
            next.extend(related_by(&state.links, subject, relation));
        }
        subjects = next;
        if subjects.is_empty() {
            break;
        }
    }
    subjects.into_iter().collect()
}

pub fn resolve_one(
    state: &RuntimeState,
    source: &str,
    path: &[String],
) -> Result<Option<String>, ServiceError> {
    let targets = resolve_all(state, source, path);
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
