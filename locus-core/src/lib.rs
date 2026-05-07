pub mod error;
pub mod resolve;
pub mod service;
pub mod state;
pub mod static_store;
pub mod types;

#[cfg(test)]
mod service_tests;

pub use error::ServiceError;
pub use service::LocusService;
pub use types::{DeleteNodeChange, Link, LinkSetChange, PropertyChange, Resolution};
