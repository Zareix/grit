//! grit-core — Git plumbing for Grit.
//!
//! Pure Rust, no UI. Everything here is testable headlessly.

pub mod branch;
pub mod commit;
pub mod diff;
pub mod error;
pub mod remote;
pub mod repo;
pub mod status;

pub use branch::Branch;
pub use commit::Commit;
pub use error::{Error, Result};
pub use git2::Oid;
pub use remote::RemoteOp;
pub use repo::Repo;
pub use status::FileEntry;
pub use status::Status;
