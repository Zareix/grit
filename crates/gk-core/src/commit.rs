//! Commits.

use std::time::SystemTime;

use git2::Oid;

use crate::error::{Error, Result};
use crate::repo::Repo;

/// Author/committer identity of a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub time: SystemTime,
}

/// A single commit (metadata only; the diff comes later).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: Oid,
    pub summary: String,
    pub message: Option<String>,
    pub author: Signature,
    pub committer: Signature,
    pub parents: Vec<Oid>,
}

impl Repo {
    /// Commit the currently staged changes.
    pub fn commit(&self, message: &str) -> Result<Oid> {
        let repo = self.git2()?;
        let signature = self.signature(&repo)?;
        let mut index = repo.index()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        index.write()?;

        let parent_commit = match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            Some(parent) => {
                let parents = [&parent];
                repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)?
            }
            None => repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])?,
        };

        Ok(parent_commit)
    }

    /// Look up a commit by oid.
    pub fn find_commit(&self, id: Oid) -> Result<Commit> {
        let repo = self.git2()?;
        let c = repo.find_commit(id)?;
        Ok(Commit {
            id: c.id(),
            summary: c.summary().ok().flatten().unwrap_or_default().to_owned(),
            message: c.message().ok().map(str::to_owned),
            author: signature_to_owned(c.author(), c.time()),
            committer: signature_to_owned(c.committer(), c.time()),
            parents: c.parent_ids().collect(),
        })
    }

    /// HEAD's commit, if any (None on an empty repository).
    pub fn head_commit(&self) -> Result<Option<Commit>> {
        let repo = self.git2()?;
        let Some(commit) = repo.head().ok().and_then(|h| h.peel_to_commit().ok()) else {
            return Ok(None);
        };
        Ok(Some(self.find_commit(commit.id())?))
    }

    /// Recent commits reachable from an arbitrary ref ("main", "origin/x"),
    /// newest first. Walk without first-parent simplification so merges show
    /// up in order.
    pub fn commit_log_ref(&self, refname: &str, limit: usize) -> Result<Vec<Commit>> {
        let repo = self.git2()?;
        let oid = repo.revparse_single(refname)?.id();

        let mut revwalk = repo.revwalk()?;
        revwalk.push(oid)?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let mut out = Vec::with_capacity(limit.min(64));
        for oid in revwalk.take(limit) {
            let c = repo.find_commit(oid?)?;
            out.push(Commit {
                id: c.id(),
                summary: c.summary().ok().flatten().unwrap_or_default().to_owned(),
                message: c.message().ok().map(str::to_owned),
                author: signature_to_owned(c.author(), c.time()),
                committer: signature_to_owned(c.committer(), c.time()),
                parents: c.parent_ids().collect(),
            })
        }
        Ok(out)
    }

    /// Recent commits, newest first (order is git's default: reverse chrono).
    pub fn commit_log(&self, limit: usize) -> Result<Vec<Commit>> {
        let repo = self.git2()?;
        let head = match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            Some(head) => head,
            None => return Ok(Vec::new()),
        };

        let mut revwalk = repo.revwalk()?;
        revwalk.push(head.id())?;
        revwalk.set_sorting(git2::Sort::TIME)?;
        // Simplify history to first-parents only for a linear view.
        revwalk.simplify_first_parent()?;

        let mut out = Vec::with_capacity(limit.min(64));
        for oid in revwalk.take(limit) {
            let oid = oid?;
            let c = repo.find_commit(oid)?;
            out.push(Commit {
                id: c.id(),
                summary: c.summary().ok().flatten().unwrap_or_default().to_owned(),
                message: c.message().ok().map(str::to_owned),
                author: signature_to_owned(c.author(), c.time()),
                committer: signature_to_owned(c.committer(), c.time()),
                parents: c.parent_ids().collect(),
            });
        }
        Ok(out)
    }

    /// Signature to use for a new commit, from global/local git config.
    fn signature(&self, repo: &git2::Repository) -> Result<git2::Signature<'static>> {
        let config = repo.config()?;
        let name: Option<String> = config.get_string("user.name").ok();
        let email: Option<String> = config.get_string("user.email").ok();

        let (Some(name), Some(email)) = (name, email) else {
            return Err(Error::MissingIdentity);
        };

        Ok(git2::Signature::now(&name, &email)?)
    }
}

fn signature_to_owned(sig: git2::Signature<'_>, time: git2::Time) -> Signature {
    Signature {
        name: sig.name().unwrap_or_default().to_owned(),
        email: sig.email().unwrap_or_default().to_owned(),
        time: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(time.seconds().max(0) as u64),
    }
}
