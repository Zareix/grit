//! gk-core — Git plumbing for git-desktop.
//!
//! Pure Rust, no UI. Everything here is testable headlessly.

pub mod branch;
pub mod commit;
pub mod diff;
pub mod error;
pub mod remote;
pub mod repo;
pub mod status;

pub use error::{Error, Result};
pub use repo::Repo;
