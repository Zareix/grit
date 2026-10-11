//! Branches.

use git2::Oid;

use crate::error::Result;
use crate::repo::Repo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BranchType {
    Local,
    Remote,
}

/// A local or remote branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub kind: BranchType,
    pub head: Oid,
    /// For local branches: the upstream tracking branch, if any.
    pub upstream: Option<String>,
}

impl Repo {
    /// All branches, local first, sorted by name.
    pub fn branches(&self) -> Result<Vec<Branch>> {
        let repo = self.git2()?;
        let mut out = Vec::new();

        for branch in repo.branches(Some(git2::BranchType::Local))? {
            let (branch, branch_type) = branch?;
            let name = branch.name()?.unwrap_or_default().to_owned();
            let head =
                branch.get().peel_to_commit().map(|c| c.id()).unwrap_or_else(|_| Oid::ZERO_SHA1);
            let upstream =
                branch.upstream().ok().and_then(|up| up.name().ok().flatten().map(str::to_owned));
            out.push(Branch { name, kind: BranchType::Local, head, upstream });
            let _ = branch_type;
        }

        for branch in repo.branches(Some(git2::BranchType::Remote))? {
            let (branch, _) = branch?;
            let name = branch.name()?.unwrap_or_default().to_owned();
            // Skip remote HEAD pointers (origin/HEAD).
            if name.ends_with("/HEAD") {
                continue;
            }
            let head =
                branch.get().peel_to_commit().map(|c| c.id()).unwrap_or_else(|_| Oid::ZERO_SHA1);
            out.push(Branch { name, kind: BranchType::Remote, head, upstream: None });
        }

        out.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        Ok(out)
    }

    /// The currently checked-out branch, if any (None when detached HEAD).
    pub fn current_branch(&self) -> Result<Option<Branch>> {
        let repo = self.git2()?;
        let Some(head) = repo.head().ok() else {
            return Ok(None); // unborn branch or empty repo
        };
        if repo.head_detached().unwrap_or(false) {
            return Ok(None);
        }
        let branch = git2::Branch::wrap(head.clone());
        let name = branch.name()?.unwrap_or_default().to_owned();
        let upstream =
            branch.upstream().ok().and_then(|up| up.name().ok().flatten().map(str::to_owned));
        let head_commit = head.peel_to_commit().map(|c| c.id()).unwrap_or_else(|_| Oid::ZERO_SHA1);
        Ok(Some(Branch { name, kind: BranchType::Local, head: head_commit, upstream }))
    }

    /// Create a new local branch at HEAD (`git branch <name>`), without checking it out.
    pub fn create_branch(&self, name: &str) -> Result<()> {
        let repo = self.git2()?;
        let head = repo.head()?.peel_to_commit()?;
        repo.branch(name, &head, false)?;
        Ok(())
    }

    /// Checkout a remote-tracking branch (`origin/main`) as a local branch
    /// tracking it (`git checkout <short>` with auto-create). When a local
    /// branch with the same short name already exists, it is checked out
    /// directly, like `git checkout`.
    pub fn checkout_remote_branch(&self, name: &str) -> Result<()> {
        let repo = self.git2()?;
        // Resolve the longest configured remote prefix: "origin/feature/x"
        // splits into remote "origin" + branch "feature/x". Fall back to the
        // first `/` when no configured remote matches (refs can exist for a
        // remote that is no longer in the config).
        let mut best: Option<(String, String)> = None;
        for r in repo.remotes()?.iter().filter_map(|r| r.ok().flatten()) {
            if let Some(s) = name.strip_prefix(&format!("{r}/"))
                && best.as_ref().is_none_or(|(_, bs)| s.len() < bs.len())
            {
                best = Some((r.to_owned(), s.to_owned()));
            }
        }
        let Some((remote, short)) =
            best.or_else(|| name.split_once('/').map(|(r, s)| (r.to_owned(), s.to_owned())))
        else {
            return Err(
                git2::Error::from_str(&format!("invalid remote branch name: {name}")).into()
            );
        };

        // A local branch with this name already exists: just switch to it.
        if repo.find_branch(&short, git2::BranchType::Local).is_ok() {
            return self.checkout_branch(&short);
        }

        let (obj, _) = repo.revparse_ext(name)?;
        let commit = obj.peel_to_commit()?;
        let mut branch = repo.branch(&short, &commit, false)?;
        branch.set_upstream(Some(&format!("{remote}/{short}")))?;
        self.checkout_branch(&short)
    }

    /// Switch the working tree to another branch (`git checkout <name>`).
    pub fn checkout_branch(&self, name: &str) -> Result<()> {
        let repo = self.git2()?;
        let (obj, reference) = repo.revparse_ext(name)?;
        repo.checkout_tree(&obj, None)?;

        let ref_name = match reference {
            Some(r) => r.name().unwrap_or_default().to_owned(),
            None => format!("refs/heads/{name}"),
        };
        repo.set_head(&ref_name)?;
        Ok(())
    }

    /// Delete a local branch (`git branch -d <name>`).
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        let repo = self.git2()?;
        let mut branch = repo.find_branch(name, git2::BranchType::Local)?;
        if repo.head()?.shorthand().ok() == Some(name) {
            // Refuse to delete the checked-out branch.
            return Err(crate::error::Error::Git(git2::Error::from_str(
                "cannot delete the currently checked out branch",
            )));
        }
        branch.delete()?;
        Ok(())
    }
}
