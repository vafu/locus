# Locus

Locus is a session D-Bus graph service for contextual desktop metadata.

The current goal is to keep a single runtime graph that small desktop tools can
publish to and query from: window manager state, project context, agent sessions,
project labels/icons, and any other contextual facts. Locus itself stays generic.
Concepts like "project", "window", "workspace", and "agent session" are graph
node metadata and relation names, not special database tables or hardcoded
domain objects.

## Current Model

Locus stores an in-memory directed graph:

```text
source --relation--> target
subject[key] = value
```

There is no durability layer right now. SQLite and durable flags were removed.
On restart, publishers are expected to republish their runtime state.

Important conventions currently used by the desktop setup:

```text
context:selected --window--> niri:window:<id>
niri:window:<id> --workspace--> niri:workspace:<id>
niri:workspace:<id> --project--> project:<path>
niri:window:<id> --agent-session--> agent-session:<agent>/<session>
```

Metadata is generic:

```text
project:/home/v47/proj/locus[kind] = project
project:/home/v47/proj/locus[name] = locus
project:/home/v47/proj/locus[path] = /home/v47/proj/locus
project:/home/v47/proj/locus[icon] = ...
niri:window:57[kind] = window
niri:workspace:6[kind] = workspace
```

`project` is therefore an external convention. The zsh hook creates project
nodes and links workspaces to projects. `locus-niri` owns Niri window/workspace
topology. AGS reads from Locus and should not need to know Niri IPC directly.

## D-Bus API

Service:

```text
io.github.Locus
```

Object path:

```text
/io/github/Locus
```

Interface:

```text
io.github.Locus.Graph
```

Methods:

```text
AddLink(source: s, relation: s, target: s)
SetLink(source: s, relation: s, target: s)
RemoveLink(source: s, relation: s, target: s)
RemoveLinks(source: s, relation: s)

GetTargets(source: s, relation: s) -> as
GetSources(target: s, relation: s) -> as
GetLinks(subject: s) -> a(sss)
GetAllLinks() -> a(sss)

SetProperty(subject: s, key: s, value: s)
RemoveProperty(subject: s, key: s)
GetProperty(subject: s, key: s) -> s
GetProperties(subject: s) -> a{ss}
GetSubjects() -> as
FindSubjects(key: s, value: s) -> as

Resolve(source: s, kind: s) -> s
SubscribeResolve(source: s, kind: s) -> s
```

Empty strings are used as "none" over D-Bus for optional string results.

`Resolve(source, kind)` performs a shortest-path search over the visible graph,
following edges in either direction, and returns the first node whose `kind`
property matches.

`SubscribeResolve(source, kind)` registers a derived query in `locusd` and returns
the current resolved target. After future graph/property mutations, `locusd`
recomputes subscribed resolutions and emits `ResolveChanged` only when the
resolved target actually changes. This is what AGS should use for derived
context such as `context:selected -> project`.

Signals:

```text
LinkAdded(source: s, relation: s, target: s)
LinkRemoved(source: s, relation: s, target: s)
LinkSet(source: s, relation: s, old_targets: as, target: s)
PropertyChanged(subject: s, key: s, value: s)
PropertyRemoved(subject: s, key: s)
ResolveChanged(source: s, kind: s, target: s)
```

`SetLink` emits `LinkSet` and compatibility `LinkRemoved`/`LinkAdded` signals.
Subscribers that only care about replacement semantics should listen to
`LinkSet`; subscribers that care about edge creation/removal may listen to the
raw add/remove signals.

No-op writes are quiet:

```text
AddLink(existing edge)       -> no signal
SetLink(same target)         -> no signal
SetProperty(same value)      -> no signal
```

## Binaries

### `locusd`

The D-Bus service. It owns `io.github.Locus` and stores the runtime graph in
memory.

```sh
locusd
```

The repo also installs a user D-Bus service file outside this tree so D-Bus can
activate `locusd`.

### `locusctl`

CLI for publishing, querying, resolving, and watching graph state.

Common commands:

```sh
locusctl link set context:selected window niri:window:57
locusctl link targets context:selected window --first
locusctl link all

locusctl prop set project:/home/v47/proj/locus kind project
locusctl prop get project:/home/v47/proj/locus name
locusctl prop subjects --key kind --value project

locusctl resolve context:selected project
locusctl resolve context:selected workspace
```

Watchers can run scripts from graph signals:

```sh
locusctl watch property-changed \
  --subject-prefix project: \
  --key name \
  --exec my-hook
```

Watch supports field filters, missing-property filters, and CEL:

```sh
locusctl watch property-changed \
  --filter 'subject.startsWith("project:") && (key == "kind" || key == "path" || key == "name")' \
  --missing-property icon \
  --exec ~/.config/scripts/autorun/locus-project-icon-hook
```

Current note: `locusctl watch` watches raw graph/property signals. `ResolveChanged`
is exposed on D-Bus and used by AGS, but `locusctl watch` has not yet grown a
first-class `resolve-changed` event mode.

### `locus-niri`

Publishes Niri topology into Locus.

On startup it:

1. Clears old Niri window/workspace edges and stale `kind` metadata.
2. Reads current Niri `Workspaces` and `Windows` snapshots.
3. Publishes current `window -> workspace` links and `kind` metadata.
4. Subscribes to Niri's event stream for live updates.

It publishes:

```text
context:selected --window--> niri:window:<focused-or-active-window>
niri:window:<id> --workspace--> niri:workspace:<id>
niri:window:<id>[kind] = window
niri:workspace:<id>[kind] = workspace
```

It intentionally does not publish `context:selected --workspace`. Workspace and
project context should be derived:

```sh
locusctl resolve context:selected workspace
locusctl resolve context:selected project
```

### `locus-graph`

Small local web UI for inspecting the graph.

```sh
locus-graph
# http://127.0.0.1:8765
```

It serves `/graph.json`, renders all visible nodes/links, and exposes force graph
controls for debugging layout.

## Desktop Integration

### zsh

The zsh hook in dotconfig watches `cd`.

When the shell enters a direct `~/proj/<project_name>` directory, it:

1. Creates/updates `project:<path>` metadata:
   - `kind=project`
   - `path=$PWD`
   - `name=${PWD:t}`
2. Resolves the selected workspace with:

   ```sh
   locusctl resolve context:selected workspace
   ```

3. Links the workspace to the project:

   ```text
   niri:workspace:<id> --project--> project:<path>
   ```

This is intentionally shell/project logic, not core Locus logic.

### AGS

AGS uses Locus as its source of context.

Current shape:

1. It reads the selected window from `context:selected --window`.
2. It calls `SubscribeResolve(context:selected, workspace)`.
3. It calls `SubscribeResolve(context:selected, project)`.
4. It updates the project widget from `ResolveChanged` and project properties.

This avoids re-resolving project metadata on every window focus change. Project
resolution changes only when the derived project target changes, for example
when switching to a workspace associated with a different project.

There are temporary AGS debug logs:

```text
[Locus] SubscribeResolve initial context:selected kind=project target=...
[Locus] SubscribeResolve initial context:selected kind=workspace target=...
[Locus] ResolveChanged context:selected kind=project target=...
[Locus] ResolveChanged context:selected kind=workspace target=...
[Locus] selected window=...
```

### agent-dbus / Codex

`agent-hook` links the selected Niri window to an agent session:

```text
niri:window:<id> --agent-session--> agent-session:codex/<session>
```

The Codex zsh wrapper reads:

```sh
locusctl context get selected window --first
```

and passes the numeric window id through `AGENT_DBUS_WINDOW_ID` so `agent-hook`
can publish the correct link.

AGS can then find the selected window's agent session and show Codex approval UI
for the relevant window/session.

## Current Design Notes

- Locus is runtime-only.
- Links are directed but resolution traverses links bidirectionally.
- Reciprocal links are rejected at the service layer to prevent accidental
  duplicate opposite edges such as both `workspace -> window` and
  `window -> workspace`.
- `context:selected` should point to the selected window only.
- Workspace and project context are derived via `Resolve`/`SubscribeResolve`.
- `project` is not a built-in type. It is a node with `kind=project` and metadata
  provided by external publishers.
- D-Bus signatures no longer include `durable` booleans. Any remaining callers
  using `(sssb)` are stale and need to be updated to `(sss)`.

## Useful Debug Commands

```sh
busctl --user introspect io.github.Locus /io/github/Locus io.github.Locus.Graph
locusctl link all
locusctl prop subjects --key kind
locusctl resolve context:selected workspace
locusctl resolve context:selected project
journalctl --user -u locus-niri.service -n 80 --no-pager
journalctl --user -u agent-dbus.service -n 80 --no-pager
```

