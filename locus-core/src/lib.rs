pub mod error;
pub mod resolve;
pub mod service;
pub mod state;

#[cfg(test)]
mod service_tests;

pub use error::ServiceError;
pub use locus_api::{
    Graph, GraphError, GraphResult, Link, LinkSetChange, PropertyChange, Resolution,
};
pub use service::LocusService;
