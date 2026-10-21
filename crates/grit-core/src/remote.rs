//! Remote operations (fetch / pull / push).
//!
//! These shell out to the `git` binary on purpose: network authentication
//! (SSH agent, HTTPS credential helpers) comes for free with the user's own
//! git config, while libgit2 would require reimplementing credential plumbing.

use std::process::Command;

use crate::error::Result;
use crate::repo::Repo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOp {
    Fetch,
    Pull,
    Push,
}

impl RemoteOp {
    /// The `git` subcommand for this operation.
    pub fn name(&self) -> &'static str {
        match self {
            RemoteOp::Fetch => "fetch",
            RemoteOp::Pull => "pull",
            RemoteOp::Push => "push",
        }
    }
}

impl Repo {
    /// Run a remote operation synchronously; call from a background thread.
    /// The fetch prunes: without it, remote-tracking refs of branches
    /// deleted upstream (Renovate post-merge cleanup, stale feature
    /// branches) stay forever and clutter the remotes tree.
    pub fn run_remote_op(&self, op: RemoteOp) -> Result<String> {
        match op {
            RemoteOp::Fetch => self.run_git(&["fetch".to_string(), "--prune".to_string()]),
            _ => self.run_git(&[op.name().to_string()]),
        }
    }

    /// Run an arbitrary git subcommand synchronously (local `git` access only;
    /// remote/auth flows stay in the shell).
    pub fn run_git(&self, args: &[String]) -> Result<String> {
        let output = Command::new("git")
            .args(["-c", "color.ui=never", "--no-pager"])
            .args(args)
            .current_dir(&self.path)
            .output()?;

        // fetch/pull/push write their report to stderr.
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_string();

        if !output.status.success() {
            return Err(crate::error::Error::Remote(text));
        }
        Ok(text)
    }

    /// Tag names published on `remote` (`git ls-remote --tags`, called
    /// from a background thread). Annotated tags list twice in the
    /// output — the object line and its peeled `^{}` duplicate; the peel
    /// lines are skipped.
    pub fn remote_tags(&self, remote: &str) -> Result<Vec<String>> {
        let out =
            self.run_git(&["ls-remote".to_string(), "--tags".to_string(), remote.to_string()])?;
        let mut names = Vec::new();
        for line in out.lines() {
            let Some((_, name)) = line.split_once('\t') else { continue };
            let Some(tag) = name.strip_prefix("refs/tags/") else { continue };
            if tag.ends_with("^{}") {
                continue;
            }
            names.push(tag.to_owned());
        }
        Ok(names)
    }

    /// Configured remote names, ordered (`origin` first).
    pub fn remotes(&self) -> Result<Vec<String>> {
        let repo = self.git2()?;
        let mut names: Vec<String> = repo
            .remotes()?
            .iter()
            .filter_map(|name| name.ok().flatten().map(|n| n.to_owned()))
            .collect();
        names.sort_by_key(|name| name != "origin");
        Ok(names)
    }

    /// Fetch/push URL of a configured remote (None when unresolvable).
    pub fn remote_url(&self, name: &str) -> Option<String> {
        let repo = self.git2().ok()?;
        let remote = repo.find_remote(name).ok()?;
        remote.url().ok().map(str::to_owned)
    }
}
