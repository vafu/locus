# locus-schema

`locus-schema` parses and validates the Locus graph schema.

Locus stores a generic property graph:

```text
source --relation--> target
subject[key] = value
```

The schema gives that graph a shared vocabulary. It declares:

- node kinds and known properties
- required properties for node kinds
- relation source/target constraints
- relation cardinality
- named read paths for clients and code generators

The schema is intentionally not a full database schema. Unknown node ids and
unknown properties are allowed. Validation is focused on writes that create
links.

## YAML Shape

```yaml
nodes:
  project:
    properties:
      path:
        required: true
      name: {}
      icon: {}

relations:
  project:
    from: workspace
    to: project
    cardinality: one-to-one

paths:
  selected-project:
    from: context:selected
    path: [window, workspace, project]
```

## Nodes

Node keys are kind names. Locus checks a node's kind through its `kind`
property:

```text
project:/home/v47/proj/locus[kind] = project
```

Known properties are declared under `properties`. A property with
`required: true` must exist before a node of that kind can participate in a
schema-validated relation.

## Relations

Relations declare `from`, `to`, `cardinality`, and optional retention metadata.

`from` and `to` can be:

```yaml
from: window
to:
  exact: context:selected
```

Supported selectors:

- `window`: shorthand for `{ kind: window }`
- `{ kind: window }`: node must have `kind=window`
- `{ exact: context:selected }`: node id must match exactly
- omitted or `any`: any node; required properties are still checked if the node
  has a known kind

Supported cardinalities:

```text
one-to-one
many-to-one
one-to-many
many-to-many
```

Short forms are also accepted:

```text
1:1
*:1
1:*
*:*
```

Cardinality controls how `SetLink` behaves in `locus-core`. For example, a
`many-to-one` relation means many sources may point to the same target, but one
source may point to only one target for that relation.

`retention: weak` means targets reached through this outgoing relation should
not outlive the source. When `DeleteNode(source)` is called, weakly retained
targets are deleted too. The default is `retention: strong`. Weak retention is
rejected for relations where multiple sources may share one target.

`retention: static` means matching links and their endpoint node properties are
part of the durable default graph. `locusd` loads them before serving D-Bus and
updates the static store after graph writes.

## Paths

Paths are named directed read-side traversals. They do not validate writes. They
exist so clients and code generators can share common graph queries without
duplicating string arrays:

```yaml
selected-project:
  from: context:selected
  path: [window, workspace, project]
```

This maps to:

```sh
locusctl resolve context:selected window workspace project
```

`locus-codegen` uses paths to emit TypeScript helpers such as
`selectedProject()` and `subscribeSelectedProject()`.
