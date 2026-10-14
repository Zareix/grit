//! Tags.

use git2::Oid;

use crate::error::Result;
use crate::repo::Repo;

/// A tag (lightweight or annotated), peeled to its target commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// The commit the tag points to (peeled through annotated tags).
    pub target: Oid,
    /// Commit time (epoch seconds) of the target commit, for
    /// most-recent-first listings.
    pub date: i64,
}

impl Repo {
    /// All tags, sorted by name.
    pub fn tags(&self) -> Result<Vec<Tag>> {
        let repo = self.git2()?;
        let mut out = Vec::new();

        for reference in repo.references_glob("refs/tags/*")? {
            let reference = reference?;
            let name = reference.shorthand().ok().unwrap_or_default().to_owned();
            if name.is_empty() {
                continue;
            }
            // Peel annotated tags down to the commit; unresolved targets are
            // skipped rather than surfaced as dangling entries.
            let Ok(commit) = reference.peel_to_commit() else { continue };
            let date = commit.time().seconds();
            out.push(Tag { name: name.to_owned(), target: commit.id(), date });
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Create a lightweight tag at the given commit (`git tag <name>
    /// <oid>`). Refuses to overwrite an existing tag, like git itself.
    pub fn create_tag(&self, name: &str, oid: Oid) -> Result<()> {
        let repo = self.git2()?;
        repo.reference(&format!("refs/tags/{name}"), oid, false, "grit: create tag")?;
        Ok(())
    }

    /// Delete the named tag locally (`git tag -d`). The commit it points
    /// at stays; only the ref leaves the tag listing.
    pub fn delete_tag(&self, name: &str) -> Result<()> {
        let repo = self.git2()?;
        repo.tag_delete(name)?;
        Ok(())
    }
}
