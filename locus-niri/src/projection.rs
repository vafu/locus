use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::{Context, anyhow};
use niri_ipc::Event;
use niri_ipc::state::{EventStreamState, EventStreamStatePart};
use tracing::trace;

use crate::graph::{
    GraphMutation, ProjectedGraph, SELECTED_CONTEXT, SELECTED_WORKSPACE_RELATION,
    WINDOW_PROPERTY_KEYS, WINDOW_RELATION, WORKSPACE_RELATION, diff_graphs, push_context_mutation,
    state_to_graph, window_subject, workspace_subject,
};

#[derive(Debug, Default)]
pub struct GraphProjection {
    niri: EventStreamState,
    graph: ProjectedGraph,
}

impl GraphProjection {
    pub fn project(&mut self, event: Event) -> anyhow::Result<Vec<GraphMutation>> {
        let graph_event = event_affects_graph(&event);
        let snapshot_event = event_uses_snapshot(&event);

        if !graph_event {
            self.apply_event(event)?;
            trace!("Niri event does not affect Locus graph");
            return Ok(Vec::new());
        }

        if snapshot_event {
            self.apply_event(event)?;
            let next = state_to_graph(&self.niri);
            let mutations = diff_graphs(&self.graph, &next);
            self.graph = next;
            return Ok(mutations);
        }

        let event_for_projection = event.clone();
        self.apply_event(event)
            .with_context(|| format!("apply Niri {} event", event_name(&event_for_projection)))?;
        let mut mutations = Vec::new();
        self.project_event(&event_for_projection, &mut mutations);
        Ok(mutations)
    }

    fn apply_event(&mut self, event: Event) -> anyhow::Result<()> {
        catch_unwind(AssertUnwindSafe(|| self.niri.apply(event)))
            .map(|_| ())
            .map_err(|payload| {
                if let Some(message) = payload.downcast_ref::<&str>() {
                    anyhow!("Niri event stream state panicked: {message}")
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    anyhow!("Niri event stream state panicked: {message}")
                } else {
                    anyhow!("Niri event stream state panicked")
                }
            })
    }

    fn project_event(&mut self, event: &Event, mutations: &mut Vec<GraphMutation>) {
        match event {
            Event::WorkspaceUrgencyChanged { id, urgent } => {
                self.set_property(
                    mutations,
                    workspace_subject(*id),
                    "urgent",
                    urgent.to_string(),
                );
            }
            Event::WorkspaceActivated { .. } => {
                self.project_workspace_selection(mutations);
                self.project_workspace_activity(mutations);
            }
            Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => {
                let focused_workspace = self
                    .niri
                    .workspaces
                    .workspaces
                    .values()
                    .find(|workspace| workspace.is_focused)
                    .map(|workspace| workspace.id);
                if Some(*workspace_id) == focused_workspace {
                    self.set_selected_window(mutations, active_window_id.map(window_subject));
                }
            }
            Event::WindowOpenedOrChanged { window } => {
                self.project_window(mutations, window);
                self.project_window_workspace(mutations, window);
                if window.is_focused {
                    self.set_selected_window(mutations, Some(window_subject(window.id)));
                }
            }
            Event::WindowClosed { id } => {
                self.remove_window(mutations, *id);
            }
            Event::WindowFocusChanged { id } => {
                self.set_selected_window(mutations, id.map(window_subject));
            }
            Event::WindowUrgencyChanged { id, urgent } => {
                self.set_property(mutations, window_subject(*id), "urgent", urgent.to_string());
            }
            Event::WindowLayoutsChanged { changes } => {
                for (id, _) in changes {
                    if let Some(window) = self.niri.windows.windows.get(id).cloned() {
                        self.project_window_layout(mutations, &window);
                    }
                }
            }
            Event::WorkspacesChanged { .. }
            | Event::WindowsChanged { .. }
            | Event::WindowFocusTimestampChanged { .. }
            | Event::KeyboardLayoutsChanged { .. }
            | Event::KeyboardLayoutSwitched { .. }
            | Event::OverviewOpenedOrClosed { .. }
            | Event::ConfigLoaded { .. }
            | Event::ScreenshotCaptured { .. } => {}
        }
    }

    fn project_workspace_selection(&mut self, mutations: &mut Vec<GraphMutation>) {
        let focused_workspace = self
            .niri
            .workspaces
            .workspaces
            .values()
            .find(|workspace| workspace.is_focused)
            .map(|workspace| workspace_subject(workspace.id));
        self.set_context(
            mutations,
            SELECTED_CONTEXT,
            SELECTED_WORKSPACE_RELATION,
            focused_workspace,
        );
    }

    fn project_workspace_activity(&mut self, mutations: &mut Vec<GraphMutation>) {
        let workspaces = self
            .niri
            .workspaces
            .workspaces
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for workspace in workspaces {
            let subject = workspace_subject(workspace.id);
            self.set_property(
                mutations,
                subject.clone(),
                "active",
                workspace.is_active.to_string(),
            );
            self.set_property(
                mutations,
                subject,
                "focused",
                workspace.is_focused.to_string(),
            );
        }
    }

    fn project_window(&mut self, mutations: &mut Vec<GraphMutation>, window: &niri_ipc::Window) {
        let subject = window_subject(window.id);
        self.set_property(mutations, subject.clone(), "kind", "window");
        self.set_property(mutations, subject.clone(), "source", "niri");
        self.set_property(
            mutations,
            subject.clone(),
            "external-id",
            window.id.to_string(),
        );
        self.set_property(
            mutations,
            subject.clone(),
            "title",
            window.title.as_deref().unwrap_or_default(),
        );
        self.set_property(
            mutations,
            subject.clone(),
            "app-id",
            window.app_id.as_deref().unwrap_or_default(),
        );
        self.set_property(
            mutations,
            subject.clone(),
            "urgent",
            window.is_urgent.to_string(),
        );
        self.project_window_layout(mutations, window);
    }

    fn project_window_layout(
        &mut self,
        mutations: &mut Vec<GraphMutation>,
        window: &niri_ipc::Window,
    ) {
        let subject = window_subject(window.id);
        if let Some((column, row)) = window.layout.pos_in_scrolling_layout {
            self.set_property(mutations, subject.clone(), "column", column.to_string());
            self.set_property(mutations, subject.clone(), "row", row.to_string());
        } else {
            self.remove_property(mutations, subject.clone(), "column");
            self.remove_property(mutations, subject.clone(), "row");
        }
        self.set_property(
            mutations,
            subject.clone(),
            "tile-width",
            window.layout.tile_size.0.to_string(),
        );
        self.set_property(
            mutations,
            subject,
            "tile-height",
            window.layout.tile_size.1.to_string(),
        );
    }

    fn project_window_workspace(
        &mut self,
        mutations: &mut Vec<GraphMutation>,
        window: &niri_ipc::Window,
    ) {
        let window_subject = window_subject(window.id);
        let next = window
            .workspace_id
            .map(|workspace_id| (workspace_subject(workspace_id), window_subject.clone()));
        let existing = self
            .graph
            .workspace_windows
            .iter()
            .find(|(_, existing_window)| existing_window == &window_subject)
            .cloned();
        if existing == next {
            return;
        }
        if let Some((workspace, window)) = existing {
            mutations.push(GraphMutation::RemoveLink {
                source: window.clone(),
                relation: WORKSPACE_RELATION.to_string(),
                target: workspace.clone(),
            });
            self.graph.workspace_windows.remove(&(workspace, window));
        }
        if let Some((workspace, window)) = next {
            self.set_property(mutations, workspace.clone(), "kind", "workspace");
            self.set_property(mutations, workspace.clone(), "source", "niri");
            if let Some(id) = workspace.strip_prefix("workspace:") {
                self.set_property(mutations, workspace.clone(), "external-id", id);
            }
            self.set_property(mutations, window.clone(), "kind", "window");
            self.set_property(mutations, window.clone(), "source", "niri");
            if let Some(id) = window.strip_prefix("window:") {
                self.set_property(mutations, window.clone(), "external-id", id);
            }
            mutations.push(GraphMutation::SetLink {
                source: window.clone(),
                relation: WORKSPACE_RELATION.to_string(),
                target: workspace.clone(),
            });
            self.graph.workspace_windows.insert((workspace, window));
        }
    }

    fn remove_window(&mut self, mutations: &mut Vec<GraphMutation>, id: u64) {
        let subject = window_subject(id);
        let links = self
            .graph
            .workspace_windows
            .iter()
            .filter(|(_, window)| window == &subject)
            .cloned()
            .collect::<Vec<_>>();
        for (workspace, window) in links {
            mutations.push(GraphMutation::RemoveLink {
                source: window.clone(),
                relation: WORKSPACE_RELATION.to_string(),
                target: workspace.clone(),
            });
            self.graph.workspace_windows.remove(&(workspace, window));
        }
        if self.graph.focused_window.as_deref() == Some(subject.as_str()) {
            self.set_selected_window(mutations, None);
        }
        for key in WINDOW_PROPERTY_KEYS {
            self.remove_property(mutations, subject.clone(), key);
        }
    }

    fn set_selected_window(
        &mut self,
        mutations: &mut Vec<GraphMutation>,
        focused_window: Option<String>,
    ) {
        if self.graph.focused_window == focused_window {
            if let Some(window) = focused_window.as_deref() {
                self.project_window_identity(mutations, window);
                self.set_property(mutations, window.to_string(), "focused", "true");
            }
            return;
        }
        if let Some(old_window) = self.graph.focused_window.clone() {
            self.set_property(mutations, old_window, "focused", "false");
        }
        if let Some(new_window) = focused_window.as_deref() {
            self.project_window_identity(mutations, new_window);
            self.set_property(mutations, new_window.to_string(), "focused", "true");
        }
        self.set_context(mutations, SELECTED_CONTEXT, WINDOW_RELATION, focused_window);
    }

    fn project_window_identity(&mut self, mutations: &mut Vec<GraphMutation>, window: &str) {
        self.set_property(mutations, window.to_string(), "kind", "window");
        self.set_property(mutations, window.to_string(), "source", "niri");
        if let Some(id) = window.strip_prefix("window:") {
            self.set_property(mutations, window.to_string(), "external-id", id);
        }
    }

    fn set_context(
        &mut self,
        mutations: &mut Vec<GraphMutation>,
        context: &str,
        relation: &str,
        target: Option<String>,
    ) {
        let stored = match relation {
            WINDOW_RELATION => &mut self.graph.focused_window,
            SELECTED_WORKSPACE_RELATION => &mut self.graph.focused_workspace,
            _ => return,
        };
        if *stored == target {
            return;
        }
        push_context_mutation(mutations, context, relation, target.clone());
        *stored = target;
    }

    fn set_property(
        &mut self,
        mutations: &mut Vec<GraphMutation>,
        subject: impl Into<String>,
        key: &str,
        value: impl Into<String>,
    ) {
        let subject = subject.into();
        let value = value.into();
        let property_key = (subject.clone(), key.to_string());
        if self.graph.properties.get(&property_key) == Some(&value) {
            return;
        }
        self.graph.properties.insert(property_key, value.clone());
        mutations.push(GraphMutation::SetProperty {
            subject,
            key: key.to_string(),
            value,
        });
    }

    fn remove_property(
        &mut self,
        mutations: &mut Vec<GraphMutation>,
        subject: impl Into<String>,
        key: &str,
    ) {
        let subject = subject.into();
        let property_key = (subject.clone(), key.to_string());
        if self.graph.properties.remove(&property_key).is_none() {
            return;
        }
        mutations.push(GraphMutation::RemoveProperty {
            subject,
            key: key.to_string(),
        });
    }
}

fn event_affects_graph(event: &Event) -> bool {
    matches!(
        event,
        Event::WorkspacesChanged { .. }
            | Event::WorkspaceUrgencyChanged { .. }
            | Event::WorkspaceActivated { .. }
            | Event::WorkspaceActiveWindowChanged { .. }
            | Event::WindowsChanged { .. }
            | Event::WindowOpenedOrChanged { .. }
            | Event::WindowClosed { .. }
            | Event::WindowFocusChanged { .. }
            | Event::WindowUrgencyChanged { .. }
            | Event::WindowLayoutsChanged { .. }
    )
}

fn event_uses_snapshot(event: &Event) -> bool {
    matches!(
        event,
        Event::WorkspacesChanged { .. } | Event::WindowsChanged { .. }
    )
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::WorkspacesChanged { .. } => "WorkspacesChanged",
        Event::WorkspaceUrgencyChanged { .. } => "WorkspaceUrgencyChanged",
        Event::WorkspaceActivated { .. } => "WorkspaceActivated",
        Event::WorkspaceActiveWindowChanged { .. } => "WorkspaceActiveWindowChanged",
        Event::WindowsChanged { .. } => "WindowsChanged",
        Event::WindowOpenedOrChanged { .. } => "WindowOpenedOrChanged",
        Event::WindowClosed { .. } => "WindowClosed",
        Event::WindowFocusChanged { .. } => "WindowFocusChanged",
        Event::WindowFocusTimestampChanged { .. } => "WindowFocusTimestampChanged",
        Event::WindowUrgencyChanged { .. } => "WindowUrgencyChanged",
        Event::WindowLayoutsChanged { .. } => "WindowLayoutsChanged",
        Event::KeyboardLayoutsChanged { .. } => "KeyboardLayoutsChanged",
        Event::KeyboardLayoutSwitched { .. } => "KeyboardLayoutSwitched",
        Event::OverviewOpenedOrClosed { .. } => "OverviewOpenedOrClosed",
        Event::ConfigLoaded { .. } => "ConfigLoaded",
        Event::ScreenshotCaptured { .. } => "ScreenshotCaptured",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inconsistent_niri_state_event_returns_error() {
        let mut projection = GraphProjection::default();
        let error = projection
            .project(Event::WorkspaceActivated {
                id: 404,
                focused: true,
            })
            .expect_err("missing workspace should be reported");

        assert!(error.chain().any(|cause| {
            cause
                .to_string()
                .contains("Niri event stream state panicked")
        }));
    }
}
