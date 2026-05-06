use std::collections::BTreeSet;

use anyhow::{Context, bail};
use clap::Parser;
use locus_dbus::{Client, ClientExt};
use niri_ipc::state::{EventStreamState, EventStreamStatePart};
use niri_ipc::{Event, Request, Response, socket::Socket};
use tokio::sync::mpsc;

const WORKSPACE_RELATION: &str = "workspace";
const WINDOW_RELATION: &str = "window";
const SELECTED_WORKSPACE_RELATION: &str = "selected-workspace";
const OUTPUT_RELATION: &str = "output";
const SELECTED_CONTEXT: &str = "selected";

#[derive(Debug, Parser)]
#[command(name = "locus-niri")]
#[command(about = "Publish Niri workspace/window state into the Locus graph")]
struct Args {
    #[arg(long)]
    trace: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NiriState {
    workspace_windows: BTreeSet<(String, String)>,
    workspace_outputs: BTreeSet<(String, String)>,
    properties: BTreeSet<(String, String, String)>,
    focused_window: Option<String>,
    focused_workspace: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let connection = zbus::Connection::session()
        .await
        .context("connect to session D-Bus")?;
    let client = Client::new(&connection)
        .await
        .context("connect to locusd")?;
    let mut previous = NiriState::default();

    clear_existing_niri_edges(&client).await;
    let mut state = initial_niri_state().context("load initial Niri state")?;
    let next = state_to_niri_state(&state);
    publish_state(&client, &previous, &next).await;
    previous = next;

    let mut events = niri_event_stream()?;
    eprintln!("locus-niri: publishing Niri graph state from snapshot and event stream");

    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else {
                    bail!("Niri event stream ended");
                };
                if args.trace {
                    eprintln!("locus-niri: event {event:?}");
                }
                let _ = state.apply(event);
                let next = state_to_niri_state(&state);
                publish_state(&client, &previous, &next).await;
                previous = next;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("wait for ctrl-c")?;
                break;
            }
        }
    }

    eprintln!("locus-niri: stopping");
    Ok(())
}

fn niri_event_stream() -> anyhow::Result<mpsc::Receiver<niri_ipc::Event>> {
    let mut socket = Socket::connect().context("connect to Niri IPC socket")?;
    match socket
        .send(Request::EventStream)
        .context("request Niri event stream")?
    {
        Ok(Response::Handled) => {}
        Ok(response) => bail!("unexpected Niri response to event stream request: {response:?}"),
        Err(message) => bail!("Niri rejected event stream request: {message}"),
    }

    let (tx, rx) = mpsc::channel(128);
    std::thread::Builder::new()
        .name("niri-event-stream".to_string())
        .spawn(move || {
            let mut read_event = socket.read_events();
            loop {
                match read_event() {
                    Ok(event) => {
                        if tx.blocking_send(event).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        eprintln!("locus-niri: failed to read Niri event stream: {error}");
                        break;
                    }
                }
            }
        })
        .context("spawn Niri event stream reader")?;

    Ok(rx)
}

fn initial_niri_state() -> anyhow::Result<EventStreamState> {
    let mut socket = Socket::connect().context("connect to Niri IPC socket")?;
    let workspaces = match socket
        .send(Request::Workspaces)
        .context("request Niri workspaces")?
    {
        Ok(Response::Workspaces(workspaces)) => workspaces,
        Ok(response) => bail!("unexpected Niri workspaces response: {response:?}"),
        Err(message) => bail!("Niri rejected workspaces request: {message}"),
    };
    let windows = match socket
        .send(Request::Windows)
        .context("request Niri windows")?
    {
        Ok(Response::Windows(windows)) => windows,
        Ok(response) => bail!("unexpected Niri windows response: {response:?}"),
        Err(message) => bail!("Niri rejected windows request: {message}"),
    };

    let mut state = EventStreamState::default();
    let _ = state.apply(Event::WorkspacesChanged { workspaces });
    let _ = state.apply(Event::WindowsChanged { windows });
    Ok(state)
}

async fn publish_state(client: &Client<'_>, previous: &NiriState, next: &NiriState) {
    for (workspace, window) in previous
        .workspace_windows
        .difference(&next.workspace_windows)
    {
        remove_workspace_window(client, workspace, window).await;
    }
    for (workspace, window) in next
        .workspace_windows
        .difference(&previous.workspace_windows)
    {
        add_workspace_window(client, workspace, window).await;
    }
    for (workspace, output) in previous
        .workspace_outputs
        .difference(&next.workspace_outputs)
    {
        remove_workspace_output(client, workspace, output).await;
    }
    for (workspace, output) in next
        .workspace_outputs
        .difference(&previous.workspace_outputs)
    {
        add_workspace_output(client, workspace, output).await;
    }
    publish_metadata(client, next).await;
    if previous.focused_window != next.focused_window {
        set_or_clear_context(
            client,
            SELECTED_CONTEXT,
            WINDOW_RELATION,
            &next.focused_window,
        )
        .await;
    }
    if previous.focused_workspace != next.focused_workspace {
        set_or_clear_context(
            client,
            SELECTED_CONTEXT,
            SELECTED_WORKSPACE_RELATION,
            &next.focused_workspace,
        )
        .await;
    }
}

async fn add_workspace_window(client: &Client<'_>, workspace: &str, window: &str) {
    let _ = client.set_property(workspace, "kind", "workspace").await;
    let _ = client.set_property(workspace, "source", "niri").await;
    if let Some(id) = workspace.strip_prefix("workspace:") {
        let _ = client.set_property(workspace, "external-id", id).await;
    }
    let _ = client.set_property(window, "kind", "window").await;
    let _ = client.set_property(window, "source", "niri").await;
    if let Some(id) = window.strip_prefix("window:") {
        let _ = client.set_property(window, "external-id", id).await;
    }
    let _ = client.set_link(window, WORKSPACE_RELATION, workspace).await;
}

async fn remove_workspace_window(client: &Client<'_>, workspace: &str, window: &str) {
    let _ = client
        .remove_link(window, WORKSPACE_RELATION, workspace)
        .await;
}

async fn add_workspace_output(client: &Client<'_>, workspace: &str, output: &str) {
    let _ = client.set_property(output, "kind", "output").await;
    let _ = client.set_property(output, "source", "niri").await;
    if let Some(connector) = output.strip_prefix("output:") {
        let _ = client.set_property(output, "connector", connector).await;
    }
    let _ = client.set_link(workspace, OUTPUT_RELATION, output).await;
}

async fn remove_workspace_output(client: &Client<'_>, workspace: &str, output: &str) {
    let _ = client.remove_link(workspace, OUTPUT_RELATION, output).await;
}

async fn publish_metadata(client: &Client<'_>, state: &NiriState) {
    for (subject, key, value) in &state.properties {
        let _ = client.set_property(subject, key, value).await;
    }
}

async fn clear_existing_niri_edges(client: &Client<'_>) {
    let Ok(links) = client.get_all_links().await else {
        return;
    };

    for (source, relation, target) in links {
        let is_workspace_window =
            relation == WINDOW_RELATION && source == context_subject(SELECTED_CONTEXT);
        let is_selected_workspace =
            relation == SELECTED_WORKSPACE_RELATION && source == context_subject(SELECTED_CONTEXT);
        let is_window_workspace = relation == WORKSPACE_RELATION
            && (source.starts_with("window:") || source.starts_with("niri:window:"));
        let is_workspace_output = relation == OUTPUT_RELATION
            && (source.starts_with("workspace:") || source.starts_with("niri:workspace:"));
        if is_workspace_window
            || is_selected_workspace
            || is_window_workspace
            || is_workspace_output
        {
            let _ = client.remove_link(&source, &relation, &target).await;
        }
    }

    let _ = client
        .remove_links(&context_subject(SELECTED_CONTEXT), WORKSPACE_RELATION)
        .await;
    let _ = client
        .remove_links(&context_subject(SELECTED_CONTEXT), WINDOW_RELATION)
        .await;
    let _ = client
        .remove_links(
            &context_subject(SELECTED_CONTEXT),
            SELECTED_WORKSPACE_RELATION,
        )
        .await;

    for kind in ["window", "workspace", "output"] {
        let Ok(subjects) = client.find_subjects_opt("kind", Some(kind)).await else {
            continue;
        };
        for subject in subjects {
            let is_niri_node = client
                .property_opt(&subject, "source")
                .await
                .ok()
                .flatten()
                .as_deref()
                == Some("niri");
            if is_niri_node
                || subject.starts_with("window:")
                || subject.starts_with("workspace:")
                || subject.starts_with("output:")
                || subject.starts_with("niri:window:")
                || subject.starts_with("niri:workspace:")
            {
                let _ = client.remove_property(&subject, "kind").await;
                let _ = client.remove_property(&subject, "source").await;
                let _ = client.remove_property(&subject, "external-id").await;
                let _ = client.remove_property(&subject, "connector").await;
            }
        }
    }
}

fn state_to_niri_state(state: &EventStreamState) -> NiriState {
    let mut properties = BTreeSet::new();

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
    let focused_workspace = focused_workspace.map(|workspace| workspace_subject(workspace.id));

    NiriState {
        workspace_windows,
        workspace_outputs,
        properties,
        focused_window,
        focused_workspace,
    }
}

fn insert_property(
    properties: &mut BTreeSet<(String, String, String)>,
    subject: &str,
    key: &str,
    value: impl Into<String>,
) {
    properties.insert((subject.to_string(), key.to_string(), value.into()));
}

fn workspace_subject(id: u64) -> String {
    format!("workspace:{id}")
}

fn window_subject(id: u64) -> String {
    format!("window:{id}")
}

fn output_subject(name: &str) -> String {
    format!("output:{name}")
}

fn context_subject(context: &str) -> String {
    format!("context:{context}")
}

async fn set_or_clear_context(
    client: &Client<'_>,
    context: &str,
    relation: &str,
    target: &Option<String>,
) {
    let source = context_subject(context);
    if let Some(target) = target {
        let _ = client.set_link(&source, relation, target).await;
    } else {
        let _ = client.remove_links(&source, relation).await;
    }
}
