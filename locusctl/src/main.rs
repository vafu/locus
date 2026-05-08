use std::{
    collections::HashMap,
    io::Write,
    process::{Command as ProcessCommand, Stdio},
};

use anyhow::{Context as AnyhowContext, bail};
use cel::{Context as CelContext, Program as CelProgram, Value as CelValue};
use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};
use futures_util::StreamExt;
use locus_dbus::{
    BUS_NAME, GraphReadProxy, GraphResolveProxy, GraphWriteProxy, NONE_STRING, WATCH_INTERFACE,
};
use zbus::Proxy;
use zbus::fdo::PropertiesProxy;
use zbus::proxy::{Builder as ProxyBuilder, CacheProperties};
use zbus::zvariant::Value;

#[derive(Debug, Parser)]
#[command(name = "locusctl")]
#[command(about = "Publish and query Locus graph state")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    DeleteNode(DeleteNodeArgs),
    Resolve(ResolveArgs),
    ResolveAll(ResolveArgs),
    FindNearest(FindNearestArgs),
    Watch(WatchArgs),
    WatchPath(WatchPathArgs),
}

#[derive(Debug, Subcommand)]
enum LinkCommand {
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
    Subjects(PropSubjects),
    Remove(PropRemove),
}

#[derive(Debug, ClapArgs)]
struct PropSet {
    subject: String,
    key: String,
    value: String,
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

#[derive(Debug, ClapArgs)]
struct PropSubjects {
    #[arg(long)]
    key: Option<String>,
    #[arg(long)]
    value: Option<String>,
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
}

#[derive(Debug, ClapArgs)]
struct ContextGet {
    name: String,
    relation: String,
    #[arg(long)]
    first: bool,
}

#[derive(Debug, ClapArgs)]
struct DeleteNodeArgs {
    subject: String,
}

#[derive(Debug, ClapArgs)]
struct ResolveArgs {
    source: String,
    path: Vec<String>,
}

#[derive(Debug, ClapArgs)]
struct FindNearestArgs {
    source: String,
    kind: String,
}

#[derive(Debug, ClapArgs)]
struct WatchArgs {
    #[arg(value_enum, default_value_t = WatchKind::Any)]
    event: WatchKind,
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    source_prefix: Option<String>,
    #[arg(long)]
    relation: Option<String>,
    #[arg(long)]
    relation_prefix: Option<String>,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    target_prefix: Option<String>,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    subject_prefix: Option<String>,
    #[arg(long)]
    key: Option<String>,
    #[arg(long)]
    key_prefix: Option<String>,
    #[arg(long)]
    value: Option<String>,
    #[arg(long)]
    value_prefix: Option<String>,
    #[arg(long = "missing-property")]
    missing_properties: Vec<String>,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long, value_enum, default_value_t = EmitField::Auto)]
    emit: EmitField,
    #[arg(long = "exec", num_args = 1.., allow_hyphen_values = true)]
    command: Vec<String>,
}

#[derive(Debug, ClapArgs)]
struct WatchPathArgs {
    source: String,
    path: Vec<String>,
    #[arg(long)]
    property: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
enum WatchKind {
    Any,
    LinkAdded,
    LinkRemoved,
    LinkSet,
    PropertyChanged,
    PropertyRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
enum EmitField {
    Auto,
    Source,
    Relation,
    Target,
    Subject,
    Key,
    Value,
    Event,
}

#[derive(Debug)]
enum WatchEvent {
    LinkAdded {
        source: String,
        relation: String,
        target: String,
    },
    LinkRemoved {
        source: String,
        relation: String,
        target: String,
    },
    LinkSet {
        source: String,
        relation: String,
        old_targets: Vec<String>,
        target: String,
    },
    PropertyChanged {
        subject: String,
        key: String,
        value: String,
    },
    PropertyRemoved {
        subject: String,
        key: String,
    },
}

struct WatchFilter {
    program: CelProgram,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let connection = zbus::Connection::session()
        .await
        .context("connect to session D-Bus")?;
    let read = GraphReadProxy::new(&connection)
        .await
        .context("connect read proxy to locusd")?;
    let write = GraphWriteProxy::new(&connection)
        .await
        .context("connect write proxy to locusd")?;
    let resolve = GraphResolveProxy::new(&connection)
        .await
        .context("connect resolve proxy to locusd")?;

    match args.command {
        Command::Link { command } => match command {
            LinkCommand::Set(args) => {
                write
                    .set_link(&args.source, &args.relation, &args.target)
                    .await?;
            }
            LinkCommand::Remove(args) => {
                write
                    .remove_link(&args.source, &args.relation, &args.target)
                    .await?;
            }
            LinkCommand::Clear(args) => {
                write.remove_links(&args.source, &args.relation).await?;
            }
            LinkCommand::Targets(args) => {
                print_query(
                    read.get_targets(&args.subject, &args.relation).await?,
                    args.first,
                );
            }
            LinkCommand::Sources(args) => {
                print_query(
                    read.get_sources(&args.subject, &args.relation).await?,
                    args.first,
                );
            }
            LinkCommand::List { subject } => {
                for (source, relation, target) in read.get_links(&subject).await? {
                    println!("{source}\t{relation}\t{target}");
                }
            }
            LinkCommand::All => {
                for (source, relation, target) in read.get_all_links().await? {
                    println!("{source}\t{relation}\t{target}");
                }
            }
        },
        Command::Prop { command } => match command {
            PropCommand::Set(args) => {
                write
                    .set_property(&args.subject, &args.key, &args.value)
                    .await?;
            }
            PropCommand::Get(args) => {
                if let Some(value) = none(read.get_property(&args.subject, &args.key).await?) {
                    println!("{value}");
                }
            }
            PropCommand::List { subject } => {
                let mut properties = read
                    .get_properties(&subject)
                    .await?
                    .into_iter()
                    .collect::<Vec<_>>();
                properties.sort_by(|a, b| a.0.cmp(&b.0));
                for (key, value) in properties {
                    println!("{key}\t{value}");
                }
            }
            PropCommand::Subjects(args) => {
                let subjects = if let Some(key) = args.key {
                    read.find_subjects(&key, args.value.as_deref().unwrap_or(NONE_STRING))
                        .await?
                } else if args.value.is_some() {
                    bail!("--value requires --key");
                } else {
                    read.get_subjects().await?
                };
                print_lines(subjects);
            }
            PropCommand::Remove(args) => {
                write.remove_property(&args.subject, &args.key).await?;
            }
        },
        Command::Context { command } => match command {
            ContextCommand::Set(args) => {
                write
                    .set_link(&context_subject(&args.name), &args.relation, &args.target)
                    .await?;
            }
            ContextCommand::Get(args) => {
                print_query(
                    read.get_targets(&context_subject(&args.name), &args.relation)
                        .await?,
                    args.first,
                );
            }
        },
        Command::DeleteNode(args) => {
            write.delete_node(&args.subject).await?;
        }
        Command::Resolve(args) => {
            if let Some(subject) = none(resolve.resolve(&args.source, args.path).await?) {
                println!("{subject}");
            }
        }
        Command::ResolveAll(args) => {
            print_lines(resolve.resolve_all(&args.source, args.path).await?);
        }
        Command::FindNearest(args) => {
            if let Some(subject) = none(resolve.find_nearest(&args.source, &args.kind).await?) {
                println!("{subject}");
            }
        }
        Command::Watch(args) => watch(&connection, &read, args).await?,
        Command::WatchPath(args) => watch_path(&connection, &read, &resolve, args).await?,
    }

    Ok(())
}

async fn watch_path(
    connection: &zbus::Connection,
    read: &GraphReadProxy<'_>,
    resolve: &GraphResolveProxy<'_>,
    args: WatchPathArgs,
) -> anyhow::Result<()> {
    let object_path = resolve
        .watch_node(&args.source, args.path)
        .await
        .context("create Locus watch")?;
    let watch = ProxyBuilder::<Proxy<'_>>::new(connection)
        .destination(BUS_NAME)?
        .path(object_path.as_str())?
        .interface(WATCH_INTERFACE)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .context("connect to Locus watch")?;

    let result = if let Some(property) = args.property.as_deref() {
        stream_watch_property(read, &watch, property).await
    } else {
        stream_watch_target(connection, object_path.as_str(), &watch).await
    };

    let close_result = watch.call::<_, _, ()>("Close", &()).await;
    result.and(close_result.context("close Locus watch"))
}

async fn stream_watch_target(
    connection: &zbus::Connection,
    object_path: &str,
    watch: &Proxy<'_>,
) -> anyhow::Result<()> {
    print_watch_value(watch.get_property::<String>("Target").await?)?;

    let properties = PropertiesProxy::builder(connection)
        .destination(BUS_NAME)?
        .path(object_path)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .context("connect to watch properties")?;
    let mut changed = properties.receive_properties_changed().await?;

    loop {
        tokio::select! {
            signal = changed.next() => {
                let Some(signal) = signal else { break; };
                let args = signal.args()?;
                let Some(value) = args.changed_properties().get("Target") else {
                    continue;
                };
                print_watch_value(owned_value_string(value)?)?;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("wait for ctrl-c")?;
                break;
            }
        }
    }

    Ok(())
}

async fn stream_watch_property(
    read: &GraphReadProxy<'_>,
    watch: &Proxy<'_>,
    property: &str,
) -> anyhow::Result<()> {
    print_watch_value(watch_property(read, watch, property).await?)?;
    let mut updated = watch.receive_signal("PropertiesUpdated").await?;

    loop {
        tokio::select! {
            signal = updated.next() => {
                let Some(signal) = signal else { break; };
                let (changed, removed) = signal
                    .body()
                    .deserialize::<(HashMap<String, String>, Vec<String>)>()?;
                if let Some(value) = changed.get(property) {
                    print_watch_value(value)?;
                } else if removed.iter().any(|key| key == property) {
                    print_watch_value("")?;
                }
            }
            result = tokio::signal::ctrl_c() => {
                result.context("wait for ctrl-c")?;
                break;
            }
        }
    }

    Ok(())
}

async fn watch_property(
    read: &GraphReadProxy<'_>,
    watch: &Proxy<'_>,
    property: &str,
) -> anyhow::Result<String> {
    let target = watch.get_property::<String>("Target").await?;
    if target == NONE_STRING {
        return Ok(String::new());
    }
    Ok(none(read.get_property(&target, property).await?).unwrap_or_default())
}

fn owned_value_string(value: &Value<'_>) -> anyhow::Result<String> {
    String::try_from(value).map_err(Into::into)
}

fn print_watch_value(value: impl AsRef<str>) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", value.as_ref()).context("write watch value")?;
    stdout.flush().context("flush watch value")
}

async fn watch(
    connection: &zbus::Connection,
    read: &GraphReadProxy<'_>,
    args: WatchArgs,
) -> anyhow::Result<()> {
    let filter = args
        .filter
        .as_deref()
        .map(WatchFilter::compile)
        .transpose()?;
    let proxy = GraphWriteProxy::new(connection)
        .await
        .context("connect signal proxy to locusd")?;
    let mut link_added = proxy.receive_link_added().await?;
    let mut link_removed = proxy.receive_link_removed().await?;
    let mut link_set = proxy.receive_link_set().await?;
    let mut property_changed = proxy.receive_property_changed().await?;
    let mut property_removed = proxy.receive_property_removed().await?;

    loop {
        tokio::select! {
            signal = link_added.next() => {
                let Some(signal) = signal else { break; };
                let event = {
                    let signal_args = signal.args()?;
                    WatchEvent::LinkAdded {
                        source: signal_args.source.to_string(),
                        relation: signal_args.relation.to_string(),
                        target: signal_args.target.to_string(),
                    }
                };
                handle_watch_event(read, &args, filter.as_ref(), event).await?;
            }
            signal = link_removed.next() => {
                let Some(signal) = signal else { break; };
                let event = {
                    let signal_args = signal.args()?;
                    WatchEvent::LinkRemoved {
                        source: signal_args.source.to_string(),
                        relation: signal_args.relation.to_string(),
                        target: signal_args.target.to_string(),
                    }
                };
                handle_watch_event(read, &args, filter.as_ref(), event).await?;
            }
            signal = link_set.next() => {
                let Some(signal) = signal else { break; };
                let event = {
                    let signal_args = signal.args()?;
                    WatchEvent::LinkSet {
                        source: signal_args.source.to_string(),
                        relation: signal_args.relation.to_string(),
                        old_targets: signal_args.old_targets.iter().map(ToString::to_string).collect(),
                        target: signal_args.target.to_string(),
                    }
                };
                handle_watch_event(read, &args, filter.as_ref(), event).await?;
            }
            signal = property_changed.next() => {
                let Some(signal) = signal else { break; };
                let event = {
                    let signal_args = signal.args()?;
                    WatchEvent::PropertyChanged {
                        subject: signal_args.subject.to_string(),
                        key: signal_args.key.to_string(),
                        value: signal_args.value.to_string(),
                    }
                };
                handle_watch_event(read, &args, filter.as_ref(), event).await?;
            }
            signal = property_removed.next() => {
                let Some(signal) = signal else { break; };
                let event = {
                    let signal_args = signal.args()?;
                    WatchEvent::PropertyRemoved {
                        subject: signal_args.subject.to_string(),
                        key: signal_args.key.to_string(),
                    }
                };
                handle_watch_event(read, &args, filter.as_ref(), event).await?;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("wait for ctrl-c")?;
                break;
            }
        }
    }

    Ok(())
}

async fn handle_watch_event(
    read: &GraphReadProxy<'_>,
    args: &WatchArgs,
    filter: Option<&WatchFilter>,
    event: WatchEvent,
) -> anyhow::Result<()> {
    if !event_matches(read, args, filter, &event).await? {
        return Ok(());
    }

    let payload = event
        .emit(args.emit)
        .context("watch event has no value for selected emit field")?;
    emit_watch_payload(args, &event, payload)
}

async fn event_matches(
    read: &GraphReadProxy<'_>,
    args: &WatchArgs,
    filter: Option<&WatchFilter>,
    event: &WatchEvent,
) -> anyhow::Result<bool> {
    if args.event != WatchKind::Any && args.event != event.kind() {
        return Ok(false);
    }

    if !matches_field(
        event.source(),
        args.source.as_deref(),
        args.source_prefix.as_deref(),
    ) {
        return Ok(false);
    }
    if !matches_field(
        event.relation(),
        args.relation.as_deref(),
        args.relation_prefix.as_deref(),
    ) {
        return Ok(false);
    }
    if !matches_field(
        event.target(),
        args.target.as_deref(),
        args.target_prefix.as_deref(),
    ) {
        return Ok(false);
    }
    if !matches_field(
        event.subject(),
        args.subject.as_deref(),
        args.subject_prefix.as_deref(),
    ) {
        return Ok(false);
    }
    if !matches_field(event.key(), args.key.as_deref(), args.key_prefix.as_deref()) {
        return Ok(false);
    }
    if !matches_field(
        event.value(),
        args.value.as_deref(),
        args.value_prefix.as_deref(),
    ) {
        return Ok(false);
    }

    if !args.missing_properties.is_empty() {
        let Some(subject) = event.subject() else {
            return Ok(false);
        };
        let properties = read.get_properties(subject).await?;
        if args
            .missing_properties
            .iter()
            .any(|key| properties.contains_key(key))
        {
            return Ok(false);
        }
    }

    if let Some(filter) = filter {
        let payload = event.emit(args.emit).unwrap_or("");
        if !filter.matches(event, payload)? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn matches_field(value: Option<&str>, exact: Option<&str>, prefix: Option<&str>) -> bool {
    if let Some(exact) = exact {
        if value != Some(exact) {
            return false;
        }
    }
    if let Some(prefix) = prefix {
        if !value.is_some_and(|value| value.starts_with(prefix)) {
            return false;
        }
    }
    true
}

fn emit_watch_payload(args: &WatchArgs, event: &WatchEvent, payload: &str) -> anyhow::Result<()> {
    if args.command.is_empty() {
        println!("{payload}");
        return Ok(());
    }

    run_watch_command(args, event, payload)
}

fn run_watch_command(args: &WatchArgs, event: &WatchEvent, payload: &str) -> anyhow::Result<()> {
    let (program, command_args) = args
        .command
        .split_first()
        .context("watch command is empty")?;
    let mut child = ProcessCommand::new(program)
        .args(command_args)
        .stdin(Stdio::piped())
        .env("LOCUS_EVENT", event.kind().as_env())
        .env("LOCUS_PAYLOAD", payload)
        .env("LOCUS_SOURCE", event.source().unwrap_or(""))
        .env("LOCUS_RELATION", event.relation().unwrap_or(""))
        .env("LOCUS_TARGET", event.target().unwrap_or(""))
        .env("LOCUS_SUBJECT", event.subject().unwrap_or(""))
        .env("LOCUS_KEY", event.key().unwrap_or(""))
        .env("LOCUS_VALUE", event.value().unwrap_or(""))
        .env("LOCUS_OLD_TARGETS", event.old_targets().join("\t"))
        .spawn()
        .with_context(|| format!("spawn watch command {program:?}"))?;

    let mut stdin = child.stdin.take().context("open watch command stdin")?;
    stdin
        .write_all(payload.as_bytes())
        .context("write watch payload")?;
    stdin
        .write_all(b"\n")
        .context("write watch payload newline")?;
    drop(stdin);

    let status = child.wait().context("wait for watch command")?;
    if !status.success() {
        eprintln!("locusctl: watch command {program:?} exited with {status}");
    }

    Ok(())
}

impl WatchFilter {
    fn compile(expression: &str) -> anyhow::Result<Self> {
        Ok(Self {
            program: CelProgram::compile(expression)
                .with_context(|| format!("compile CEL filter {expression:?}"))?,
        })
    }

    fn matches(&self, event: &WatchEvent, payload: &str) -> anyhow::Result<bool> {
        let mut context = CelContext::default();
        context.add_variable_from_value("event", event.kind().as_env());
        context.add_variable_from_value("payload", payload);
        context.add_variable_from_value("source", event.source().unwrap_or(""));
        context.add_variable_from_value("relation", event.relation().unwrap_or(""));
        context.add_variable_from_value("target", event.target().unwrap_or(""));
        context.add_variable_from_value("subject", event.subject().unwrap_or(""));
        context.add_variable_from_value("key", event.key().unwrap_or(""));
        context.add_variable_from_value("value", event.value().unwrap_or(""));
        context.add_variable_from_value("old_targets", event.old_targets().to_vec());

        let value = self
            .program
            .execute(&context)
            .context("execute CEL filter")?;
        match value {
            CelValue::Bool(result) => Ok(result),
            other => bail!(
                "CEL filter must evaluate to bool, got {:?}",
                other.type_of()
            ),
        }
    }
}

impl WatchEvent {
    fn kind(&self) -> WatchKind {
        match self {
            Self::LinkAdded { .. } => WatchKind::LinkAdded,
            Self::LinkRemoved { .. } => WatchKind::LinkRemoved,
            Self::LinkSet { .. } => WatchKind::LinkSet,
            Self::PropertyChanged { .. } => WatchKind::PropertyChanged,
            Self::PropertyRemoved { .. } => WatchKind::PropertyRemoved,
        }
    }

    fn source(&self) -> Option<&str> {
        match self {
            Self::LinkAdded { source, .. }
            | Self::LinkRemoved { source, .. }
            | Self::LinkSet { source, .. } => Some(source),
            Self::PropertyChanged { .. } | Self::PropertyRemoved { .. } => None,
        }
    }

    fn relation(&self) -> Option<&str> {
        match self {
            Self::LinkAdded { relation, .. }
            | Self::LinkRemoved { relation, .. }
            | Self::LinkSet { relation, .. } => Some(relation),
            Self::PropertyChanged { .. } | Self::PropertyRemoved { .. } => None,
        }
    }

    fn target(&self) -> Option<&str> {
        match self {
            Self::LinkAdded { target, .. }
            | Self::LinkRemoved { target, .. }
            | Self::LinkSet { target, .. } => Some(target),
            Self::PropertyChanged { .. } | Self::PropertyRemoved { .. } => None,
        }
    }

    fn subject(&self) -> Option<&str> {
        match self {
            Self::LinkAdded { source, .. }
            | Self::LinkRemoved { source, .. }
            | Self::LinkSet { source, .. } => Some(source),
            Self::PropertyChanged { subject, .. } | Self::PropertyRemoved { subject, .. } => {
                Some(subject)
            }
        }
    }

    fn key(&self) -> Option<&str> {
        match self {
            Self::PropertyChanged { key, .. } | Self::PropertyRemoved { key, .. } => Some(key),
            Self::LinkAdded { .. } | Self::LinkRemoved { .. } | Self::LinkSet { .. } => None,
        }
    }

    fn value(&self) -> Option<&str> {
        match self {
            Self::PropertyChanged { value, .. } => Some(value),
            Self::PropertyRemoved { .. }
            | Self::LinkAdded { .. }
            | Self::LinkRemoved { .. }
            | Self::LinkSet { .. } => None,
        }
    }

    fn old_targets(&self) -> &[String] {
        match self {
            Self::LinkSet { old_targets, .. } => old_targets,
            Self::LinkAdded { .. }
            | Self::LinkRemoved { .. }
            | Self::PropertyChanged { .. }
            | Self::PropertyRemoved { .. } => &[],
        }
    }

    fn emit(&self, field: EmitField) -> Option<&str> {
        match field {
            EmitField::Auto => match self {
                Self::LinkAdded { target, .. }
                | Self::LinkRemoved { target, .. }
                | Self::LinkSet { target, .. } => Some(target),
                Self::PropertyChanged { subject, .. } | Self::PropertyRemoved { subject, .. } => {
                    Some(subject)
                }
            },
            EmitField::Source => self.source(),
            EmitField::Relation => self.relation(),
            EmitField::Target => self.target(),
            EmitField::Subject => self.subject(),
            EmitField::Key => self.key(),
            EmitField::Value => self.value(),
            EmitField::Event => Some(self.kind().as_env()),
        }
    }
}

impl WatchKind {
    fn as_env(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::LinkAdded => "link_added",
            Self::LinkRemoved => "link_removed",
            Self::LinkSet => "link_set",
            Self::PropertyChanged => "property_changed",
            Self::PropertyRemoved => "property_removed",
        }
    }
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

fn none(value: String) -> Option<String> {
    (value != NONE_STRING).then_some(value)
}

fn context_subject(context: &str) -> String {
    format!("context:{context}")
}
