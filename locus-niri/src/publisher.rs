use anyhow::Context;
use locus_dbus::{
    GraphReadProxy, GraphWriteProxy, MUTATION_DELETE_NODE, MUTATION_REMOVE_LINK,
    MUTATION_REMOVE_LINKS, MUTATION_REMOVE_PROPERTY, MUTATION_SET_LINK, MUTATION_SET_PROPERTY,
    MutationTuple, NONE_STRING,
};

use crate::graph::{
    GraphMutation, OUTPUT_PROPERTY_KEYS, OUTPUT_RELATION, SELECTED_CONTEXT, WINDOW_PROPERTY_KEYS,
    WINDOW_RELATION, WORKSPACE_PROPERTY_KEYS, WORKSPACE_RELATION, context_subject,
};

const STALE_SELECTED_WORKSPACE_RELATION: &str = "selected-workspace";

pub async fn apply_mutations(
    write: &GraphWriteProxy<'_>,
    mutations: Vec<GraphMutation>,
) -> anyhow::Result<()> {
    let span = tracing::trace_span!("locus.apply_mutations", count = mutations.len());
    let _guard = span.enter();
    if mutations.is_empty() {
        return Ok(());
    }

    let mutations = mutations
        .into_iter()
        .map(|mutation| match mutation {
            GraphMutation::SetLink {
                source,
                relation,
                target,
            } => mutation_tuple(MUTATION_SET_LINK, source, relation, target),
            GraphMutation::RemoveLink {
                source,
                relation,
                target,
            } => mutation_tuple(MUTATION_REMOVE_LINK, source, relation, target),
            GraphMutation::RemoveLinks { source, relation } => {
                mutation_tuple(MUTATION_REMOVE_LINKS, source, relation, String::new())
            }
            GraphMutation::DeleteNode { subject } => {
                mutation_tuple(MUTATION_DELETE_NODE, subject, String::new(), String::new())
            }
            GraphMutation::SetProperty {
                subject,
                key,
                value,
            } => mutation_tuple(MUTATION_SET_PROPERTY, subject, key, value),
            GraphMutation::RemoveProperty { subject, key } => {
                mutation_tuple(MUTATION_REMOVE_PROPERTY, subject, key, String::new())
            }
        })
        .collect::<Vec<_>>();

    write
        .apply_mutations(mutations)
        .await
        .context("apply Locus mutation batch")?;
    Ok(())
}

fn mutation_tuple(operation: &str, first: String, second: String, third: String) -> MutationTuple {
    (operation.to_string(), first, second, third)
}

pub async fn clear_existing_niri_edges(
    read: &GraphReadProxy<'_>,
    write: &GraphWriteProxy<'_>,
) -> anyhow::Result<()> {
    let links = read
        .get_all_links()
        .await
        .context("get existing Locus links")?;

    for (source, relation, target) in links {
        let is_workspace_window =
            relation == WINDOW_RELATION && source == context_subject(SELECTED_CONTEXT);
        let is_selected_workspace = relation == STALE_SELECTED_WORKSPACE_RELATION
            && source == context_subject(SELECTED_CONTEXT);
        let is_window_workspace = relation == WORKSPACE_RELATION
            && (source.starts_with("window:") || source.starts_with("niri:window:"));
        let is_workspace_output = relation == OUTPUT_RELATION
            && (source.starts_with("workspace:") || source.starts_with("niri:workspace:"));
        if is_workspace_window
            || is_selected_workspace
            || is_window_workspace
            || is_workspace_output
        {
            write
                .remove_link(&source, &relation, &target)
                .await
                .with_context(|| {
                    format!("remove stale Niri link {source} --{relation}--> {target}")
                })?;
        }
    }

    write
        .remove_links(&context_subject(SELECTED_CONTEXT), WORKSPACE_RELATION)
        .await
        .context("remove stale selected workspace links")?;
    write
        .remove_links(&context_subject(SELECTED_CONTEXT), WINDOW_RELATION)
        .await
        .context("remove stale selected window links")?;
    write
        .remove_links(
            &context_subject(SELECTED_CONTEXT),
            STALE_SELECTED_WORKSPACE_RELATION,
        )
        .await
        .context("remove stale selected-workspace links")?;

    for (kind, keys) in [
        ("window", WINDOW_PROPERTY_KEYS),
        ("workspace", WORKSPACE_PROPERTY_KEYS),
        ("output", OUTPUT_PROPERTY_KEYS),
    ] {
        let subjects = read
            .find_subjects("kind", kind)
            .await
            .with_context(|| format!("find stale Niri {kind} subjects"))?;
        for subject in subjects {
            let is_niri_node = none(
                read.get_property(&subject, "source")
                    .await
                    .with_context(|| format!("read {subject}[source]"))?,
            )
            .as_deref()
                == Some("niri");
            if is_niri_node
                || subject.starts_with("window:")
                || subject.starts_with("workspace:")
                || subject.starts_with("output:")
                || subject.starts_with("niri:window:")
                || subject.starts_with("niri:workspace:")
            {
                for key in keys {
                    write
                        .remove_property(&subject, key)
                        .await
                        .with_context(|| format!("remove stale Niri property {subject}[{key}]"))?;
                }
            }
        }
    }
    Ok(())
}

fn none(value: String) -> Option<String> {
    (value != NONE_STRING).then_some(value)
}
