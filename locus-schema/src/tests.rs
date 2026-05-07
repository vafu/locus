use super::*;

#[test]
fn parses_nodes_relations_and_paths() {
    let schema = GraphSchema::parse_yaml(
        r#"
nodes:
  workspace: {}
  project:
    properties:
      path:
        required: true
      name: {}

relations:
  project:
    from: workspace
    to: project
    cardinality: one-to-one
    retention: static

paths:
  selected-project:
    from: context:selected
    path: [window, workspace, project]
  workspace-projects:
    from: workspace
    path: [project]
    many: true
"#,
    )
    .unwrap();

    assert!(schema.node("project").is_some());
    assert!(schema.node("project").unwrap().properties["path"].required);
    assert_eq!(
        schema.relation("project").unwrap().source,
        NodeSelector::Kind("workspace".to_string())
    );
    assert_eq!(
        schema.relation("project").unwrap().retention,
        Retention::Static
    );
    assert_eq!(
        schema.path("selected-project").unwrap().path,
        vec!["window", "workspace", "project"]
    );
    assert!(schema.path("workspace-projects").unwrap().many);
}

#[test]
fn rejects_invalid_cardinality() {
    let error = GraphSchema::parse_yaml(
        r#"
relations:
  project:
    from: workspace
    to: project
    cardinality: sometimes
"#,
    )
    .unwrap_err();

    assert!(matches!(error, SchemaError::InvalidCardinality { .. }));
}

#[test]
fn rejects_weak_retention_when_target_can_be_shared() {
    let error = GraphSchema::parse_yaml(
        r#"
relations:
  project:
    from: workspace
    to: project
    cardinality: many-to-one
    retention: weak
"#,
    )
    .unwrap_err();

    assert!(matches!(error, SchemaError::UnsafeWeakRetention { .. }));
}

#[test]
fn rejects_invalid_retention() {
    let error = GraphSchema::parse_yaml(
        r#"
relations:
  project:
    from: workspace
    to: project
    cardinality: one-to-one
    retention: sticky
"#,
    )
    .unwrap_err();

    assert!(matches!(error, SchemaError::InvalidRetention { .. }));
}
