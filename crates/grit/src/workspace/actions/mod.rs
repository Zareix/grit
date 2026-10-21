//! Workspace actions: everything the UI can trigger on the model.
//!
//! Topic submodules hold the action groups (`fs` for staging and file
//! operations, `commit` for the selection + commit machinery, `branches`
//! for checkout / ref creation / stashes, `remote` for network and
//! branch-to-branch operations, `dialogs` for the shared dialogs and
//! launchers); this module keeps the refresh / state plumbing, the shared
//! background git-job runners and the diff panel machinery.

mod branches;
mod commit;
mod dialogs;
mod fs;
mod github;
mod remote;
mod update;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::diff::{DiffFile, DiffMode};
use gpui_kit::*;
use grit_core::{
    Oid, Repo,
    branch::{Branch, BranchType},
    commit::Commit,
    graph::GraphData,
    status::{FileEntry, Status},
    tags::Tag,
    watcher::{RepoWatcher, wait_batch},
};

use super::{CommitRef, CommitRefKind, Stash, Workspace};

/// Commit-log depth loaded on every refresh; also quoted by the
/// "older than window" info message in `reveal_branch`.
pub(crate) const MAX_LOG_COMMITS: usize = 500;

/// Full repository state read in one background pass; `refresh` applies
/// it on the UI thread when ready.
struct RepoSnapshot {
    status: Status,
    branches: Vec<Branch>,
    current_branch: Option<Branch>,
    tags: Vec<Tag>,
    /// Lane layout of the log (see `graph::GraphData`).
    graph: Option<GraphData>,
    log: Vec<Commit>,
    stashes: Vec<Stash>,
    /// Commit of each stash, parallel to `stashes` (`None` when the stash's
    /// commit failed to resolve — the row renders without clickable content).
    stash_commits: Vec<Option<Commit>>,
    /// Ahead/behind counts between all branch tip pairs (`relations[a][b]`
    /// = commits unique to `a`, then commits unique to `b`). Shares a
    /// tip's row when two branches point at the same commit. On read
    /// failure the caller's previous table is kept.
    branch_relations: HashMap<Oid, HashMap<Oid, (usize, usize)>>,
}

impl RepoSnapshot {
    fn read(
        repo: &Repo,
        max_log: usize,
        previous_relations: &HashMap<Oid, HashMap<Oid, (usize, usize)>>,
    ) -> grit_core::Result<Self> {
        let status = repo.status()?;
        let branches = repo.branches()?;
        let current_branch = repo.current_branch()?;
        let tags = repo.tags()?;
        let log = repo.commit_log_all(max_log)?;
        let graph = Some(GraphData::from_commits(&log));
        let stashes = repo.stashes()?;
        // Each stash is a commit: resolve it for the history rows (its diff
        // against its first parent is the stash's content). A stash whose
        // commit fails to resolve keeps `None` and renders without content.
        let stash_commits = stashes.iter().map(|stash| repo.find_commit(stash.oid).ok()).collect();

        let mut tips: Vec<Oid> = branches.iter().map(|b| b.head).collect();
        tips.sort_unstable();
        tips.dedup();
        let branch_relations =
            repo.ahead_behind_all(&tips).unwrap_or_else(|_| previous_relations.clone());

        Ok(Self {
            status,
            branches,
            current_branch,
            tags,
            graph,
            log,
            stashes,
            stash_commits,
            branch_relations,
        })
    }

    /// Whether this snapshot would change any rendered field. A snapshot
    /// that matches the state already on screen is dropped wholesale: no
    /// redraw, no derived rebuilds, no selected-file diff re-read.
    fn same_state(&self, ws: &Workspace) -> bool {
        // `tags` is sorted by date on apply; compare the sorted form.
        let mut tags = self.tags.clone();
        tags.sort_by_key(|t| std::cmp::Reverse(t.date));
        tags == ws.tags
            && self.status == ws.status
            && self.branches == ws.branches
            && self.current_branch == ws.current_branch
            && self.graph == ws.graph
            && self.log == ws.log
            && self.stashes == ws.stashes
            && self.stash_commits == ws.stash_commits
            && self.branch_relations == ws.branch_relations
    }
}

impl Workspace {
    pub(crate) fn open(&mut self, path: String, cx: &mut Context<Self>) {
        match Repo::open(&path) {
            Ok(repo) => {
                let repo_path = repo.path.clone();
                self.error = None;
                self.repo = Some(repo);
                // Watch the repo (worktree + .git) so status changes made
                // outside Grit feed refreshes; falls back to polling only.
                self.watcher = RepoWatcher::new(&repo_path).ok();
                // The auto-refresh loop lives as long as this Workspace.
                self.start_auto_refresh(cx);
                self.refresh(cx);
                // OAuth bootstrap: restore the Keychain token in the
                // background (-signed-in state on a later tick).
                self.init_github_auth(cx);
            }
            Err(e) => {
                self.repo = None;
                self.watcher = None;
                self.error = Some(format!("Failed to open \"{path}\": {e}").into());
                cx.notify();
            }
        }
    }

    /// Start the auto-refresh timer loop — one per Workspace, idempotent.
    /// One-second ticks: every tick reads the Settings controls live (so a
    /// change in the Settings window applies at once) and runs whatever is
    /// due: periodic refresh, auto-fetch, watcher arming.
    pub(crate) fn start_auto_refresh(&mut self, cx: &mut Context<Self>) {
        if self.auto_refresh_started {
            return;
        }
        self.auto_refresh_started = true;
        // Cloned out of `cx`: the closure must own it ('static lifetime).
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(Duration::from_secs(1)).await;
                // An update failure means the Workspace was dropped: end the
                // loop (the watcher ends with it, so its waiter does too).
                let alive = this.update(cx, |this, cx| this.auto_refresh_tick(cx)).is_ok();
                if !alive {
                    return;
                }
            }
        })
        .detach();
    }

    /// One auto-refresh tick (1 s cadence).
    fn auto_refresh_tick(&mut self, cx: &mut Context<Self>) {
        if !crate::settings::auto_refresh_enabled(cx) {
            return;
        }
        let now = Instant::now();
        if self.last_auto_refresh.is_none_or(|t| {
            now.duration_since(t) >= Duration::from_secs(crate::settings::auto_refresh_interval(cx))
        }) {
            self.last_auto_refresh = Some(now);
            self.refresh(cx);
        }
        self.maybe_auto_fetch(now, cx);
        self.service_watcher(cx);
    }

    /// Run the background `git fetch` when its interval is due. Skipped
    /// entirely for repos without remotes (`git fetch` would just fail) and
    /// while a manual remote op is running. A failed attempt still counts:
    /// a dead remote (VPN off) must not cause a reconnect storm.
    pub(crate) fn maybe_auto_fetch(&mut self, now: Instant, cx: &mut Context<Self>) {
        let fetch_interval = crate::settings::auto_fetch_interval(cx);
        if self.remote_busy.is_some() || fetch_interval == 0 {
            return;
        }
        if self
            .last_fetch
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(fetch_interval))
        {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        if repo.remotes().map(|r| r.is_empty()).unwrap_or(true) {
            return;
        }
        self.last_fetch = Some(now);
        self.silent_fetch(cx);
    }

    /// Network fetch that never touches the status bar: no busy label, no
    /// info/error message; only the following refresh updates the UI.
    fn silent_fetch(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let job = cx.background_spawn(async move { repo.run_git(&["fetch".to_string()]) });
        cx.spawn(async move |this, cx| {
            // Silently ignored: success refreshes through the snapshot, a
            // failure (offline, locked worktree) retries on a later fetch.
            let _ = job.await;
            this.update(cx, |this, cx| this.refresh(cx)).ok();
        })
        .detach();
    }

    /// The window regained focus: refresh at once (cheap, local), fetch
    /// only when its interval is due — see `maybe_auto_fetch`. Both are
    /// part of auto-refresh, so they honor the Settings toggle too.
    pub(crate) fn on_window_activated(&mut self, cx: &mut Context<Self>) {
        if self.repo.is_none() || !crate::settings::auto_refresh_enabled(cx) {
            return;
        }
        let now = Instant::now();
        self.last_auto_refresh = Some(now);
        self.refresh(cx);
        self.maybe_auto_fetch(now, cx);
    }

    /// Arm one watcher wait chain (a single one at a time): a background
    /// task blocks until the next on-disk change batch that git considers
    /// a status change, then the UI refreshes and the next tick re-arms the
    /// chain. Gitignored files / build output are filtered inside
    /// `wait_batch`. When the Workspace closes, its watcher drops and the
    /// parked task reports `Closed` — the watcher is then released.
    fn service_watcher(&mut self, cx: &mut Context<Self>) {
        if self.watcher_armed {
            return;
        }
        let Some(events) = self.watcher.as_ref().map(|w| w.events.clone()) else { return };
        let Some(repo) = self.repo.clone() else { return };
        self.watcher_armed = true;
        let job = cx.background_spawn(async move { wait_batch(&events, &repo) });
        cx.spawn(async move |this, cx| {
            let batch = job.await;
            this.update(cx, |this, cx| {
                if batch == grit_core::watcher::WatchBatch::Closed {
                    // Channel disconnected (watcher dropped): stop re-arming.
                    this.watcher = None;
                }
                this.watcher_armed = false;
                if batch == grit_core::watcher::WatchBatch::Changed {
                    // The fs watcher effectively IS a refresh trigger; count
                    // it against the periodic interval to avoid a double
                    // reload.
                    this.last_auto_refresh = Some(Instant::now());
                    this.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reload status, branches and commit log from disk. The read runs on
    /// the background executor (it walks the worktree and the ODB); the
    /// snapshot applies on the UI thread when ready. A generation counter
    /// drops snapshots of refreshes superseded while in flight.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        self.refresh_seq += 1;
        let seq = self.refresh_seq;
        let previous_relations = self.branch_relations.clone();

        let job = cx.background_spawn(async move {
            RepoSnapshot::read(&repo, MAX_LOG_COMMITS, &previous_relations)
        });
        cx.spawn(async move |this, cx| {
            let snapshot = job.await;
            this.update(cx, |this, cx| {
                match snapshot {
                    Ok(snapshot) if seq == this.refresh_seq => {
                        // An unchanged snapshot re-renders nothing.
                        if this.apply_snapshot(snapshot, cx) {
                            cx.notify();
                        }
                    }
                    Err(_) if seq == this.refresh_seq => {
                        this.error = Some("Failed to read repository state".into());
                        cx.notify();
                    }
                    _ => {} // stale snapshot: a newer refresh superseded it
                }
            })
            .ok();
        })
        .detach();
    }

    /// Apply a freshly read snapshot: cached fields, then the derived
    /// state, then the selection sync against the new status. Returns
    /// whether any rendered field changed; callers skip their redraw when
    /// nothing did (a routine auto-refresh on an idle repo re-renders
    /// nothing at all).
    fn apply_snapshot(&mut self, snapshot: RepoSnapshot, cx: &mut Context<Self>) -> bool {
        if snapshot.same_state(self) {
            // Still re-read the selected file's diff in the background: an
            // untracked file can change content while keeping its status
            // bit, and an open commit-file diff is cheap to re-check. The
            // patch-identity check keeps an unchanged diff from redrawing.
            self.sync_selection(cx);
            return false;
        }
        self.status = snapshot.status;
        self.branches = snapshot.branches;
        self.current_branch = snapshot.current_branch;
        // Most-recent-first drives the sidebar listing and the chip order
        // (stable sort: the sidebar also wants alphabetical on ties).
        let mut tags = snapshot.tags;
        tags.sort_by_key(|t| std::cmp::Reverse(t.date));
        self.tags = tags;
        self.graph = snapshot.graph;
        self.log = snapshot.log;
        self.stashes = snapshot.stashes;
        self.stash_commits = snapshot.stash_commits;
        self.branch_relations = snapshot.branch_relations;
        self.rebuild_refs();
        self.prune_selection();
        self.sync_remote_tree(cx);
        self.sync_remote_tags(cx);
        self.error = None;
        self.sync_selection(cx);
        true
    }

    /// Drop the selection's files that left the selected staging side:
    /// staging/unstaging rewrites both lists and stale paths must not stay
    /// highlighted. `selected_path` is cleaned by `sync_selection` itself.
    fn prune_selection(&mut self) {
        let side: &Vec<FileEntry> =
            if self.selected_staged { &self.status.staged } else { &self.status.unstaged };
        self.selected_paths.retain(|p| side.iter().any(|e| e.path == *p));
        if let Some(anchor) = &self.selection_anchor
            && !self.selected_paths.iter().any(|p| p == anchor)
        {
            self.selection_anchor = None;
        }
    }

    /// Push the current remote branches into the tree state as a GitKraken-
    /// style hierarchy (remote root → folders per `/` segment → leaves), and
    /// prune collapse records for folders that no longer exist.
    fn sync_remote_tree(&mut self, cx: &mut Context<Self>) {
        let items = super::components::branches::remote_tree_items(self);
        super::components::branches::prune_collapsed(items.iter(), &mut self.remote_collapsed);
        self.remote_tree.update(cx, |state, cx| state.set_items(items, cx));
    }

    /// Regroup branches + tags by the commit they point at, for the log's
    /// ref chips. Per commit: local branches first, then tags, then remotes.
    pub(crate) fn rebuild_refs(&mut self) {
        self.refs_by_commit.clear();
        let current = self.current_branch.as_ref().map(|b| b.name.clone());

        for branch in &self.branches {
            let kind = match branch.kind {
                BranchType::Local => CommitRefKind::Local,
                BranchType::Remote => CommitRefKind::Remote,
            };
            self.refs_by_commit.entry(branch.head).or_default().push(CommitRef {
                name: branch.name.clone().into(),
                kind,
                is_current: current.as_deref() == Some(branch.name.as_str()),
            });
        }
        for tag in &self.tags {
            self.refs_by_commit.entry(tag.target).or_default().push(CommitRef {
                name: tag.name.clone().into(),
                kind: CommitRefKind::Tag,
                is_current: false,
            });
        }
        for refs in self.refs_by_commit.values_mut() {
            refs.sort_by_key(|r| match r.kind {
                CommitRefKind::Local => 0,
                CommitRefKind::Tag => 1,
                CommitRefKind::Remote => 2,
            });
        }
    }

    /// Click on a branch: scroll the commit log to its tip commit. The log
    /// covers every ref, so tips are normally in it; an older-than-window
    /// tip (beyond `MAX_LOG_COMMITS`) surfaces an info message instead.
    pub(crate) fn reveal_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(tip) = self.branches.iter().find(|b| b.name == name).map(|b| b.head) else {
            return;
        };
        self.reveal_commit(name, tip, cx);
    }

    /// Scroll the log to the commit the named tag points at.
    pub(crate) fn reveal_tag(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(target) = self.tags.iter().find(|t| t.name == name).map(|t| t.target) else {
            return;
        };
        self.reveal_commit(name, target, cx);
    }

    /// Shared scroll-to-oid with the "older than window" note.
    fn reveal_commit(&mut self, name: &str, oid: Oid, cx: &mut Context<Self>) {
        match self.log.iter().position(|c| c.id == oid) {
            Some(ix) => {
                self.log_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                cx.notify();
            }
            None => {
                self.info = Some(
                    format!(
                        "\"{name}\" is older than the {MAX_LOG_COMMITS} \
                                          commits shown"
                    )
                    .into(),
                );
                cx.notify();
            }
        }
    }

    /// Run a remote operation through the git binary on the background
    /// thread (network + credentials), then refresh the UI.
    pub(crate) fn run_git_remote(
        &mut self,
        label: &'static str,
        args: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.remote_busy.is_some() {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };

        self.remote_busy = Some(label);
        self.info = None;
        self.error = None;
        cx.notify();

        let job = cx.background_spawn(async move { repo.run_git(&args) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                this.remote_busy = None;
                match &result {
                    Ok(out) => {
                        this.info = Some(if out.is_empty() {
                            format!("{label}: ok").into()
                        } else {
                            format!("{label}: {out}").into()
                        });
                        this.refresh(cx);
                    }
                    Err(e) => {
                        this.error = Some(e.to_string().into());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Run a short sequence of git commands on the background thread,
    /// stopping at the first failure; refreshes the UI afterwards either
    /// way (a failed step often leaves repo state worth showing).
    pub(crate) fn run_git_steps(
        &mut self,
        label: &'static str,
        steps: Vec<Vec<String>>,
        cx: &mut Context<Self>,
    ) {
        if self.remote_busy.is_some() {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };

        self.remote_busy = Some(label);
        self.info = None;
        self.error = None;
        cx.notify();

        let job = cx.background_spawn(async move {
            let mut out = String::new();
            for step in &steps {
                let text = repo.run_git(step)?;
                out.push_str(&text);
            }
            Ok(out) as Result<String, grit_core::error::Error>
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                this.remote_busy = None;
                match &result {
                    Ok(out) => {
                        this.info = Some(if out.is_empty() {
                            format!("{label}: ok").into()
                        } else {
                            format!("{label}: {out}").into()
                        });
                        this.refresh(cx);
                    }
                    Err(e) => {
                        this.error = Some(e.to_string().into());
                        this.refresh(cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Shared completion handling for launcher jobs: busy flag + status bar
    /// message (error surfaces the failed command), no repo refresh.
    pub(crate) fn run_launcher(
        &mut self,
        label: &'static str,
        job: Task<Result<(), String>>,
        cx: &mut Context<Self>,
    ) {
        if self.remote_busy.is_some() {
            return;
        }
        self.remote_busy = Some(label);
        self.info = None;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                this.remote_busy = None;
                match result {
                    Ok(()) => {
                        this.info = Some(format!("{label}: ok").into());
                        cx.notify();
                    }
                    Err(e) => {
                        this.error = Some(e.into());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn show_diff_panel(&mut self, cx: &mut Context<Self>) {
        // First open of the panel in this session: while the panel was
        // hidden, the layout state adopted the history panel at the whole
        // container height. Restart its panel record so the opening frame
        // adopts [history, diff] consistently instead of rescaling the
        // stale record (which would shrink the fresh default size).
        if !self.diff_open {
            self.diff_open = true;
            self.center_layout.update(cx, |state, _| state.clear());
        }
    }

    pub(crate) fn hide_diff_panel(&mut self) {
        if self.diff_open {
            self.diff_open = false;
        }
    }

    /// Compute a patch off the UI thread — git read *and* `DiffFile::parse`
    /// — and install it in the diff viewer when it lands. Both steps can
    /// take tens of milliseconds on real patches; a stale result (the
    /// selection or the diff target changed meanwhile) is dropped. An
    /// identical patch (the auto-refresh re-reads the selected file every
    /// interval) is skipped: `DiffState::set_files` resets scroll,
    /// expansion and selection, so re-pushing the same content would jump
    /// the view back to the top on every refresh.
    pub(crate) fn request_diff(
        &mut self,
        compute: impl FnOnce(&Repo) -> grit_core::Result<String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repo.clone() else { return };
        self.diff_seq += 1;
        let seq = self.diff_seq;
        let job = cx.background_spawn(async move {
            compute(&repo).map(|patch| match DiffFile::parse(&patch) {
                Ok(files) => Ok((patch, files)),
                // `DiffFile::parse` has its own error type; the landing
                // side only displays it.
                Err(e) => Err(e.to_string()),
            })
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                if seq != this.diff_seq {
                    return; // superseded request
                }
                match result {
                    Ok(Ok((patch, files))) => {
                        if this.diff_patch.as_deref() != Some(patch.as_str()) {
                            this.diff.update(cx, |state, cx| state.set_files(files, cx));
                            this.diff_patch = Some(patch);
                            cx.notify();
                        }
                    }
                    Err(e) => {
                        this.error = Some(e.to_string().into());
                        this.clear_diff(cx);
                        cx.notify();
                    }
                    Ok(Err(msg)) => {
                        this.error = Some(msg.into());
                        this.clear_diff(cx);
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Show the empty diff viewer (no file selected). Bumps the diff
    /// generation so an in-flight patch cannot land into a cleared viewer.
    pub(crate) fn clear_diff(&mut self, cx: &mut Context<Self>) {
        self.diff_seq += 1;
        self.diff_patch = None;
        self.diff.update(cx, |state, cx| state.set_files([], cx));
        cx.notify();
    }

    /// Toggle the diff viewer between unified and side-by-side layout.
    pub(crate) fn toggle_diff_mode(&mut self, cx: &mut Context<Self>) {
        self.diff.update(cx, |state, cx| {
            let next =
                if state.mode() == DiffMode::Split { DiffMode::Unified } else { DiffMode::Split };
            state.set_mode(next, cx);
        });
    }
}
