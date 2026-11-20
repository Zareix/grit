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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

/// A multi-commit git operation the repository is in the middle of: the
/// worktree holds its conflicts until the user continues or aborts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Operation::Merge => "Merge",
            Operation::Rebase => "Rebase",
            Operation::CherryPick => "Cherry-pick",
            Operation::Revert => "Revert",
        }
    }

    /// `git <op> --continue` argument list (finishes once the index is
    /// clean; the commit message comes from the prepared one).
    pub fn continue_args(self) -> Vec<String> {
        let op = match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
        };
        vec![op.to_string(), "--continue".to_string()]
    }

    /// `git <op> --abort` argument list (drops the operation's work).
    pub fn abort_args(self) -> Vec<String> {
        let mut args = self.continue_args();
        args[1] = "--abort".to_string();
        args
    }
}

/// A side of a conflict that deleted the file (stage 2 = this branch's side,
/// stage 3 = the incoming one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSide {
    Ours,
    Theirs,
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
    /// The operation the repository is in the middle of, if any.
    pub fn operation(&self) -> Option<Operation> {
        match self.git2().ok()?.state() {
            git2::RepositoryState::Merge => Some(Operation::Merge),
            git2::RepositoryState::Rebase
            | git2::RepositoryState::RebaseInteractive
            | git2::RepositoryState::RebaseMerge => Some(Operation::Rebase),
            git2::RepositoryState::CherryPick | git2::RepositoryState::CherryPickSequence => {
                Some(Operation::CherryPick)
            }
            git2::RepositoryState::Revert | git2::RepositoryState::RevertSequence => {
                Some(Operation::Revert)
            }
            _ => None,
        }
    }

    /// Working-tree text of `path`, conflict markers included: the input of
    /// the conflict viewer.
    pub fn working_file_text(&self, path: impl AsRef<Path>) -> Result<String> {
        Ok(std::fs::read_to_string(self.path.join(path.as_ref()))?)
    }

    /// Write the resolved text of a conflicted file and stage it — the
    /// `git add` half of a conflict resolution, which marks the conflict
    /// resolved for the in-progress operation.
    pub fn resolve_file(&self, path: impl AsRef<Path>, text: &str) -> Result<()> {
        std::fs::write(self.path.join(path.as_ref()), text)?;
        self.stage_file(path)
    }

    /// The side that deleted an unmerged path, when one did (a
    /// modify/delete conflict: git keeps the surviving side in the worktree,
    /// without markers). `None` for a content conflict or a clean path.
    pub fn deleted_side(&self, path: impl AsRef<Path>) -> Result<Option<ConflictSide>> {
        let index = self.git2()?.index()?;
        let path = path.as_ref();
        let present = |stage: i32| index.get_path(path, stage).is_some();
        Ok(match (present(2), present(3)) {
            (false, true) => Some(ConflictSide::Ours),
            (true, false) => Some(ConflictSide::Theirs),
            _ => None,
        })
    }

    /// Drop a conflicted file from the index and the working tree: the
    /// `git rm` resolution of a modify/delete conflict.
    pub fn remove_conflicted(&self, path: impl AsRef<Path>) -> Result<()> {
        let repo = self.git2()?;
        let path = path.as_ref();
        let mut index = repo.index()?;
        // Removes the path's conflict stages too.
        index.remove_path(path)?;
        index.write()?;
        std::fs::remove_file(self.path.join(path)).ok();
        Ok(())
    }

    /// Branch names of an in-progress merge: the checked-out side (ours) and
    /// the ref being merged in (theirs, resolved from `MERGE_HEAD`). Either
    /// name is `None` when it cannot be resolved (merging a bare commit, or
    /// no merge at all).
    pub fn merge_sides(&self) -> (Option<String>, Option<String>) {
        let Ok(mut repo) = self.git2() else { return (None, None) };
        let ours = repo
            .head()
            .ok()
            .and_then(|head| head.shorthand().ok().map(str::to_owned))
            .filter(|name| name != "HEAD");

        let mut merge_head = None;
        repo.mergehead_foreach(|oid| {
            merge_head = Some(*oid);
            false // first parent only: one MERGE_HEAD per merge
        })
        .ok();

        (ours, merge_head.and_then(|oid| branch_at(&repo, oid)))
    }

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

/// Short name of the branch (local first, then remote-tracking) pointing at
/// `oid`, for banners naming the two sides of a merge.
fn branch_at(repo: &git2::Repository, oid: git2::Oid) -> Option<String> {
    for (pattern, prefix) in [("refs/heads/*", "refs/heads/"), ("refs/remotes/*", "refs/remotes/")]
    {
        let refs = repo.references_glob(pattern).ok()?;
        for reference in refs.flatten() {
            if reference.target() == Some(oid) {
                let name = reference.name().ok()?;
                return name.strip_prefix(prefix).map(str::to_owned);
            }
        }
    }
    None
}
