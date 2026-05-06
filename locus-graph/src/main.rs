use std::collections::{BTreeSet, HashMap};
use std::env;
use std::sync::Arc;

use anyhow::Context;
use locus_dbus::{Client, LinkTuple};
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
    let client = Client::new(connection).await.context("connect to locusd")?;
    let links = client.get_all_links().await.context("get graph links")?;
    let mut subjects = BTreeSet::new();
    for (source, _, target) in &links {
        subjects.insert(source.clone());
        subjects.insert(target.clone());
    }

    let mut properties = HashMap::new();
    for subject in &subjects {
        properties.insert(
            subject.clone(),
            client.get_properties(subject).await.unwrap_or_default(),
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

    Ok(format!("{{\"nodes\":[{nodes}],\"links\":[{links}]}}"))
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
    :root { color-scheme: dark; --bg: #111318; --panel: #181b22; --fg: #e8e9ef; --dim: #9da3b4; --line: #4e566b; --accent: #e8ac38; --green: #68c48c; --red: #df6b6b; }
    * { box-sizing: border-box; }
    body { margin: 0; height: 100vh; overflow: hidden; background: var(--bg); color: var(--fg); font: 13px/1.4 system-ui, sans-serif; }
    #app { display: grid; grid-template-columns: minmax(0, 1fr) 360px; height: 100vh; }
    #graph { width: 100%; height: 100%; cursor: grab; }
    #graph:active { cursor: grabbing; }
    aside { border-left: 1px solid #2a2f3b; background: var(--panel); display: grid; grid-template-rows: auto auto auto minmax(0,1fr); min-width: 0; }
    header { padding: 14px 16px; border-bottom: 1px solid #2a2f3b; }
    h1 { margin: 0; font-size: 15px; }
    #stats { margin-top: 4px; color: var(--dim); }
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
    .link-line { stroke: var(--line); stroke-width: 1.4; marker-end: url(#arrow); }
    .link-label { fill: var(--dim); font-size: 12px; pointer-events: none; }
    .node circle { stroke: #f0c15c; stroke-width: 1.5; fill: #252b36; }
    .node.project circle { fill: #21352b; stroke: #77d49a; }
    .node.context circle { fill: #332f1f; stroke: #e8c15a; }
    .node.agent-session circle { fill: #2f2638; stroke: #cb91f2; }
    .node text { fill: var(--fg); font-size: 13px; pointer-events: none; text-anchor: middle; paint-order: stroke; stroke: var(--bg); stroke-width: 3px; }
    .node.selected circle { stroke-width: 3; }
  </style>
</head>
<body>
<div id="app">
  <svg id="graph">
    <defs>
      <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#4e566b"></path>
      </marker>
    </defs>
    <g id="viewport"><g id="links"></g><g id="labels"></g><g id="nodes"></g></g>
  </svg>
  <aside>
    <header><h1>Locus Graph</h1><div id="stats">connecting...</div></header>
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
let graph = { nodes: [], links: [] };
let nodeState = new Map();
let previousLinks = new Set();
let selected = '';
let pan = { x: 0, y: 0, scale: 1 };
let drag = null;
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

function key(l) { return `${l.source}\t${l.relation}\t${l.target}`; }
function esc(s) { return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
function kind(id) { return id.includes(':') ? id.split(':')[0] : 'node'; }
function radius(n) { return Math.max(22, Math.min(44, 13 + n.label.length * 2.2)); }

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
    updateDetails();
  } catch (e) {
    stats.textContent = `error: ${e}`;
  }
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
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
    line.setAttribute('class', 'link-line');
    line.setAttribute('x1', a.x); line.setAttribute('y1', a.y);
    line.setAttribute('x2', b.x); line.setAttribute('y2', b.y);
    linksG.append(line);
    const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    text.setAttribute('class', 'link-label');
    text.setAttribute('x', (a.x + b.x) / 2); text.setAttribute('y', (a.y + b.y) / 2 - 4);
    text.textContent = l.relation;
    labelsG.append(text);
  }
  for (const n of graph.nodes) {
    const s = nodeState.get(n.id); if (!s) continue;
    const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    g.setAttribute('class', `node ${kind(n.id)} ${selected === n.id ? 'selected' : ''}`);
    g.setAttribute('transform', `translate(${s.x},${s.y})`);
    g.addEventListener('pointerdown', e => { e.stopPropagation(); selected = n.id; s.fixed = true; drag = { type: 'node', id: n.id }; updateDetails(); });
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

svg.addEventListener('pointerdown', e => { drag = { type: 'pan', x: e.clientX, y: e.clientY, px: pan.x, py: pan.y }; });
window.addEventListener('pointermove', e => {
  if (!drag) return;
  if (drag.type === 'pan') { pan.x = drag.px + e.clientX - drag.x; pan.y = drag.py + e.clientY - drag.y; }
  else {
    const s = nodeState.get(drag.id);
    if (s) { s.x = (e.clientX - pan.x) / pan.scale; s.y = (e.clientY - pan.y) / pan.scale; s.vx = s.vy = 0; }
  }
});
window.addEventListener('pointerup', () => { drag = null; });
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
