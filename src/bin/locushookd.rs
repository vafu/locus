use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail};
use clap::Parser;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use locus::api::{GraphProxy, LocusClient};

const PROJECT_PREFIX: &str = "project:";

#[derive(Debug, Parser)]
#[command(name = "locushookd")]
#[command(about = "Run reactive hooks for Locus graph changes")]
struct Args {
    #[arg(long, default_value = "pick-icon")]
    icon_picker: PathBuf,
    #[arg(long)]
    durable_icons: bool,
}

#[derive(Debug, Clone)]
enum HookEvent {
    LinkAdded {
        source: String,
        relation: String,
        target: String,
    },
    PropertyChanged {
        subject: String,
        key: String,
        value: String,
    },
}

struct HookContext<'a> {
    client: &'a LocusClient<'a>,
}

trait Hook {
    fn name(&self) -> &'static str;

    fn handle<'a>(
        &'a self,
        event: &'a HookEvent,
        context: &'a HookContext<'a>,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
}

struct HookRunner {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookRunner {
    fn new(hooks: Vec<Box<dyn Hook>>) -> Self {
        Self { hooks }
    }

    async fn handle(&self, event: HookEvent, context: &HookContext<'_>) {
        for hook in &self.hooks {
            if let Err(error) = hook.handle(&event, context).await {
                eprintln!("locushookd: hook {} failed: {error:#}", hook.name());
            }
        }
    }
}

#[derive(Debug)]
struct ProjectIconHook {
    icon_picker: PathBuf,
    durable_icons: bool,
}

impl ProjectIconHook {
    fn new(icon_picker: PathBuf, durable_icons: bool) -> Self {
        Self {
            icon_picker,
            durable_icons,
        }
    }

    async fn maybe_update_icon(
        &self,
        subject: &str,
        context: &HookContext<'_>,
    ) -> anyhow::Result<()> {
        if !subject.starts_with(PROJECT_PREFIX) {
            return Ok(());
        }

        let properties = context
            .client
            .properties(subject)
            .await
            .with_context(|| format!("read properties for {subject}"))?;
        if properties.get("kind").map(String::as_str) != Some("project") {
            return Ok(());
        }
        if properties.contains_key("icon") {
            return Ok(());
        }

        let icon = pick_project_icon(&self.icon_picker, &properties)
            .with_context(|| format!("pick icon for {subject}"))?;
        eprintln!("locushookd: setting {subject} icon={icon}");
        context
            .client
            .set_property(subject, "icon", &icon, self.durable_icons)
            .await
            .with_context(|| format!("set icon for {subject}"))?;
        Ok(())
    }
}

impl Hook for ProjectIconHook {
    fn name(&self) -> &'static str {
        "project-icon"
    }

    fn handle<'a>(
        &'a self,
        event: &'a HookEvent,
        context: &'a HookContext<'a>,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            match event {
                HookEvent::PropertyChanged {
                    subject,
                    key,
                    value,
                } => {
                    let _ = value;
                    if matches!(key.as_str(), "kind" | "path" | "name") {
                        self.maybe_update_icon(subject, context).await?;
                    }
                }
                HookEvent::LinkAdded {
                    source,
                    relation,
                    target,
                } => {
                    let _ = (source, relation, target);
                }
            }
            Ok(())
        })
    }
}

fn pick_project_icon(
    icon_picker: &Path,
    properties: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let mut command = std::process::Command::new(icon_picker);
    command.arg("-n").arg("1");

    if let Some(name) = properties.get("name") {
        command.arg("-s").arg(name);
    }
    if let Some(path) = properties.get("path") {
        command.arg("-s").arg(path);
        for filename in ["README.md", "README", "Cargo.toml", "package.json"] {
            let candidate = Path::new(path).join(filename);
            if candidate.is_file() {
                command.arg("-f").arg(candidate);
                break;
            }
        }
    }

    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("run {}", icon_picker.display()))?;
    if !output.status.success() {
        bail!(
            "{} exited with {status}: {stderr}",
            icon_picker.display(),
            status = output.status,
            stderr = String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8(output.stdout).context("icon picker output is not UTF-8")?;
    stdout
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .context("icon picker returned no icon")
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
    let proxy = GraphProxy::new(&connection)
        .await
        .context("connect signal proxy to locusd")?;

    let runner = HookRunner::new(vec![Box::new(ProjectIconHook::new(
        args.icon_picker,
        args.durable_icons,
    ))]);
    let context = HookContext { client: &client };
    let mut link_added = proxy.receive_link_added().await?;
    let mut property_changed = proxy.receive_property_changed().await?;

    eprintln!("locushookd: listening for Locus graph signals");
    loop {
        tokio::select! {
            signal = link_added.next() => {
                let Some(signal) = signal else { break; };
                let args = signal.args()?;
                runner.handle(HookEvent::LinkAdded {
                    source: args.source.to_string(),
                    relation: args.relation.to_string(),
                    target: args.target.to_string(),
                }, &context).await;
            }
            signal = property_changed.next() => {
                let Some(signal) = signal else { break; };
                let args = signal.args()?;
                runner.handle(HookEvent::PropertyChanged {
                    subject: args.subject.to_string(),
                    key: args.key.to_string(),
                    value: args.value.to_string(),
                }, &context).await;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("wait for ctrl-c")?;
                break;
            }
        }
    }
    eprintln!("locushookd: stopping");
    Ok(())
}
