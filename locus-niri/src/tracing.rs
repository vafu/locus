use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use niri_ipc::Event;
use tracing_perfetto::PerfettoLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init(trace_perfetto: Option<&PathBuf>) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    if let Some(path) = trace_perfetto {
        let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
        tracing_subscriber::registry()
            .with(filter)
            .with(PerfettoLayer::new(Mutex::new(file)))
            .init();
        eprintln!("locus-niri: writing Perfetto trace to {}", path.display());
    } else {
        tracing_subscriber::registry().with(filter).init();
    }
    Ok(())
}

pub fn trace_span_for_event(event: &Event) -> ::tracing::Span {
    ::tracing::trace_span!("niri.event", event = event_name(event))
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::WorkspacesChanged { .. } => "WorkspacesChanged",
        Event::WorkspaceUrgencyChanged { .. } => "WorkspaceUrgencyChanged",
        Event::WorkspaceActivated { .. } => "WorkspaceActivated",
        Event::WorkspaceActiveWindowChanged { .. } => "WorkspaceActiveWindowChanged",
        Event::WindowsChanged { .. } => "WindowsChanged",
        Event::WindowOpenedOrChanged { .. } => "WindowOpenedOrChanged",
        Event::WindowClosed { .. } => "WindowClosed",
        Event::WindowFocusChanged { .. } => "WindowFocusChanged",
        Event::WindowFocusTimestampChanged { .. } => "WindowFocusTimestampChanged",
        Event::WindowUrgencyChanged { .. } => "WindowUrgencyChanged",
        Event::WindowLayoutsChanged { .. } => "WindowLayoutsChanged",
        Event::KeyboardLayoutsChanged { .. } => "KeyboardLayoutsChanged",
        Event::KeyboardLayoutSwitched { .. } => "KeyboardLayoutSwitched",
        Event::OverviewOpenedOrClosed { .. } => "OverviewOpenedOrClosed",
        Event::ConfigLoaded { .. } => "ConfigLoaded",
        Event::ScreenshotCaptured { .. } => "ScreenshotCaptured",
    }
}
