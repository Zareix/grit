//! GitHub features for the open repository: resolve the GitHub coordinates
//! from the remotes, keep the open-PR list cached, open web pages.
//!
//! The sign-in session is the app-level global (`crate::github`); this
//! module only ever reads its token. All network runs on the background
//! executor like the git remote operations, invalidated by the same-style
//! generation counters (`github.seq`).

use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::workspace::GitHubState;

use super::Workspace;

/// Interval between successful open-PR lookups (the list is not urgent).
const PR_INTERVAL: Duration = Duration::from_secs(120);
/// Sooner retry after a failed lookup (offline, API hiccup).
const PR_RETRY: Duration = Duration::from_secs(60);

impl Workspace {
    /// Discover the GitHub coordinates of the remotes on repo open
    /// (origin first) and clear the previous repo's PR cache. The
    /// generation counter advances before the reset: lookups or resolution
    /// landing from the previous repo are dropped.
    pub(crate) fn init_github(&mut self, cx: &mut Context<Self>) {
        // Advance the counter across the reset so landings from the
        // previous repo stay stale forever.
        let seq = self.github.seq.wrapping_add(1);
        self.github = GitHubState::default();
        self.github.seq = seq;

        let Some(repo) = self.repo.clone() else { return };
        let job =
            cx.background_spawn(async move { grit_core::github::resolve_github_coords(&repo) });
        cx.spawn(async move |this, cx| {
            let found = job.await;
            this.update(cx, |this, cx| {
                if seq == this.github.seq
                    && let Some((remote, coords)) = found
                {
                    this.github.remote = Some(remote);
                    this.github.coords = Some(coords);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Run the background open-PR lookup when its throttle allows. The
    /// cadence follows the last result: successes re-check at
    /// [`PR_INTERVAL`], failures retry at [`PR_RETRY`].
    pub(crate) fn maybe_refresh_github(&mut self, now: Instant, cx: &mut Context<Self>) {
        if !crate::github::signed_in(cx) {
            return;
        }
        let Some(_) = self.github.coords else { return };
        let delay = match (self.github.last_pr_success, self.github.last_pr_attempt) {
            (Some(s), Some(a)) if s >= a => PR_INTERVAL,
            (_, Some(_)) => PR_RETRY, // failures (and never-attempted) go now
            _ => Duration::ZERO,
        };
        if self.github.last_pr_attempt.is_some_and(|t| now.duration_since(t) < delay) {
            return;
        }
        self.refresh_github_prs(cx);
    }

    /// Background open-PR lookup (one API call) into `github.prs`. Errors
    /// stay silent: the throttle makes the next tick retry, and the PR
    /// data is decorative.
    pub(crate) fn refresh_github_prs(&mut self, cx: &mut Context<Self>) {
        let Some(coords) = self.github.coords.clone() else { return };
        let Some(token) = crate::github::token(cx) else { return };
        self.github.seq += 1;
        let seq = self.github.seq;
        self.github.last_pr_attempt = Some(Instant::now());
        let job =
            cx.background_spawn(
                async move { grit_core::github::open_pull_requests(&coords, &token) },
            );
        cx.spawn(async move |this, cx| {
            let fetched = job.await;
            this.update(cx, |this, cx| {
                if seq != this.github.seq {
                    return; // superseded (repo switch, newer lookup)
                }
                if let Ok(prs) = fetched {
                    this.github.last_pr_success = Some(Instant::now());
                    if this.github.prs != prs {
                        this.github.prs = prs;
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Open a GitHub web page through the shared launcher machinery
    /// (status-bar messaging, no repo refresh, brief busy flag).
    pub(crate) fn open_github_url(&mut self, url: String, cx: &mut Context<Self>) {
        let job = cx.background_spawn(async move {
            match std::process::Command::new("open").arg(&url).status() {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("open failed with {status}")),
                Err(e) => Err(e.to_string()),
            }
        });
        self.run_launcher("open GitHub", job, cx);
    }
}
