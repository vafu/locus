# Locus

Locus is a session D-Bus graph service for contextual desktop metadata.

It keeps a single runtime graph that desktop tools can publish to and query
from: window manager state, project context, agent sessions, labels/icons, and
other local contextual facts. Locus itself stays generic. Concepts like
`window`, `workspace`, `project`, and `agent-session` are declared in schema,
not hardcoded into the daemon.

## Model

Locus stores an in-memory property graph:

```text
source --relation--> target
subject[key] = value
```

There is no durability layer. On restart, publishers are expected to republish
runtime state.

Current desktop graph shape is declared in `schema.yaml`:

```text
context:selected --window--> window:<id>
window:<id> --workspace--> workspace:<id>
workspace:<id> --project--> project:<path>
window:<id> --app-instance--> app-instance:<app>/<instance>
app-instance:<app>/<instance> --agent-session--> agent-session:<agent>/<session>
```

Niri is treated as a publisher, not as part of the node identity:

```text
window:57[kind] = window
window:57[source] = niri
window:57[external-id] = 57
workspace:6[kind] = workspace
workspace:6[source] = niri
```

## Schema

Schema support lives in the `locus-schema` crate. The daemon depends on that
crate for parsing and validation; future TypeScript code generation should build
on the same crate rather than reverse-engineering daemon internals.

`locusd` loads a YAML schema from:

```text
$XDG_CONFIG_HOME/locus/schema.yaml
~/.config/locus/schema.yaml
```

or from an explicit path:

```sh
locusd --schema ./schema.yaml
```

The schema declares node properties, relation validation, and cardinality:

```yaml
nodes:
  project:
    properties:
      path:
        required: true
      name: {}
      icon: {}

relations:
  workspace:
    from: window
    to: workspace
    cardinality: many-to-one
```

This means many windows may point to the same workspace, but one window may
point to only one workspace. `SetLink(window:57, workspace, workspace:6)`
atomically removes old `window:57 --workspace--> *` links.

Invalid writes are rejected. Unknown relations are rejected. Source/target kind
checks use explicit `kind` metadata, not ID prefixes.

Property schema is intentionally light:

- `required: true` means a node of that kind must have the property before it can
  participate in a schema-validated relation.
- `{}` means a known optional property.
- Unknown properties are allowed.
- There is no property type validation yet.

`kind` remains implicit and is still required for relation validation.

Supported cardinalities:

```text
one-to-one
many-to-one
one-to-many
many-to-many
```

Prefixes such as `window:`, `workspace:`, and `project:` are ID namespaces for
readability and collision avoidance. They are conventions, not the type system.
The source of truth is `kind` metadata plus relation schema.

## Workspace Layout

The repository is a Cargo workspace. Each crate owns one responsibility:

```text
locus-codegen TypeScript helper generator from schema.yaml
locus-core    in-memory graph runtime and schema-enforced graph behavior
locus-dbus    D-Bus adapter, generated proxy, client helpers, wire conventions
locus-schema  schema model, YAML parser, validation helpers
locusd        daemon binary: loads schema, starts locus-core over locus-dbus
locusctl      CLI client binary
locus-niri    Niri publisher binary
locus-graph   local graph inspection UI binary
```

`locus-core` owns the in-process runtime. D-Bus is the public process boundary:
client/publisher crates that talk to the running daemon should depend on
`locus-dbus`, not daemon internals.

## D-Bus API

Service:

```text
io.github.Locus
```

Object path:

```text
/io/github/Locus
```

Interfaces:

```text
io.github.Locus.Graph.Read
io.github.Locus.Graph.Write
io.github.Locus.Graph.Resolve
```

Methods:

```text
io.github.Locus.Graph.Write
SetLink(source: s, relation: s, target: s)
RemoveLink(source: s, relation: s, target: s)
RemoveLinks(source: s, relation: s)
DeleteNode(subject: s)
SetProperty(subject: s, key: s, value: s)
RemoveProperty(subject: s, key: s)
ApplyMutations(mutations: a(ssss))

io.github.Locus.Graph.Read
GetTargets(source: s, relation: s) -> as
GetSources(target: s, relation: s) -> as
GetLinks(subject: s) -> a(sss)
GetAllLinks() -> a(sss)
GetProperty(subject: s, key: s) -> s
GetProperties(subject: s) -> a{ss}
GetSubjects() -> as
FindSubjects(key: s, value: s) -> as

io.github.Locus.Graph.Resolve
Resolve(source: s, path: as) -> s
ResolveAll(source: s, path: as) -> as
SubscribeResolve(source: s, path: as) -> s
FindNearest(source: s, kind: s) -> s
WatchNode(source: s, path: as) -> o
```

Empty strings represent optional `None` over D-Bus.

`Resolve` follows an exact relation path, traversing matching relation edges in
the `source --relation--> target` direction, and returns one target. If the path
resolves to multiple targets, it returns an error. Use `ResolveAll` for
intentionally-many paths. Use `GetSources` when reverse lookup is intended.

Examples:

```sh
locusctl resolve context:selected window workspace project
locusctl resolve context:selected window app-instance agent-session
locusctl link sources workspace:6 workspace
```

`FindNearest(source, kind)` is a fuzzy shortest-path/debug query over outgoing
edges. Application UI should prefer exact `Resolve` paths.

`SubscribeResolve(source, path)` registers a derived query in `locusd` and
returns the current resolved target. After future graph/property mutations,
`locusd` recomputes subscribed resolutions and emits `ResolveChanged` only when
the resolved target actually changes.

Signals:

```text
LinkAdded(source: s, relation: s, target: s)
LinkRemoved(source: s, relation: s, target: s)
LinkSet(source: s, relation: s, old_targets: as, target: s)
PropertyChanged(subject: s, key: s, value: s)
PropertyRemoved(subject: s, key: s)
ResolveChanged(source: s, path: as, target: s)
```

Write signals are emitted on `io.github.Locus.Graph.Write`; resolve signals are
emitted on `io.github.Locus.Graph.Resolve`. `SetLink` emits `LinkSet` and
compatibility `LinkRemoved`/`LinkAdded` signals. No-op writes are quiet.

## Binaries

### `locusd`

The D-Bus service. It owns `io.github.Locus`, loads the YAML schema, and stores
the runtime graph in memory.

```sh
locusd
locusd --schema ~/.config/locus/schema.yaml
```

From the workspace:

```sh
cargo run -p locusd -- --schema ./schema.yaml
```

### `locusctl`

CLI for publishing, querying, resolving, and watching graph state.

Common commands:

```sh
locusctl link set context:selected window window:57
locusctl link targets context:selected window --first
locusctl link all

locusctl prop set project:/home/v47/proj/locus kind project
locusctl prop get project:/home/v47/proj/locus name
locusctl prop subjects --key kind --value project

locusctl resolve context:selected window workspace project
locusctl resolve context:selected window workspace
locusctl find-nearest context:selected project
```

Watchers can run scripts from graph signals:

```sh
locusctl watch property-changed \
  --filter 'subject.startsWith("project:") && (key == "kind" || key == "path" || key == "name")' \
  --missing-property icon \
  --exec ~/.config/scripts/autorun/locus-project-icon-hook
```

### `locus-niri`

Publishes Niri topology into Locus.

On startup it:

1. Clears old Niri-published window/workspace runtime edges and stale window
   metadata.
2. Reads current Niri `Workspaces` and `Windows` snapshots.
3. Publishes current `window -> workspace` links and metadata.
4. Subscribes to Niri's event stream for live updates.

It publishes generic Locus node IDs:

```text
context:selected --window--> window:<focused-or-active-window>
window:<id> --workspace--> workspace:<id>
window:<id> --app-instance--> app-instance:<app>/<instance>
window:<id>[kind] = window
workspace:<id>[kind] = workspace
```

Selected workspace and project context are derived through the selected window:

```sh
locusctl resolve context:selected window workspace
locusctl resolve context:selected window workspace project
```

### `locus-graph`

Small local web UI for inspecting the graph.

```sh
locus-graph
# http://127.0.0.1:8765
```

### `locus-codegen`

Generates TypeScript helpers from the schema:

```sh
cargo run -p locus-codegen -- --schema schema.yaml
cargo run -p locus-codegen -- --schema schema.yaml --out /tmp/locus.generated.ts
```

The generated file contains `NodeKind`, `Relation`, and `NamedPath` unions, a
`locusSchema` constant, a small `LocusDbusClient` interface, and
`LocusSchemaClient` helpers such as `selectedProject()` and
`subscribeSelectedProject()`.

Install all Locus binaries from the workspace with:

```sh
cargo install --path locus-codegen
cargo install --path locusd
cargo install --path locusctl
cargo install --path locus-niri
cargo install --path locus-graph
```

## Desktop Integration

### zsh

The zsh hook watches `cd`.

When the shell enters a direct `~/proj/<project_name>` directory, it:

1. Creates/updates `project:<path>` metadata:
   - `kind=project`
   - `path=$PWD`
   - `name=${PWD:t}`
2. Resolves the selected workspace:

   ```sh
   locusctl resolve context:selected window workspace
   ```

3. Links the workspace to the project:

   ```text
   workspace:<id> --project--> project:<path>
   ```

### AGS

AGS uses Locus as its source of context.

Current shape:

1. Reads the selected window from `context:selected --window`.
2. Subscribes to `[window, workspace]`.
3. Subscribes to `[window, workspace, project]`.
4. Updates the project widget from `ResolveChanged` and project properties.

This keeps AGS independent from Niri IPC.

### agent-dbus / Codex

`agent-hook` links the active app instance to an agent session:

```text
window:<id> --app-instance--> app-instance:codex/<instance>
app-instance:codex/<instance> --agent-session--> agent-session:codex/<session>
```

The Codex zsh wrapper creates the app-instance node, links it to the selected
window, and passes it through `LOCUS_APP_INSTANCE`.

## Useful Debug Commands

```sh
busctl --user introspect io.github.Locus /io/github/Locus io.github.Locus.Graph.Read
busctl --user introspect io.github.Locus /io/github/Locus io.github.Locus.Graph.Write
busctl --user introspect io.github.Locus /io/github/Locus io.github.Locus.Graph.Resolve
locusctl link all
locusctl prop subjects --key kind
locusctl resolve context:selected window workspace
locusctl resolve context:selected window workspace project
journalctl --user -u locus-niri.service -n 80 --no-pager
journalctl --user -u agent-dbus.service -n 80 --no-pager
```
