//! Workspace actions: everything the UI can trigger on the model.

use std::path::PathBuf;

use gk_core::{Oid, RemoteOp, Repo};
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    v_flex,
};
use gpui_kit::*;

use super::Workspace;

const MAX_LOG_COMMITS: usize = 500;

impl Workspace {
    pub(crate) fn open(&mut self, path: String, cx: &mut Context<Self>) {
        match Repo::open(&path) {
            Ok(repo) => {
                self.error = None;
                self.repo = Some(repo);
                self.refresh(cx);
            }
            Err(e) => {
                self.repo = None;
                self.error = Some(format!("Failed to open \"{path}\": {e}").into());
                cx.notify();
            }
        }
    }

    /// Reload status, branches and commit log from disk.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };

        match (
            repo.status(),
            repo.branches(),
            repo.current_branch(),
            repo.commit_log(MAX_LOG_COMMITS),
        ) {
            (Ok(status), Ok(branches), Ok(current), Ok(log)) => {
                self.status = status;
                self.branches = branches;
                self.current_branch = current;
                self.log = log;
                self.error = None;
                self.sync_viewed_branch(&repo);
                self.sync_selection(&repo);
            }
            _ => {
                self.error = Some("Failed to read repository state".into());
            }
        }
        cx.notify();
    }

    /// Re-read the branch log of the viewed branch; stop viewing branches
    /// that no longer exist.
    pub(crate) fn sync_viewed_branch(&mut self, repo: &Repo) {
        let Some(name) = self.viewed_branch.clone() else {
            return;
        };
        match repo.commit_log_ref(&name, MAX_LOG_COMMITS) {
            Ok(log) => self.branch_log = log,
            Err(_) => {
                self.viewed_branch = None;
                self.branch_log.clear();
            }
        }
    }

    /// Click on a branch: show its history without checking it out. Clicking
    /// the current branch (or the already-viewed one) goes back to the
    /// default view of the current branch's history.
    pub(crate) fn view_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let is_current = self.current_branch.as_ref().is_some_and(|c| c.name == name);
        if is_current || self.viewed_branch.as_deref() == Some(name) {
            self.view_branch_current(cx);
        } else if let Some(repo) = self.repo.clone() {
            self.viewed_branch = Some(name.to_string().into());
            self.branch_log =
                repo.commit_log_ref(name, MAX_LOG_COMMITS).unwrap_or_else(|_| self.log.clone());
        }
        cx.notify();
    }

    /// Return the center column to the current branch's history (HEAD).
    pub(crate) fn view_branch_current(&mut self, cx: &mut Context<Self>) {
        if self.viewed_branch.take().is_some() {
            cx.notify();
        }
    }

    /// Re-read the selected file's diff after the status changed; drop the
    /// selection when the file disappeared from both sections.
    pub(crate) fn sync_selection(&mut self, repo: &Repo) {
        let Some(sel) = self.selected_path.clone() else {
            return;
        };

        // Diff of a file as changed by the selected commit.
        if let Some(commit) = self.selected_commit.clone() {
            if self.commit_files.iter().any(|f| f.path == sel) {
                self.diff_from_commit = true;
                self.selected_staged = false;
                let result = repo.commit_file_diff(commit.id, &sel);
                self.set_diff(result);
                return;
            }
            // File no longer part of the commit view: clear it.
            self.selected_path = None;
            self.selected_staged = false;
            self.diff_from_commit = false;
            self.diff_rows.clear();
            return;
        }

        // Worktree diff of a staged/unstaged file.
        let staged = self.status.staged.iter().any(|f| f.path == sel);
        let present = staged || self.status.unstaged.iter().any(|f| f.path == sel);

        if present {
            self.selected_staged = staged;
            self.diff_from_commit = false;
            let result = if staged { repo.diff_staged(&sel) } else { repo.diff_unstaged(&sel) };
            self.set_diff(result);
        } else {
            self.selected_path = None;
            self.selected_staged = false;
            self.diff_from_commit = false;
            self.diff_rows.clear();
        }
    }

    /// Click on a file row: select it and show its diff.
    pub(crate) fn select_file(&mut self, path: PathBuf, staged: bool, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        self.selected_path = Some(path.clone());
        self.selected_staged = staged;
        self.diff_from_commit = false;
        let result = if staged { repo.diff_staged(&path) } else { repo.diff_unstaged(&path) };
        self.set_diff(result);
        cx.notify();
    }

    /// Click on a changed file inside the commit details view: show that
    /// file's diff extracted from the selected commit.
    pub(crate) fn select_commit_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(commit) = self.selected_commit.clone() else { return };
        let Some(repo) = self.repo.clone() else { return };
        self.selected_path = Some(path.clone());
        self.selected_staged = false;
        self.diff_from_commit = true;
        let result = repo.commit_file_diff(commit.id, &path);
        self.set_diff(result);
        cx.notify();
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected_path = None;
        self.selected_staged = false;
        self.diff_from_commit = false;
        self.diff_rows.clear();
        cx.notify();
    }

    /// Click on a history row: show the commit's details on the right,
    /// replacing the staging panel. Click on the WIP row or the ✕ goes back.
    pub(crate) fn select_commit(&mut self, oid: Oid, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let commit = match repo.find_commit(oid) {
            Ok(commit) => commit,
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
                return;
            }
        };
        match repo.commit_changed_files(oid) {
            Ok(files) => {
                self.selected_commit = Some(commit);
                self.commit_files = files;
                self.selected_path = None;
                self.selected_staged = false;
                self.diff_from_commit = false;
                self.diff_rows.clear();
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
            }
        }
        cx.notify();
    }

    /// Back from the commit details view to the staging panel.
    pub(crate) fn close_commit(&mut self, cx: &mut Context<Self>) {
        if self.selected_commit.take().is_some() {
            self.commit_files.clear();
            self.selected_path = None;
            self.selected_staged = false;
            self.diff_from_commit = false;
            self.diff_rows.clear();
            cx.notify();
        }
    }

    /// Click on a staged file unstages it; on an unstaged file stages it.
    pub(crate) fn toggle_file(&mut self, path: PathBuf, staged: bool, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let result = if staged { repo.unstage_file(&path) } else { repo.stage_file(&path) };
        if let Err(e) = result {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    pub(crate) fn stage_all(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = repo.stage_all() {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    pub(crate) fn unstage_all(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = repo.unstage_all() {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    pub(crate) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let message = self.message.read(cx).value().trim().to_string();
        if message.is_empty() {
            self.error = Some("Commit message is empty".into());
            cx.notify();
            return;
        }

        match repo.commit(&message) {
            Ok(_) => {
                self.message.update(cx, |state, cx| state.set_value("", window, cx));
                self.refresh(cx);
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    pub(crate) fn checkout_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.checkout_branch(name) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    /// Create a local branch at HEAD from the inline input (Enter or + button).
    pub(crate) fn create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let name = self.branch_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.error = Some("Branch name is empty".into());
            cx.notify();
            return;
        }

        match repo.create_branch(&name) {
            Ok(()) => {
                self.creating_branch = false;
                self.branch_input.update(cx, |state, cx| state.set_value("", window, cx));
                self.refresh(cx);
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    pub(crate) fn delete_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.delete_branch(name) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    pub(crate) fn toggle_create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating_branch = !self.creating_branch;
        if !self.creating_branch {
            self.branch_input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    /// Run a remote operation on a background thread (network + credentials),
    /// then refresh the UI.
    pub(crate) fn run_remote(&mut self, op: gk_core::remote::RemoteOp, cx: &mut Context<Self>) {
        if self.remote_busy.is_some() {
            return;
        }
        let args = [op.name().to_string()];
        self.run_git_remote(op.name(), args.to_vec(), cx);
    }

    /// Push, prompting to create the upstream branch first.
    /// User-requested push; opens the set-upstream dialog when the current
    /// branch does not track a remote yet.
    pub(crate) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_busy.is_some() {
            return;
        }
        let branch = match &self.current_branch {
            Some(branch) => branch.clone(),
            None => {
                self.error = Some("Cannot push: no current branch".into());
                cx.notify();
                return;
            }
        };

        if branch.upstream.is_none() {
            self.open_upstream_dialog(&branch.name, window, cx);
            return;
        }

        self.run_remote(RemoteOp::Push, cx);
    }

    /// Dialog asking to publish a branch as `<remote>/<branch>` and set it as
    /// upstream (`push --set-upstream`).
    pub(crate) fn open_upstream_dialog(
        &mut self,
        branch_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remotes = match self.repo.as_ref().map(gk_core::Repo::remotes) {
            Some(Ok(remotes)) if !remotes.is_empty() => remotes,
            _ => {
                self.error = Some("Cannot set upstream: no remote configured".into());
                cx.notify();
                return;
            }
        };
        let remote = remotes.first().cloned().expect("non-empty remotes");
        let branch_name = branch_name.to_string();
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            dialog
                .title("Set upstream branch?")
                .child(
                    v_flex()
                        .gap_2()
                        .child(format!("\"{}\" does not track a remote branch yet.", branch_name))
                        .child(div().text_color(dialog_cx.theme().muted_foreground).child(
                            format!("Publish it as {remote}/{branch_name} and set it as upstream?"),
                        )),
                )
                // 0.7.1: footer takes a value; the styled OK/Cancel buttons from
                // button_props are only rendered by AlertDialog — build ours.
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm")
                                .primary()
                                .label("Push and set upstream")
                                .on_click({
                                    let weak = weak.clone();
                                    let remote = remote.clone();
                                    let branch_name = branch_name.clone();
                                    move |_, window, cx| {
                                        window.close_dialog(cx);
                                        weak.update(cx, |ws, cx| {
                                            ws.set_upstream_and_push(
                                                remote.clone(),
                                                branch_name.clone(),
                                                cx,
                                            )
                                        })
                                        .ok();
                                    }
                                }),
                        ),
                )
        });
    }

    /// Created by the set-upstream dialog: push with `--set-upstream`.
    pub(crate) fn set_upstream_and_push(
        &mut self,
        remote: String,
        branch: String,
        cx: &mut Context<Self>,
    ) {
        let args = vec!["push".to_string(), "--set-upstream".to_string(), remote, branch];
        self.run_git_remote("push", args, cx);
    }

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
}

impl Workspace {
    /// Store a freshly produced patch as pre-parsed render rows.
    pub(crate) fn set_diff(&mut self, result: gk_core::Result<String>) {
        self.diff_rows = match result {
            Ok(patch) => super::components::diff_view::parse_patch(&patch),
            Err(e) => {
                self.error = Some(e.to_string().into());
                Vec::new()
            }
        };
    }
}
