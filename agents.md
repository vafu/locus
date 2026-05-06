# Locus Development Notes

## Crate Boundaries

Keep the workspace split by responsibility:

```text
locus-api     transport-neutral graph trait and shared graph types
locus-core    in-memory graph runtime and schema-enforced graph behavior
locus-dbus    D-Bus adapter, generated proxy, client helpers, wire conventions
locus-schema  schema model, YAML parser, validation helpers, codegen input
locusd        daemon binary: loads schema, starts locus-core over locus-dbus
locusctl      CLI client binary
locus-niri    Niri publisher binary
locus-graph   local graph inspection UI binary
```

Client and publisher crates that talk to the running daemon should depend on
`locus-dbus`, not on daemon internals. Pure Rust graph contracts belong in
`locus-api`.

## Public API Rules

Prefer traits for public contracts whenever practical. Put public traits and
transport-neutral graph types in `locus-api` when they describe the logical
Locus API.

Examples of things that belong in `locus-api`:

- `Graph` and other transport-neutral traits.
- Shared graph types such as `Link`, `LinkSetChange`, `PropertyChange`, and `Resolution`.
- Error/result types that are not tied to a transport.

Examples of things that belong in `locus-dbus`:

- D-Bus constants and wire type aliases.
- zbus interface definitions and generated proxies.
- `LocusClient` and D-Bus client helpers.
- D-Bus signal emission and conversion from `locus-api` errors to fdo errors.

Examples of things that do not belong in `locus-api`:

- In-memory graph storage.
- Schema enforcement internals.
- Niri-specific publishing logic.
- CLI formatting and argument parsing.
- Web UI rendering.

## Schema Rules

Schema language and validation helpers belong in `locus-schema`.

`locus-schema` must not depend on `locus-core` or `locusd`. If schema
validation needs graph data, expose a small trait in `locus-schema` and
implement it in `locus-core`. This keeps schema reusable for future TypeScript
code generation.

Do not hardcode desktop concepts in daemon runtime code. Concepts such as `window`,
`workspace`, `project`, and `agent-session` must come from schema/config or
publisher data.

## Daemon Runtime Rules

`locus-core` owns runtime behavior:

- graph state
- schema-enforced `SetLink`
- property storage
- path resolution
- derived subscription state

Keep runtime modules separate from D-Bus and CLI concerns. `service`, `state`,
`resolve`, and `error` should not import clap, zbus, niri-ipc, HTTP, AGS, or
shell-specific code. D-Bus adaptation belongs in `locus-dbus`, and it should
wrap `locus-api::Graph`.

## Binary Rules

Binary crates should be thin:

- `locusd` exposes the daemon runtime over D-Bus.
- `locusctl` is a CLI over `locus-dbus`.
- `locus-niri` publishes Niri state through `locus-dbus`.
- `locus-graph` visualizes state through `locus-dbus`.

If a binary grows reusable behavior, move that behavior into a library crate
with a clear boundary before adding more features.

## Testing

Run workspace checks after structural changes:

```sh
cargo fmt --check
cargo test --workspace
cargo check --workspace --all-targets
```

When changing `locus-dbus`, also check downstream local consumers such as
`agent-dbus`.
