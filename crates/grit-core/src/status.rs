//! Working tree status + staging (git add / reset).

use std::path::{Path, PathBuf};

use git2::{Delta, Status as GitStatus};

use crate::error::Result;
use crate::repo::Repo;

/// Change kind for a file in the working tree or the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Conflicted,
}

impl ChangeKind {
    fn from(status: GitStatus, delta: Delta) -> Option<Self> {
        if status.is_conflicted() {
            return Some(ChangeKind::Conflicted);
        }
        match delta {
            Delta::Added => Some(ChangeKind::Added),
            Delta::Modified => Some(ChangeKind::Modified),
            Delta::Deleted => Some(ChangeKind::Deleted),
            Delta::Renamed => Some(ChangeKind::Renamed),
            Delta::Typechange => Some(ChangeKind::TypeChange),
            // Untracked files surface as "new" files to stage.
            Delta::Untracked | Delta::Unreadable => Some(ChangeKind::Added),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::TypeChange => "T",
            ChangeKind::Conflicted => "U",
        }
    }
}

/// One changed file. When a file is both staged and modified in the working
/// tree, it appears twice (once in `staged`, once in `unstaged`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the repo root, forward slashes.
    pub path: PathBuf,
    pub kind: ChangeKind,
}

/// Full status snapshot of the working tree.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Between HEAD and the index (what will be committed).
    pub staged: Vec<FileEntry>,
    /// Between the index and the working tree (not staged yet).
    pub unstaged: Vec<FileEntry>,
}

impl Status {
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty()
    }

    pub fn len(&self) -> usize {
        self.staged.len() + self.unstaged.len()
    }

    pub fn count(&self, kind: ChangeKind) -> usize {
        self.iter().filter(|f| f.kind == kind).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = &FileEntry> {
        self.staged.iter().chain(self.unstaged.iter())
    }
}

/// Status bits that mean "changed between HEAD and the index".
fn staged_mask() -> GitStatus {
    GitStatus::INDEX_NEW
        | GitStatus::INDEX_MODIFIED
        | GitStatus::INDEX_DELETED
        | GitStatus::INDEX_RENAMED
        | GitStatus::INDEX_TYPECHANGE
}

/// Status bits that mean "changed between the index and the working tree".
fn unstaged_mask() -> GitStatus {
    GitStatus::WT_NEW
        | GitStatus::WT_MODIFIED
        | GitStatus::WT_DELETED
        | GitStatus::WT_RENAMED
        | GitStatus::WT_TYPECHANGE
        | GitStatus::CONFLICTED
}

impl Repo {
    /// Compute the working tree status.
    pub fn status(&self) -> Result<Status> {
        let repo = self.git2()?;
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true).include_ignored(false);
        opts.recurse_untracked_dirs(true);
        opts.exclude_submodules(false);
        opts.sort_case_sensitively(true);

        let mut status = Status::default();
        for entry in repo.statuses(Some(&mut opts))?.iter() {
            // Ignore index-only entries (already staged without workdir changes
            // shows up in the staged half through head_to_index).
            let path = match entry.path() {
                Ok(p) => PathBuf::from(p),
                Err(_) => continue,
            };
            let s = entry.status();
            let head_to_idx_status = s & staged_mask();
            let idx_to_wt_status = s & unstaged_mask();

            // HEAD -> index (staged)
            if !head_to_idx_status.is_empty() {
                let delta = entry.head_to_index().map(|d| d.status());
                if let Some(kind) =
                    ChangeKind::from(head_to_idx_status, delta.unwrap_or(Delta::Unmodified))
                {
                    status.staged.push(FileEntry { path: path.clone(), kind });
                }
            }

            // index -> workdir (unstaged)
            if !idx_to_wt_status.is_empty() {
                let delta = entry.index_to_workdir().map(|d| d.status());
                if let Some(kind) =
                    ChangeKind::from(idx_to_wt_status, delta.unwrap_or(Delta::Unmodified))
                {
                    status.unstaged.push(FileEntry { path, kind });
                }
            }
        }
        Ok(status)
    }

    /// Stage a file (`git add <path>`). Handles untracked, modified and deleted files.
    pub fn stage_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let repo = self.git2()?;
        let path = path.as_ref();
        let mut index = repo.index()?;
        index.add_path(path)?;
        // Stage deletions too (like `git add` from git 2.x).
        index.update_all([path], None)?;
        index.write()?;
        Ok(())
    }

    /// Stage everything (`git add -A`).
    pub fn stage_all(&self) -> Result<()> {
        let repo = self.git2()?;
        let mut index = repo.index()?;
        index.add_all(["."], git2::IndexAddOption::DEFAULT, None)?;
        index.update_all(["."], None)?;
        index.write()?;
        Ok(())
    }

    /// Unstage a file (`git reset -- <path>`); the file keeps its workdir changes.
    pub fn unstage_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let repo = self.git2()?;
        let path = path.as_ref().to_path_buf();

        if repo.head().is_ok() {
            let head_commit = repo.head()?.peel_to_commit()?;
            repo.reset_default(Some(head_commit.as_object()), [path])?;
        } else {
            // No HEAD yet: clear the index entry entirely (untracked file).
            let mut index = repo.index()?;
            index.remove_all([path], None)?;
            index.write()?;
        }
        Ok(())
    }

    /// Unstage everything (`git reset` — mixed, keeps workdir).
    pub fn unstage_all(&self) -> Result<()> {
        let repo = self.git2()?;
        if repo.head().is_err() {
            // No HEAD yet: clear the index entirely (everything was staged as new).
            let mut index = repo.index()?;
            index.clear()?;
            index.write()?;
            return Ok(());
        }
        let head_commit = repo.head()?.peel_to_commit()?;
        // A mixed reset updates the index to HEAD without touching the workdir.
        repo.reset(head_commit.as_object(), git2::ResetType::Mixed, None)?;
        Ok(())
    }

    /// Discard every local change to a file (both staged and unstaged): a
    /// path tracked by HEAD goes back to HEAD (index and worktree), every
    /// other path — untracked, staged or not — is dropped from the index
    /// and deleted from the working tree.
    pub fn discard_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let repo = self.git2()?;
        let path = path.as_ref();
        let tracked = repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok())
            .and_then(|tree| tree.get_path(path).ok())
            .is_some();

        if !tracked {
            // Fresh files: gone from disk, and off the index when staged.
            std::fs::remove_file(self.path.join(path)).ok();
            let mut index = repo.index()?;
            if index.get_path(path, 0).is_some() {
                index.remove_path(path)?;
                index.write()?;
            }
            return Ok(());
        }

        // `git checkout HEAD -- <path>`: reset index and worktree together.
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force().update_index(true).path(path);
        repo.checkout_head(Some(&mut checkout))?;
        Ok(())
    }
}
