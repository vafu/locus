mod error;
mod interfaces;
mod state;
mod watch;

use locus_core::LocusService;
use zbus::Connection;

pub use interfaces::{GraphReadProxy, GraphResolveProxy, GraphWriteProxy};

pub const BUS_NAME: &str = "io.github.Locus";
pub const ROOT_PATH: &str = "/io/github/Locus";
pub const GRAPH_READ_INTERFACE: &str = "io.github.Locus.Graph.Read";
pub const GRAPH_WRITE_INTERFACE: &str = "io.github.Locus.Graph.Write";
pub const GRAPH_RESOLVE_INTERFACE: &str = "io.github.Locus.Graph.Resolve";
pub const WATCH_INTERFACE: &str = "io.github.Locus.Watch";
pub const WATCH_ROOT_PATH: &str = "/io/github/Locus/Watch";
pub const NONE_STRING: &str = "";
pub const MUTATION_SET_LINK: &str = "set-link";
pub const MUTATION_REMOVE_LINK: &str = "remove-link";
pub const MUTATION_REMOVE_LINKS: &str = "remove-links";
pub const MUTATION_DELETE_NODE: &str = "delete-node";
pub const MUTATION_SET_PROPERTY: &str = "set-property";
pub const MUTATION_REMOVE_PROPERTY: &str = "remove-property";

pub type LinkTuple = (String, String, String);
pub type MutationTuple = (String, String, String, String);

pub async fn serve(service: LocusService) -> zbus::Result<Connection> {
    interfaces::serve(service).await
}
