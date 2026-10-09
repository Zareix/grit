//! The workspace: application state, Git actions, and top-level layout.
//!
//! The layout pieces live in [`components`]; this module owns the state.

mod components;

use std::path::PathBuf;

use gk_core::{Repo, branch::Branch, commit::Commit, status::Status};
use gpui_kit::component::{
    ActiveTheme as _, ResizableState, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_resizable,
    input::{InputEvent, InputState},
    resizable_panel, v_flex, v_resizable,
};
use gpui_kit::*;

use self::components::{branches, diff_view, history, staging, status_bar, toolbar};

const MAX_LOG_COMMITS: usize = 500;

pub struct Workspace {
    repo: Option<Repo>,
    /// Source of truth for counts / commit button state.
    status: Status,
    message: Entity<InputState>,
    /// Inline "new branch" input, toggled by the + button.
    branch_input: Entity<InputState>,
    creating_branch: bool,
    /// Currently selected file (shown in the diff view).
    selected_path: Option<PathBuf>,
    /// Whether the selected file's diff is the staged one (HEAD -> index).
    selected_staged: bool,
    /// Unified patch text of the selected file.
    diff_text: String,
    log: Vec<Commit>,
    branches: Vec<Branch>,
    current_branch: Option<Branch>,
    /// Horizontal panel layout: branches | center | staging.
    layout: Entity<ResizableState>,
    /// Vertical split of the center column: history | diff.
    center_layout: Entity<ResizableState>,
    /// Label of the remote operation currently running, if any.
    remote_busy: Option<&'static str>,
    /// Transient success/info message for the status bar.
    info: Option<SharedString>,
    /// Branch whose history is shown in the center column (None = HEAD).
    viewed_branch: Option<SharedString>,
    /// Log of `viewed_branch`, refreshed along with `status`.
    branch_log: Vec<Commit>,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(path: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx
            .new(|cx| InputState::new(window, cx).placeholder("Commit message…").clean_on_escape());

        let branch_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("New branch name…").clean_on_escape()
        });
        let enter_subscription =
            cx.subscribe_in(&branch_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.create_branch(window, cx);
                }
            });

        let layout = cx.new(|_| ResizableState::default());
        let center_layout = cx.new(|_| ResizableState::default());

        let mut this = Self {
            repo: None,
            status: Status::default(),
            message,
            branch_input,
            creating_branch: false,
            selected_path: None,
            selected_staged: false,
            diff_text: String::new(),
            log: Vec::new(),
            branches: Vec::new(),
            current_branch: None,
            layout,
            center_layout,
            remote_busy: None,
            info: None,
            viewed_branch: None,
            branch_log: Vec::new(),
            error: None,
            _subscriptions: vec![enter_subscription],
        };
        this.open(path, cx);
        this
    }

    fn open(&mut self, path: String, cx: &mut Context<Self>) {
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
    fn sync_viewed_branch(&mut self, repo: &Repo) {
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
    /// the same branch again goes back to the current branch's history.
    fn view_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.viewed_branch.as_deref() == Some(name) {
            self.view_branch_current(cx);
        } else if let Some(repo) = self.repo.clone() {
            self.viewed_branch = Some(name.to_string().into());
            self.branch_log =
                repo.commit_log_ref(name, MAX_LOG_COMMITS).unwrap_or_else(|_| self.log.clone());
        }
        cx.notify();
    }

    /// Return the center column to the current branch's history (HEAD).
    fn view_branch_current(&mut self, cx: &mut Context<Self>) {
        if self.viewed_branch.take().is_some() {
            cx.notify();
        }
    }

    /// Re-read the selected file's diff after the status changed; drop the
    /// selection when the file disappeared from both sections.
    fn sync_selection(&mut self, repo: &Repo) {
        let Some(sel) = self.selected_path.clone() else {
            return;
        };
        let staged = self.status.staged.iter().any(|f| f.path == sel);
        let present = staged || self.status.unstaged.iter().any(|f| f.path == sel);

        if present {
            self.selected_staged = staged;
            let result = if staged { repo.diff_staged(&sel) } else { repo.diff_unstaged(&sel) };
            self.diff_text = result.unwrap_or_else(|e| {
                self.error = Some(e.to_string().into());
                String::new()
            });
        } else {
            self.selected_path = None;
            self.selected_staged = false;
            self.diff_text.clear();
        }
    }

    /// Click on a file row: select it and show its diff.
    fn select_file(&mut self, path: PathBuf, staged: bool, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        self.selected_path = Some(path.clone());
        self.selected_staged = staged;
        let result = if staged { repo.diff_staged(&path) } else { repo.diff_unstaged(&path) };
        self.diff_text = result.unwrap_or_else(|e| {
            self.error = Some(e.to_string().into());
            String::new()
        });
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected_path = None;
        self.selected_staged = false;
        self.diff_text.clear();
        cx.notify();
    }

    /// Click on a staged file unstages it; on an unstaged file stages it.
    fn toggle_file(&mut self, path: PathBuf, staged: bool, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let result = if staged { repo.unstage_file(&path) } else { repo.stage_file(&path) };
        if let Err(e) = result {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    fn stage_all(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = repo.stage_all() {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    fn unstage_all(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = repo.unstage_all() {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn checkout_branch(&mut self, name: &str, cx: &mut Context<Self>) {
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
    fn create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn delete_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.delete_branch(name) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    fn toggle_create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating_branch = !self.creating_branch;
        if !self.creating_branch {
            self.branch_input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    /// Run a remote operation on a background thread (network + credentials),
    /// then refresh the UI.
    fn run_remote(&mut self, op: gk_core::remote::RemoteOp, cx: &mut Context<Self>) {
        if self.remote_busy.is_some() {
            return;
        }
        let args = [op.name().to_string()];
        self.run_git_remote(op.name(), args.to_vec(), cx);
    }

    /// Push, prompting to create the upstream branch first.
    /// User-requested push; opens the set-upstream dialog when the current
    /// branch does not track a remote yet.
    fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

        self.run_remote(gk_core::remote::RemoteOp::Push, cx);
    }

    /// Dialog asking to publish a branch as `<remote>/<branch>` and set it as
    /// upstream (`push --set-upstream`).
    fn open_upstream_dialog(
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
    fn set_upstream_and_push(&mut self, remote: String, branch: String, cx: &mut Context<Self>) {
        let args = vec!["push".to_string(), "--set-upstream".to_string(), remote, branch];
        self.run_git_remote("push", args, cx);
    }

    fn run_git_remote(&mut self, label: &'static str, args: Vec<String>, cx: &mut Context<Self>) {
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

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let foreground = cx.theme().foreground;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .text_color(foreground)
            // ---- toolbar ----
            .child(toolbar::render(self, cx))
            // ---- main split: branches | history + diff | staging ----
            // The resizable group always renders size_full — it must live in a
            // constrained wrapper, otherwise it overflows past the toolbar and
            // the status bar.
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("main-layout")
                        .with_state(&self.layout)
                        .child(
                            resizable_panel()
                                .size(px(180.))
                                .size_range(px(120.)..px(320.))
                                .child(branches::render(self, cx)),
                        )
                        // center column: history on top, diff below (when selected)
                        .child(
                            v_resizable("center-layout")
                                .with_state(&self.center_layout)
                                .child(resizable_panel().child(history::render(self, cx)))
                                .child(
                                    resizable_panel()
                                        .size(px(240.))
                                        .size_range(px(120.)..gpui::Pixels::MAX)
                                        .visible(self.selected_path.is_some())
                                        .child(diff_view::render(self, cx)),
                                ),
                        )
                        .child(
                            resizable_panel()
                                .size(px(340.))
                                .size_range(px(240.)..px(520.))
                                .child(staging::render(self, cx)),
                        ),
                ),
            )
            // ---- status bar ----
            .child(status_bar::render(self, cx))
    }
}
