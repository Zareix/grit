//! Unified diff production for single files.

use std::path::Path;

use git2::{Delta, DiffFormat, Oid};

use crate::error::Result;
use crate::repo::Repo;
use crate::status::{ChangeKind, FileEntry};

impl Repo {
    /// Unified patch of a staged file (HEAD -> index). Empty repo: empty tree.
    pub fn diff_staged(&self, path: impl AsRef<Path>) -> Result<String> {
        let repo = self.git2()?;
        let mut opts = git2::DiffOptions::new();
        opts.pathspec(path.as_ref());

        let tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());
        let diff = repo.diff_tree_to_index(tree.as_ref(), None, Some(&mut opts))?;
        patch_text(&diff)
    }

    /// Unified patch of an unstaged file (index -> workdir). Untracked files
    /// appear as added content.
    pub fn diff_unstaged(&self, path: impl AsRef<Path>) -> Result<String> {
        let repo = self.git2()?;
        let mut opts = git2::DiffOptions::new();
        opts.pathspec(path.as_ref());
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.show_untracked_content(true);

        let diff = repo.diff_index_to_workdir(None, Some(&mut opts))?;
        patch_text(&diff)
    }

    /// Files changed by a commit (parent -> commit); root commit: empty tree.
    pub fn commit_changed_files(&self, oid: Oid) -> Result<Vec<FileEntry>> {
        let repo = self.git2()?;
        let commit = repo.find_commit(oid)?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let tree = commit.tree()?;

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let mut out = Vec::new();
        for delta in diff.deltas() {
            let Some(path) =
                delta.new_file().path().or_else(|| delta.old_file().path()).map(Path::to_path_buf)
            else {
                continue;
            };
            let kind = match delta.status() {
                Delta::Added => ChangeKind::Added,
                Delta::Deleted => ChangeKind::Deleted,
                Delta::Modified => ChangeKind::Modified,
                Delta::Renamed => ChangeKind::Renamed,
                Delta::Typechange => ChangeKind::TypeChange,
                Delta::Copied => ChangeKind::Added,
                Delta::Untracked | Delta::Unreadable => ChangeKind::Added,
                _ => continue,
            };
            out.push(FileEntry { path, kind });
        }
        Ok(out)
    }

    /// Unified patch of one file as changed by a commit (parent -> commit).
    pub fn commit_file_diff(&self, oid: Oid, path: impl AsRef<Path>) -> Result<String> {
        let repo = self.git2()?;
        let commit = repo.find_commit(oid)?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let tree = commit.tree()?;

        let mut opts = git2::DiffOptions::new();
        opts.pathspec(path.as_ref());
        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))?;
        patch_text(&diff)
    }
}

/// Render a git2 diff as standard unified patch text.
fn patch_text(diff: &git2::Diff) -> Result<String> {
    let mut out = String::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        match line.origin() {
            // 'F' = file header lines, 'H' = hunk header (@@).
            // '+'/'-'/' ' carry the origin char itself.
            'F' | 'H' => push_bytes(&mut out, line.content()),
            '+' | '-' | ' ' => {
                out.push(line.origin());
                push_bytes(&mut out, line.content());
            }
            _ => {}
        }
        true
    })?;
    Ok(out)
}

fn push_bytes(out: &mut String, bytes: &[u8]) {
    if let Ok(text) = std::str::from_utf8(bytes) {
        out.push_str(text);
    }
}
