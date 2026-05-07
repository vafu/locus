use std::collections::{BTreeMap, BTreeSet};

use niri_ipc::state::EventStreamState;

pub const WORKSPACE_RELATION: &str = "workspace";
pub const WINDOW_RELATION: &str = "window";
pub const OUTPUT_RELATION: &str = "output";
pub const SELECTED_CONTEXT: &str = "selected";

pub const WINDOW_PROPERTY_KEYS: &[&str] = &[
    "kind",
    "source",
    "external-id",
    "title",
    "app-id",
    "focused",
    "urgent",
    "column",
    "row",
    "tile-width",
    "tile-height",
];

pub const WORKSPACE_PROPERTY_KEYS: &[&str] = &[
    "kind",
    "source",
    "external-id",
    "idx",
    "name",
    "active",
    "focused",
    "urgent",
];

pub const OUTPUT_PROPERTY_KEYS: &[&str] = &["kind", "source", "connector"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectedGraph {
    pub workspace_windows: BTreeSet<(String, String)>,
    pub workspace_outputs: BTreeSet<(String, String)>,
    pub properties: BTreeMap<(String, String), String>,
    pub focused_window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphMutation {
    SetLink {
        source: String,
        relation: String,
        target: String,
    },
    RemoveLink {
        source: String,
        relation: String,
        target: String,
    },
    RemoveLinks {
        source: String,
        relation: String,
    },
    DeleteNode {
        subject: String,
    },
    SetProperty {
        subject: String,
        key: String,
        value: String,
    },
    RemoveProperty {
        subject: String,
        key: String,
    },
}

pub fn state_to_graph(state: &EventStreamState) -> ProjectedGraph {
    let mut properties = BTreeMap::new();

    let workspace_windows = state
        .windows
        .windows
        .values()
        .filter_map(|window| {
            let workspace_id = window.workspace_id?;
            Some((workspace_subject(workspace_id), window_subject(window.id)))
        })
        .collect();

    for window in state.windows.windows.values() {
        let subject = window_subject(window.id);
        insert_property(&mut properties, &subject, "kind", "window");
        insert_property(&mut properties, &subject, "source", "niri");
        insert_property(
            &mut properties,
            &subject,
            "external-id",
            window.id.to_string(),
        );
        insert_property(
            &mut properties,
            &subject,
            "title",
            window.title.as_deref().unwrap_or_default(),
        );
        insert_property(
            &mut properties,
            &subject,
            "app-id",
            window.app_id.as_deref().unwrap_or_default(),
        );
        insert_property(
            &mut properties,
            &subject,
            "focused",
            window.is_focused.to_string(),
        );
        insert_property(
            &mut properties,
            &subject,
            "urgent",
            window.is_urgent.to_string(),
        );
        if let Some((column, row)) = window.layout.pos_in_scrolling_layout {
            insert_property(&mut properties, &subject, "column", column.to_string());
            insert_property(&mut properties, &subject, "row", row.to_string());
        }
        insert_property(
            &mut properties,
            &subject,
            "tile-width",
            window.layout.tile_size.0.to_string(),
        );
        insert_property(
            &mut properties,
            &subject,
            "tile-height",
            window.layout.tile_size.1.to_string(),
        );
    }

    let workspace_outputs: BTreeSet<(String, String)> = state
        .workspaces
        .workspaces
        .values()
        .filter_map(|workspace| {
            let output = workspace.output.as_ref()?;
            Some((workspace_subject(workspace.id), output_subject(output)))
        })
        .collect();

    for workspace in state.workspaces.workspaces.values() {
        let subject = workspace_subject(workspace.id);
        insert_property(&mut properties, &subject, "kind", "workspace");
        insert_property(&mut properties, &subject, "source", "niri");
        insert_property(
            &mut properties,
            &subject,
            "external-id",
            workspace.id.to_string(),
        );
        insert_property(&mut properties, &subject, "idx", workspace.idx.to_string());
        insert_property(
            &mut properties,
            &subject,
            "name",
            workspace
                .name
                .as_deref()
                .map(str::to_string)
                .unwrap_or_else(|| workspace.idx.to_string()),
        );
        insert_property(
            &mut properties,
            &subject,
            "active",
            workspace.is_active.to_string(),
        );
        insert_property(
            &mut properties,
            &subject,
            "focused",
            workspace.is_focused.to_string(),
        );
        insert_property(
            &mut properties,
            &subject,
            "urgent",
            workspace.is_urgent.to_string(),
        );
    }

    for pair in &workspace_outputs {
        let output = &pair.1;
        insert_property(&mut properties, output, "kind", "output");
        insert_property(&mut properties, output, "source", "niri");
        if let Some(connector) = output.strip_prefix("output:") {
            insert_property(&mut properties, output, "connector", connector);
        }
    }

    let focused_workspace = state
        .workspaces
        .workspaces
        .values()
        .find(|workspace| workspace.is_focused)
        .cloned();
    let focused_window = state
        .windows
        .windows
        .values()
        .find(|window| window.is_focused)
        .map(|window| window_subject(window.id))
        .or_else(|| {
            focused_workspace
                .as_ref()
                .and_then(|workspace| workspace.active_window_id)
                .map(window_subject)
        });
    if let Some(window) = focused_window.as_deref() {
        insert_property(&mut properties, window, "kind", "window");
        insert_property(&mut properties, window, "source", "niri");
        if let Some(id) = window.strip_prefix("window:") {
            insert_property(&mut properties, window, "external-id", id);
        }
    }

    ProjectedGraph {
        workspace_windows,
        workspace_outputs,
        properties,
        focused_window,
    }
}

pub fn diff_graphs(previous: &ProjectedGraph, next: &ProjectedGraph) -> Vec<GraphMutation> {
    let mut mutations = Vec::new();
    let deleted_windows = deleted_subjects(previous, next, "window");
    for (subject, key) in previous.properties.keys() {
        if deleted_windows.contains(subject) {
            continue;
        }
        if !next
            .properties
            .contains_key(&(subject.clone(), key.clone()))
        {
            mutations.push(GraphMutation::RemoveProperty {
                subject: subject.clone(),
                key: key.clone(),
            });
        }
    }
    for ((subject, key), value) in &next.properties {
        if previous.properties.get(&(subject.clone(), key.clone())) != Some(value) {
            mutations.push(GraphMutation::SetProperty {
                subject: subject.clone(),
                key: key.clone(),
                value: value.clone(),
            });
        }
    }
    for (workspace, window) in previous
        .workspace_windows
        .difference(&next.workspace_windows)
    {
        if deleted_windows.contains(window) {
            continue;
        }
        mutations.push(GraphMutation::RemoveLink {
            source: window.clone(),
            relation: WORKSPACE_RELATION.to_string(),
            target: workspace.clone(),
        });
    }
    for window in &deleted_windows {
        mutations.push(GraphMutation::DeleteNode {
            subject: window.clone(),
        });
    }
    for (workspace, window) in next
        .workspace_windows
        .difference(&previous.workspace_windows)
    {
        mutations.push(GraphMutation::SetLink {
            source: window.clone(),
            relation: WORKSPACE_RELATION.to_string(),
            target: workspace.clone(),
        });
    }
    for (workspace, output) in previous
        .workspace_outputs
        .difference(&next.workspace_outputs)
    {
        mutations.push(GraphMutation::RemoveLink {
            source: workspace.clone(),
            relation: OUTPUT_RELATION.to_string(),
            target: output.clone(),
        });
    }
    for (workspace, output) in next
        .workspace_outputs
        .difference(&previous.workspace_outputs)
    {
        mutations.push(GraphMutation::SetLink {
            source: workspace.clone(),
            relation: OUTPUT_RELATION.to_string(),
            target: output.clone(),
        });
    }
    if previous.focused_window != next.focused_window {
        push_context_mutation(
            &mut mutations,
            SELECTED_CONTEXT,
            WINDOW_RELATION,
            next.focused_window.clone(),
        );
    }
    mutations
}

fn deleted_subjects(
    previous: &ProjectedGraph,
    next: &ProjectedGraph,
    kind: &str,
) -> BTreeSet<String> {
    previous
        .properties
        .iter()
        .filter(|((_, key), value)| key == "kind" && value.as_str() == kind)
        .map(|((subject, _), _)| subject.clone())
        .filter(|subject| {
            !next
                .properties
                .contains_key(&(subject.clone(), "kind".to_string()))
        })
        .collect()
}

pub fn push_context_mutation(
    mutations: &mut Vec<GraphMutation>,
    context: &str,
    relation: &str,
    target: Option<String>,
) {
    let source = context_subject(context);
    if let Some(target) = target {
        mutations.push(GraphMutation::SetLink {
            source,
            relation: relation.to_string(),
            target,
        });
    } else {
        mutations.push(GraphMutation::RemoveLinks {
            source,
            relation: relation.to_string(),
        });
    }
}

fn insert_property(
    properties: &mut BTreeMap<(String, String), String>,
    subject: &str,
    key: &str,
    value: impl Into<String>,
) {
    properties.insert((subject.to_string(), key.to_string()), value.into());
}

pub fn workspace_subject(id: u64) -> String {
    format!("workspace:{id}")
}

pub fn window_subject(id: u64) -> String {
    format!("window:{id}")
}

pub fn output_subject(name: &str) -> String {
    format!("output:{name}")
}

pub fn context_subject(context: &str) -> String {
    format!("context:{context}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_sets_changed_property_without_remove() {
        let mut previous = ProjectedGraph::default();
        previous.properties.insert(
            ("window:1".to_string(), "title".to_string()),
            "old".to_string(),
        );
        let mut next = ProjectedGraph::default();
        next.properties.insert(
            ("window:1".to_string(), "title".to_string()),
            "new".to_string(),
        );

        assert_eq!(
            diff_graphs(&previous, &next),
            vec![GraphMutation::SetProperty {
                subject: "window:1".to_string(),
                key: "title".to_string(),
                value: "new".to_string(),
            }]
        );
    }

    #[test]
    fn diff_removes_property_when_key_disappears() {
        let mut previous = ProjectedGraph::default();
        previous.properties.insert(
            ("window:1".to_string(), "title".to_string()),
            "old".to_string(),
        );

        assert_eq!(
            diff_graphs(&previous, &ProjectedGraph::default()),
            vec![GraphMutation::RemoveProperty {
                subject: "window:1".to_string(),
                key: "title".to_string(),
            }]
        );
    }

    #[test]
    fn diff_deletes_window_node_when_window_disappears() {
        let mut previous = ProjectedGraph::default();
        previous
            .workspace_windows
            .insert(("workspace:1".to_string(), "window:1".to_string()));
        previous.properties.insert(
            ("window:1".to_string(), "kind".to_string()),
            "window".to_string(),
        );
        previous.properties.insert(
            ("window:1".to_string(), "title".to_string()),
            "gone".to_string(),
        );

        assert_eq!(
            diff_graphs(&previous, &ProjectedGraph::default()),
            vec![GraphMutation::DeleteNode {
                subject: "window:1".to_string(),
            }]
        );
    }
}
