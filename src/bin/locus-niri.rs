use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::Parser;
use locus::Client;
use niri_ipc::{Request, Response, socket::Socket};

const WORKSPACE_RELATION: &str = "workspace";
const WINDOW_RELATION: &str = "window";

#[derive(Debug, Parser)]
#[command(name = "locus-niri")]
#[command(about = "Publish Niri workspace/window state into the Locus graph")]
struct Args {
    #[arg(long, default_value_t = 750)]
    interval_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NiriState {
    workspace_windows: BTreeSet<(String, String)>,
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
    let interval = Duration::from_millis(args.interval_ms);
    let mut previous = NiriState::default();

    clear_existing_niri_edges(&client).await;
    eprintln!("locus-niri: publishing Niri graph state");

    loop {
        match read_niri_state() {
            Ok(next) => {
                publish_state(&client, &previous, &next).await;
                previous = next;
            }
            Err(error) => eprintln!("locus-niri: failed to read Niri state: {error:#}"),
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            result = tokio::signal::ctrl_c() => {
                result.context("wait for ctrl-c")?;
                break;
            }
        }
    }

    eprintln!("locus-niri: stopping");
    Ok(())
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
}

fn read_niri_state() -> anyhow::Result<NiriState> {
    let mut socket = Socket::connect().context("connect to Niri IPC socket")?;
    let reply = socket
        .send(Request::Windows)
        .context("request Niri windows")?;
    let windows = match reply {
        Ok(Response::Windows(windows)) => windows,
        Ok(response) => bail!("unexpected Niri response to windows request: {response:?}"),
        Err(message) => bail!("Niri rejected windows request: {message}"),
    };

    let workspace_windows = windows
        .into_iter()
        .filter_map(|window| {
            let workspace_id = window.workspace_id?;
            Some((workspace_subject(workspace_id), window_subject(window.id)))
        })
        .collect();

    Ok(NiriState { workspace_windows })
}

fn workspace_subject(id: u64) -> String {
    format!("niri:workspace:{id}")
}

fn window_subject(id: u64) -> String {
    format!("niri:window:{id}")
}
