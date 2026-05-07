mod graph;
mod ipc;
mod projection;
mod publisher;
mod tracing;

use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::Parser;
use locus_dbus::{GraphReadProxy, GraphWriteProxy};
use projection::GraphProjection;
use publisher::{apply_mutations, clear_existing_niri_edges};

#[derive(Debug, Parser)]
#[command(name = "locus-niri")]
#[command(about = "Publish Niri workspace/window state into the Locus graph")]
struct Args {
    #[arg(long)]
    trace: bool,

    #[arg(long, value_name = "PATH")]
    trace_perfetto: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    tracing::init(args.trace_perfetto.as_ref())?;

    let connection = zbus::Connection::session()
        .await
        .context("connect to session D-Bus")?;
    let read = GraphReadProxy::new(&connection)
        .await
        .context("connect read proxy to locusd")?;
    let write = GraphWriteProxy::new(&connection)
        .await
        .context("connect write proxy to locusd")?;
    let mut projection = GraphProjection::default();

    clear_existing_niri_edges(&read, &write)
        .await
        .context("clear old Niri graph state")?;
    let mut events = ipc::event_stream()?;
    eprintln!("locus-niri: publishing Niri graph state from event stream");

    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else {
                    bail!("Niri event stream ended");
                };
                if args.trace {
                    eprintln!("locus-niri: event {event:?}");
                }
                let span = tracing::trace_span_for_event(&event);
                let _guard = span.enter();
                let mutations = projection.project(event)?;
                apply_mutations(&write, mutations)
                    .await
                    .context("publish Niri graph mutations")?;
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
