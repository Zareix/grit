//! The workspace: application state, Git actions, and top-level layout.
//!
//! The layout pieces live in [`components`]; this module owns the state.

mod components;

use std::path::PathBuf;

use gk_core::{Repo, branch::Branch, commit::Commit, status::Status};
use gpui_kit::component::{
    ActiveTheme as _, ResizableState, h_resizable,
    input::{InputEvent, InputState},
    resizable_panel, v_resizable,
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
                self.sync_selection(&repo);
            }
            _ => {
                self.error = Some("Failed to read repository state".into());
            }
        }
        cx.notify();
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
            .child(toolbar::render(cx))
            // ---- main split: branches | history + diff | staging ----
            .child(
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
            )
            // ---- status bar ----
            .child(status_bar::render(self, cx))
    }
}
