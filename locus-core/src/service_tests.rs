use crate::error::ServiceError;
use crate::service::LocusService;
use crate::{LinkSetChange, PropertyChange, Resolution};
use locus_schema::{GraphSchema, SchemaError};

fn service() -> LocusService {
    LocusService::with_schema(
        GraphSchema::parse_yaml(
            r#"
relations:
  current:
    from: any
    to: any
    cardinality: many-to-one
  rel:
    from: any
    to: any
    cardinality: many-to-many
  window:
    from:
      exact: context:selected
    to: window
    cardinality: many-to-one
  workspace:
    from: window
    to: workspace
    cardinality: many-to-one
  project:
    from: workspace
    to: project
    cardinality: one-to-one
  app-instance:
    from: window
    to: app-instance
    cardinality: one-to-one
    retention: weak
  agent-session:
    from: app-instance
    to: agent-session
    cardinality: one-to-one
    retention: weak
"#,
        )
        .unwrap(),
    )
}

fn set_kind(service: &LocusService, subject: &str, kind: &str) {
    service.set_property(subject, "kind", kind).unwrap();
}

fn static_project_schema() -> GraphSchema {
    GraphSchema::parse_yaml(
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
    cardinality: many-to-one
    retention: static
"#,
    )
    .unwrap()
}

fn temp_static_store(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "locus-core-{name}-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn returns_reverse_sources_for_multi_target_relations() {
    let service = service();
    service.set_link("session:1", "rel", "project:a").unwrap();
    service.set_link("session:2", "rel", "project:a").unwrap();

    assert_eq!(
        service.sources("project:a", "rel").unwrap(),
        vec!["session:1", "session:2"]
    );
}

#[test]
fn rejects_reciprocal_links() {
    let service = service();
    service.set_link("workspace:6", "rel", "window:57").unwrap();

    let error = service
        .set_link("window:57", "rel", "workspace:6")
        .unwrap_err();

    assert!(matches!(error, ServiceError::ReciprocalLink { .. }));
    assert_eq!(service.all_links().unwrap().len(), 1);
}

#[test]
fn set_property_replaces_existing_property() {
    let service = service();
    service.set_property("project:a", "name", "Old").unwrap();
    service.set_property("project:a", "name", "New").unwrap();

    assert_eq!(
        service.property("project:a", "name").unwrap().as_deref(),
        Some("New")
    );
}

#[test]
fn subjects_include_links_and_properties() {
    let service = service();
    service.set_link("a", "rel", "b").unwrap();
    service.set_property("c", "kind", "thing").unwrap();

    assert_eq!(service.subjects().unwrap(), vec!["a", "b", "c"]);
}

#[test]
fn finds_subjects_by_property() {
    let service = service();
    service.set_property("a", "kind", "project").unwrap();
    service.set_property("b", "kind", "workspace").unwrap();
    service.set_property("c", "kind", "project").unwrap();

    assert_eq!(
        service
            .subjects_with_property("kind", Some("project"))
            .unwrap(),
        vec!["a", "c"]
    );
    assert_eq!(
        service.subjects_with_property("kind", None).unwrap(),
        vec!["a", "b", "c"]
    );
}

#[test]
fn resolves_shortest_outgoing_path_to_kind() {
    let service = service();
    service
        .set_property("project:a", "kind", "project")
        .unwrap();
    service
        .set_property("project:b", "kind", "project")
        .unwrap();
    service.set_property("window:1", "kind", "window").unwrap();
    service
        .set_property("workspace:1", "kind", "workspace")
        .unwrap();
    service
        .set_link("context:selected", "window", "window:1")
        .unwrap();
    service
        .set_link("window:1", "workspace", "workspace:1")
        .unwrap();
    service
        .set_link("workspace:1", "project", "project:a")
        .unwrap();
    service.set_link("window:1", "rel", "project:b").unwrap();

    assert_eq!(
        service.resolve_kind("context:selected", "project").unwrap(),
        Some("project:b".to_string())
    );
}

#[test]
fn resolve_paths_follow_relation_direction() {
    let service = service();
    set_kind(&service, "window:1", "window");
    set_kind(&service, "workspace:1", "workspace");
    service
        .set_link("window:1", "workspace", "workspace:1")
        .unwrap();

    assert_eq!(
        service
            .resolve_path("window:1", &["workspace".to_string()])
            .unwrap(),
        Some("workspace:1".to_string())
    );
    assert_eq!(
        service
            .resolve_path("workspace:1", &["workspace".to_string()])
            .unwrap(),
        None
    );
}

#[test]
fn find_nearest_follows_outgoing_links_only() {
    let service = service();
    set_kind(&service, "window:1", "window");
    set_kind(&service, "workspace:1", "workspace");
    service
        .set_link("window:1", "workspace", "workspace:1")
        .unwrap();

    assert_eq!(
        service.resolve_kind("window:1", "workspace").unwrap(),
        Some("workspace:1".to_string())
    );
    assert_eq!(service.resolve_kind("workspace:1", "window").unwrap(), None);
}

#[test]
fn subscribed_resolution_only_reports_changed_target() {
    let service = service();
    assert_eq!(
        service
            .subscribe_resolution(
                "context:selected",
                &[
                    "window".to_string(),
                    "workspace".to_string(),
                    "project".to_string()
                ],
            )
            .unwrap()
            .target,
        None
    );

    set_kind(&service, "window:1", "window");
    service
        .set_link("context:selected", "window", "window:1")
        .unwrap();
    assert!(service.refresh_resolutions().unwrap().is_empty());

    set_kind(&service, "workspace:1", "workspace");
    service
        .set_property("project:a", "kind", "project")
        .unwrap();
    service
        .set_link("window:1", "workspace", "workspace:1")
        .unwrap();
    service
        .set_link("workspace:1", "project", "project:a")
        .unwrap();
    assert_eq!(
        service.refresh_resolutions().unwrap(),
        vec![Resolution {
            source: "context:selected".to_string(),
            path: vec![
                "window".to_string(),
                "workspace".to_string(),
                "project".to_string()
            ],
            target: Some("project:a".to_string()),
        }]
    );

    service.set_link("unrelated", "rel", "node").unwrap();
    assert!(service.refresh_resolutions().unwrap().is_empty());
}

#[test]
fn set_link_replaces_previous_relation() {
    let service = service();
    assert!(matches!(
        service.set_link("a", "current", "b").unwrap(),
        LinkSetChange::Changed { .. }
    ));
    assert!(matches!(
        service.set_link("a", "current", "c").unwrap(),
        LinkSetChange::Changed { .. }
    ));

    assert_eq!(service.targets("a", "current").unwrap(), vec!["c"]);
    assert_eq!(
        service
            .all_links()
            .unwrap()
            .into_iter()
            .map(|link| link.to_tuple())
            .collect::<Vec<_>>(),
        vec![("a".to_string(), "current".to_string(), "c".to_string())]
    );
}

#[test]
fn set_link_is_noop_when_visible_target_is_unchanged() {
    let service = service();
    assert!(matches!(
        service.set_link("a", "current", "b").unwrap(),
        LinkSetChange::Changed { .. }
    ));
    assert_eq!(
        service.set_link("a", "current", "b").unwrap(),
        LinkSetChange::Unchanged
    );
}

#[test]
fn set_link_is_noop_when_many_to_many_link_already_exists() {
    let service = service();
    assert!(matches!(
        service.set_link("a", "rel", "b").unwrap(),
        LinkSetChange::Changed { .. }
    ));
    assert_eq!(
        service.set_link("a", "rel", "b").unwrap(),
        LinkSetChange::Unchanged
    );
}

#[test]
fn required_node_properties_are_validated_on_link_set() {
    let service = LocusService::with_schema(
        GraphSchema::parse_yaml(
            r#"
nodes:
  workspace: {}
  project:
    properties:
      path:
        required: true
relations:
  project:
    from: workspace
    to: project
    cardinality: one-to-one
"#,
        )
        .unwrap(),
    );
    service
        .set_property("workspace:1", "kind", "workspace")
        .unwrap();
    service
        .set_property("project:a", "kind", "project")
        .unwrap();

    assert!(matches!(
        service
            .set_link("workspace:1", "project", "project:a")
            .unwrap_err(),
        ServiceError::Schema(SchemaError::MissingRequiredProperty { .. })
    ));

    service
        .set_property("project:a", "path", "/tmp/project-a")
        .unwrap();
    assert!(matches!(
        service.set_link("workspace:1", "project", "project:a"),
        Ok(LinkSetChange::Changed { .. })
    ));
}

#[test]
fn set_property_is_noop_when_visible_value_is_unchanged() {
    let service = service();
    assert_eq!(
        service.set_property("a", "name", "A").unwrap(),
        PropertyChange::Changed
    );
    assert_eq!(
        service.set_property("a", "name", "A").unwrap(),
        PropertyChange::Unchanged
    );
}

#[test]
fn all_links_returns_links() {
    let service = service();
    service.set_link("a", "rel", "b").unwrap();
    service.set_link("b", "rel", "c").unwrap();

    let links = service
        .all_links()
        .unwrap()
        .into_iter()
        .map(|link| link.to_tuple())
        .collect::<Vec<_>>();

    assert_eq!(
        links,
        vec![
            ("a".to_string(), "rel".to_string(), "b".to_string()),
            ("b".to_string(), "rel".to_string(), "c".to_string()),
        ]
    );
}

#[test]
fn delete_node_cascades_through_owned_outgoing_links() {
    let service = service();
    set_kind(&service, "window:1", "window");
    set_kind(&service, "workspace:1", "workspace");
    set_kind(&service, "app-instance:nvim", "app-instance");
    set_kind(&service, "agent-session:nvim/1", "agent-session");
    service
        .set_link("window:1", "workspace", "workspace:1")
        .unwrap();
    service
        .set_link("window:1", "app-instance", "app-instance:nvim")
        .unwrap();
    service
        .set_link("app-instance:nvim", "agent-session", "agent-session:nvim/1")
        .unwrap();

    let change = service.delete_node("window:1").unwrap();
    assert_eq!(change.removed_links.len(), 3);
    assert_eq!(service.subjects().unwrap(), vec!["workspace:1".to_string()]);
}

#[test]
fn remove_weak_link_deletes_orphaned_target() {
    let service = service();
    set_kind(&service, "window:1", "window");
    set_kind(&service, "app-instance:nvim", "app-instance");
    set_kind(&service, "agent-session:nvim/1", "agent-session");
    service
        .set_link("window:1", "app-instance", "app-instance:nvim")
        .unwrap();
    service
        .set_link("app-instance:nvim", "agent-session", "agent-session:nvim/1")
        .unwrap();

    let change = service
        .remove_link("window:1", "app-instance", "app-instance:nvim")
        .unwrap();

    assert_eq!(change.removed_links.len(), 2);
    assert_eq!(service.subjects().unwrap(), vec!["window:1".to_string()]);
}

#[test]
fn replacing_weak_link_deletes_previous_target() {
    let service = service();
    set_kind(&service, "window:1", "window");
    set_kind(&service, "app-instance:old", "app-instance");
    set_kind(&service, "app-instance:new", "app-instance");
    service
        .set_link("window:1", "app-instance", "app-instance:old")
        .unwrap();

    service
        .set_link("window:1", "app-instance", "app-instance:new")
        .unwrap();

    assert_eq!(
        service.subjects().unwrap(),
        vec!["app-instance:new".to_string(), "window:1".to_string(),]
    );
}

#[test]
fn delete_node_does_not_cascade_through_incoming_links() {
    let service = service();
    set_kind(&service, "window:1", "window");
    set_kind(&service, "workspace:1", "workspace");
    service
        .set_link("window:1", "workspace", "workspace:1")
        .unwrap();

    service.delete_node("workspace:1").unwrap();

    assert_eq!(service.subjects().unwrap(), vec!["window:1".to_string()]);
}

#[test]
fn static_links_and_endpoint_properties_are_loaded_as_initial_state() {
    let schema = static_project_schema();
    let path = temp_static_store("load");
    std::fs::write(
        &path,
        r#"{
  "links": [
    {
      "source": "workspace:1",
      "relation": "project",
      "target": "project:/tmp/locus"
    }
  ],
  "properties": [
    {
      "subject": "workspace:1",
      "key": "kind",
      "value": "workspace"
    },
    {
      "subject": "project:/tmp/locus",
      "key": "kind",
      "value": "project"
    },
    {
      "subject": "project:/tmp/locus",
      "key": "path",
      "value": "/tmp/locus"
    }
  ]
}"#,
    )
    .unwrap();

    let service = LocusService::with_static_store(schema, &path).unwrap();

    assert_eq!(
        service.targets("workspace:1", "project").unwrap(),
        vec!["project:/tmp/locus".to_string()]
    );
    assert_eq!(
        service
            .property("project:/tmp/locus", "path")
            .unwrap()
            .as_deref(),
        Some("/tmp/locus")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn static_links_are_persisted_after_writes() {
    let schema = static_project_schema();
    let path = temp_static_store("write");
    let service = LocusService::with_static_store(schema, &path).unwrap();
    set_kind(&service, "workspace:1", "workspace");
    set_kind(&service, "project:/tmp/locus", "project");
    service
        .set_property("project:/tmp/locus", "path", "/tmp/locus")
        .unwrap();

    service
        .set_link("workspace:1", "project", "project:/tmp/locus")
        .unwrap();

    let persisted = std::fs::read_to_string(&path).unwrap();
    assert!(persisted.contains("\"relation\": \"project\""));
    assert!(persisted.contains("\"subject\": \"project:/tmp/locus\""));
    std::fs::remove_file(path).unwrap();
}
