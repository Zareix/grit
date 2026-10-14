//! Commits.

use git2::Oid;

use crate::error::{Error, Result};
use crate::repo::Repo;

/// Author/committer identity of a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub email: String,
    /// Author time with its UTC offset (so it can be rendered in local time).
    pub time: git2::Time,
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

    /// Replace the HEAD commit with a new one carrying the staged index and
    /// the given message (`git commit --amend -m`). author is left untouched
    /// (unchanged original authorship), committer refreshed. Only valid
    /// while a branch is checked out (not unborn / detached HEAD).
    pub fn commit_amend(&self, message: &str) -> Result<Oid> {
        let repo = self.git2()?;
        let signature = self.signature(&repo)?;
        let mut index = repo.index()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        index.write()?;

        let head = repo.head()?.peel_to_commit()?;
        Ok(head.amend(Some("HEAD"), None, Some(&signature), None, Some(message), Some(&tree))?)
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

        collect_commits(&repo, revwalk, limit)
    }

    /// Recent commits, newest first (order is git's default: reverse chrono).
    /// Full walk: merge commits and side branches are all included, which the
    /// graph view needs.
    pub fn commit_log(&self, limit: usize) -> Result<Vec<Commit>> {
        let repo = self.git2()?;
        let head = match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            Some(head) => head,
            None => return Ok(Vec::new()),
        };

        let mut revwalk = repo.revwalk()?;
        revwalk.push(head.id())?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        collect_commits(&repo, revwalk, limit)
    }

    /// Recent commits across every ref (HEAD, all branches, all tags),
    /// newest first by committer time. The graph shows the whole constellation:
    /// commits that live only on other branches — stale remote tips in
    /// particular — must appear too, carrying their ref chips.
    ///
    /// Sort is TOPOLOGICAL with time as the tiebreaker, like `git log
    /// --graph`'s implicit ordering: a merge parent appears right after
    /// its children, so the lane layout closes forks quickly (with plain
    /// date order a fork whose base is old keeps its lane open for the
    /// whole window and pushes the main line off to the right).
    pub fn commit_log_all(&self, limit: usize) -> Result<Vec<Commit>> {
        let repo = self.git2()?;
        let mut revwalk = repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

        // HEAD (skipped when unborn: empty repo, detached infant state).
        if let Some(head) = repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            revwalk.push(head.id())?;
        }
        // Unborn branches (fresh `git branch` with no commits) fail to peel:
        // skipping them is correct, they point at no history yet.
        for branch in repo.branches(None)? {
            let (branch, _) = branch?;
            if let Ok(commit) = branch.get().peel_to_commit() {
                revwalk.push(commit.id())?;
            }
        }
        for reference in repo.references_glob("refs/tags/*")? {
            let reference = reference?;
            if let Ok(commit) = reference.peel_to_commit() {
                revwalk.push(commit.id())?;
            }
        }

        collect_commits(&repo, revwalk, limit)
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

pub(crate) fn signature_to_owned(sig: git2::Signature<'_>, time: git2::Time) -> Signature {
    Signature {
        name: sig.name().unwrap_or_default().to_owned(),
        email: sig.email().unwrap_or_default().to_owned(),
        time,
    }
}

/// Drain a configured revwalk into `Commit`s, at most `limit` of them.
fn collect_commits(
    repo: &git2::Repository,
    revwalk: git2::Revwalk<'_>,
    limit: usize,
) -> Result<Vec<Commit>> {
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
        });
    }
    Ok(out)
}

impl Signature {
    /// Local wall-clock time of this signature, as `dd/mm/yyyy HH:MM`.
    /// The offset comes from git (the committer's local zone at the time).
    pub fn date_time(&self) -> String {
        let local_secs = self.time.seconds() + self.time.offset_minutes() as i64 * 60;
        let days = local_secs.div_euclid(86_400);
        let secs = local_secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        format!("{day:02}/{month:02}/{year} {:02}:{:02}", secs / 3600, (secs % 3600) / 60)
    }
}

/// Howard Hinnant's civil-from-days: epoch day number -> (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe + era * 400;
    (y - u64::from(m <= 2) as i64, m, d)
}
