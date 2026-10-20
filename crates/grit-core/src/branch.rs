//! Branches.

use std::collections::HashMap;

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

/// Full ref name of a branch (`refs/heads/x` / `refs/remotes/origin/x`),
/// usable as an unambiguous git argument.
pub fn full_ref(kind: BranchType, name: &str) -> String {
    match kind {
        BranchType::Local => format!("refs/heads/{name}"),
        BranchType::Remote => format!("refs/remotes/{name}"),
    }
}

/// Ahead/behind counts between pairs of tips, keyed `relations[a][b]` =
/// (commits unique to `a`, commits unique to `b`).
pub type AheadBehindMap = HashMap<Oid, HashMap<Oid, (usize, usize)>>;

/// Tips per paint pass: a commit's reachability mask is one `u64`.
const PAINT_CHUNK: usize = u64::BITS as usize;

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

    /// Create a new local branch at the given commit (`git branch <name>
    /// <oid>`), without checking it out.
    pub fn create_branch_at(&self, name: &str, oid: Oid) -> Result<()> {
        let repo = self.git2()?;
        let commit = repo.find_commit(oid)?;
        repo.branch(name, &commit, false)?;
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

    /// Switch the working tree to a specific commit (detached HEAD, like
    /// `git checkout <oid>`); used by the log's "Checkout this commit".
    pub fn checkout_commit(&self, oid: Oid) -> Result<()> {
        let repo = self.git2()?;
        let commit = repo.find_commit(oid)?;
        repo.checkout_tree(commit.as_object(), None)?;
        repo.set_head_detached(oid)?;
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

    /// Commits unique to each tip: (`ahead`, `behind`) — commits reachable
    /// from `a` but not from `b`, then the other way round. For unrelated
    /// histories every commit counts on each side.
    pub fn ahead_behind(&self, a: Oid, b: Oid) -> Result<(usize, usize)> {
        Ok(self.git2()?.graph_ahead_behind(a, b)?)
    }

    /// Ahead/behind counts between every pair of the given tips (e.g. branch
    /// heads), keyed by tip OID. `relations[a][b]` counts commits unique to
    /// `a`, then commits unique to `b`. Duplicate OIDs are collapsed; a tip
    /// compared with itself stays (0, 0).
    ///
    /// Instead of one `graph_ahead_behind` walk per pair (O(n²) full walks),
    /// one topological walk paints every commit with the bit-set of tips
    /// that reach it, then every pair count is derived from those masks:
    /// `ahead(a, b)` is the number of painted commits carrying `a`'s bit
    /// but not `b`'s. Walks are chunked 64 tips at a time (the mask width);
    /// commits absent from a chunk's map are reachable from none of its
    /// tips. Pairs count over distinct masks only — commits on a shared
    /// linear segment all carry the same mask, so the histogram stays small.
    pub fn ahead_behind_all(&self, tips: &[Oid]) -> Result<AheadBehindMap> {
        let mut tips = tips.to_vec();
        tips.sort_unstable();
        tips.dedup();
        let mut out: AheadBehindMap = HashMap::with_capacity(tips.len());
        if tips.is_empty() {
            return Ok(out);
        }

        // Single pair: git2's own walk is cheaper than the paint pass (it
        // stops at the merge base instead of walking the whole DAG).
        if tips.len() == 2 {
            let counts = self.git2()?.graph_ahead_behind(tips[0], tips[1])?;
            put_pair(&mut out, tips[0], tips[1], counts.0, counts.1);
            out.entry(tips[0]).or_default().insert(tips[0], (0, 0));
            out.entry(tips[1]).or_default().insert(tips[1], (0, 0));
            return Ok(out);
        }

        let chunks: Vec<&[Oid]> = tips.chunks(PAINT_CHUNK).collect();
        let painted: Vec<HashMap<Oid, u64>> =
            chunks.iter().map(|chunk| self.paint_reachability(chunk)).collect::<Result<_>>()?;
        // Distinct-mask histogram per chunk.
        let histograms: Vec<HashMap<u64, usize>> = painted
            .iter()
            .map(|masks| {
                let mut histogram: HashMap<u64, usize> = HashMap::new();
                for mask in masks.values() {
                    *histogram.entry(*mask).or_default() += 1;
                }
                histogram
            })
            .collect();

        for (g, chunk) in chunks.iter().enumerate() {
            // Same-chunk pairs, straight from that chunk's histogram.
            for i in 0..chunk.len() {
                for j in i + 1..chunk.len() {
                    let mut ahead = 0;
                    let mut behind = 0;
                    for (&mask, &count) in &histograms[g] {
                        let in_a = mask >> i & 1 == 1;
                        let in_b = mask >> j & 1 == 1;
                        if in_a && !in_b {
                            ahead += count;
                        } else if in_b && !in_a {
                            behind += count;
                        }
                    }
                    let (a, b) = (chunk[i], chunk[j]);
                    put_pair(&mut out, a, b, ahead, behind);
                }
            }

            // Cross-chunk pairs use the joint mask distribution of the two
            // chunks (a in the earlier chunk, b in the later one).
            for (h, later) in chunks.iter().enumerate().skip(g + 1) {
                let joint = joint_histogram(&painted[g], &painted[h]);
                for (i, &a) in chunk.iter().enumerate() {
                    for (j, &b) in later.iter().enumerate() {
                        let mut ahead = 0;
                        let mut behind = 0;
                        for (&(mask_a, mask_b), &count) in &joint {
                            let in_a = mask_a >> i & 1 == 1;
                            let in_b = mask_b >> j & 1 == 1;
                            if in_a && !in_b {
                                ahead += count;
                            } else if in_b && !in_a {
                                behind += count;
                            }
                        }
                        put_pair(&mut out, a, b, ahead, behind);
                    }
                }
            }
        }

        // Self-comparison is always equal.
        for &tip in &tips {
            out.entry(tip).or_default().insert(tip, (0, 0));
        }
        Ok(out)
    }

    /// For every commit reachable from any of `tips`, the bit-set of tips
    /// that reach it (a tip carries its own bit). One topological walk —
    /// children before parents — so a single forward pass accumulates each
    /// child's finished mask into its parents.
    fn paint_reachability(&self, tips: &[Oid]) -> Result<HashMap<Oid, u64>> {
        let repo = self.git2()?;
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        for tip in tips {
            walk.push(*tip)?;
        }

        let mut masks: HashMap<Oid, u64> = HashMap::new();
        // Masks propagated from already-painted children, keyed by parent.
        let mut incoming: HashMap<Oid, u64> = HashMap::new();
        for oid in walk {
            let oid = oid?;
            let mut mask = incoming.remove(&oid).unwrap_or(0);
            if let Some(bit) = tips.iter().position(|tip| *tip == oid) {
                mask |= 1 << bit;
            }
            let commit = repo.find_commit(oid)?;
            for parent in commit.parent_ids() {
                *incoming.entry(parent).or_default() |= mask;
            }
            masks.insert(oid, mask);
        }
        Ok(masks)
    }
}

/// Store a pair result and its mirrored row: `relations[a][b]` =
/// (unique to `a`, unique to `b`), `relations[b][a]` the other way round.
fn put_pair(out: &mut AheadBehindMap, a: Oid, b: Oid, ahead: usize, behind: usize) {
    out.entry(a).or_default().insert(b, (ahead, behind));
    out.entry(b).or_default().insert(a, (behind, ahead));
}

/// Joint distribution of two chunks' painted masks over the union of their
/// commits. A commit present in only one map is reachable from none of the
/// other chunk's tips — it counts with mask 0 there.
fn joint_histogram(a: &HashMap<Oid, u64>, b: &HashMap<Oid, u64>) -> HashMap<(u64, u64), usize> {
    let mut joint: HashMap<(u64, u64), usize> = HashMap::new();
    for (&oid, &mask_a) in a {
        *joint.entry((mask_a, b.get(&oid).copied().unwrap_or(0))).or_default() += 1;
    }
    for (&oid, &mask_b) in b {
        if !a.contains_key(&oid) {
            *joint.entry((0, mask_b)).or_default() += 1;
        }
    }
    joint
}
