//! Stashes: list entries, stash selected paths, apply / pop / drop.

use std::path::PathBuf;

use crate::error::Result;
use crate::repo::Repo;

/// One entry of `git stash list`: the stack position (`stash@{index}`) and
/// the message git wrote on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub index: usize,
    pub message: String,
}

/// git arguments for stashing exactly `paths` — worktree + index states go
/// to `stash@{0}` and the paths return to their HEAD state. `-u` folds
/// untracked files matched by the pathspecs into the stash; the message is
/// left unset so git autogenerates the usual "WIP on <branch>: …".
pub fn stash_push_args(paths: &[PathBuf]) -> Vec<String> {
    let mut args: Vec<String> =
        vec!["stash".into(), "push".into(), "--include-untracked".into(), "--".into()];
    for path in paths {
        args.push(path.to_string_lossy().into_owned());
    }
    args
}

impl Repo {
    /// All stashes, newest first (`stash@{0}` on top).
    pub fn stashes(&self) -> Result<Vec<Stash>> {
        let mut repo = self.git2()?;
        let mut out = Vec::new();
        // The callback stops at the first `false`.
        repo.stash_foreach(|index, message, _oid| {
            out.push(Stash { index, message: message.to_owned() });
            true
        })?;
        Ok(out)
    }

    /// Stash the given files only (`git stash push -u -- <paths>`).
    pub fn stash_paths(&self, paths: &[PathBuf]) -> Result<()> {
        self.run_git(&stash_push_args(paths))?;
        Ok(())
    }

    /// Reapply a stash but keep it in the list (`git stash apply`).
    pub fn stash_apply(&self, index: usize) -> Result<()> {
        let mut repo = self.git2()?;
        Ok(repo.stash_apply(index, None)?)
    }

    /// Reapply a stash and drop it (`git stash pop`).
    pub fn stash_pop(&mut self, index: usize) -> Result<()> {
        let mut repo = self.git2()?;
        Ok(repo.stash_pop(index, None)?)
    }

    /// Remove a stash entry (`git stash drop`).
    pub fn stash_drop(&mut self, index: usize) -> Result<()> {
        let mut repo = self.git2()?;
        Ok(repo.stash_drop(index)?)
    }
}
