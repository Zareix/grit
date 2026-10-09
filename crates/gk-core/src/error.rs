//! Error type for gk-core.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("path is not inside a git work tree: {}", .0.display())]
    NotAWorkTree(PathBuf),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error(
        "no user.name/user.email configured — set them with `git config --global user.name \"…\"`"
    )]
    MissingIdentity,
}

pub type Result<T> = std::result::Result<T, Error>;
