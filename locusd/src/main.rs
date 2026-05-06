use anyhow::Context;
use clap::Parser;
use locus_core::LocusService;
use locus_schema::GraphSchema;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "locusd")]
#[command(about = "Run the Locus D-Bus graph service")]
struct Args {
    #[arg(long)]
    schema: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let schema_path = args.schema.unwrap_or_else(default_schema_path);
    let schema = GraphSchema::load(&schema_path)
        .with_context(|| format!("load schema {}", schema_path.display()))?;

    eprintln!("locusd: starting");
    let service = LocusService::with_schema(schema);
    let _connection = locus_dbus::serve(service)
        .await
        .context("start D-Bus service")?;
    eprintln!("locusd: listening on D-Bus name {}", locus_dbus::BUS_NAME);
    tokio::signal::ctrl_c().await.context("wait for ctrl-c")?;
    eprintln!("locusd: stopping");
    Ok(())
}

fn default_schema_path() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home).join("locus/schema.yaml");
    }
    let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".config/locus/schema.yaml")
}
