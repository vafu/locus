use anyhow::Context;
use locus::{LocusService, dbus, storage::SqliteStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("locusd: starting");
    let store = SqliteStore::open_default().context("open locus SQLite store")?;
    let service = LocusService::new(store).context("initialize locus service")?;
    let _connection = dbus::serve(service).await.context("start D-Bus service")?;
    eprintln!("locusd: listening on D-Bus name {}", locus::api::BUS_NAME);
    tokio::signal::ctrl_c().await.context("wait for ctrl-c")?;
    eprintln!("locusd: stopping");
    Ok(())
}
