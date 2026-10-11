//! Repository handle — the entry point of grit-core.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// An open git repository. Cheap to clone; each operation opens its own
/// `git2::Repository` handle so this stays usable from background threads.
#[derive(Clone, Debug)]
pub struct Repo {
    /// Absolute path to the work tree root (where `.git` lives).
    pub path: PathBuf,
}

impl Repo {
    /// Open the repository containing `path` (works from any subdirectory).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = std::fs::canonicalize(path.as_ref())?;
        let discovered = match git2::Repository::discover(&path) {
            Ok(repo) => repo,
            Err(e) if e.code() == git2::ErrorCode::NotFound => {
                return Err(Error::NotAWorkTree(path));
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path: discovered
                .workdir()
                .ok_or_else(|| Error::NotAWorkTree(path.clone()))?
                .to_path_buf(),
        })
    }

    /// Open a fresh `git2::Repository` handle for the work tree.
    pub(crate) fn git2(&self) -> Result<git2::Repository> {
        Ok(git2::Repository::open(&self.path)?)
    }

    pub fn name(&self) -> Option<String> {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned())
    }
}
