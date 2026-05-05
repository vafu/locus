use std::collections::BTreeSet;

use anyhow::{Context, bail};
use clap::Parser;
use locus::Client;
use niri_ipc::state::{EventStreamState, EventStreamStatePart};
use niri_ipc::{Request, Response, socket::Socket};
use tokio::sync::mpsc;

const WORKSPACE_RELATION: &str = "workspace";
const WINDOW_RELATION: &str = "window";
const PROJECT_RELATION: &str = "project";
const SELECTED_CONTEXT: &str = "selected";
const ACTIVE_CONTEXT: &str = "active";

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
    focused_workspace: Option<String>,
    focused_window: Option<String>,
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
    let mut state = EventStreamState::default();
    let mut events = niri_event_stream()?;
    eprintln!("locus-niri: publishing Niri graph state from Niri event stream");

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
                sync_active_project(&client, next.focused_workspace.as_deref()).await;
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
    if previous.focused_workspace != next.focused_workspace {
        set_or_clear_context(client, SELECTED_CONTEXT, WORKSPACE_RELATION, &next.focused_workspace)
            .await;
    }
    if previous.focused_window != next.focused_window {
        set_or_clear_context(client, SELECTED_CONTEXT, WINDOW_RELATION, &next.focused_window).await;
    }
}

async fn add_workspace_window(client: &Client<'_>, workspace: &str, window: &str) {
    let _ = client
        .add_link(workspace, WINDOW_RELATION, window, false)
        .await;
}

async fn remove_workspace_window(client: &Client<'_>, workspace: &str, window: &str) {
    let _ = client.remove_link(workspace, WINDOW_RELATION, window).await;
    let _ = client
        .remove_link(window, WORKSPACE_RELATION, workspace)
        .await;
}

async fn clear_existing_niri_edges(client: &Client<'_>) {
    let Ok(links) = client.all_links().await else {
        return;
    };

    for (source, relation, target) in links {
        let is_workspace_window =
            relation == WINDOW_RELATION && source.starts_with("niri:workspace:");
        let is_window_workspace =
            relation == WORKSPACE_RELATION && source.starts_with("niri:window:");
        if is_workspace_window || is_window_workspace {
            let _ = client.remove_link(&source, &relation, &target).await;
        }
    }

    let _ = client
        .remove_links(&context_subject(SELECTED_CONTEXT), WORKSPACE_RELATION)
        .await;
    let _ = client
        .remove_links(&context_subject(SELECTED_CONTEXT), WINDOW_RELATION)
        .await;
}

fn state_to_niri_state(state: &EventStreamState) -> NiriState {
    let workspace_windows = state
        .windows
        .windows
        .values()
        .filter_map(|window| {
            let workspace_id = window.workspace_id?;
            Some((workspace_subject(workspace_id), window_subject(window.id)))
        })
        .collect();

    let focused_workspace = state
        .workspaces
        .workspaces
        .values()
        .find(|workspace| workspace.is_focused)
        .map(|workspace| workspace_subject(workspace.id));
    let focused_window = state
        .windows
        .windows
        .values()
        .find(|window| window.is_focused)
        .map(|window| window_subject(window.id))
        .or_else(|| {
            state
                .workspaces
                .workspaces
                .values()
                .find(|workspace| workspace.is_focused)
                .and_then(|workspace| workspace.active_window_id)
                .map(window_subject)
        });

    NiriState {
        workspace_windows,
        focused_workspace,
        focused_window,
    }
}

fn workspace_subject(id: u64) -> String {
    format!("niri:workspace:{id}")
}

fn window_subject(id: u64) -> String {
    format!("niri:window:{id}")
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
        let _ = client.set_link(&source, relation, target, false).await;
    } else {
        let _ = client.remove_links(&source, relation).await;
    }
}

async fn sync_active_project(client: &Client<'_>, focused_workspace: Option<&str>) {
    let Some(workspace) = focused_workspace else {
        let _ = client
            .remove_links(&context_subject(ACTIVE_CONTEXT), PROJECT_RELATION)
            .await;
        return;
    };

    let project = client
        .targets(workspace, PROJECT_RELATION)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|target| target.starts_with("project:"));

    set_or_clear_context(client, ACTIVE_CONTEXT, PROJECT_RELATION, &project).await;
}
