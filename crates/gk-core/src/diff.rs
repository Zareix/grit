//! Unified diff production for single files.

use std::path::Path;

use git2::DiffFormat;

use crate::error::Result;
use crate::repo::Repo;

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
