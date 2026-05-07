use std::collections::{BTreeSet, HashMap};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use locus_dbus::{GraphReadProxy, LinkTuple};
use locus_schema::{Cardinality, GraphSchema, NodeSelector, Retention};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEFAULT_ADDR: &str = "127.0.0.1:8765";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    let connection = Arc::new(
        zbus::Connection::session()
            .await
            .context("connect to session D-Bus")?,
    );

    eprintln!("locus-graph: http://{addr}");
    loop {
        let (stream, _) = listener.accept().await?;
        let connection = Arc::clone(&connection);
        tokio::spawn(async move {
            if let Err(error) = handle(stream, connection).await {
                eprintln!("locus-graph: request failed: {error:#}");
            }
        });
    }
}

async fn handle(mut stream: TcpStream, connection: Arc<zbus::Connection>) -> anyhow::Result<()> {
    let mut buffer = vec![0; 8192];
    let read = stream.read(&mut buffer).await?;
    if read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    match path {
        "/" => respond(&mut stream, "text/html; charset=utf-8", INDEX_HTML).await,
        "/graph.json" => {
            let body = graph_json(&connection).await?;
            respond(&mut stream, "application/json", &body).await
        }
        _ => respond_status(&mut stream, "404 Not Found", "text/plain", "not found").await,
    }
}

async fn graph_json(connection: &zbus::Connection) -> anyhow::Result<String> {
    let read = GraphReadProxy::new(connection)
        .await
        .context("connect read proxy to locusd")?;
    let links = read.get_all_links().await.context("get graph links")?;
    let mut subjects = BTreeSet::new();
    for (source, _, target) in &links {
        subjects.insert(source.clone());
        subjects.insert(target.clone());
    }

    let mut properties = HashMap::new();
    for subject in &subjects {
        properties.insert(
            subject.clone(),
            read.get_properties(subject).await.unwrap_or_default(),
        );
    }

    let nodes = subjects
        .iter()
        .map(|subject| {
            let props = properties.get(subject).cloned().unwrap_or_default();
            node_json(subject, &props)
        })
        .collect::<Vec<_>>()
        .join(",");
    let links = links.iter().map(link_json).collect::<Vec<_>>().join(",");
    let relations = load_schema()
        .map(|schema| {
            schema
                .relations()
                .iter()
                .map(|(name, relation)| {
                    format!(
                        "{{\"name\":{},\"from\":{},\"to\":{},\"cardinality\":{},\"retention\":{}}}",
                        json_string(name),
                        selector_json(&relation.source),
                        selector_json(&relation.target),
                        json_string(cardinality_name(
                            relation.sources_per_target,
                            relation.targets_per_source,
                        )),
                        json_string(retention_name(relation.retention)),
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    Ok(format!(
        "{{\"nodes\":[{nodes}],\"links\":[{links}],\"relations\":[{relations}]}}"
    ))
}

fn node_json(subject: &str, properties: &HashMap<String, String>) -> String {
    let label = properties
        .get("name")
        .or_else(|| properties.get("path"))
        .map(String::as_str)
        .unwrap_or_else(|| short_label(subject));
    let kind = properties
        .get("kind")
        .map(String::as_str)
        .unwrap_or_else(|| subject.split(':').next().unwrap_or("node"));
    let properties = properties
        .iter()
        .map(|(key, value)| format!("{}:{}", json_string(key), json_string(value)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":{},\"label\":{},\"kind\":{},\"properties\":{{{}}}}}",
        json_string(subject),
        json_string(label),
        json_string(kind),
        properties
    )
}

fn link_json((source, relation, target): &LinkTuple) -> String {
    format!(
        "{{\"source\":{},\"relation\":{},\"target\":{}}}",
        json_string(source),
        json_string(relation),
        json_string(target)
    )
}

fn load_schema() -> Option<GraphSchema> {
    GraphSchema::load(default_schema_path()).ok()
}

fn default_schema_path() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home).join("locus/schema.yaml");
    }
    let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".config/locus/schema.yaml")
}

fn selector_json(selector: &NodeSelector) -> String {
    match selector {
        NodeSelector::Any => "{\"type\":\"any\"}".to_string(),
        NodeSelector::Kind(kind) => format!("{{\"type\":\"kind\",\"kind\":{}}}", json_string(kind)),
        NodeSelector::Exact(id) => format!("{{\"type\":\"exact\",\"id\":{}}}", json_string(id)),
    }
}

fn cardinality_name(
    sources_per_target: Cardinality,
    targets_per_source: Cardinality,
) -> &'static str {
    match (sources_per_target, targets_per_source) {
        (Cardinality::One, Cardinality::One) => "1:1",
        (Cardinality::Many, Cardinality::One) => "*:1",
        (Cardinality::One, Cardinality::Many) => "1:*",
        (Cardinality::Many, Cardinality::Many) => "*:*",
    }
}

fn retention_name(retention: Retention) -> &'static str {
    match retention {
        Retention::Strong => "strong",
        Retention::Weak => "weak",
    }
}

fn short_label(subject: &str) -> &str {
    subject
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .or_else(|| subject.rsplit_once(':').map(|(_, tail)| tail))
        .unwrap_or(subject)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

async fn respond(stream: &mut TcpStream, content_type: &str, body: &str) -> anyhow::Result<()> {
    respond_status(stream, "200 OK", content_type, body).await
}

async fn respond_status(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    Ok(())
}

const INDEX_HTML: &str = r##"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Locus Graph</title>
  <style>
    :root { color-scheme: dark; --bg: #111318; --panel: #181b22; --fg: #e8e9ef; --dim: #9da3b4; --line: #5b6478; --accent: #e8ac38; --green: #68c48c; --red: #df6b6b; }
    * { box-sizing: border-box; }
    body { margin: 0; height: 100vh; overflow: hidden; background: var(--bg); color: var(--fg); font: 13px/1.4 system-ui, sans-serif; }
    #app { display: grid; grid-template-columns: minmax(0, 1fr) 360px; height: 100vh; }
    body.panel-hidden #app { grid-template-columns: minmax(0, 1fr); }
    body.panel-hidden aside { display: none; }
    #graph { width: 100%; height: 100%; cursor: grab; touch-action: none; user-select: none; -webkit-user-select: none; }
    #graph:active { cursor: grabbing; }
    aside { border-left: 1px solid #2a2f3b; background: var(--panel); display: grid; grid-template-rows: auto auto auto minmax(0,1fr); min-width: 0; }
    #panel-toggle { position: fixed; right: 12px; top: 12px; z-index: 10; background: #242a35; color: var(--fg); border: 1px solid #3a4252; border-radius: 6px; padding: 7px 10px; box-shadow: 0 8px 24px #0008; }
    header { padding: 14px 16px; border-bottom: 1px solid #2a2f3b; }
    h1 { margin: 0; font-size: 15px; }
    #stats { margin-top: 4px; color: var(--dim); }
    #legend { display: flex; flex-wrap: wrap; gap: 6px 10px; margin-top: 10px; color: var(--dim); font-size: 12px; }
    .legend-item { display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
    .legend-dot { width: 10px; height: 10px; border-radius: 50%; border: 1px solid currentColor; background: var(--kind-fill, #252b36); color: var(--kind-stroke, #f0c15c); }
    .legend-line { width: 18px; height: 0; border-top: 2px solid var(--line); }
    .legend-line.weak { border-top-style: dashed; border-color: var(--accent); }
    #details { padding: 12px 16px; border-bottom: 1px solid #2a2f3b; min-height: 132px; }
    #details .id { overflow-wrap: anywhere; color: var(--accent); font-weight: 700; }
    #details table { margin-top: 8px; width: 100%; border-collapse: collapse; }
    #details td { padding: 2px 0; vertical-align: top; }
    #details td:first-child { color: var(--dim); padding-right: 10px; width: 1%; white-space: nowrap; }
    #controls { padding: 10px 16px; border-bottom: 1px solid #2a2f3b; display: grid; gap: 7px; }
    #controls label { display: grid; grid-template-columns: 92px minmax(0, 1fr) 54px; gap: 8px; align-items: center; color: var(--dim); }
    #controls input[type="range"] { width: 100%; }
    #controls output { color: var(--fg); text-align: right; font-variant-numeric: tabular-nums; }
    .control-actions { display: flex; gap: 8px; margin-top: 2px; }
    .control-actions button { background: #242a35; color: var(--fg); border: 1px solid #3a4252; border-radius: 6px; padding: 4px 8px; }
    #log { overflow: auto; padding: 8px 10px 16px; }
    .event { border-left: 3px solid var(--line); margin: 8px 0; padding: 6px 8px; background: #131720; }
    .event.add { border-color: var(--green); }
    .event.remove { border-color: var(--red); }
    .event.set { border-color: var(--accent); }
    .event .kind { font-weight: 700; }
    .event .link { color: var(--dim); overflow-wrap: anywhere; }
    .link-line { stroke: var(--line); stroke-width: 1.6; marker-end: url(#arrow); }
    .link-line.weak { stroke: var(--accent); stroke-dasharray: 5 4; marker-end: url(#arrow-weak); }
    .link-label { fill: var(--dim); font-size: 12px; pointer-events: none; }
    .node circle { stroke: var(--kind-stroke, #f0c15c); stroke-width: 1.5; fill: var(--kind-fill, #252b36); }
    .node text { fill: var(--fg); font-size: 13px; pointer-events: none; text-anchor: middle; paint-order: stroke; stroke: var(--bg); stroke-width: 3px; }
    .node.selected circle { stroke-width: 3; }
    @media (max-width: 760px) {
      #app { grid-template-columns: minmax(0, 1fr); }
      aside { position: fixed; inset: 0 0 0 auto; width: min(88vw, 360px); z-index: 9; box-shadow: -20px 0 40px #0008; }
      header { padding: 10px 12px; }
      #details { padding: 10px 12px; min-height: 86px; max-height: 26vh; overflow: auto; }
      #controls { padding: 10px 12px; gap: 10px; }
      #controls label { grid-template-columns: minmax(0, 1fr) 48px; gap: 4px 8px; }
      #controls label span { grid-column: 1; }
      #controls label input { grid-column: 1 / -1; grid-row: 2; }
      #controls label output { grid-column: 2; grid-row: 1; }
      .control-actions { flex-wrap: wrap; }
      .control-actions button { min-height: 34px; }
      #log { display: none; }
      body.panel-hidden aside { display: none; }
    }
  </style>
</head>
<body>
<div id="app">
  <button id="panel-toggle" type="button" aria-expanded="true">Hide panel</button>
  <svg id="graph">
    <defs>
      <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#4e566b"></path>
      </marker>
      <marker id="arrow-weak" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#e8ac38"></path>
      </marker>
    </defs>
    <g id="viewport"><g id="links"></g><g id="labels"></g><g id="nodes"></g></g>
  </svg>
  <aside>
    <header><h1>Locus Graph</h1><div id="stats">connecting...</div><div id="legend"></div></header>
    <section id="details"><div class="id">Select a node</div></section>
    <section id="controls"></section>
    <section id="log"></section>
  </aside>
</div>
<script>
const svg = document.getElementById('graph');
const viewport = document.getElementById('viewport');
const linksG = document.getElementById('links');
const labelsG = document.getElementById('labels');
const nodesG = document.getElementById('nodes');
const stats = document.getElementById('stats');
const details = document.getElementById('details');
const controls = document.getElementById('controls');
const log = document.getElementById('log');
const legend = document.getElementById('legend');
const panelToggle = document.getElementById('panel-toggle');
let graph = { nodes: [], links: [], relations: [] };
let nodeState = new Map();
let previousLinks = new Set();
let selected = '';
let pan = { x: 0, y: 0, scale: 1 };
let drag = null;
let pointers = new Map();
let pinch = null;
let paused = false;
const defaults = {
  linkDistance: 72,
  linkStrength: 0.018,
  repulsion: 1200,
  centerPull: 0.0008,
  damping: 0.86,
  initialSpread: 3,
};
let params = { ...defaults, ...JSON.parse(localStorage.getItem('locusGraphParams') || '{}') };
let panelHidden = JSON.parse(localStorage.getItem('locusGraphPanelHidden') || (window.innerWidth <= 760 ? 'true' : 'false'));

function key(l) { return `${l.source}\t${l.relation}\t${l.target}`; }
function esc(s) { return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
function classNamePart(s) { return String(s || 'node').replace(/[^A-Za-z0-9_-]/g, '_'); }
function radius(n) { return Math.max(22, Math.min(44, 13 + n.label.length * 2.2)); }
const kindColors = {
  context: ['#332f1f', '#e8c15a'],
  window: ['#223349', '#78aeea'],
  workspace: ['#243724', '#79d083'],
  output: ['#3a2b22', '#e99d62'],
  project: ['#24343a', '#66c7d9'],
  'app-instance': ['#342832', '#ee8fc5'],
  'agent-session': ['#2f2638', '#cb91f2'],
};
const fallbackColors = [
  ['#252b36', '#f0c15c'],
  ['#2a3530', '#9bd58f'],
  ['#352c3b', '#bd9cff'],
  ['#23363b', '#6fd3c6'],
  ['#3b2d2d', '#ef8b8b'],
];
function colorsForKind(kind) {
  if (kindColors[kind]) return kindColors[kind];
  let hash = 0;
  for (const ch of String(kind)) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return fallbackColors[hash % fallbackColors.length];
}
function relationSpec(name) {
  return (graph.relations || []).find(r => r.name === name) || {};
}
function applyKindColors(el, kind) {
  const [fill, stroke] = colorsForKind(kind);
  el.style.setProperty('--kind-fill', fill);
  el.style.setProperty('--kind-stroke', stroke);
}

function updatePanelVisibility() {
  document.body.classList.toggle('panel-hidden', panelHidden);
  panelToggle.textContent = panelHidden ? 'Show panel' : 'Hide panel';
  panelToggle.setAttribute('aria-expanded', String(!panelHidden));
}

panelToggle.addEventListener('click', () => {
  panelHidden = !panelHidden;
  localStorage.setItem('locusGraphPanelHidden', JSON.stringify(panelHidden));
  updatePanelVisibility();
});

function makeControls() {
  const specs = [
    ['linkDistance', 'Link dist', 20, 240, 1],
    ['linkStrength', 'Link pull', 0, 0.08, 0.001],
    ['repulsion', 'Repulsion', 0, 6000, 50],
    ['centerPull', 'Gravity', 0, 0.006, 0.0001],
    ['damping', 'Damping', 0.5, 0.98, 0.01],
    ['initialSpread', 'Spawn gap', 0, 24, 1],
  ];
  controls.innerHTML = specs.map(([key, label, min, max, step]) => `
    <label>
      <span>${label}</span>
      <input data-key="${key}" type="range" min="${min}" max="${max}" step="${step}" value="${params[key]}">
      <output id="out-${key}">${formatParam(key, params[key])}</output>
    </label>
  `).join('') + '<div class="control-actions"><button id="reset-layout">Reset layout</button><button id="reset-params">Reset params</button><button id="pause-layout">Pause</button></div>';

  controls.querySelectorAll('input').forEach(input => {
    input.addEventListener('input', () => {
      const key = input.dataset.key;
      params[key] = Number(input.value);
      document.getElementById(`out-${key}`).textContent = formatParam(key, params[key]);
      localStorage.setItem('locusGraphParams', JSON.stringify(params));
    });
  });
  document.getElementById('reset-layout').addEventListener('click', () => { nodeState.clear(); seedNodes(); });
  document.getElementById('reset-params').addEventListener('click', () => {
    params = { ...defaults };
    localStorage.setItem('locusGraphParams', JSON.stringify(params));
    makeControls();
  });
  document.getElementById('pause-layout').addEventListener('click', e => {
    paused = !paused;
    e.target.textContent = paused ? 'Resume' : 'Pause';
  });
}

function formatParam(key, value) {
  if (key === 'linkStrength' || key === 'centerPull') return Number(value).toFixed(4);
  if (key === 'damping') return Number(value).toFixed(2);
  return String(Math.round(value));
}

async function refresh() {
  try {
    const next = await fetch('/graph.json', { cache: 'no-store' }).then(r => r.json());
    diffLog(next.links);
    graph = next;
    seedNodes();
    stats.textContent = `${graph.nodes.length} nodes · ${graph.links.length} links`;
    updateLegend();
    updateDetails();
  } catch (e) {
    stats.textContent = `error: ${e}`;
  }
}

function updateLegend() {
  const kinds = [...new Set(graph.nodes.map(n => n.kind || 'node'))].sort();
  const nodeItems = kinds.map(kind => {
    const [fill, stroke] = colorsForKind(kind);
    return `<span class="legend-item"><span class="legend-dot" style="--kind-fill:${fill};--kind-stroke:${stroke}"></span>${esc(kind)}</span>`;
  }).join('');
  legend.innerHTML = nodeItems
    + '<span class="legend-item"><span class="legend-line"></span>source -> target</span>'
    + '<span class="legend-item"><span class="legend-line weak"></span>weak retention</span>';
}

function diffLog(links) {
  const current = new Set(links.map(key));
  const added = links.filter(l => !previousLinks.has(key(l)));
  const removed = [...previousLinks].filter(old => !current.has(old)).map(old => {
    const [source, relation, target] = old.split('\t');
    return { source, relation, target };
  });
  const addedBySlot = new Map();
  for (const l of added) {
    const slot = `${l.source}\t${l.relation}`;
    if (!addedBySlot.has(slot)) addedBySlot.set(slot, []);
    addedBySlot.get(slot).push(l);
  }
  const removedBySlot = new Map();
  for (const l of removed) {
    const slot = `${l.source}\t${l.relation}`;
    if (!removedBySlot.has(slot)) removedBySlot.set(slot, []);
    removedBySlot.get(slot).push(l);
  }
  const usedAdds = new Set();
  const usedRemoves = new Set();
  for (const [slot, oldLinks] of removedBySlot) {
    const newLinks = addedBySlot.get(slot) || [];
    if (newLinks.length !== 1) continue;
    const next = newLinks[0];
    event('set', next, { oldTargets: oldLinks.map(l => l.target) });
    usedAdds.add(key(next));
    for (const old of oldLinks) usedRemoves.add(key(old));
  }
  for (const l of added) if (!usedAdds.has(key(l))) event('add', l);
  for (const l of removed) {
    if (!usedRemoves.has(key(l))) event('remove', l);
  }
  previousLinks = current;
}

function event(type, l, meta = {}) {
  const row = document.createElement('div');
  row.className = `event ${type}`;
  const detail = type === 'set' && meta.oldTargets?.length
    ? `<div class="link">was ${esc(meta.oldTargets.join(', '))}</div>`
    : '';
  row.innerHTML = `<div><span class="kind">${type}</span> ${new Date().toLocaleTimeString()}</div><div class="link">${esc(l.source)} --${esc(l.relation)}--> ${esc(l.target)}</div>${detail}`;
  log.prepend(row);
  while (log.children.length > 80) log.lastChild.remove();
}

function seedNodes() {
  const ids = new Set(graph.nodes.map(n => n.id));
  for (const id of [...nodeState.keys()]) if (!ids.has(id)) nodeState.delete(id);
  const rect = svg.getBoundingClientRect();
  for (const n of graph.nodes) {
    if (!nodeState.has(n.id)) {
      const a = nodeState.size * 2.399963;
      const r = 36 + nodeState.size * params.initialSpread;
      nodeState.set(n.id, { x: rect.width / 2 + Math.cos(a) * r, y: rect.height / 2 + Math.sin(a) * r, vx: 0, vy: 0, fixed: false });
    }
  }
}

function tick() {
  if (paused) { draw(); requestAnimationFrame(tick); return; }
  const states = [...nodeState.entries()];
  const byId = Object.fromEntries(states);
  for (const [, a] of states) {
    a.vx *= params.damping; a.vy *= params.damping;
    if (!a.fixed) {
      const rect = svg.getBoundingClientRect();
      a.vx += (rect.width / 2 - a.x) * params.centerPull;
      a.vy += (rect.height / 2 - a.y) * params.centerPull;
    }
  }
  for (let i = 0; i < states.length; i++) for (let j = i + 1; j < states.length; j++) {
    const a = states[i][1], b = states[j][1];
    const dx = b.x - a.x, dy = b.y - a.y, d2 = Math.max(80, dx * dx + dy * dy);
    const f = params.repulsion / d2, fx = dx * f, fy = dy * f;
    if (!a.fixed) { a.vx -= fx; a.vy -= fy; }
    if (!b.fixed) { b.vx += fx; b.vy += fy; }
  }
  for (const l of graph.links) {
    const a = byId[l.source], b = byId[l.target];
    if (!a || !b) continue;
    const dx = b.x - a.x, dy = b.y - a.y, d = Math.max(1, Math.hypot(dx, dy));
    const f = (d - params.linkDistance) * params.linkStrength;
    const fx = dx / d * f, fy = dy / d * f;
    if (!a.fixed) { a.vx += fx; a.vy += fy; }
    if (!b.fixed) { b.vx -= fx; b.vy -= fy; }
  }
  for (const [, s] of states) if (!s.fixed) { s.x += s.vx; s.y += s.vy; }
  draw();
  requestAnimationFrame(tick);
}

function draw() {
  viewport.setAttribute('transform', `translate(${pan.x},${pan.y}) scale(${pan.scale})`);
  const nodeById = new Map(graph.nodes.map(n => [n.id, n]));
  linksG.innerHTML = '';
  labelsG.innerHTML = '';
  nodesG.innerHTML = '';
  for (const l of graph.links) {
    const a = nodeState.get(l.source), b = nodeState.get(l.target);
    if (!a || !b) continue;
    const sourceNode = nodeById.get(l.source);
    const targetNode = nodeById.get(l.target);
    const dx = b.x - a.x, dy = b.y - a.y, d = Math.max(1, Math.hypot(dx, dy));
    const sourceRadius = sourceNode ? radius(sourceNode) + 2 : 24;
    const targetRadius = targetNode ? radius(targetNode) + 8 : 30;
    const spec = relationSpec(l.relation);
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
    line.setAttribute('class', `link-line ${spec.retention === 'weak' ? 'weak' : ''}`);
    line.setAttribute('x1', a.x + dx / d * sourceRadius); line.setAttribute('y1', a.y + dy / d * sourceRadius);
    line.setAttribute('x2', b.x - dx / d * targetRadius); line.setAttribute('y2', b.y - dy / d * targetRadius);
    linksG.append(line);
    const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    text.setAttribute('class', 'link-label');
    text.setAttribute('x', (a.x + b.x) / 2); text.setAttribute('y', (a.y + b.y) / 2 - 4);
    text.textContent = spec.cardinality ? `${l.relation} ${spec.cardinality}` : l.relation;
    labelsG.append(text);
  }
  for (const n of graph.nodes) {
    const s = nodeState.get(n.id); if (!s) continue;
    const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    g.setAttribute('class', `node kind-${classNamePart(n.kind)} ${selected === n.id ? 'selected' : ''}`);
    applyKindColors(g, n.kind || 'node');
    g.setAttribute('transform', `translate(${s.x},${s.y})`);
    g.addEventListener('pointerdown', e => {
      e.preventDefault();
      e.stopPropagation();
      svg.setPointerCapture?.(e.pointerId);
      selected = n.id;
      s.fixed = true;
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      drag = { type: 'node', id: n.id, pointerId: e.pointerId };
      updateDetails();
    });
    g.addEventListener('dblclick', () => { s.fixed = false; });
    const c = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    c.setAttribute('r', radius(n));
    const t = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    t.setAttribute('y', 4);
    t.textContent = n.label;
    g.append(c, t);
    nodesG.append(g);
  }
}

function updateDetails() {
  const n = graph.nodes.find(n => n.id === selected);
  if (!n) { details.innerHTML = '<div class="id">Select a node</div>'; return; }
  const props = Object.entries(n.properties || {}).map(([k,v]) => `<tr><td>${esc(k)}</td><td>${esc(v)}</td></tr>`).join('');
  const adjacent = graph.links.filter(l => l.source === n.id || l.target === n.id).length;
  details.innerHTML = `<div class="id">${esc(n.id)}</div><table><tr><td>kind</td><td>${esc(n.kind)}</td></tr><tr><td>links</td><td>${adjacent}</td></tr>${props}</table>`;
}

function updatePinch() {
  if (pointers.size < 2) { pinch = null; return; }
  const points = [...pointers.values()].slice(0, 2);
  const cx = (points[0].x + points[1].x) / 2;
  const cy = (points[0].y + points[1].y) / 2;
  const distance = Math.max(1, Math.hypot(points[1].x - points[0].x, points[1].y - points[0].y));
  if (!pinch) pinch = { cx, cy, distance, panX: pan.x, panY: pan.y, scale: pan.scale };
}

svg.addEventListener('pointerdown', e => {
  e.preventDefault();
  svg.setPointerCapture?.(e.pointerId);
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  if (pointers.size >= 2) {
    drag = null;
    updatePinch();
  } else {
    drag = { type: 'pan', pointerId: e.pointerId, x: e.clientX, y: e.clientY, px: pan.x, py: pan.y };
  }
});
window.addEventListener('pointermove', e => {
  if (pointers.has(e.pointerId)) pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  if (pointers.size >= 2) {
    if (!pinch) updatePinch();
    const points = [...pointers.values()].slice(0, 2);
    const cx = (points[0].x + points[1].x) / 2;
    const cy = (points[0].y + points[1].y) / 2;
    const distance = Math.max(1, Math.hypot(points[1].x - points[0].x, points[1].y - points[0].y));
    const next = Math.max(0.25, Math.min(3, pinch.scale * distance / pinch.distance));
    pan.x = cx - (pinch.cx - pinch.panX) * next / pinch.scale;
    pan.y = cy - (pinch.cy - pinch.panY) * next / pinch.scale;
    pan.scale = next;
    return;
  }
  if (!drag) return;
  if (drag.pointerId !== undefined && e.pointerId !== drag.pointerId) return;
  if (drag.type === 'pan') { pan.x = drag.px + e.clientX - drag.x; pan.y = drag.py + e.clientY - drag.y; }
  else {
    const s = nodeState.get(drag.id);
    if (s) { s.x = (e.clientX - pan.x) / pan.scale; s.y = (e.clientY - pan.y) / pan.scale; s.vx = s.vy = 0; }
  }
});
function releasePointer(e) {
  pointers.delete(e.pointerId);
  svg.releasePointerCapture?.(e.pointerId);
  if (drag?.pointerId === e.pointerId) drag = null;
  updatePinch();
}
window.addEventListener('pointerup', releasePointer);
window.addEventListener('pointercancel', releasePointer);
svg.addEventListener('wheel', e => {
  e.preventDefault();
  const old = pan.scale;
  const next = Math.max(0.25, Math.min(3, old * (e.deltaY > 0 ? 0.9 : 1.1)));
  pan.x = e.clientX - (e.clientX - pan.x) * next / old;
  pan.y = e.clientY - (e.clientY - pan.y) * next / old;
  pan.scale = next;
}, { passive: false });

makeControls();
refresh();
setInterval(refresh, 750);
requestAnimationFrame(tick);
</script>
</body>
</html>
"##;
