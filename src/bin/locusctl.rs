use anyhow::Context;
use clap::{Parser, Subcommand};
use locus::api::LocusClient;

#[derive(Debug, Parser)]
#[command(name = "locusctl")]
#[command(about = "Publish and query Locus project state")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Register {
        #[arg(long)]
        path: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        icon: Option<String>,
    },
    BindWorkspace {
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        path: String,
    },
    UnbindWorkspace {
        #[arg(long)]
        workspace_id: String,
    },
    Active,
    List,
    Metadata {
        #[command(subcommand)]
        command: MetadataCommand,
    },
}

#[derive(Debug, Subcommand)]
enum MetadataCommand {
    Set {
        #[arg(long)]
        path: String,
        key: String,
        value: String,
    },
    Remove {
        #[arg(long)]
        path: String,
        key: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let connection = zbus::Connection::session()
        .await
        .context("connect to session D-Bus")?;
    let client = LocusClient::new(&connection)
        .await
        .context("connect to locusd")?;

    match args.command {
        Command::Register { path, name, icon } => {
            let id = client
                .register_project(&path, name.as_deref(), icon.as_deref())
                .await?;
            println!("{id}");
        }
        Command::BindWorkspace { workspace_id, path } => {
            let id = client.bind_workspace(&workspace_id, &path).await?;
            println!("{id}");
        }
        Command::UnbindWorkspace { workspace_id } => {
            client.unbind_workspace(&workspace_id).await?;
        }
        Command::Active => {
            if let Some(id) = client.active_project().await? {
                println!("{id}");
            }
        }
        Command::List => {
            for project in client.list_projects().await? {
                println!("{}", project.id);
            }
        }
        Command::Metadata { command } => match command {
            MetadataCommand::Set { path, key, value } => {
                client.set_metadata(&path, &key, &value).await?;
            }
            MetadataCommand::Remove { path, key } => {
                client.remove_metadata(&path, &key).await?;
            }
        },
    }

    Ok(())
}
