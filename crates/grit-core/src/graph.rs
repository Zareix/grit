//! The commit graph as git itself draws it.
//!
//! A single `git log --graph --all --topo-order` pass is the source of
//! truth: it computes the lane layout (merge lines, backtracks) exactly
//! like the CLI does, and we only translate its ASCII art into pixels.
//! The output interleaves two line kinds:
//!
//! - **commit lines** contain the `%x01` marker followed by the full hash
//!   (they are the lines that also carry the `*` node),
//! - **art lines** (`|\`, `|/`, ...) carry no marker and describe how the
//!   lanes travel from the previous commit to what follows.
//!
//! Art lines are folded into the [`GraphRow`] of the commit they decorate,
//! so one graph row == one(commit + its art) == one log row of `h_7`.

use std::process::Command;

use git2::Oid;

use crate::commit::Commit;
use crate::error::Result;
use crate::repo::Repo;

/// One commit plus the art lines git drew for it. The first art line is
/// the commit's own (it holds the `*` at `node_col`); continuation lines
/// follow in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub commit: Commit,
    /// The commit's own art line, then any `|\` / `|/` continuation lines
    /// git drew underneath it (lines without a commit id of their own).
    pub art: Vec<String>,
}

impl GraphRow {
    /// Column of the `*` in the first art line (`None` when the art is
    /// missing — the renderer falls back to a plain dot).
    pub fn node_col(&self) -> Option<usize> {
        art_col(&self.art.first()?.chars().collect::<Vec<_>>(), '*')
    }
}

/// First column holding `wanted` in a padded art row.
fn art_col(chars: &[char], wanted: char) -> Option<usize> {
    chars.iter().position(|c| *c == wanted)
}

impl Repo {
    /// The whole constellation as clickable rows: every ref (HEAD, all
    /// branches, tags, remotes), topologically ordered to match the art,
    /// each commit carrying the art lines git drew below it.
    ///
    ///Parsing duties are git's: we do not reimplement lane assignment.
    pub fn graph_log_all(&self, limit: usize) -> Result<Vec<GraphRow>> {
        // Captured directly (stdout only, stderr excluded) so warnings can
        // never pollute the art stream.
        let output = Command::new("git")
            .args(["-c", "color.ui=never", "--no-pager"])
            .args(["log", "--graph", "--all", "--topo-order", "--max-count"])
            .arg(limit.to_string())
            .arg("--format=%x01%H")
            .current_dir(&self.path)
            .output()?;

        if !output.status.success() {
            return Err(crate::error::Error::Remote(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }

        // git2 hydrates the full metadata (summary, author, parents...) —
        // the stream only names the ids and draws the lanes.
        let git2_repo = self.git2()?;
        let mut rows: Vec<GraphRow> = Vec::with_capacity(limit.min(64));

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            match line.split_once('\u{1}') {
                // Commit line: the art prefix carries its own node.
                Some((art, hash)) => {
                    let oid = Oid::from_str(hash.trim())
                        .map_err(|e| crate::error::Error::Remote(e.to_string()))?;
                    let c = git2_repo.find_commit(oid)?;
                    rows.push(GraphRow {
                        commit: Commit {
                            id: c.id(),
                            summary: c.summary().ok().flatten().unwrap_or_default().to_owned(),
                            message: c.message().ok().map(str::to_owned),
                            author: crate::commit::signature_to_owned(c.author(), c.time()),
                            committer: crate::commit::signature_to_owned(c.committer(), c.time()),
                            parents: c.parent_ids().collect(),
                        },
                        art: vec![art.trim_end().to_owned()],
                    });
                }
                // Art only: continuation of the previous commit's cell.
                None => {
                    let art = line.trim_end().to_owned();
                    if let Some(row) = rows.last_mut() {
                        row.art.push(art);
                    }
                    // Art before the first commit cannot happen with
                    // `--topo-order`; if it ever does, it is dropped.
                }
            }
        }
        Ok(rows)
    }
}
