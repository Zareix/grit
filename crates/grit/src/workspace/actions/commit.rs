//! Selection & commit actions: file selection (plain / ⌘ / ⇧ clicks),
//! the worktree and commit diff display wiring, and the commit / amend
//! flow.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use grit_core::{Oid, status::FileEntry};

use crate::workspace::Workspace;

impl Workspace {
    /// Re-read the selected file's diff after the status changed; drop the
    /// selection when the file disappeared from both sections. Runs on the
    /// UI thread after a snapshot lands; the diff computation itself is
    /// backgrounded by `request_diff`.
    pub(crate) fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let Some(sel) = self.selected_path.clone() else {
            return;
        };

        // Diff of a file as changed by the selected commit.
        if let Some(commit) = self.selected_commit.clone() {
            if self.commit_files.iter().any(|f| f.path == sel) {
                self.diff_from_commit = true;
                self.selected_staged = false;
                self.request_diff(
                    {
                        let sel = sel.clone();
                        move |repo| repo.commit_file_diff(commit.id, &sel)
                    },
                    cx,
                );
                return;
            }
            // File no longer part of the commit view: clear it.
            self.selected_path = None;
            self.selected_staged = false;
            self.diff_from_commit = false;
            self.hide_diff_panel();
            self.clear_diff(cx);
            return;
        }

        // Worktree diff of a staged/unstaged file.
        let staged = self.status.staged.iter().any(|f| f.path == sel);
        let present = staged || self.status.unstaged.iter().any(|f| f.path == sel);

        if present {
            self.selected_staged = staged;
            self.diff_from_commit = false;
            self.request_diff(
                {
                    let sel = sel.clone();
                    move |repo| {
                        if staged { repo.diff_staged(&sel) } else { repo.diff_unstaged(&sel) }
                    }
                },
                cx,
            );
        } else {
            self.selected_path = None;
            self.selected_staged = false;
            self.diff_from_commit = false;
            self.hide_diff_panel();
            self.clear_diff(cx);
        }
    }

    /// Click on a file row: select it and show its diff. Resets any
    /// multi-selection to this single file.
    pub(crate) fn select_file(&mut self, path: PathBuf, staged: bool, cx: &mut Context<Self>) {
        self.selected_paths = vec![path.clone()];
        self.selection_anchor = Some(path.clone());
        self.show_worktree_diff(path, staged, cx);
    }

    /// Command/Ctrl-click on a file row: toggle the file into the current
    /// selection (same staging side, else starts a fresh single selection
    /// on the clicked side). The diff pane follows the file.
    pub(crate) fn toggle_file_selection(
        &mut self,
        path: PathBuf,
        staged: bool,
        cx: &mut Context<Self>,
    ) {
        if self.selected_staged != staged || self.selected_paths.is_empty() {
            return self.select_file(path, staged, cx);
        }
        if let Some(pos) = self.selected_paths.iter().position(|p| *p == path) {
            self.selected_paths.remove(pos);
            if self.selected_paths.is_empty() {
                self.selected_path = None;
                self.hide_diff_panel();
                self.clear_diff(cx);
            }
        } else {
            self.selected_paths.push(path.clone());
            self.show_worktree_diff(path.clone(), staged, cx);
        }
        self.selection_anchor = Some(path);
        cx.notify();
    }

    /// Shift-click on a file row: extend the selection from the anchor
    /// file (last plain/Command click) up to this one, on the same side.
    /// The anchor stays put, so a further shift-click re-extends it.
    pub(crate) fn extend_file_selection(
        &mut self,
        path: PathBuf,
        staged: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(anchor) =
            self.selection_anchor.clone().filter(|a| self.selected_paths.iter().any(|p| p == a))
        else {
            return self.select_file(path, staged, cx);
        };
        let list: &Vec<FileEntry> =
            if staged { &self.status.staged } else { &self.status.unstaged };
        let (Some(a), Some(c)) = (position_of(list, &anchor), position_of(list, &path)) else {
            return self.select_file(path, staged, cx);
        };
        let (start, end) = (a.min(c), a.max(c) + 1);
        self.selected_paths = list[start..end].iter().map(|e| e.path.clone()).collect();
        self.show_worktree_diff(path, staged, cx);
    }

    /// Show the worktree diff of one file, as the diff pane's selection.
    pub(crate) fn show_worktree_diff(
        &mut self,
        path: PathBuf,
        staged: bool,
        cx: &mut Context<Self>,
    ) {
        self.show_diff_panel(cx);
        self.selected_path = Some(path.clone());
        self.selected_staged = staged;
        self.diff_from_commit = false;
        self.request_diff(
            move |repo| if staged { repo.diff_staged(&path) } else { repo.diff_unstaged(&path) },
            cx,
        );
        cx.notify();
    }

    /// Click on a changed file inside the commit details view: show that
    /// file's diff extracted from the selected commit.
    pub(crate) fn select_commit_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(commit) = self.selected_commit.clone() else { return };
        self.show_diff_panel(cx);
        self.selected_path = Some(path.clone());
        self.selected_staged = false;
        self.diff_from_commit = true;
        self.request_diff(move |repo| repo.commit_file_diff(commit.id, &path), cx);
        cx.notify();
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected_path = None;
        self.selected_staged = false;
        self.diff_from_commit = false;
        self.hide_diff_panel();
        self.clear_diff(cx);
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
                self.hide_diff_panel();
                self.clear_diff(cx);
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
            self.hide_diff_panel();
            self.clear_diff(cx);
            cx.notify();
        }
    }

    pub(crate) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let message = self.message.read(cx).value().trim().to_string();
        if message.is_empty() {
            self.error = Some("Commit message is empty".into());
            cx.notify();
            return;
        }

        let result = if self.amend {
            repo.commit_amend(&message).map(|_| ())
        } else {
            repo.commit(&message).map(|_| ())
        };

        match result {
            Ok(()) => {
                // The amend is done: back to plain-commit mode and empty
                // message box.
                self.amend = false;
                self.message.update(cx, |state, cx| state.set_value("", window, cx));
                self.refresh(cx);
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    /// Toggle the "Amend" checkbox: turning it on prefills the message box
    /// with the HEAD commit's message, turning it off empties it. Only
    /// meaningful when a branch (and thus a HEAD commit) exists.
    pub(crate) fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = !self.amend;
        let message = if self.amend {
            let Some(repo) = self.repo.clone() else { return };
            repo.head_commit().ok().flatten().and_then(|c| c.message.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        self.message.update(cx, |state, cx| state.set_value(message.as_str(), window, cx));
        cx.notify();
    }
}

/// Position of a file path inside a staging list (None when absent).
fn position_of(list: &[FileEntry], path: &Path) -> Option<usize> {
    list.iter().position(|e| e.path == path)
}
