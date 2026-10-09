//! The workspace: application state, Git actions, and top-level layout.
//!
//! The layout pieces live in [`components`]; this module owns the state.

mod actions;
mod components;

use std::path::PathBuf;

use gk_core::{
    Repo,
    branch::Branch,
    commit::Commit,
    status::{FileEntry, Status},
};
use gpui_kit::component::{
    ActiveTheme as _, ResizableState, h_resizable,
    input::{InputEvent, InputState},
    resizable_panel, v_resizable,
};
use gpui_kit::*;

use self::components::diff_view::PatchLine;
use self::components::{
    branches, commit_details, diff_view, history, staging, status_bar, toolbar,
};

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
    /// Pre-parsed patch lines of the selected file.
    diff_rows: Vec<PatchLine>,
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
    /// Commit selected in the history (its details replace the staging panel).
    selected_commit: Option<Commit>,
    /// Files changed by the selected commit.
    commit_files: Vec<FileEntry>,
    /// Whether `selected_path`'s diff comes from the selected commit (rather
    /// than the worktree).
    diff_from_commit: bool,
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
            diff_rows: Vec::new(),
            log: Vec::new(),
            branches: Vec::new(),
            current_branch: None,
            layout,
            center_layout,
            remote_busy: None,
            info: None,
            viewed_branch: None,
            branch_log: Vec::new(),
            selected_commit: None,
            commit_files: Vec::new(),
            diff_from_commit: false,
            error: None,
            _subscriptions: vec![enter_subscription],
        };
        this.open(path, cx);
        this
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
                            resizable_panel().size(px(340.)).size_range(px(240.)..px(520.)).child(
                                match self.selected_commit.clone() {
                                    Some(_) => commit_details::render(self, cx),
                                    None => staging::render(self, cx),
                                },
                            ),
                        ),
                ),
            )
            // ---- status bar ----
            .child(status_bar::render(self, cx))
    }
}
