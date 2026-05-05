use anyhow::Context;
use clap::{Args as ClapArgs, Parser, Subcommand};
use locus::api::LocusClient;

#[derive(Debug, Parser)]
#[command(name = "locusctl")]
#[command(about = "Publish and query Locus graph state")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Link {
        #[command(subcommand)]
        command: LinkCommand,
    },
    Prop {
        #[command(subcommand)]
        command: PropCommand,
    },
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    Ensure(ProjectEnsure),
    List,
}

#[derive(Debug, ClapArgs)]
struct ProjectEnsure {
    path: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    icon: Option<String>,
    #[arg(long)]
    durable: bool,
}

#[derive(Debug, Subcommand)]
enum LinkCommand {
    Add(LinkAdd),
    Set(LinkAdd),
    Remove(LinkRemove),
    Clear(LinkClear),
    Targets(LinkQuery),
    Sources(LinkQuery),
    List { subject: String },
    All,
}

#[derive(Debug, ClapArgs)]
struct LinkAdd {
    source: String,
    relation: String,
    target: String,
    #[arg(long)]
    durable: bool,
}

#[derive(Debug, ClapArgs)]
struct LinkRemove {
    source: String,
    relation: String,
    target: String,
}

#[derive(Debug, ClapArgs)]
struct LinkClear {
    source: String,
    relation: String,
}

#[derive(Debug, ClapArgs)]
struct LinkQuery {
    subject: String,
    relation: String,
    #[arg(long)]
    first: bool,
}

#[derive(Debug, Subcommand)]
enum PropCommand {
    Set(PropSet),
    Get(PropGet),
    List { subject: String },
    Remove(PropRemove),
}

#[derive(Debug, ClapArgs)]
struct PropSet {
    subject: String,
    key: String,
    value: String,
    #[arg(long)]
    durable: bool,
}

#[derive(Debug, ClapArgs)]
struct PropGet {
    subject: String,
    key: String,
}

#[derive(Debug, ClapArgs)]
struct PropRemove {
    subject: String,
    key: String,
}

#[derive(Debug, Subcommand)]
enum ContextCommand {
    Set(ContextSet),
    Get(ContextGet),
}

#[derive(Debug, ClapArgs)]
struct ContextSet {
    name: String,
    relation: String,
    target: String,
    #[arg(long)]
    durable: bool,
}

#[derive(Debug, ClapArgs)]
struct ContextGet {
    name: String,
    relation: String,
    #[arg(long)]
    first: bool,
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
        Command::Project { command } => match command {
            ProjectCommand::Ensure(args) => {
                let subject = client
                    .ensure_project(
                        &args.path,
                        args.name.as_deref(),
                        args.icon.as_deref(),
                        args.durable,
                    )
                    .await?;
                println!("{subject}");
            }
            ProjectCommand::List => print_lines(client.list_projects().await?),
        },
        Command::Link { command } => match command {
            LinkCommand::Add(args) => {
                client
                    .add_link(&args.source, &args.relation, &args.target, args.durable)
                    .await?;
            }
            LinkCommand::Set(args) => {
                client
                    .set_link(&args.source, &args.relation, &args.target, args.durable)
                    .await?;
            }
            LinkCommand::Remove(args) => {
                client
                    .remove_link(&args.source, &args.relation, &args.target)
                    .await?;
            }
            LinkCommand::Clear(args) => {
                client.remove_links(&args.source, &args.relation).await?;
            }
            LinkCommand::Targets(args) => {
                print_query(
                    client.targets(&args.subject, &args.relation).await?,
                    args.first,
                );
            }
            LinkCommand::Sources(args) => {
                print_query(
                    client.sources(&args.subject, &args.relation).await?,
                    args.first,
                );
            }
            LinkCommand::List { subject } => {
                for (source, relation, target) in client.links(&subject).await? {
                    println!("{source}\t{relation}\t{target}");
                }
            }
            LinkCommand::All => {
                for (source, relation, target) in client.all_links().await? {
                    println!("{source}\t{relation}\t{target}");
                }
            }
        },
        Command::Prop { command } => match command {
            PropCommand::Set(args) => {
                client
                    .set_property(&args.subject, &args.key, &args.value, args.durable)
                    .await?;
            }
            PropCommand::Get(args) => {
                if let Some(value) = client.property(&args.subject, &args.key).await? {
                    println!("{value}");
                }
            }
            PropCommand::List { subject } => {
                let mut properties = client
                    .properties(&subject)
                    .await?
                    .into_iter()
                    .collect::<Vec<_>>();
                properties.sort_by(|a, b| a.0.cmp(&b.0));
                for (key, value) in properties {
                    println!("{key}\t{value}");
                }
            }
            PropCommand::Remove(args) => {
                client.remove_property(&args.subject, &args.key).await?;
            }
        },
        Command::Context { command } => match command {
            ContextCommand::Set(args) => {
                client
                    .set_context_link(&args.name, &args.relation, &args.target, args.durable)
                    .await?;
            }
            ContextCommand::Get(args) => {
                print_query(
                    client.context_targets(&args.name, &args.relation).await?,
                    args.first,
                );
            }
        },
    }

    Ok(())
}

fn print_query(values: Vec<String>, first: bool) {
    if first {
        if let Some(value) = values.first() {
            println!("{value}");
        }
    } else {
        print_lines(values);
    }
}

fn print_lines(values: Vec<String>) {
    for value in values {
        println!("{value}");
    }
}
